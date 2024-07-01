use std::future::IntoFuture;

use alloy::transports::http::{Client, Http};
use alloy_primitives::keccak256;
use alloy_provider::{Provider, RootProvider};
use alloy_transport::TransportResult;
use futures::Future;
use reth_primitives::{Account, Address, BlockNumber, StorageKey, StorageValue};
use reth_provider::{ProviderError, ProviderResult};
use validation::common::lru_db::{BlockStateProvider, BlockStateProviderFactory};

fn async_to_sync<F: Future>(f: F) -> F::Output {
    let handle = tokio::runtime::Handle::try_current().expect("No tokion runtime found");
    tokio::task::block_in_place(|| handle.block_on(f))
}

#[derive(Clone, Debug)]
pub struct RpcStateProvider {
    block:    u64,
    provider: RootProvider<Http<Client>>
}

impl RpcStateProvider {
    async fn get_account(&self, address: Address) -> TransportResult<Account> {
        let (nonce, balance, bytecode) = futures::try_join!(
            self.provider.get_transaction_count(address).into_future(),
            self.provider.get_balance(address).into_future(),
            // TODO: Ensure correct handling of EOA empty accounts.
            self.provider.get_code_at(address).into_future()
        )?;

        let hash = keccak256(bytecode);

        Ok(Account { nonce, balance, bytecode_hash: Some(hash) })
    }
}

impl BlockStateProvider for RpcStateProvider {
    fn get_storage(
        &self,
        address: Address,
        key: StorageKey
    ) -> ProviderResult<Option<StorageValue>> {
        async_to_sync(
            self.provider
                .get_storage_at(address, key.into())
                .into_future()
        )
        .map(Some)
        // TODO: Better error.
        .map_err(|_| ProviderError::StorageChangesetNotFound {
            block_number: self.block,
            address,
            storage_key: Box::new(key)
        })
    }

    fn get_basic_account(&self, address: Address) -> ProviderResult<Option<Account>> {
        async_to_sync(self.get_account(address))
            .map(Some)
            .map_err(|_| ProviderError::AccountChangesetNotFound {
                block_number: self.block,
                address
            })
    }
}

#[derive(Clone, Debug)]
pub struct RpcStateProviderFactory {
    provider: RootProvider<Http<Client>>
}

impl RpcStateProviderFactory {
    pub fn new(provider: RootProvider<Http<Client>>) -> eyre::Result<Self> {
        Ok(Self { provider })
    }
}

impl BlockStateProviderFactory for RpcStateProviderFactory {
    type Provider = RpcStateProvider;

    fn state_by_block(&self, block: u64) -> ProviderResult<Self::Provider> {
        Ok(RpcStateProvider { block, provider: self.provider.clone() })
    }

    fn best_block_number(&self) -> ProviderResult<BlockNumber> {
        Ok(async_to_sync(self.provider.get_block_number())
            .map_err(|_| ProviderError::BestBlockNotFound)
            .unwrap_or_default())
    }
}
