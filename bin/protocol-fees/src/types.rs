use std::collections::{BTreeMap, BTreeSet};

use alloy_primitives::{Address, Bytes, TxHash, U256, aliases::U24, utils::format_units};
use alloy_rpc_types::Log;
use alloy_sol_types::{SolCall, SolEvent, SolValue};
use angstrom_types_primitives::{
    ANGSTROM_ADDRESS, ANGSTROM_DEPLOYED_BLOCK, CONTROLLER_V1_ADDRESS, POOL_MANAGER_ADDRESS,
    contract_bindings::{controller_v_1::ControllerV1, pool_manager::PoolManager},
    contract_payloads::Asset
};
use eyre::{Context, ensure, eyre};

pub fn angstrom_deployed_block() -> u64 {
    *ANGSTROM_DEPLOYED_BLOCK.get().unwrap()
}

pub fn controller_v1_address() -> Address {
    *CONTROLLER_V1_ADDRESS.get().unwrap()
}

pub fn angstrom_address() -> Address {
    *ANGSTROM_ADDRESS.get().unwrap()
}

pub fn pool_manager_address() -> Address {
    *POOL_MANAGER_ADDRESS.get().unwrap()
}

pub struct TokenMeta {
    pub asset:    Address,
    pub symbol:   String,
    pub decimals: u8
}

impl TokenMeta {
    /// Raw units first: raw integers are the ledger, decimals are display only.
    pub fn format(&self, amount: U256) -> eyre::Result<String> {
        Ok(format!("{amount} raw ({} {})", format_units(amount, self.decimals)?, self.symbol))
    }
}

pub struct ProtocolFeeCalculationBuilder {
    pub blocks: Vec<ProtocolFeeBlockCalculationBuilder>,
    pub tokens: Vec<TokenMeta>
}

impl ProtocolFeeCalculationBuilder {
    /// Section 2.3's ledger, replayed in block order:
    /// `candidateOutstandingSaved = savedGross - pulledAgainstSaved`.
    ///
    /// Every pull is classified as claimed against saved bundle fees. Section
    /// 2.3 calls an unlabeled pull a hard stop, so this is a conservative
    /// lower bound on what is still outstanding rather than exact bucket
    /// accounting.
    ///
    /// Blocks are the finest ordering available — the bundle scan keeps no
    /// transaction position for a save — so a pull is checked against an
    /// outstanding balance that already includes its own block's save.
    pub fn ledger(&self) -> eyre::Result<Vec<AssetLedgerRow>> {
        let mut saved = BTreeMap::<Address, U256>::new();
        let mut pulled = BTreeMap::<Address, U256>::new();

        for block in &self.blocks {
            for (asset, amount) in block.saved_gross()? {
                accumulate(&mut saved, asset, amount)?;
            }
            for (asset, amount) in block.pulled()? {
                accumulate(&mut pulled, asset, amount)?;
                // Section 2.6's first gate: `savedGross >= pulledAgainstSaved`. A
                // pull that outruns the commitments seen so far means the scanned
                // history is incomplete, not that the protocol earned extra.
                let taken = pulled[&asset];
                let committed = saved.get(&asset).copied().unwrap_or_default();
                ensure!(
                    taken <= committed,
                    "block {}: {asset} pulled {taken} against {committed} saved",
                    block.block_number
                );
            }
        }

        saved
            .keys()
            .chain(pulled.keys())
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|asset| {
                let saved_gross = saved.get(&asset).copied().unwrap_or_default();
                let pulled_against_saved = pulled.get(&asset).copied().unwrap_or_default();
                Ok(AssetLedgerRow {
                    asset,
                    saved_gross,
                    pulled_against_saved,
                    candidate_outstanding_saved: saved_gross
                        .checked_sub(pulled_against_saved)
                        .ok_or_else(|| eyre!("{asset} pulled more than it saved"))?
                })
            })
            .collect()
    }

    /// Symbol and decimals for an asset, when the token scan resolved them.
    pub fn token(&self, asset: Address) -> Option<&TokenMeta> {
        self.tokens.iter().find(|token| token.asset == asset)
    }
}

pub struct ProtocolFeeBlockCalculationBuilder {
    pub block_number:     u64,
    pub bundle_saved:     Vec<Asset>,
    pub distribute_calls: Vec<DecodedLogWithMeta<ControllerV1::distributeFeesCall>>
}

impl ProtocolFeeBlockCalculationBuilder {
    /// Per-asset `save` that this block's bundle committed.
    pub fn saved_gross(&self) -> eyre::Result<BTreeMap<Address, U256>> {
        let mut totals = BTreeMap::new();
        for asset in &self.bundle_saved {
            accumulate(&mut totals, asset.addr, U256::from(asset.save))
                .wrap_err_with(|| format!("save overflow at block {}", self.block_number))?;
        }
        Ok(totals)
    }

    /// Per-asset amount that this block's `distributeFees` calls pulled out of
    /// Angstrom.
    ///
    /// `ControllerV1.distributeFees` reverts unless each asset's distributions
    /// sum to its pulled total, so an executed call that fails this check
    /// was decoded wrong.
    pub fn pulled(&self) -> eyre::Result<BTreeMap<Address, U256>> {
        let mut totals = BTreeMap::new();
        for call in &self.distribute_calls {
            for asset in &call.data.assets {
                let distributed = asset
                    .dists
                    .iter()
                    .try_fold(U256::ZERO, |sum, dist| sum.checked_add(dist.amount))
                    .ok_or_else(|| eyre!("distribution overflow in {}", call.tx_hash))?;
                ensure!(
                    distributed == asset.total,
                    "{} pulls {} of {} but distributes {distributed}",
                    call.tx_hash,
                    asset.total,
                    asset.addr
                );
                accumulate(&mut totals, asset.addr, asset.total)
                    .wrap_err_with(|| format!("pull overflow at block {}", self.block_number))?;
            }
        }
        Ok(totals)
    }
}

/// One per-asset row of the bundle-held ledger, in raw token units.
///
/// `candidate_outstanding_saved` is section 2.3's `savedGross -
/// pulledAgainstSaved`. It is a collective commitment remainder, **not** a
/// withdrawal amount: the ownership rule (2.4), the protected-balance replay
/// (2.5) and the reservation gates (2.6) are not evaluated here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetLedgerRow {
    pub asset: Address,
    pub saved_gross: U256,
    pub pulled_against_saved: U256,
    pub candidate_outstanding_saved: U256
}

fn accumulate(
    totals: &mut BTreeMap<Address, U256>,
    asset: Address,
    amount: U256
) -> eyre::Result<()> {
    let total = totals.entry(asset).or_default();
    *total = total
        .checked_add(amount)
        .ok_or_else(|| eyre!("amount overflow for {asset}"))?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedLogWithMeta<T> {
    pub block_number: u64,
    pub tx_hash:      TxHash,
    pub tx_index:     u64,
    pub log_index:    u64,
    pub data:         T
}

impl<T: PartialEq + PartialOrd> PartialOrd for DecodedLogWithMeta<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some((self.block_number, self.tx_index, self.log_index).cmp(&(
            other.block_number,
            other.tx_index,
            other.log_index
        )))
    }
}

impl<T: PartialEq + Eq + PartialOrd> Ord for DecodedLogWithMeta<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.block_number, self.tx_index, self.log_index).cmp(&(
            other.block_number,
            other.tx_index,
            other.log_index
        ))
    }
}

pub fn decode_distribute_fees_logs(
    logs: Vec<Log>
) -> eyre::Result<Vec<DecodedLogWithMeta<ControllerV1::distributeFeesCall>>> {
    let mut valid_logs = Vec::new();
    for log in logs {
        // Timelock puts id/index in topics; the data contains
        // target/value/calldata.
        let (target, _, calldata) = <(Address, U256, Bytes)>::abi_decode_params(&log.data().data)?;
        if target == controller_v1_address()
            && calldata.starts_with(&ControllerV1::distributeFeesCall::SELECTOR)
        {
            let call = ControllerV1::distributeFeesCall::abi_decode(&calldata)?;
            if call.assets.iter().any(|asset| !asset.total.is_zero()) {
                let meta_log = DecodedLogWithMeta {
                    block_number: log.block_number.unwrap(),
                    tx_hash:      log.transaction_hash.unwrap(),
                    tx_index:     log.transaction_index.unwrap(),
                    log_index:    log.log_index.unwrap(),
                    data:         call
                };
                valid_logs.push(meta_log);
            }
        }
    }
    Ok(valid_logs)
}

pub fn decode_angstrom_pool_manager_logs(
    logs: Vec<Log>
) -> eyre::Result<Vec<DecodedLogWithMeta<()>>> {
    let mut valid_logs = Vec::new();
    for log in logs {
        if let Ok(swap_log) = PoolManager::Swap::decode_log(&log.inner)
            && swap_log.fee == U24::ZERO
        {
            let meta_log = DecodedLogWithMeta {
                block_number: log.block_number.unwrap(),
                tx_hash:      log.transaction_hash.unwrap(),
                tx_index:     log.transaction_index.unwrap(),
                log_index:    log.log_index.unwrap(),
                data:         ()
            };
            valid_logs.push(meta_log);
        }
    }
    Ok(valid_logs)
}

#[cfg(test)]
mod tests {
    use alloy_primitives::B256;

    use super::*;

    fn asset(byte: u8) -> Address {
        Address::repeat_byte(byte)
    }

    fn saved(addr: Address, save: u128) -> Asset {
        Asset { addr, save, take: 0, settle: 0 }
    }

    fn pull(
        addr: Address,
        total: u64,
        dists: &[u64]
    ) -> DecodedLogWithMeta<ControllerV1::distributeFeesCall> {
        let call = ControllerV1::distributeFeesCall {
            assets: vec![ControllerV1::Asset {
                addr,
                total: U256::from(total),
                dists: dists
                    .iter()
                    .map(|amount| ControllerV1::Distribution {
                        to:     asset(9),
                        amount: U256::from(*amount)
                    })
                    .collect()
            }]
        };
        DecodedLogWithMeta {
            block_number: 0,
            tx_hash:      B256::ZERO,
            tx_index:     0,
            log_index:    0,
            data:         call
        }
    }

    fn block(
        block_number: u64,
        bundle_saved: Vec<Asset>,
        distribute_calls: Vec<DecodedLogWithMeta<ControllerV1::distributeFeesCall>>
    ) -> ProtocolFeeBlockCalculationBuilder {
        ProtocolFeeBlockCalculationBuilder { block_number, bundle_saved, distribute_calls }
    }

    #[test]
    fn a_block_sums_saves_and_pulls_per_asset() {
        let one = block(
            1,
            vec![saved(asset(1), 7), saved(asset(2), 4)],
            vec![pull(asset(1), 3, &[1, 2])]
        );
        assert_eq!(
            one.saved_gross().unwrap(),
            BTreeMap::from([(asset(1), U256::from(7)), (asset(2), U256::from(4))])
        );
        assert_eq!(one.pulled().unwrap(), BTreeMap::from([(asset(1), U256::from(3))]));
    }

    #[test]
    fn a_pull_whose_distributions_do_not_sum_to_its_total_is_a_hard_stop() {
        // `distributeFees` reverts on this, so an executed call that shows it was
        // decoded wrong.
        assert!(
            block(1, vec![], vec![pull(asset(1), 5, &[1, 2])])
                .pulled()
                .is_err()
        );
    }

    #[test]
    fn the_ledger_subtracts_pulls_from_saves_across_blocks() {
        let calculation = ProtocolFeeCalculationBuilder {
            blocks: vec![
                block(1, vec![saved(asset(1), 10), saved(asset(2), 6)], vec![]),
                block(2, vec![saved(asset(1), 5)], vec![pull(asset(1), 12, &[12])]),
            ],
            tokens: vec![]
        };
        assert_eq!(
            calculation.ledger().unwrap(),
            vec![
                AssetLedgerRow {
                    asset: asset(1),
                    saved_gross: U256::from(15),
                    pulled_against_saved: U256::from(12),
                    candidate_outstanding_saved: U256::from(3)
                },
                AssetLedgerRow {
                    asset: asset(2),
                    saved_gross: U256::from(6),
                    pulled_against_saved: U256::ZERO,
                    candidate_outstanding_saved: U256::from(6)
                },
            ]
        );
    }

    #[test]
    fn a_pull_outrunning_the_saves_seen_so_far_is_a_hard_stop() {
        // Section 2.6 requires `savedGross >= pulledAgainstSaved`; an earlier save
        // cannot be inferred from a later one.
        let calculation = ProtocolFeeCalculationBuilder {
            blocks: vec![
                block(1, vec![], vec![pull(asset(1), 4, &[4])]),
                block(2, vec![saved(asset(1), 9)], vec![]),
            ],
            tokens: vec![]
        };
        assert!(calculation.ledger().is_err());
    }

    #[test]
    fn token_metadata_shows_raw_units_alongside_decimals() {
        let calculation = ProtocolFeeCalculationBuilder {
            blocks: vec![],
            tokens: vec![TokenMeta {
                asset:    asset(1),
                symbol:   "USDC".to_string(),
                decimals: 6
            }]
        };
        let token = calculation.token(asset(1)).unwrap();
        assert_eq!(
            token.format(U256::from(36_411_377_866u64)).unwrap(),
            "36411377866 raw (36411.377866 USDC)"
        );
        assert!(calculation.token(asset(2)).is_none());
    }
}
