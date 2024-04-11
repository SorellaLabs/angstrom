#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]
#![allow(unreachable_code)]
pub mod bundle;
pub mod common;
pub mod order;
pub mod validator;

use std::pin::Pin;

use angstrom_eth::manager::EthEvent;
use futures::Stream;
use reth_provider::StateProviderFactory;
use tokio::sync::mpsc::unbounded_channel;

use crate::validator::ValidationClient;

const TOKEN_CONFIG_FILE: &str = "./crates/validation/token_config.toml";

pub fn init_validation<DB: StateProviderFactory + Unpin + 'static>(
    db: DB,
    block_stream: Pin<Box<dyn Stream<Item = EthEvent> + Send>>
) -> ValidationClient {
    let (tx, rx) = unbounded_channel();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(3)
            .build()
            .unwrap();
    });

    ValidationClient(tx)
}
