use std::{future::IntoFuture, marker::PhantomData};

use alloy::{
    network::{BlockResponse, Network},
    primitives::{Address, BlockNumber, StorageKey, StorageValue, keccak256},
    providers::Provider,
    transports::TransportResult
};
use alloy_rpc_types::BlockNumberOrTag;
use angstrom_types::reth_db_wrapper::DBError;
use op_alloy_network::primitives::HeaderResponse;
use reth_primitives::Account;
use reth_provider::{ProviderError, ProviderResult};
use revm::bytecode::Bytecode;
use validation::common::db::{BlockStateProvider, BlockStateProviderFactory};

use super::utils::async_to_sync;

#[derive(Clone, Debug)]
pub struct RpcStateProvider<N: Network, P: Provider<N>> {
    block:    u64,
    provider: P,
    _phantom: PhantomData<N>
}

impl<N: Network, P: Provider<N>> RpcStateProvider<N, P> {
    pub fn new(block: u64, provider: P) -> Self {
        Self { block, provider, _phantom: PhantomData }
    }

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

impl<N: Network, P: Provider<N>> BlockStateProvider for RpcStateProvider<N, P> {
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
pub struct RpcStateProviderFactory<N: Network, P: Provider<N>> {
    provider: P,
    _phantom: PhantomData<N>
}

impl<N: Network, P: Provider<N>> RpcStateProviderFactory<N, P> {
    pub fn new(provider: P) -> eyre::Result<Self> {
        Ok(Self { provider, _phantom: PhantomData })
    }
}

impl<N: Network, P: Provider<N>> reth_revm::DatabaseRef for RpcStateProviderFactory<N, P> {
    type Error = DBError;

    fn basic_ref(&self, address: Address) -> Result<Option<revm::state::AccountInfo>, Self::Error> {
        let acc = async_to_sync(self.provider.get_account(address).latest().into_future())?;
        let code = async_to_sync(self.provider.get_code_at(address).latest().into_future())?;
        let code = Some(Bytecode::new_raw(code));

        Ok(Some(revm::state::AccountInfo {
            code_hash: acc.code_hash,
            balance: acc.balance,
            nonce: acc.nonce,
            code
        }))
    }

    fn storage_ref(
        &self,
        address: Address,
        index: alloy::primitives::U256
    ) -> Result<alloy::primitives::U256, Self::Error> {
        let acc = async_to_sync(self.provider.get_storage_at(address, index).into_future())?;
        Ok(acc)
    }

    fn block_hash_ref(&self, number: u64) -> Result<alloy::primitives::B256, Self::Error> {
        let acc = async_to_sync(
            self.provider
                .get_block_by_number(BlockNumberOrTag::Number(number))
                .into_future()
        )?;

        let Some(block) = acc else { return Err(DBError::String("no block".to_string())) };
        Ok(block.header().hash())
    }

    fn code_by_hash_ref(&self, _: alloy::primitives::B256) -> Result<Bytecode, Self::Error> {
        panic!("This should not be called, as the code is already loaded");
    }
}

impl<N: Network, P: Provider<N> + Clone> BlockStateProviderFactory
    for RpcStateProviderFactory<N, P>
{
    type Provider = RpcStateProvider<N, P>;

    fn state_by_block(&self, block: u64) -> ProviderResult<Self::Provider> {
        Ok(RpcStateProvider { block, provider: self.provider.clone(), _phantom: PhantomData })
    }

    fn best_block_number(&self) -> ProviderResult<BlockNumber> {
        async_to_sync(self.provider.get_block_number())
            .map_err(|_| ProviderError::BestBlockNotFound)
    }
}
