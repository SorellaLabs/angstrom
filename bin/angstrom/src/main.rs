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
    use alloy::signers::local::PrivateKeySigner;
    use alloy_primitives::PrimitiveSignature;
    use angstrom_types::orders::CancelOrderRequest;
    use pade::PadeDecode;

    #[test]
    fn nina() {
        let item = serde_json::json!({"signature":"0x1cbd0ca8a089d0abf988d88e132a8cb25f6bf9b918443f21f686ab6ce0c10ec26d6a14e5840e20680f097e552383d7ba686fdec7084dc5b38e71b59d81621713ff","user_address":"0x201f23a6197c396Acf7D6981287B18eba6A5078b","order_id":"0x1aeb07ab759e7f028c5d848294aee4dbf9badc9d29c3a51bb9e87d5a7da30ea3"});
        let key: PrivateKeySigner =
            "3dc39e5072e5c707efda706d39afadae72d12c9282b11a7ba6fbddb21d571092"
                .parse()
                .unwrap();
        let item: CancelOrderRequest = serde_json::from_value(item).unwrap();
        let sig = item.sign(&key);
        let s = item.signature.to_vec();
        let buf = &mut s.as_slice();
        let rpc_sig = PrimitiveSignature::pade_decode(buf, None).unwrap();

        assert_eq!(sig, rpc_sig, "rust sig != got sig");
    }
}
