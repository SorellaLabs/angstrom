use alloy_primitives::{Address, U256};
use angstrom_testing::{
    order_generator::OrderGenerator,
    providers::WalletProvider,
    type_generator::{
        amm::AMMSnapshotBuilder,
        consensus::pool::{Pool, PoolBuilder},
        orders::StoredOrderBuilder
    },
    validation::TestOrderValidator
};
use angstrom_types::{
    matching::sqrtprice::SqrtPriceX96, orders::orderpool::OrderId,
    primitive::signer::AngstromSigner, sol_bindings::ext::grouped_orders::OrderWithStorageData
};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use eyre::Result;

async fn setup_validator() -> Result<TestOrderValidator> {
    // Setup basic test environment
    let provider = WalletProvider::new_test()?;
    let validator = TestOrderValidator::new_test(provider.provider(), 0).await?;
    Ok(validator)
}

fn generate_test_pool() -> Pool {
    // Create a test pool with some liquidity
    let snapshot = AMMSnapshotBuilder::new(SqrtPriceX96::from_float_price(1000.0))
        .with_single_position(100, 1_000_000)
        .build();

    PoolBuilder::new()
        .with_key(Default::default())
        .snapshot(snapshot)
        .tob(Address::random())
        .build()
}

fn generate_valid_tob_order(pool: &Pool) -> OrderWithStorageData {
    let mut gen = OrderGenerator::new_test(pool.clone());
    gen.generate_tob_order(true, None)
}

fn generate_invalid_signature_order(pool: &Pool) -> OrderWithStorageData {
    let mut order = generate_valid_tob_order(pool);
    // Corrupt signature
    order.order.signature = Default::default();
    order
}

fn generate_insufficient_balance_order(pool: &Pool) -> OrderWithStorageData {
    let mut order = generate_valid_tob_order(pool);
    // Set unrealistic quantity
    order.order.quantity_in = U256::MAX.to_raw();
    order
}

fn criterion_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let validator = rt.block_on(setup_validator()).unwrap();
    let pool = generate_test_pool();

    let mut group = c.benchmark_group("tob_order_validation");

    // Benchmark valid order validation
    group.bench_function("valid_tob_order", |b| {
        let order = generate_valid_tob_order(&pool);
        b.iter(|| rt.block_on(async { black_box(validator.validate_tob_order(&order).await) }));
    });

    // Benchmark invalid signature validation
    group.bench_function("invalid_signature_tob_order", |b| {
        let order = generate_invalid_signature_order(&pool);
        b.iter(|| rt.block_on(async { black_box(validator.validate_tob_order(&order).await) }));
    });

    // Benchmark insufficient balance validation
    group.bench_function("insufficient_balance_tob_order", |b| {
        let order = generate_insufficient_balance_order(&pool);
        b.iter(|| rt.block_on(async { black_box(validator.validate_tob_order(&order).await) }));
    });

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
