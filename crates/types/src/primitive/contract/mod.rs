use std::collections::HashMap;

use alloy::{
    dyn_abi::Eip712Domain,
    primitives::{aliases::U24, Address},
    sol,
    sol_types::eip712_domain
};

use crate::contract_bindings::angstrom::Angstrom::PoolKey;

sol! {
#![sol(all_derives = true)]
ERC20,
"src/primitive/contract/ERC20.json"}

pub use ERC20::*;

use crate::primitive::PoolId;

pub const TESTNET_ANGSTROM_ADDRESS: Address =
    alloy::primitives::address!("293954613283cC7B82BfE9676D3cc0fb0A58fAa0");

// The `eip712_domain` macro lets you easily define an EIP-712 domain
// object :)
#[cfg(not(feature = "testnet"))]
pub const ANGSTROM_DOMAIN: Eip712Domain = eip712_domain!(
    name: "Angstrom",
    version: "v1",
    chain_id: 1,
    verifying_contract: TESTNET_ANGSTROM_ADDRESS,
);

#[cfg(feature = "testnet")]
pub const ANGSTROM_DOMAIN: Eip712Domain = eip712_domain!(
    name: "Angstrom",
    version: "v1",
    chain_id: 344567,
    verifying_contract: TESTNET_ANGSTROM_ADDRESS,
);

#[derive(Default, Clone)]
pub struct UniswapPoolRegistry {
    pools:              HashMap<PoolId, PoolKey>,
    pub conversion_map: HashMap<PoolId, PoolId>
}
impl UniswapPoolRegistry {
    pub fn get(&self, pool_id: &PoolId) -> Option<&PoolKey> {
        self.pools.get(pool_id)
    }

    pub fn pools(&self) -> HashMap<PoolId, PoolKey> {
        self.pools.clone()
    }
}
impl From<Vec<PoolKey>> for UniswapPoolRegistry {
    fn from(pools: Vec<PoolKey>) -> Self {
        let pubmap = pools
            .iter()
            .map(|pool_key| {
                let pool_id = PoolId::from(pool_key.clone());
                (pool_id, pool_key.clone())
            })
            .collect();

        let priv_map = pools
            .into_iter()
            .map(|mut pool_key| {
                let pool_id_pub = PoolId::from(pool_key.clone());
                pool_key.fee = U24::from(0x800000);
                let pool_id_priv = PoolId::from(pool_key.clone());
                (pool_id_pub, pool_id_priv)
            })
            .collect();
        Self { pools: pubmap, conversion_map: priv_map }
    }
}

/*
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use alloy::signers::{local::LocalSigner, SignerSync};
    use alloy_primitives::{
        address, bytes, keccak256, normalize_v, Bytes, PrimitiveSignature, U256
    };
    use pade::{PadeDecode, PadeEncode};

    use super::*;
    use crate::sol_bindings::rpc_orders::{ExactFlashOrder, OmitOrderMeta};

    #[test]
    fn test_this() {
        // mine
        let private_key = "7bbd27422e113bc608150c92768febbf9c4d6e6cea369e121f459cb2e3a07c08";

        // james
        // let private_key =
        // "3aa3e5f24a62ec34b483702c019b4dfa5e1676378dbe7e07a91f9f80a61ef538";

        let signer = LocalSigner::from_str(private_key).unwrap();

        let order = ExactFlashOrder {
            ref_id:               0,
            exact_in:             true,
            amount:               1000000000000000000,
            max_extra_fee_asset0: 1000000000000000000,
            min_price:            U256::from(36461679927861141504_u128),
            use_internal:         false,
            asset_in:             address!("0x3d85e7b30be9fd7a4bad709d6ed2d130579f9a2e"),
            asset_out:            address!("0xd550015f84142abecd5e82c8f296083df3c9a80d"),
            recipient:            address!("0xa7f1aeb6e43443c683865fdb9e15dd01386c955b"),
            hook_data:            Default::default(),
            valid_for_block:      21825215,
            meta:                 Default::default()
        };

        println!("Angstrom Domain Struct Encode Type: {}", ANGSTROM_DOMAIN.encode_type());

        let hash = order.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
        let prehash = order.no_meta_eip712_signing_prehash(&ANGSTROM_DOMAIN);
        let prim_sig = signer.sign_hash_sync(&hash).unwrap();
        let sig: Bytes = prim_sig.pade_encode().into();
        let bytes_sig: Bytes = prim_sig.as_bytes().into();
        println!("\n-------RUST-------");
        println!(
            "Hash: {hash:?}\nPre-Hash: {prehash:?}\nBytes Sig: {bytes_sig:?}\nPade Sig: \
             {sig:?}\nPrimitive Sig: {prim_sig:?}\n\n"
        );

        // real - 0x853c1c30956b8cb46936166216f139cd84cba88a14988f49eb8760c84f5b8d47018c3cefb0ef946cfcaea17bc3cc35d7bcc268b6b390240509fe09262170cac71c
        // play - 0x4355c47d63924e8a72e509b65029052eb6c299d53a04e167c5775fd466751c9d07299936d304c153f6443dfa05f40ff007d72911b6f72307f996231605b915621c
        let js_sig = bytes!("0xdfbb91cc053c6817fe2108ecfc80a47f927951a0c1c7be9542b5eb0a0b05c3a619b7b0fb882d9627c7c50a335e181251a30737423c919074fe4b0e9fea68a75c1b");
        let js_hash = keccak256(&js_sig);
        println!("-------JS-------\n");
        println!("Hash: {js_hash:?}\nSig: {js_sig:?}\n\n");
        println!("LEN: {:?}", (**js_sig).len());
        let pade_decoded_sig: Bytes = PrimitiveSignature::pade_decode(&mut &**js_sig, None)
            .unwrap()
            .pade_encode()
            .into();

        let bytes_decoded_sig = PrimitiveSignature::try_from(&**js_sig).unwrap();
        let pade_encoded_from_js_sig: Bytes = bytes_decoded_sig.pade_encode().into();

        let big_endian_decoded_sig = PrimitiveSignature::new(
            U256::from_be_slice(&js_sig[0..32]),
            U256::from_be_slice(&js_sig[32..64]),
            normalize_v(js_sig[64] as u64).unwrap()
        );
        println!(
            "Big Endian Decoded: {big_endian_decoded_sig:?}\nBytes Decoded: \
             {bytes_decoded_sig:?}\nPade Encoded From Js Decoded: {pade_encoded_from_js_sig:?}\n\n"
        );

        // assert_eq!(decoded_sig, sig);
    }
}

*/
