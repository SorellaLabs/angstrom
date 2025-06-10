pub mod bundle;
pub mod common;
pub mod order;
pub mod validator;

use std::{
    fmt::Debug,
    pin::Pin,
    sync::{Arc, atomic::AtomicU64}
};

use alloy::primitives::Address;
use angstrom_types::{
    contract_payloads::angstrom::AngstromPoolConfigStore, pair_with_price::PairsWithPrice
};
use bundle::BundleValidator;
use common::SharedTools;
use futures::Stream;
use telemetry::client::TelemetryHandle;
use tokio::sync::mpsc::UnboundedReceiver;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;
use validator::Validator;

pub use crate::order::state::db_state_utils::finders::{
    find_slot_offset_for_approval, find_slot_offset_for_balance
};
use crate::{
    common::{TokenPriceGenerator, key_split_threadpool::KeySplitThreadpool},
    order::{
        order_validator::OrderValidator,
        sim::SimValidation,
        state::{db_state_utils::FetchUtils, pools::AngstromPoolsTracker}
    },
    validator::{ValidationClient, ValidationRequest}
};

const MAX_VALIDATION_PER_ADDR: usize = 1;

#[allow(clippy::too_many_arguments)]
pub fn init_validation<
    DB: Unpin + Clone + 'static + reth_provider::BlockNumReader + revm::DatabaseRef + Send + Sync,
    Telemetry: TelemetryHandle
>(
    db: DB,
    current_block: u64,
    angstrom_address: Address,
    node_address: Address,
    update_stream: Pin<Box<dyn Stream<Item = (u128, Vec<PairsWithPrice>)> + Send + 'static>>,
    uniswap_pools: SyncedUniswapPools,
    price_generator: TokenPriceGenerator,
    pool_store: Arc<AngstromPoolConfigStore>,
    validator_rx: UnboundedReceiver<ValidationRequest>,
    telemetry: Option<Telemetry>
) where
    <DB as revm::DatabaseRef>::Error: Send + Sync + Debug
{
    let current_block = Arc::new(AtomicU64::new(current_block));
    let revm_lru = Arc::new(db);
    let fetch = FetchUtils::new(angstrom_address, revm_lru.clone());

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(4)
            .build()
            .unwrap();

        let handle = rt.handle().clone();
        let pools = AngstromPoolsTracker::new(angstrom_address, pool_store);
        // load storage slot state + pools
        let thread_pool = KeySplitThreadpool::new(handle, MAX_VALIDATION_PER_ADDR);
        let sim = SimValidation::new(revm_lru.clone(), angstrom_address, node_address);

        let order_validator =
            rt.block_on(OrderValidator::new(sim, current_block, pools, fetch, uniswap_pools));

        let bundle_validator =
            BundleValidator::new(revm_lru.clone(), angstrom_address, node_address);
        let shared_utils = SharedTools::new(price_generator, update_stream, thread_pool);

        rt.block_on(async {
            Validator::new(validator_rx, order_validator, bundle_validator, shared_utils, telemetry)
                .await
        })
    });
}
