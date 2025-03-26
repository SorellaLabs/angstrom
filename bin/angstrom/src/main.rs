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
    use angstrom_types::sol_bindings::{
        grouped_orders::{AllOrders, StandingVariants},
        rpc_orders::OmitOrderMeta,
    };
    use pade::PadeDecode;
    use serde_json::json;

    #[test]
    fn nina_test() {
        let order_json = json!(
            {"Standing":{"Exact":{"ref_id":0,"exact_in":true,"amount":1000000,"max_extra_fee_asset0":1,"min_price":1,"use_internal":false,"asset_in":"0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238","asset_out":"0xfFf9976782d46CC05630D1f6eBAb18b2324d6B14","recipient":"0x201f23a6197c396Acf7D6981287B18eba6A5078b","hook_data":"0x","nonce":1234,"deadline":1742949899,"meta":{"isEcdsa":true,"from":"0x201f23a6197c396Acf7D6981287B18eba6A5078b","signature":"0x1cdeaedec51173c2d11fd6e03d3e67d377eef5e76a4cd3114f4e99200c814efdd374bacb58ec5b4832ad05f6c6a3d68a2583b7b15d6e4663fc1577af08d09cf066"}}}}
        );
        let order: AllOrders = serde_json::from_value(order_json).unwrap();

        let AllOrders::Standing(StandingVariants::Exact(order)) = order else { panic!() };

        let s = order.meta.signature.to_vec();
        let mut slice = s.as_slice();

        let Ok(sig) = Signature::pade_decode(&mut slice, None) else { panic!() };
        let hash = order.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);

        let address = sig.recover_address_from_prehash(&hash).unwrap();
        println!("{hash:?}, {address:?}, {ANGSTROM_DOMAIN:?}");
    }
}
