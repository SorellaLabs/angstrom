use std::{
    collections::HashMap,
    sync::{Arc, RwLock}
};

use alloy::primitives::U256;
use alloy_primitives::Address;
use angstrom_types::{
    orders::{OrderId, OrderPriorityData},
    primitive::PoolId,
    sol_bindings::{
        ext::{RawPoolOrder, grouped_orders::OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use matching_engine::{
    book::{BookOrder, OrderBook, sort::SortStrategy},
    strategy::BinarySearchStrategy
};
use testing_tools::order_generator::{GeneratedPoolOrders, PoolOrderGenerator};
use uniswap_v3_math::{tick_bitmap::flip_tick, tick_math::get_sqrt_ratio_at_tick};
use uniswap_v4::uniswap::{
    pool::{EnhancedUniswapPool, TickInfo},
    pool_data_loader::DataLoader,
    pool_manager::SyncedUniswapPool
};

const TPS_BUCKETS: [u64; 9] = [20, 40, 100, 200, 500, 1000, 5000, 25_000, 100_000];

async fn setup_synced_pool_for_order_generation() -> SyncedUniswapPool {
    let mut token_0 = Address::random();
    let mut token_1 = Address::random();
    if token_0 > token_1 {
        std::mem::swap(&mut token_0, &mut token_1);
    }

    let tick_map = vec![
        (
            99_700,
            TickInfo {
                liquidity_net:   100_000_000,
                liquidity_gross: 300_000_000,
                is_edge:         false,
                initialized:     true,
                zfo:             true
            }
        ),
        (
            99_800,
            TickInfo {
                liquidity_net:   100_000_000,
                liquidity_gross: 400_000_000,
                is_edge:         false,
                initialized:     true,
                zfo:             true
            }
        ),
        (
            99_900,
            TickInfo {
                liquidity_net:   100_000_000,
                liquidity_gross: 500_000_000,
                is_edge:         false,
                initialized:     true,
                zfo:             true
            }
        ),
        (
            100_000,
            TickInfo {
                liquidity_net:   100_000_000,
                liquidity_gross: 600_000_000,
                is_edge:         false,
                initialized:     true,
                zfo:             true
            }
        ),
        (
            100_100,
            TickInfo {
                liquidity_net:   100_000_000,
                liquidity_gross: 700_000_000,
                is_edge:         false,
                initialized:     true,
                zfo:             true
            }
        ),
        (
            100_200,
            TickInfo {
                liquidity_net:   100_000_000,
                liquidity_gross: 800_000_000,
                is_edge:         false,
                initialized:     true,
                zfo:             true
            }
        ),
        (
            100_300,
            TickInfo {
                liquidity_net:   100_000_000,
                liquidity_gross: 900_000_000,
                is_edge:         false,
                initialized:     true,
                zfo:             true
            }
        ),
    ]
    .into_iter()
    .collect::<HashMap<_, _>>();

    let tick_spacing = 100;

    let mut bitmap = HashMap::new();
    for tick in [99700, 99800, 99900, 100000, 100100, 100200, 100300] {
        flip_tick(&mut bitmap, tick, tick_spacing).unwrap();
    }

    let current_tick = 100_000;
    let sqrt_price = get_sqrt_ratio_at_tick(current_tick).unwrap();
    let liquidity = 600_000_000;

    let mut pool = EnhancedUniswapPool::new(DataLoader::default(), 0);

    // tokens
    pool.token0 = token_0;
    pool.token0_decimals = 18;
    pool.token1 = token_1;
    pool.token1_decimals = 18;
    pool.liquidity = liquidity;
    pool.sqrt_price = sqrt_price;
    pool.tick = current_tick;
    pool.tick_spacing = tick_spacing;
    pool.ticks = tick_map;
    pool.tick_bitmap = bitmap;

    Arc::new(RwLock::new(pool))
}

// messy
fn setup_inputs(
    pools: GeneratedPoolOrders,
    amm: &SyncedUniswapPool
) -> (OrderBook, OrderWithStorageData<TopOfBlockOrder>) {
    let GeneratedPoolOrders { pool_id, tob, book } = pools;
    let (_, _, amm) = amm.read().unwrap().fetch_pool_snapshot().unwrap();
    let asks = book
        .iter()
        .filter(|f| !f.is_bid())
        .map(|ask| OrderWithStorageData {
            invalidates: vec![],
            order: ask.clone(),
            priority_data: OrderPriorityData {
                price:     *ask.price(),
                volume:    ask.amount(),
                gas:       U256::ZERO,
                gas_units: 0
            },
            is_bid: false,
            is_valid: true,
            is_currently_valid: None,
            order_id: OrderId {
                flash_block: None,
                reuse_avoidance: angstrom_types::sol_bindings::RespendAvoidanceMethod::Block(0),
                hash: ask.order_hash(),
                address: Default::default(),
                deadline: None,
                pool_id,
                location: angstrom_types::orders::OrderLocation::Limit
            },
            pool_id,
            valid_block: 0,
            tob_reward: U256::ZERO
        })
        .collect::<Vec<BookOrder>>();
    let bids = book
        .iter()
        .filter(|f| f.is_bid())
        .map(|bid| OrderWithStorageData {
            invalidates: vec![],
            order: bid.clone(),
            priority_data: OrderPriorityData {
                price:     *bid.price(),
                volume:    bid.amount(),
                gas:       U256::ZERO,
                gas_units: 0
            },
            is_bid: true,
            is_valid: true,
            is_currently_valid: None,
            order_id: OrderId {
                flash_block: None,
                reuse_avoidance: angstrom_types::sol_bindings::RespendAvoidanceMethod::Block(0),
                hash: bid.order_hash(),
                address: Default::default(),
                deadline: None,
                pool_id,
                location: angstrom_types::orders::OrderLocation::Limit
            },
            pool_id,
            valid_block: 0,
            tob_reward: U256::ZERO
        })
        .collect::<Vec<BookOrder>>();

    let tob = OrderWithStorageData {
        invalidates: vec![],
        order: tob.clone(),
        priority_data: OrderPriorityData {
            price:     tob.limit_price(),
            volume:    tob.amount(),
            gas:       U256::ZERO,
            gas_units: 0
        },
        is_bid: tob.is_bid(),
        is_valid: true,
        is_currently_valid: None,
        order_id: OrderId {
            flash_block: None,
            reuse_avoidance: angstrom_types::sol_bindings::RespendAvoidanceMethod::Block(0),
            hash: tob.order_hash(),
            address: Default::default(),
            deadline: None,
            pool_id,
            location: angstrom_types::orders::OrderLocation::Limit
        },
        pool_id,
        valid_block: 0,
        tob_reward: U256::ZERO
    };

    let book = OrderBook::new(pool_id, Some(amm), bids, asks, Some(SortStrategy::ByPriceByVolume));

    (book, tob)
}

pub fn tps(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let pool = rt.block_on(setup_synced_pool_for_order_generation());

    let mut group = c.benchmark_group("Matching Engine TPS");
    let mut generator =
        PoolOrderGenerator::new_with_cfg_distro(PoolId::default(), pool.clone(), 0, 0.2);
    for bucket in TPS_BUCKETS {
        group.throughput(criterion::Throughput::Elements(bucket));
        // updates the random prices
        generator.new_block(1);
        let set = generator.generate_set(bucket as usize, 0.5);
        let (book, tob) = setup_inputs(set, &pool);

        group.bench_function(BenchmarkId::from_parameter(bucket), |bench| {
            bench.iter(|| black_box(BinarySearchStrategy::run(&book, Some(tob.clone()))));
        });
    }
    group.finish();
}

criterion_group!(benches, tps);
criterion_main!(benches);
