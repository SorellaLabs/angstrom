use alloy_eips::BlockId;
use alloy_network::Network;
use alloy_primitives::{Address, B256, U256};
use alloy_provider::Provider;

use crate::{
    contract_bindings::angstrom_protocol_fee_config::AngstromProtocolFeeConfig::{
        self, LpDonationSplitsSet
    },
    primitive::{ANGSTROM_ADDRESS, PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK}
};

/// Slot 0 of `AngstromProtocolFeeConfig` holds both shares packed into one
/// word. Part of the contract's interface: see [`DonationSplits::from_slot0`].
pub const PROTOCOL_FEE_CONFIG_SLOT: u32 = 0;

/// The configuration `AngstromProtocolFeeConfig` is deployed with, which
/// preserves pre-activation economics exactly.
///
/// Module-private: it is the one value nothing outside here may construct, so
/// no caller can quietly substitute a default for a real read.
///
/// Its block identity is fixed rather than the caller's, so a pre-deployment
/// resolution is distinguishable from a chain read. Consumers that check parent
/// identity must expect that.
const DEPLOYED_INITIAL_PROTOCOL_FEE_CONFIG: DonationSplitSnapshot = DonationSplitSnapshot {
    block_number: 0,
    block_hash:   B256::ZERO,
    // Bypasses `DonationSplits::new`; both literals are within `DENOM` by inspection.
    splits:       DonationSplits { user_lp_share_e6: 750_000, tob_lp_share_e6: 1_000_000 }
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DonationSplits {
    user_lp_share_e6: u32,
    tob_lp_share_e6:  u32
}

impl DonationSplits {
    pub const DENOM: u32 = 1_000_000;

    /// The only constructor. Rejects either share above DENOM.
    pub fn new(user_lp_share_e6: u32, tob_lp_share_e6: u32) -> eyre::Result<Self> {
        if user_lp_share_e6 > Self::DENOM {
            return Err(eyre::eyre!("`user_lp_share_e6` must be at most `1_000_000`"));
        }
        if tob_lp_share_e6 > Self::DENOM {
            return Err(eyre::eyre!("`tob_lp_share_e6` must be at most `1_000_000`"));
        }
        Ok(Self { user_lp_share_e6, tob_lp_share_e6 })
    }

    /// Decodes both shares from slot 0 of `AngstromProtocolFeeConfig`, where
    /// `user_lp_share_e6` occupies bits 0..32 and `tob_lp_share_e6` bits
    /// 32..64.
    ///
    /// Rejects nonzero padding above bit 63.
    ///
    /// A zero word decodes to a valid pair of 0% shares, so this cannot tell a
    /// deliberate 0% configuration from an empty account. Callers must confirm
    /// the address holds the expected code before trusting the result.
    pub fn from_slot0(word: U256) -> eyre::Result<Self> {
        if word >> 64usize != U256::ZERO {
            return Err(eyre::eyre!("slot0 has nonzero padding above bit 63: {word:#x}"));
        }

        let mask = U256::from(u32::MAX);
        let user_lp_share_e6 = (word & mask).to::<u32>();
        let tob_lp_share_e6 = ((word >> 32usize) & mask).to::<u32>();

        Self::new(user_lp_share_e6, tob_lp_share_e6)
    }

    /// The pair the setter overwrote, for inverting a reorged-out change.
    ///
    /// Bounded on-chain exactly like the new pair, so the same reasoning as
    /// [`DonationSplits::from`] applies.
    pub fn overwritten_by(value: &LpDonationSplitsSet) -> Self {
        Self::new(value.oldUserLpShareE6, value.oldTobLpShareE6)
            .expect("this is not possible - verification is done onchain")
    }

    /// Splits `gross` total user fees into `(lp, protocol)`.
    pub fn split_user(&self, gross: u128) -> (u128, u128) {
        split(gross, self.user_lp_share_e6)
    }

    /// Splits a `gross` top-of-block payment into `(lp, protocol)`.
    pub fn split_tob(&self, gross: u128) -> (u128, u128) {
        split(gross, self.tob_lp_share_e6)
    }
}

impl From<LpDonationSplitsSet> for DonationSplits {
    /// The pair the setter wrote.
    ///
    /// `AngstromProtocolFeeConfig` rejects either share above `1_000_000`, and
    /// the eth manager only decodes logs emitted by that address, so the bounds
    /// hold by construction. A panic here means a misconfigured address or a
    /// stale ABI, not bad input.
    fn from(value: LpDonationSplitsSet) -> Self {
        Self::new(value.newUserLpShareE6, value.newTobLpShareE6)
            .expect("this is not possible - verification is done onchain")
    }
}

fn split(gross: u128, share_e6: u32) -> (u128, u128) {
    let lp =
        (U256::from(gross) * U256::from(share_e6) / U256::from(DonationSplits::DENOM)).to::<u128>();
    (lp, gross - lp) // LP rounds down, protocol takes the exact remainder
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DonationSplitSnapshot {
    pub block_number: u64,
    pub block_hash:   B256,
    pub splits:       DonationSplits
}

impl DonationSplitSnapshot {
    /// The configuration in force at `block_number` / `block_hash`.
    ///
    /// A block at or before the contract's deployment resolves to the deployed
    /// configuration without touching the provider, which is what lets replay
    /// run either side of activation without the caller special-casing it. That
    /// snapshot carries its own fixed block identity, not the requested one.
    ///
    /// Past deployment, every read is pinned to `block_hash`, so the pair can
    /// never be composed from two different states.
    pub async fn load_from_chain<N, P>(
        config_address: Address,
        block_number: u64,
        block_hash: B256,
        provider: &P
    ) -> eyre::Result<Self>
    where
        N: Network,
        P: Provider<N>
    {
        // `AngstromAddressConfig::try_init` skips a zero deployed block, so unset and
        // genesis are indistinguishable here. Treating unset as genesis sends the call
        // down the read path, where the code check gives the actionable error.
        let deployed_block = PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK
            .get()
            .copied()
            .unwrap_or_default();
        if block_number <= deployed_block {
            return Ok(DEPLOYED_INITIAL_PROTOCOL_FEE_CONFIG);
        }

        let block_id = BlockId::Hash(block_hash.into());

        // An empty account reads as zero storage, which decodes as a valid pair of 0%
        // shares. Without this check a wrong address silently runs the node at 0/0.
        let code = provider
            .get_code_at(config_address)
            .block_id(block_id)
            .await?;
        if code.is_empty() {
            return Err(eyre::eyre!("no code at protocol fee config address {config_address}"));
        }

        // A config bound to a different Angstrom would hand us someone else's rates.
        let bound_angstrom = AngstromProtocolFeeConfig::new(config_address, provider)
            .angstrom()
            .block(block_id)
            .call()
            .await?;
        let expected_angstrom = *ANGSTROM_ADDRESS
            .get()
            .ok_or_else(|| eyre::eyre!("`ANGSTROM_ADDRESS` is not initialized"))?;
        if bound_angstrom != expected_angstrom {
            return Err(eyre::eyre!(
                "protocol fee config {config_address} is bound to angstrom {bound_angstrom}, \
                 expected {expected_angstrom}"
            ));
        }

        let word = provider
            .get_storage_at(config_address, U256::from(PROTOCOL_FEE_CONFIG_SLOT))
            .block_id(block_id)
            .await?;

        Ok(Self { block_number, block_hash, splits: DonationSplits::from_slot0(word)? })
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{Bytes, bytes};
    use alloy_provider::ProviderBuilder;
    use alloy_sol_types::SolCall;
    use alloy_transport::mock::Asserter;

    use super::*;
    use crate::primitive::AngstromAddressConfig;

    /// The exact slot-0 word `AngstromProtocolFeeConfig(_, 750_000, 1_000_000)`
    /// produces, verified against forge.
    const DEPLOYED_WORD: u128 = (1_000_000u128 << 32) | 750_000u128;

    /// Any nonempty runtime code: `load_from_chain` only checks that the
    /// account is not empty, the `angstrom()` call proves the rest.
    const SOME_CODE: Bytes = bytes!("0x60806040");

    /// A block past `PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK`, which the tests leave
    /// unset (so it reads as genesis).
    const POST_DEPLOYMENT_BLOCK: u64 = 100;

    fn angstrom_address() -> Address {
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();
        *ANGSTROM_ADDRESS.get().unwrap()
    }

    /// Queues the responses `load_from_chain` consumes, in call order.
    fn asserter(code: Bytes, bound_angstrom: Address, slot0: U256) -> Asserter {
        let a = Asserter::new();
        a.push_success(&code);
        a.push_success(&Bytes::from(AngstromProtocolFeeConfig::angstromCall::abi_encode_returns(
            &bound_angstrom
        )));
        a.push_success(&B256::from(slot0));
        a
    }

    async fn load_at(block_number: u64, asserter: Asserter) -> eyre::Result<DonationSplitSnapshot> {
        let provider = ProviderBuilder::new().connect_mocked_client(asserter);
        DonationSplitSnapshot::load_from_chain(
            Address::repeat_byte(0xcf),
            block_number,
            B256::repeat_byte(0xbb),
            &provider
        )
        .await
    }

    async fn load(asserter: Asserter) -> eyre::Result<DonationSplitSnapshot> {
        load_at(POST_DEPLOYMENT_BLOCK, asserter).await
    }

    #[tokio::test]
    async fn reads_both_shares_from_one_block() {
        let snapshot = load(asserter(SOME_CODE, angstrom_address(), U256::from(DEPLOYED_WORD)))
            .await
            .unwrap();

        assert_eq!(snapshot.splits, DonationSplits::new(750_000, 1_000_000).unwrap());
        // A chain read carries the requested block's identity.
        assert_eq!(snapshot.block_number, POST_DEPLOYMENT_BLOCK);
        assert_eq!(snapshot.block_hash, B256::repeat_byte(0xbb));
    }

    #[tokio::test]
    async fn at_or_before_deployment_resolves_without_a_provider_call() {
        angstrom_address();
        // An asserter with nothing queued panics on the first request, so reaching the
        // provider at all fails this test.
        let snapshot = load_at(0, Asserter::new()).await.unwrap();

        assert_eq!(snapshot, DEPLOYED_INITIAL_PROTOCOL_FEE_CONFIG);
        assert_eq!(snapshot.splits, DonationSplits::new(750_000, 1_000_000).unwrap());
        // The const's own identity, not the requested block's, so a pre-deployment
        // resolution is distinguishable from a chain read.
        assert_eq!(snapshot.block_number, 0);
        assert_eq!(snapshot.block_hash, B256::ZERO);
    }

    #[tokio::test]
    async fn empty_account_is_an_error() {
        // Would otherwise decode as two valid 0% shares.
        let err = load(asserter(Bytes::new(), angstrom_address(), U256::ZERO))
            .await
            .unwrap_err();

        assert!(err.to_string().contains("no code"), "{err}");
    }

    #[tokio::test]
    async fn config_bound_to_another_angstrom_is_an_error() {
        let ours = angstrom_address();
        let err = load(asserter(SOME_CODE, Address::repeat_byte(0xaa), U256::from(DEPLOYED_WORD)))
            .await
            .unwrap_err();

        assert!(err.to_string().contains("bound to angstrom"), "{err}");
        assert_ne!(ours, Address::repeat_byte(0xaa));
    }

    #[tokio::test]
    async fn malformed_slot0_is_an_error() {
        let err = load(asserter(SOME_CODE, angstrom_address(), U256::MAX))
            .await
            .unwrap_err();

        assert!(err.to_string().contains("padding"), "{err}");
    }

    #[test]
    fn decodes_the_deployed_word() {
        let s = DonationSplits::from_slot0(U256::from(DEPLOYED_WORD)).unwrap();
        assert_eq!(s.user_lp_share_e6, 750_000);
        assert_eq!(s.tob_lp_share_e6, 1_000_000);
        // The const and the word the deployed contract produces agree.
        assert_eq!(s, DEPLOYED_INITIAL_PROTOCOL_FEE_CONFIG.splits);
    }

    #[test]
    fn rejects_padding_above_bit_63() {
        let word = U256::from(DEPLOYED_WORD) | (U256::from(1u8) << 64usize);
        assert!(DonationSplits::from_slot0(word).is_err());
        assert!(DonationSplits::from_slot0(U256::MAX).is_err());
    }

    #[test]
    fn rejects_out_of_range_shares() {
        assert!(DonationSplits::from_slot0(U256::from(1_000_001u64)).is_err());
        assert!(DonationSplits::from_slot0(U256::from(1_000_001u128 << 32)).is_err());
        assert!(DonationSplits::from_slot0(U256::ZERO).is_ok(), "empty account decodes as 0/0");
    }

    #[test]
    fn splits_conserve_and_round_lp_down() {
        let s = DonationSplits::new(750_000, 1_000_000).unwrap();
        for gross in [0u128, 1, 3, 7, 99, 1_000_000, u128::MAX, u128::MAX / 3] {
            let (lp, protocol) = s.split_user(gross);
            assert_eq!(lp.checked_add(protocol), Some(gross), "user conservation at {gross}");
            assert!(lp <= gross);
            let (lp, protocol) = s.split_tob(gross);
            assert_eq!(lp, gross, "100% tob share is all LP");
            assert_eq!(protocol, 0);
        }
        // 75% of 7 rounds down to 5, protocol takes the exact remainder.
        assert_eq!(s.split_user(7), (5, 2));
    }

    #[test]
    fn extremes() {
        let all_protocol = DonationSplits::new(0, 0).unwrap();
        assert_eq!(all_protocol.split_user(1_000), (0, 1_000));
        assert_eq!(all_protocol.split_tob(1_000), (0, 1_000));
        let all_lp = DonationSplits::new(1_000_000, 1_000_000).unwrap();
        assert_eq!(all_lp.split_user(u128::MAX), (u128::MAX, 0));
        assert_eq!(all_lp.split_tob(u128::MAX), (u128::MAX, 0));
        // The two shares are independent and not transposed.
        let asym = DonationSplits::new(1_000_000, 0).unwrap();
        assert_eq!(asym.split_user(1_000), (1_000, 0));
        assert_eq!(asym.split_tob(1_000), (0, 1_000));
    }
}
