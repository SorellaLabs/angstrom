use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc
};

use alloy_consensus::Transaction;
use alloy_eips::{BlockId, BlockNumHash};
use alloy_primitives::{Address, Bytes, U256, utils::format_units};
use alloy_provider::Provider;
use alloy_rpc_types::{Filter, Log};
use alloy_sol_types::{SolCall, SolValue};
use angstrom_types_primitives::{
    contract_bindings::{
        angstrom::Angstrom, controller_v_1::ControllerV1, mintable_mock_erc_20::MintableMockERC20
    },
    contract_payloads::angstrom::AngstromBundle
};
use eyre::{Context, Result, ensure, eyre};
use futures::StreamExt;
use pade::PadeDecode;

use crate::types::*;

pub struct ProtocolFeeFetcher<P> {
    provider:  Arc<P>,
    max_block: u64
}

impl<P: Provider> ProtocolFeeFetcher<P> {
    pub async fn new(provider: P) -> Result<Self> {
        let max_block = provider.get_block_number().await?;
        Ok(Self { provider: Arc::new(provider), max_block })
    }

    pub async fn calculate(&self) -> Result<BundleFees> {
        let number = self.max_block;
        let header = self
            .provider
            .get_block_by_number(number.into())
            .await?
            .unwrap()
            .header;
        let block = BlockNumHash::new(number, header.hash);
        let last_collection = self.last_collection().await?;
        let saved = self.saved_gross(last_collection).await?;
        let mut tokens = Vec::new();

        for (asset, amount) in saved {
            let token = MintableMockERC20::new(asset, &self.provider);
            let pinned = BlockId::hash_canonical(block.hash);
            let (symbol, decimals) =
                tokio::try_join!(async { token.symbol().block(pinned).call().await }, async {
                    token.decimals().block(pinned).call().await
                })?;
            tokens.push(TokenSavings {
                asset,
                symbol,
                saved_gross: format_units(amount, decimals)?
            });
        }
        let current = self
            .provider
            .get_block_by_number(number.into())
            .await?
            .ok_or_else(|| eyre!("missing accounting block {number}"))?;
        ensure!(current.header.hash == block.hash, "accounting block reorganized; rerun");
        Ok(BundleFees { block, tokens })
    }

    async fn last_collection(&self) -> Result<Vec<Log>> {
        let owner = ControllerV1::new(controller_v1_address(), &self.provider)
            .owner()
            .block(self.max_block.into())
            .call()
            .await?;
        let filter = Filter::new()
            .address(owner)
            .event("CallExecuted(bytes32,uint256,address,uint256,bytes)");

        let total_blocks = self.max_block - angstrom_deployed_block() + 1;
        let block_chunks = (angstrom_deployed_block()..=self.max_block)
            .step_by(2_000)
            .map(|from| (from, (from + 1_999).min(self.max_block)))
            .collect::<Vec<_>>();

        let provider = self.provider.clone();
        let mut buffered_log_stream = futures::stream::iter(block_chunks)
            .map(|(from, to)| {
                let provider = provider.clone();
                let filter = filter.clone().from_block(from).to_block(to);
                async move {
                    let logs = provider.get_logs(&filter).await?;

                    let mut valid_logs = Vec::new();
                    for log in logs {
                        // Timelock puts id/index in topics; the data contains
                        // target/value/calldata.
                        let (target, _, calldata) =
                            <(Address, U256, Bytes)>::abi_decode_params(&log.data().data)?;
                        if target == controller_v1_address()
                            && calldata.starts_with(&ControllerV1::distributeFeesCall::SELECTOR)
                        {
                            let call = ControllerV1::distributeFeesCall::abi_decode(&calldata)?;
                            if call.assets.iter().any(|asset| !asset.total.is_zero()) {
                                valid_logs.push(log);
                            }
                        }
                    }

                    eyre::Ok(((from, to), valid_logs))
                }
            })
            .buffer_unordered(100);

        // The chunks complete out of order, so fold each one into a running maximum
        // instead of taking the last collection the stream happens to yield.
        let mut total_progress = 0;
        let mut all_valid_logs = Vec::new();
        while let Some(((from, to), batch_logs)) = buffered_log_stream.next().await.transpose()? {
            total_progress += to - from + 1;
            all_valid_logs.extend(batch_logs);
            tracing::info!(
                blocks_searched = total_progress,
                total_blocks,
                total_progress = (total_progress as f64 / total_blocks as f64),
                "DISTRIBUTE FEES log progress - completed batch"
            )
        }

        tracing::info!(total_blocks, "DISTRIBUTE FEES log progress - COMPLETE");

        all_valid_logs.sort_by_key(|log| {
            (log.block_number.unwrap(), log.transaction_index.unwrap(), log.log_index.unwrap())
        });

        Ok(all_valid_logs)
    }

    async fn saved_gross(&self, last_collection: Option<Log>) -> Result<BTreeMap<Address, U256>> {
        let start = last_collection
            .as_ref()
            .and_then(|log| log.block_number)
            .unwrap_or_else(angstrom_deployed_block);
        let mut saved = BTreeMap::<Address, U256>::new();
        let mut processed = HashSet::new();
        for from in (start..=self.max_block).step_by(2_000) {
            let to = (from + 1_999).min(self.max_block);
            let filter = Filter::new()
                .address(angstrom_address())
                .from_block(from)
                .to_block(to);
            let logs = self.provider.get_logs(&filter).await?;
            // Settlement emits the bundle savings commitment as an anonymous log.
            for log in logs.into_iter().filter(|log| log.topics().is_empty()) {
                if last_collection.as_ref().is_some_and(|last| {
                    (log.block_number, log.log_index) <= (last.block_number, last.log_index)
                }) {
                    continue
                }
                let hash = log
                    .transaction_hash
                    .ok_or_else(|| eyre!("missing bundle transaction hash"))?;
                if !processed.insert(hash) {
                    continue
                }
                let transaction = self
                    .provider
                    .get_transaction_by_hash(hash)
                    .await?
                    .ok_or_else(|| eyre!("missing bundle transaction {hash}"))?;

                let execute_call = Angstrom::executeCall::abi_decode(transaction.input())?;
                let bundle =
                    AngstromBundle::pade_decode(&mut &execute_call.encoded.as_ref(), None)?;
                for asset in bundle.assets {
                    let total = saved.entry(asset.addr).or_default();
                    *total = total
                        .checked_add(U256::from(asset.save))
                        .ok_or_else(|| eyre!("saved amount overflow for {}", asset.addr))?;
                }
            }
            tracing::info!(from, to, bundles = processed.len(), "scanned bundle savings");
        }
        Ok(saved)
    }
}

#[cfg(test)]
mod tests {
    use alloy_consensus::{Signed, TxEnvelope, TxLegacy, transaction::Recovered};
    use alloy_primitives::{B256, Signature, U64, keccak256};
    use alloy_provider::ProviderBuilder;
    use alloy_transport::mock::Asserter;
    use angstrom_types_primitives::{
        contract_bindings::angstrom::Angstrom::executeCall,
        contract_payloads::{Asset, angstrom::AngstromBundle}
    };
    use pade::PadeEncode;

    use super::*;

    async fn client() -> (Asserter, ProtocolFeeFetcher<impl Provider>) {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| angstrom_types_primitives::init_with_chain_id(1));
        let rpc = Asserter::new();
        rpc.push_success(&U64::from(angstrom_deployed_block() + 10));
        let provider = ProviderBuilder::new().connect_mocked_client(rpc.clone());
        (rpc, ProtocolFeeFetcher::new(provider).await.unwrap())
    }

    fn log(index: u64, data: Bytes) -> Log {
        Log {
            inner: alloy_primitives::Log::new_unchecked(angstrom_address(), vec![], data),
            block_number: Some(angstrom_deployed_block() + 10),
            log_index: Some(index),
            transaction_hash: Some(B256::repeat_byte(index as u8)),
            ..Default::default()
        }
    }

    fn distribution(index: u64, target: Address, total: Option<u64>) -> Log {
        let mut call = ControllerV1::distributeFeesCall::default();
        if let Some(total) = total {
            call.assets.resize_with(1, Default::default);
            call.assets[0].addr = Address::repeat_byte(1);
            call.assets[0].total = U256::from(total);
            call.assets[0].dists.resize_with(1, Default::default);
            call.assets[0].dists[0].to = Address::repeat_byte(2);
            call.assets[0].dists[0].amount = U256::from(total);
        }
        log(
            index,
            (target, U256::ZERO, Bytes::from(call.abi_encode()))
                .abi_encode_params()
                .into()
        )
    }

    #[tokio::test]
    async fn calculation_keeps_the_constructor_block_and_defaults_to_deployment() {
        let (rpc, client) = client().await;
        rpc.push_success(&U64::from(client.max_block + 1));
        assert_eq!(client.provider.get_block_number().await.unwrap(), client.max_block + 1);
        let mut block = alloy_rpc_types::Block::<alloy_rpc_types::Transaction>::default();
        block.header.number = client.max_block;
        block.header.hash = B256::repeat_byte(1);
        rpc.push_success(&block);
        rpc.push_success(&Bytes::from(Address::repeat_byte(3).abi_encode()));
        rpc.push_success(&Vec::<Log>::new()); // No previous collection.
        rpc.push_success(&Vec::<Log>::new()); // No savings since deployment.
        rpc.push_success(&block);
        let fees = client.calculate().await.unwrap();
        assert_eq!(fees.block.number, client.max_block);
        assert!(fees.tokens.is_empty());
        assert!(rpc.read_q().is_empty());
    }

    #[tokio::test]
    async fn selects_the_latest_nonzero_distribution_to_controller() {
        let (rpc, client) = client().await;
        let target = controller_v1_address();
        let latest = distribution(3, target, Some(9));
        rpc.push_success(&Bytes::from(Address::repeat_byte(3).abi_encode()));
        rpc.push_success(&vec![
            latest.clone(),
            distribution(7, Address::repeat_byte(4), Some(12)),
            distribution(1, target, Some(5)),
            distribution(6, target, None),
            distribution(5, target, Some(0)),
        ]);
        assert_eq!(client.last_collection().await.unwrap(), Some(latest));
        assert!(rpc.read_q().is_empty());
    }

    #[tokio::test]
    async fn savings_include_only_later_logs_in_the_collection_block_once() {
        let (rpc, client) = client().await;
        let asset = Asset { addr: Address::repeat_byte(1), save: 9, take: 9, settle: 0 };
        let bundle = AngstromBundle::new(vec![asset.clone()], vec![], vec![], vec![], vec![]);
        let mut summary = asset.addr.as_slice().to_vec();
        summary.extend_from_slice(&asset.save.to_be_bytes());
        let later = log(3, keccak256(summary).to_vec().into());
        rpc.push_success(&vec![log(1, Bytes::new()), later.clone(), later.clone()]);
        let signed = Signed::new_unchecked(
            TxLegacy {
                input: executeCall { encoded: bundle.pade_encode().into() }
                    .abi_encode()
                    .into(),
                ..Default::default()
            },
            Signature::new(U256::from(1), U256::from(2), false),
            later.transaction_hash.unwrap()
        );
        rpc.push_success(&alloy_rpc_types::Transaction {
            inner:               Recovered::new_unchecked(
                TxEnvelope::Legacy(signed),
                Address::ZERO
            ),
            block_hash:          None,
            block_number:        Some(client.max_block),
            transaction_index:   Some(1),
            effective_gas_price: Some(0)
        });
        let savings = client
            .saved_gross(Some(log(2, Bytes::new())))
            .await
            .unwrap();
        assert_eq!(savings, BTreeMap::from([(asset.addr, U256::from(9))]));
        assert!(rpc.read_q().is_empty());
    }
}
