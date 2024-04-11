pub mod error;
pub mod executor;
pub mod lru_db;
pub mod pool_map;
pub mod revm;
pub mod state;

use reth_provider::StateProviderFactory;
use tokio::sync::mpsc::unbounded_channel;

use crate::{
    bundle::errors::SimError,
    common::{lru_db::RevmLRU, revm::Revm}
};
// pub struct ValidatorSimConfig<DB: StateProviderFactory + Clone + Send + Sync
// + Unpin + 'static> {     pub db:          DB, pub cache_bytes: usize
// }
//
// pub fn spawn_revm_sim<DB: StateProviderFactory + Clone + Send + Sync + Unpin
// + 'static>(     config: ValidatorSimConfig<DB>
// ) -> Result<RevmClient, SimError> {
//     let (tx, rx) = unbounded_channel();
//     std::thread::spawn(move || {
//         let lru = RevmLRU::new(config.cache_bytes, config.db.into());
//         let revm_client = Revm::new(rx, lru).unwrap();
//         let handle = revm_client.get_threadpool_handle();
//
//         handle.block_on(revm_client);
//     });
//
//     Ok(RevmClient::new(tx))
// }
