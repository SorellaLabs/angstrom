use std::future::IntoFuture;

use alloy::{
    primitives::{keccak256, Address, BlockNumber, StorageKey, StorageValue},
    providers::Provider,
    transports::TransportResult
};
use alloy_rpc_types::{BlockNumberOrTag, BlockTransactionsKind};
use eyre::bail;
use reth_primitives::Account;
use reth_provider::{ProviderError, ProviderResult};
use reth_revm::primitives::Bytecode;
use validation::common::db::{BlockStateProvider, BlockStateProviderFactory};

use super::utils::async_to_sync;
use crate::contracts::anvil::WalletProviderRpc;

#[derive(Clone, Debug)]
pub struct RpcStateProvider {
    block:    u64,
    provider: WalletProviderRpc
}

impl RpcStateProvider {
    pub fn new(block: u64, provider: WalletProviderRpc) -> Self {
        Self { block, provider }
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
    pub provider: WalletProviderRpc
}

impl RpcStateProviderFactory {
    pub fn new(provider: WalletProviderRpc) -> eyre::Result<Self> {
        Ok(Self { provider })
    }
}

impl reth_revm::DatabaseRef for RpcStateProviderFactory {
    type Error = eyre::Error;

    fn basic_ref(
        &self,
        address: Address
    ) -> Result<Option<reth_revm::primitives::AccountInfo>, Self::Error> {
        let acc = async_to_sync(self.provider.get_account(address).latest().into_future())?;
        let code = async_to_sync(self.provider.get_code_at(address).latest().into_future())?;
        let code = Some(Bytecode::new_raw(code));

        Ok(Some(reth_revm::primitives::AccountInfo {
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
                .get_block_by_number(
                    BlockNumberOrTag::Number(number),
                    BlockTransactionsKind::Hashes
                )
                .into_future()
        )?;

        let Some(block) = acc else { bail!("failed to load block") };
        Ok(block.header.hash)
    }

    fn code_by_hash_ref(
        &self,
        _: alloy::primitives::B256
    ) -> Result<reth_revm::primitives::Bytecode, Self::Error> {
        panic!("This should not be called, as the code is already loaded");
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
