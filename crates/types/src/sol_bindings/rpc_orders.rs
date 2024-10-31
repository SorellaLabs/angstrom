use std::borrow::Cow;

use alloy::{
    primitives::{keccak256, B256},
    sol,
    sol_types::{Eip712Domain, SolStruct}
};
use serde::{Deserialize, Serialize};

sol! {
    #[derive(Debug, Default, PartialEq, Eq, Hash,Serialize, Deserialize)]
    struct OrderMeta {
        bool isEcdsa;
        address from;
        bytes signature;
    }


    #[derive(Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]

    struct PartialStandingOrder {
        uint32 ref_id;
        uint128 min_amount_in;
        uint128 max_amount_in;
        uint128 max_extra_fee_asset0;
        uint256 min_price;
        bool use_internal;
        address asset_in;
        address asset_out;
        address recipient;
        bytes hook_data;
        uint64 nonce;
        uint40 deadline;
        OrderMeta meta;
    }

    #[derive(Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]

    struct ExactStandingOrder {
        uint32 ref_id;
        bool exact_in;
        uint128 amount;
        uint128 max_extra_fee_asset0;
        uint256 min_price;
        bool use_internal;
        address asset_in;
        address asset_out;
        address recipient;
        bytes hook_data;
        uint64 nonce;
        uint40 deadline;
        OrderMeta meta;
    }

    #[derive(Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]

    struct PartialFlashOrder {
        uint32 ref_id;
        uint128 min_amount_in;
        uint128 max_amount_in;
        uint128 max_extra_fee_asset0;
        uint256 min_price;
        bool use_internal;
        address asset_in;
        address asset_out;
        address recipient;
        bytes hook_data;
        uint64 valid_for_block;
        OrderMeta meta;
    }

    #[derive(Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]

    struct ExactFlashOrder {
        uint32 ref_id;
        bool exact_in;
        uint128 amount;
        uint128 max_extra_fee_asset0;
        uint256 min_price;
        bool use_internal;
        address asset_in;
        address asset_out;
        address recipient;
        bytes hook_data;
        uint64 valid_for_block;
        OrderMeta meta;
    }

    #[derive(Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]

    struct TopOfBlockOrder {
        uint128 quantity_in;
        uint128 quantity_out;
        uint128 max_gas_asset0;
        bool use_internal;
        address asset_in;
        address asset_out;
        address recipient;
        uint64 valid_for_block;
        OrderMeta meta;
    }
}

pub trait OmitOrderMeta: SolStruct {
    /// Returns component EIP-712 types. These types are used to construct
    /// the `encodeType` string. These are the types of the struct's fields,
    /// and should not include the root type.
    fn eip712_components(&self) -> Vec<Cow<'static, str>> {
        vec![]
    }

    /// Encodes this domain using [EIP-712 `encodeData`](https://eips.ethereum.org/EIPS/eip-712#definition-of-encodedata).
    fn eip712_encode_data(&self) -> Vec<u8> {
        let r = <Self as SolStruct>::eip712_encode_data(self);
        r[..r.len() - 32].to_vec()
    }

    /// Return the root EIP-712 type. This type is used to construct the
    /// `encodeType` string.
    fn eip712_root_type(&self) -> Cow<'static, str> {
        let r = <Self as SolStruct>::eip712_root_type();
        let r = r.to_string();
        let res = r.replace(",OrderMeta meta", "");
        Cow::Owned(res)
    }

    fn eip712_encode_type(&self) -> Cow<'static, str> {
        fn eip712_encode_types(
            root_type: Cow<'static, str>,
            mut components: Vec<Cow<'static, str>>
        ) -> Cow<'static, str> {
            if components.is_empty() {
                return root_type
            }

            components.sort_unstable();
            components.dedup();

            let mut s = String::with_capacity(
                root_type.len() + components.iter().map(|s| s.len()).sum::<usize>()
            );
            s.push_str(&root_type);
            for component in components {
                s.push_str(&component);
            }
            Cow::Owned(s)
        }

        eip712_encode_types(
            <Self as OmitOrderMeta>::eip712_root_type(self),
            <Self as OmitOrderMeta>::eip712_components(self)
        )
    }

    #[inline]
    fn eip712_type_hash(&self) -> B256 {
        keccak256(<Self as OmitOrderMeta>::eip712_encode_type(self).as_bytes())
    }

    #[inline]
    fn eip712_hash_struct(&self) -> B256 {
        let mut hasher = alloy::primitives::Keccak256::new();
        hasher.update(<Self as OmitOrderMeta>::eip712_type_hash(self));
        hasher.update(<Self as OmitOrderMeta>::eip712_encode_data(self));
        hasher.finalize()
    }

    /// See [EIP-712 `signTypedData`](https://eips.ethereum.org/EIPS/eip-712#specification-of-the-eth_signtypeddata-json-rpc).
    #[inline]
    fn no_meta_eip712_signing_hash(&self, domain: &Eip712Domain) -> B256 {
        let mut digest_input = [0u8; 2 + 32 + 32];
        digest_input[0] = 0x19;
        digest_input[1] = 0x01;
        digest_input[2..34].copy_from_slice(&domain.hash_struct()[..]);
        digest_input[34..66]
            .copy_from_slice(&<Self as OmitOrderMeta>::eip712_hash_struct(self)[..]);
        keccak256(digest_input)
    }
}

impl OmitOrderMeta for PartialStandingOrder {}
impl OmitOrderMeta for ExactStandingOrder {}
impl OmitOrderMeta for PartialFlashOrder {}
impl OmitOrderMeta for ExactFlashOrder {}
impl OmitOrderMeta for TopOfBlockOrder {}

#[cfg(test)]
pub mod test {
    use super::*;

    const TEST_DOMAIN: Eip712Domain = alloy::sol_types::eip712_domain! {
        name: "Angstrom",
        version: "0.61.0",
    };

    mod a {
        alloy::sol! {
            #[derive(Default)]
            struct PartialStandingOrder {
                uint32 ref_id;
                uint128 min_amount_in;
                uint128 max_amount_in;
                uint128 max_extra_fee_asset0;
                uint256 min_price;
                bool use_internal;
                address asset_in;
                address asset_out;
                address recipient;
                bytes hook_data;
                uint64 nonce;
                uint40 deadline;
            }
        }
    }
    #[test]
    fn ensure_eip712_omit_works() {
        let default_omit = a::PartialStandingOrder::default();
        let standard_order = PartialStandingOrder::default();

        // check type hash
        let d_typehash = default_omit.eip712_type_hash();
        let s_typehash = <PartialStandingOrder as OmitOrderMeta>::eip712_type_hash(&standard_order);
        assert_eq!(d_typehash, s_typehash);

        // check encode data
        let s_e = <PartialStandingOrder as OmitOrderMeta>::eip712_encode_data(&standard_order);
        let d_e = default_omit.eip712_encode_data();
        assert_eq!(s_e, d_e);

        // test hash struct
        let s_e = <PartialStandingOrder as OmitOrderMeta>::eip712_hash_struct(&standard_order);
        let d_e = default_omit.eip712_hash_struct();
        assert_eq!(s_e, d_e);

        let result = standard_order.no_meta_eip712_signing_hash(&TEST_DOMAIN);
        let expected = default_omit.eip712_signing_hash(&TEST_DOMAIN);

        assert_eq!(expected, result)
    }
}
