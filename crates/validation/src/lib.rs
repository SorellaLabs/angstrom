#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]
#![allow(unreachable_code)]
pub mod bundle;
pub mod common;
pub mod order;
pub mod validator;

use std::{path::Path, pin::Pin, sync::Arc};

use angstrom_eth::manager::EthEvent;
use common::lru_db::RevmLRU;
use futures::Stream;
use order::state::config::load_validation_config;
use reth_provider::StateProviderFactory;
use tokio::sync::mpsc::unbounded_channel;
use validator::Validator;

use crate::validator::ValidationClient;

const TOKEN_CONFIG_FILE: &str = "./crates/validation/state_config.toml";

pub fn init_validation<DB: StateProviderFactory + Unpin + Clone + 'static>(
    db: DB,
    cache_max_bytes: usize,
    block_stream: Pin<Box<dyn Stream<Item = EthEvent> + Send>>
) -> ValidationClient {
    let (tx, rx) = unbounded_channel();
    let config_path = Path::new(TOKEN_CONFIG_FILE);
    let config = load_validation_config(config_path).unwrap();
    let revm_lru = Arc::new(RevmLRU::new(cache_max_bytes, Arc::new(db)));

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(4)
            .build()
            .unwrap();

        rt.block_on(Validator::new(rx, block_stream, revm_lru, config))
    });

    ValidationClient(tx)
}
