use std::sync::Arc;

use alloy_primitives::keccak256;
use alloy_provider::{Provider, ProviderBuilder, ReqwestProvider};
use alloy_transport::TransportResult;
use futures::Future;
use reth_interfaces::provider::{ProviderError, ProviderResult};
use reth_primitives::{Account, Address, BlockNumber, StorageKey, StorageValue};
use tokio;
use validation::common::lru_db::{BlockStateProvider, BlockStateProviderFactory};

fn async_to_sync<O, F: Future<Output = O>>(f: F) -> O {
    let handle = tokio::runtime::Handle::try_current().expect("No tokion runtime found");
    tokio::task::block_in_place(|| handle.block_on(f))
}

#[derive(Clone, Debug)]
pub struct RpcStateProvider {
    block:    u64,
    provider: Arc<ReqwestProvider>
}

impl RpcStateProvider {
    async fn get_account(&self, address: Address) -> TransportResult<Account> {
        let block_id = self.block.into();
        let nonce = self
            .provider
            .get_transaction_count(address, Some(block_id))
            .await?;
        let balance = self.provider.get_balance(address, Some(block_id)).await?;
        // TODO: Ensure correct handling of EOA empty accounts.
        let bytecode = self.provider.get_code_at(address, block_id).await?;

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
                .get_storage_at(address, key.into(), Some(self.block.into()))
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
    provider: Arc<ReqwestProvider>
}

impl RpcStateProviderFactory {
    pub fn new(raw_rpc_url: &str) -> eyre::Result<Self> {
        let rpc_url = raw_rpc_url.parse()?;
        let provider = ProviderBuilder::new().on_http(rpc_url)?;
        Ok(Self { provider: Arc::new(provider) })
    }
}

impl BlockStateProviderFactory for RpcStateProviderFactory {
    type Provider = RpcStateProvider;

    fn state_by_block(&self, block: u64) -> ProviderResult<Self::Provider> {
        Ok(RpcStateProvider { block, provider: self.provider.clone() })
    }

    fn best_block_number(&self) -> ProviderResult<BlockNumber> {
        async_to_sync(self.provider.get_block_number())
            .map_err(|_| ProviderError::BestBlockNotFound)
    }
}
