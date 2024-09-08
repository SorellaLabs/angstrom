#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]
#![allow(unreachable_code)]

pub mod common;
pub mod order;
pub mod validator;

use std::{
    path::Path,
    pin::Pin,
    sync::{atomic::AtomicU64, Arc}
};

use angstrom_eth::manager::EthEvent;
use common::lru_db::{BlockStateProviderFactory, RevmLRU};
use futures::Stream;
use order::state::{
    config::load_validation_config,
    db_state_utils::{FetchUtils, StateFetchUtils},
    pools::{AngstromPoolsTracker, PoolsTracker}
};
use reth_provider::StateProviderFactory;
use tokio::sync::mpsc::unbounded_channel;
use validator::Validator;

use crate::validator::ValidationClient;

pub const TOKEN_CONFIG_FILE: &str = "./crates/validation/state_config.toml";

pub fn init_validation<DB: BlockStateProviderFactory + Unpin + Clone + 'static>(
    db: DB,
    cache_max_bytes: usize
) -> ValidationClient {
    let (tx, rx) = unbounded_channel();
    let config_path = Path::new(TOKEN_CONFIG_FILE);
    let config = load_validation_config(config_path).unwrap();
    let current_block = Arc::new(AtomicU64::new(db.best_block_number().unwrap()));
    let revm_lru = Arc::new(RevmLRU::new(cache_max_bytes, Arc::new(db), current_block.clone()));

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(4)
            .build()
            .unwrap();
        let handle = rt.handle().clone();

        // load storage slot state + pools
        let fetch = FetchUtils::new(config.clone());
        let pools = AngstromPoolsTracker::new(config.clone());

        rt.block_on(async {
            Validator::new(
                rx,
                revm_lru,
                current_block.clone(),
                config.max_validation_per_user,
                pools,
                fetch,
                handle
            )
            .await
        })
    });

    ValidationClient(tx)
}

pub fn init_validation_tests<
    DB: BlockStateProviderFactory + Unpin + Clone + 'static,
    State: StateFetchUtils + Sync + 'static,
    Pool: PoolsTracker + Sync + 'static
>(
    db: DB,
    cache_max_bytes: usize,
    state: State,
    pool: Pool
) -> (ValidationClient, Arc<RevmLRU<DB>>) {
    let (tx, rx) = unbounded_channel();
    let config_path = Path::new(TOKEN_CONFIG_FILE);
    let config = load_validation_config(config_path).unwrap();
    let current_block = Arc::new(AtomicU64::new(db.best_block_number().unwrap()));
    let revm_lru = Arc::new(RevmLRU::new(cache_max_bytes, Arc::new(db), current_block.clone()));

    let task_db = revm_lru.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(4)
            .build()
            .unwrap();
        let handle = rt.handle().clone();

        rt.block_on(Validator::new(rx, task_db, current_block.clone(), 10, pool, state, handle))
    });

    (ValidationClient(tx), revm_lru)
}

pub trait BundleValidator: Send + Sync + Clone + Unpin + 'static {}

impl BundleValidator for ValidationClient {}
