use std::future::Future;

use alloy::{self, transports::BoxTransport};
use angstrom_types::sol_bindings::testnet::TestnetHub::TestnetHubInstance;

use crate::contracts::anvil::WalletProviderRpc;

pub const CACHE_VALIDATION_SIZE: usize = 100_000_000;

pub type StromContractInstance = TestnetHubInstance<BoxTransport, WalletProviderRpc>;

pub fn async_to_sync<F: Future>(f: F) -> F::Output {
    let handle = tokio::runtime::Handle::try_current().expect("No tokio runtime found");
    tokio::task::block_in_place(|| handle.block_on(f))
}
