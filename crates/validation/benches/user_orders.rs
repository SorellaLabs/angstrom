use std::{collections::HashMap, sync::Arc};

use alloy::primitives::{Address, U256};
use angstrom_metrics::validation::ValidationMetrics;
use angstrom_types::{
    contract_payloads::angstrom::UserOrder,
    orders::orderpool::OrderId,
    primitive::AngstromSigner,
    sol_bindings::{grouped_orders::AllOrders, ray::Ray}
};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use testing_tools::type_generator::orders::UserOrderBuilder;
use tokio::runtime::Runtime;
use validation::{
    common::{
        key_split_threadpool::KeySplitThreadpool, token_pricing::TokenPriceGenerator, SharedTools
    },
    order::{
        order_validator::OrderValidator, sim::SimValidation, state::{
            account::user::{UserAccounts, UserAddress},
            pools::{AngstromPoolsTracker, MockPoolTracker}
        }, OrderValidationRequest
    }
};

fn setup_mock_validator(rt: tokio::runtime::Handle) -> (OrderValidator, SharedTools, UserAccounts) {
    // Create mock components
    let pool_tracker = Arc::new(MockPoolTracker::default());
    let token_pricing = TokenPriceGenerator::default();
    let thread_pool = KeySplitThreadpool::new(rt, 10);

    let tools = SharedTools::new(token_pricing, Box::pin(futures::stream::empty()), thread_pool);

    let user_accounts = UserAccounts::new();
    let sim_validation = SimValidation::new(db, angstrom_address, node_address)
    let validator = OrderValidator::new(pool_tracker, Arc::new(user_accounts.clone()));

    (validator, tools, user_accounts)
}

fn generate_valid_order() -> AllOrders {
    UserOrderBuilder::new()
        .asset_in(Address::random())
        .asset_out(Address::random())
        .amount(1000u128)
        .min_price(Ray::from(U256::from(1u64)))
        .signing_key(Some(AngstromSigner::random()))
        .build()
        .into()
}

fn benchmark_validation_stages(c: &mut Criterion) {
    let mut group = c.benchmark_group("order_validation");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(10)
        .enable_all()
        .build()
        .unwrap();

    let (validator, tools, accounts) = setup_mock_validator(rt.handle().clone());

    // Benchmark valid order (baseline)
    let metrics = ValidationMetrics::new();
    group.bench_function("valid_order", |b| {
        let order = generate_valid_order();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let req = OrderValidationRequest::ValidateOrder(
            tx,
            order,
            angstrom_types::orders::OrderOrigin::External
        );
        b.to_async(&rt).iter(|| async {
            black_box(validator.validate_order(
                req,
                tools.token_pricing.clone(),
                &mut tools.thread_pool,
                metrics.clone()
            ));
            rx.await.unwrap()
        });
    });

    // Invalid signature
    group.bench_function("invalid_signature", |b| {
        let mut order = generate_valid_order();
        order.signature = Default::default(); // Invalid signature
        b.iter(|| {
            black_box(validator.validate_order(&order, &tools));
        });
    });

    // Insufficient balance
    group.bench_function("insufficient_balance", |b| {
        let order = generate_valid_order();
        let user = order.signature.recover_signer();
        accounts.set_balance_for_user(user, order.asset_in, U256::ZERO);
        b.to_async().iter(|| async {
            black_box(validator.validate_order(&order, &tools));
        });
    });

    // Invalid approval
    group.bench_function("invalid_approval", |b| {
        let order = generate_valid_order();
        let user = order.signature.recover_signer();
        accounts.set_approval_for_user(user, order.asset_in, U256::ZERO);
        b.iter(|| {
            black_box(validator.validate_order(&order, &tools));
        });
    });

    // Invalid nonce
    group.bench_function("invalid_nonce", |b| {
        let mut order = generate_valid_order();
        order.standing_validation = Some(Default::default()); // Invalid nonce
        b.iter(|| {
            black_box(validator.validate_order(&order, &tools));
        });
    });

    // Invalid pool
    group.bench_function("invalid_pool", |b| {
        let order = generate_valid_order();
        // Don't register the pool in MockPoolTracker
        b.iter(|| {
            black_box(validator.validate_order(&order, &tools));
        });
    });

    group.finish();
}

criterion_group!(benches, benchmark_validation_stages);
criterion_main!(benches);
