use std::time::{SystemTime, UNIX_EPOCH};

use alloy_sol_types::SolStruct;
use angstrom_types::{
    orders::PooledOrder,
    primitive::{Order, ANGSTROM_DOMAIN},
    rpc::SignedLimitOrder,
    sol_bindings::{grouped_orders::AllOrders, sol::StandingOrder}
};
use rand::{rngs::ThreadRng, thread_rng, Rng};
use reth_primitives::{Bytes, U256};
use secp256k1::SecretKey;

pub fn generate_random_valid_order() -> AllOrders {
    let mut rng = thread_rng();
    let sk = SecretKey::new(&mut rng);
    let mut baseline_order = generate_order(&mut rng);

    let sign_hash = baseline_order.eip712_signing_hash(&ANGSTROM_DOMAIN);

    let signature =
        reth_primitives::sign_message(alloy_primitives::FixedBytes(sk.secret_bytes()), sign_hash)
            .unwrap();

    let our_sig = angstrom_types::primitive::Signature(signature);
    baseline_order.signature = Bytes::from_iter(our_sig.to_bytes());
    AllOrders::Partial(baseline_order)
}

pub fn generate_rand_valid_limit_order() -> AllOrders {
    let mut rng = thread_rng();
    let sk = SecretKey::new(&mut rng);
    let mut baseline_order = generate_order(&mut rng);

    let sign_hash = baseline_order.eip712_signing_hash(&ANGSTROM_DOMAIN);

    let signature =
        reth_primitives::sign_message(alloy_primitives::FixedBytes(sk.secret_bytes()), sign_hash)
            .unwrap();

    let our_sig = angstrom_types::primitive::Signature(signature);
    baseline_order.signature = Bytes::from_iter(our_sig.to_bytes());

    AllOrders::Partial(baseline_order)
}

fn generate_order(rng: &mut ThreadRng) -> StandingOrder {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 30;

    StandingOrder {
        nonce: rng.gen_range(0..u64::MAX),
        asset_in: rng.gen(),
        deadline: U256::from(timestamp),
        ..Default::default()
    }
}
