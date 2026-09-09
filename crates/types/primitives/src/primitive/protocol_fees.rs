use alloy_primitives::{B256, U256};

use crate::contract_bindings::angstrom_protocol_fee_config::AngstromProtocolFeeConfig::LpDonationSplitsSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DonationSplits {
    user_lp_share_e6: u32,
    tob_lp_share_e6:  u32
}

impl From<LpDonationSplitsSet> for DonationSplits {
    fn from(value: LpDonationSplitsSet) -> Self {
        Self { user_lp_share_e6: value.newUserLpShareE6, tob_lp_share_e6: value.newTobLpShareE6 }
    }
}

impl DonationSplits {
    pub const DENOM: u32 = 1_000_000;

    /// The only constructor. Rejects either share above DENOM.
    pub fn new(user_lp_share_e6: u32, tob_lp_share_e6: u32) -> eyre::Result<Self> {
        if user_lp_share_e6 > Self::DENOM {
            return Err(eyre::eyre!("`user_lp_share_e6` must be greater than `1_000_000`"));
        }
        if tob_lp_share_e6 > Self::DENOM {
            return Err(eyre::eyre!("`tob_lp_share_e6` must be greater than `1_000_000`"));
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

    /// Splits `gross` total user fees into `(lp, protocol)`.
    pub fn split_user(&self, gross: u128) -> (u128, u128) {
        split(gross, self.user_lp_share_e6)
    }

    /// Splits a `gross` top-of-block payment into `(lp, protocol)`.
    pub fn split_tob(&self, gross: u128) -> (u128, u128) {
        split(gross, self.tob_lp_share_e6)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact slot-0 word `AngstromProtocolFeeConfig(_, 750_000, 1_000_000)`
    /// produces, verified against forge.
    const DEPLOYED_WORD: u128 = (1_000_000u128 << 32) | 750_000u128;

    #[test]
    fn decodes_the_deployed_word() {
        let s = DonationSplits::from_slot0(U256::from(DEPLOYED_WORD)).unwrap();
        assert_eq!(s.user_lp_share_e6, 750_000);
        assert_eq!(s.tob_lp_share_e6, 1_000_000);
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
