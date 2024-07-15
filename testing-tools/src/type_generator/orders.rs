use angstrom_types::{
    orders::{OrderId, OrderPriorityData},
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        sol::FlashOrder
    }
};
use rand::{rngs::ThreadRng, Rng};

// pub fn generate_random_valid_order() -> AllOrders {
//     let mut rng = thread_rng();
//     let sk = SecretKey::new(&mut rng);
//     let mut baseline_order = generate_order(&mut rng);
//
//     let sign_hash = baseline_order.eip712_signing_hash(&ANGSTROM_DOMAIN);
//
//     let signature =
//         reth_primitives::sign_message(alloy_primitives::FixedBytes(sk.
// secret_bytes()), sign_hash)             .unwrap();
//
//     let our_sig = angstrom_types::primitive::Signature(signature);
//     baseline_order.signature = Bytes::from_iter(our_sig.to_bytes());
//     AllOrders::Partial(baseline_order)
// }
//
// pub fn generate_rand_valid_limit_order() -> AllOrders {
//     let mut rng = thread_rng();
//     let sk = SecretKey::new(&mut rng);
//     let mut baseline_order = generate_order(&mut rng);
//
//     let sign_hash = baseline_order.eip712_signing_hash(&ANGSTROM_DOMAIN);
//
//     let signature =
//         reth_primitives::sign_message(alloy_primitives::FixedBytes(sk.
// secret_bytes()), sign_hash)             .unwrap();
//
//     let our_sig = angstrom_types::primitive::Signature(signature);
//     baseline_order.signature = Bytes::from_iter(our_sig.to_bytes());
//
//     AllOrders::Partial(baseline_order)
// }
//
// pub fn generate_rand_valid_searcher_order() -> SignedSearcherOrder {
//     let l = generate_rand_valid_limit_order();
//     SignedSearcherOrder { hash: l.hash, order: l.order, signature:
// l.signature } }
//
// fn generate_order(rng: &mut ThreadRng) -> Order {
//     let timestamp = SystemTime::now()
//         .duration_since(UNIX_EPOCH)
//         .unwrap()
//         .as_secs()
//         + 30;
//     Order {
//         nonce:        U256::from(rng.gen_range(0..u64::MAX)),
//         orderType:    angstrom_types::primitive::OrderType::Limit,
//         currencyIn:   rng.gen(),
//         preHook:      Bytes::new(),
//         postHook:     Bytes::new(),
//         amountIn:     rng.gen(),
//         deadline:     U256::from(timestamp),
//         currencyOut:  rng.gen(),
//         amountOutMin: rng.gen()
//     }
// }

fn generate_kof_order(rng: &mut ThreadRng) -> OrderWithStorageData<GroupedVanillaOrder> {
    let order =
        FlashOrder { max_amount_in_or_out: rng.gen(), min_price: rng.gen(), ..Default::default() };
    // Todo: Sign It, make this overall better
    OrderWithStorageData {
        order:              GroupedVanillaOrder::KillOrFill(order),
        priority_data:      OrderPriorityData::default(),
        is_bid:             true,
        is_currently_valid: true,
        is_valid:           true,
        order_id:           OrderId::default(),
        pool_id:            0,
        valid_block:        0
    }
}
