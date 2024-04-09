use std::time::{SystemTime, UNIX_EPOCH};

use alloy_sol_types::SolStruct;
use angstrom_types::{
    orders::PooledOrder,
    primitive::{Order, ANGSTROM_DOMAIN}
};
use rand::{rngs::ThreadRng, thread_rng, Rng};
use reth_primitives::{Bytes, U256};
use secp256k1::SecretKey;

pub fn generate_valid_order() -> PooledOrder {
    let mut rng = thread_rng();
    let sk = SecretKey::new(&mut rng);
    let baseline_order = generate_order(&mut rng);

    let order_hash = baseline_order.eip712_hash_struct();
    let sign_hash = baseline_order.eip712_signing_hash(&ANGSTROM_DOMAIN);

    let signature =
        reth_primitives::sign_message(alloy_primitives::FixedBytes(sk.secret_bytes()), sign_hash)
            .unwrap();

    let our_sig = angstrom_types::primitive::Signature(signature);

    PooledOrder::Limit(angstrom_types::rpc::SignedLimitOrder {
        hash:      order_hash,
        order:     baseline_order,
        signature: our_sig
    })
}

fn generate_order(rng: &mut ThreadRng) -> Order {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 30;

    Order {
        nonce:        rng.gen(),
        orderType:    angstrom_types::primitive::OrderType::User,
        currencyIn:   rng.gen(),
        preHook:      Bytes::new(),
        postHook:     Bytes::new(),
        amountIn:     rng.gen(),
        deadline:     U256::from(timestamp),
        currencyOut:  rng.gen(),
        amountOutMin: rng.gen()
    }
}
