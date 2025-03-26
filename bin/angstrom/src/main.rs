// We use jemalloc for performance reasons
#[cfg(all(feature = "jemalloc", unix))]
#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn main() {
    if let Err(err) = angstrom::run() {
        eprintln!("Error: {err:?}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod test {
    use alloy::signers::Signature;
    use angstrom_types::primitive::ANGSTROM_DOMAIN;
    use angstrom_types::sol_bindings::grouped_orders::FlashVariants;
    use angstrom_types::sol_bindings::{
        grouped_orders::{AllOrders, StandingVariants},
        rpc_orders::OmitOrderMeta,
    };
    use pade::PadeDecode;
    use serde_json::json;

    #[test]
    fn nina_test() {
        let order_json = json!(
        {"Flash":{"Exact":{"ref_id":0,"exact_in":true,"amount":1000000,"max_extra_fee_asset0":1,"min_price":1,"use_internal":false,"asset_in":"0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238","asset_out":"0x1f9840a85d5aF5bf1D1762F925BDADdC4201F984","recipient":"0x201f23a6197c396Acf7D6981287B18eba6A5078b","hook_data":"0x","valid_for_block":7987447,"meta":{"isEcdsa":true,"from":"0x201f23a6197c396Acf7D6981287B18eba6A5078b","signature":"0x1c7db22c974302d998ee83448c0d4d20650cec48cdcf69466d31acb3eaff37e5a429875de3102cbff43b34daabbdd435cd2a16037365442f0ac2f73d670afd61b1"}}}}
                        );

        let expected_order_hash = alloy::primitives::fixed_bytes!(
            "0be00b7b9d725ac8b0039f29b17876f9fa4fe9a0dd6946089bc9c6f5dc37b49a"
        );
        let order: AllOrders = serde_json::from_value(order_json).unwrap();

        let AllOrders::Flash(FlashVariants::Exact(order)) = order else { panic!() };

        let hash = order.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);

        println!("{ANGSTROM_DOMAIN:#?}");
        println!("{order:#?}");
        assert_eq!(expected_order_hash, hash, "expected != got");

        let s = order.meta.signature.to_vec();
        let mut slice = s.as_slice();
        let Ok(sig) = Signature::pade_decode(&mut slice, None) else { panic!() };
        let address = sig.recover_address_from_prehash(&hash).unwrap();
        println!("{hash:?}, {address:?}");
    }
}
