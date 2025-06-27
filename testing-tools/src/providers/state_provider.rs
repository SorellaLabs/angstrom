use std::{future::IntoFuture, time::Duration};

use alloy::{providers::Provider, rpc::types::Block};
use alloy_primitives::{Address, B256, BlockNumber, U256};
use alloy_rpc_types::{BlockId, TransactionReceipt};
use angstrom_types::reth_db_wrapper::DBError;
use futures::stream::StreamExt;
use reth_primitives::EthPrimitives;
use reth_provider::{
    BlockHashReader, BlockNumReader, CanonStateNotification, CanonStateNotifications,
    CanonStateSubscriptions, NodePrimitivesProvider, ProviderError, ProviderResult
};
use revm::{bytecode::Bytecode, state::AccountInfo};
use tokio::sync::broadcast;
use validation::common::db::BlockStateProviderFactory;

use super::{RpcStateProvider, WalletProvider};
use crate::{
    mocks::canon_state::AnvilConsensusCanonStateNotification, providers::utils::async_to_sync,
    types::WithWalletProvider
};

#[derive(Debug, Clone)]
pub struct AnvilStateProvider<P> {
    provider:           P,
    canon_state:        AnvilConsensusCanonStateNotification,
    pub canon_state_tx: broadcast::Sender<CanonStateNotification>
}

impl<P: WithWalletProvider> AnvilStateProvider<P> {
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            canon_state: AnvilConsensusCanonStateNotification::new(),
            canon_state_tx: broadcast::channel(1000).0
        }
    }

    pub fn current_chain_block(&self) -> u64 {
        self.canon_state.current_block()
    }

    pub(crate) fn update_canon_chain(
        &self,
        new_block: &Block,
        receipts: Vec<TransactionReceipt>
    ) -> eyre::Result<()> {
        let state = self.canon_state.new_block(new_block, receipts);
        if self.canon_state_tx.receiver_count() == 0 {
            tracing::warn!("no canon state rx")
        } else {
            let _ = self
                .canon_state_tx
                .send(CanonStateNotification::Commit { new: state })?;
        }

        Ok(())
    }

    pub(crate) fn provider(&self) -> &P {
        &self.provider
    }

    pub fn provider_mut(&mut self) -> &mut P {
        &mut self.provider
    }

    pub fn as_wallet_state_provider(&self) -> AnvilStateProvider<WalletProvider> {
        AnvilStateProvider {
            provider:       self.provider.wallet_provider(),
            canon_state:    self.canon_state.clone(),
            canon_state_tx: self.canon_state_tx.clone()
        }
    }

    /// used for testnet to make sure cannon notifications work
    pub async fn listen_to_new_blocks(self) {
        let mut new_blocks = self
            .provider()
            .rpc_provider()
            .watch_blocks()
            .await
            .unwrap()
            .with_poll_interval(Duration::from_millis(100))
            .into_stream();

        while let Some(block_hash) = new_blocks.next().await {
            if let Some(block_hash) = block_hash.first() {
                tracing::info!("got new blockhash");
                let block = self
                    .provider()
                    .rpc_provider()
                    .get_block(BlockId::Hash(alloy_rpc_types::RpcBlockHash {
                        block_hash:        *block_hash,
                        require_canonical: None
                    }))
                    .full()
                    .await
                    .unwrap()
                    .unwrap();

                let recipts = self
                    .provider()
                    .rpc_provider()
                    .get_block_receipts(BlockId::Hash(alloy_rpc_types::RpcBlockHash {
                        block_hash:        *block_hash,
                        require_canonical: None
                    }))
                    .await
                    .unwrap()
                    .unwrap();
                tracing::info!("updating cannon chain");

                self.update_canon_chain(&block, recipts).unwrap();
            }
        }
    }
}

impl<P: WithWalletProvider> reth_revm::DatabaseRef for AnvilStateProvider<P> {
    type Error = DBError;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let acc = async_to_sync(
            self.provider
                .rpc_provider()
                .get_account(address)
                .latest()
                .into_future()
        )?;
        let code = async_to_sync(
            self.provider
                .rpc_provider()
                .get_code_at(address)
                .latest()
                .into_future()
        )?;
        let code = Some(Bytecode::new_raw(code));

        Ok(Some(AccountInfo {
            code_hash: acc.code_hash,
            balance: acc.balance,
            nonce: acc.nonce,
            code
        }))
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let acc = async_to_sync(
            self.provider
                .rpc_provider()
                .get_storage_at(address, index)
                .into_future()
        )?;
        Ok(acc)
    }

    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        let acc = async_to_sync(
            self.provider
                .rpc_provider()
                .get_block_by_number(alloy_rpc_types::BlockNumberOrTag::Number(number))
                .into_future()
        )?;

        let Some(block) = acc else { return Err(DBError::String("no block".to_string())) };
        Ok(block.header.hash)
    }

    fn code_by_hash_ref(&self, _: B256) -> Result<Bytecode, Self::Error> {
        panic!("This should not be called, as the code is already loaded");
    }
}
impl<P: WithWalletProvider> BlockNumReader for AnvilStateProvider<P> {
    fn chain_info(&self) -> ProviderResult<reth_chainspec::ChainInfo> {
        panic!("never used");
    }

    fn block_number(&self, _: alloy_primitives::B256) -> ProviderResult<Option<BlockNumber>> {
        panic!("never used");
    }

    fn convert_number(
        &self,
        _: alloy_rpc_types::BlockHashOrNumber
    ) -> ProviderResult<Option<alloy_primitives::B256>> {
        panic!("never used");
    }

    fn best_block_number(&self) -> ProviderResult<BlockNumber> {
        Ok(async_to_sync(
            self.provider
                .rpc_provider()
                .get_block_number()
                .into_future()
        )
        .unwrap())
    }

    fn last_block_number(&self) -> ProviderResult<BlockNumber> {
        Ok(async_to_sync(
            self.provider
                .rpc_provider()
                .get_block_number()
                .into_future()
        )
        .unwrap())
    }

    fn convert_hash_or_number(
        &self,
        _: alloy_rpc_types::BlockHashOrNumber
    ) -> ProviderResult<Option<BlockNumber>> {
        panic!("never used");
    }
}
impl<P: WithWalletProvider> BlockHashReader for AnvilStateProvider<P> {
    fn block_hash(&self, _: BlockNumber) -> ProviderResult<Option<alloy_primitives::B256>> {
        panic!("never used");
    }

    fn convert_block_hash(
        &self,
        _: alloy_rpc_types::BlockHashOrNumber
    ) -> ProviderResult<Option<alloy_primitives::B256>> {
        panic!("never used");
    }

    fn canonical_hashes_range(
        &self,
        _: BlockNumber,
        _: BlockNumber
    ) -> ProviderResult<Vec<alloy_primitives::B256>> {
        panic!("never used");
    }
}

impl<P: WithWalletProvider> BlockStateProviderFactory for AnvilStateProvider<P> {
    type Provider = RpcStateProvider;

    fn state_by_block(&self, block: u64) -> ProviderResult<Self::Provider> {
        Ok(RpcStateProvider::new(block, self.provider.rpc_provider()))
    }

    fn best_block_number(&self) -> ProviderResult<BlockNumber> {
        async_to_sync(self.provider.rpc_provider().get_block_number())
            .map_err(|_| ProviderError::BestBlockNotFound)
    }
}

impl<P: WithWalletProvider> CanonStateSubscriptions for AnvilStateProvider<P> {
    fn subscribe_to_canonical_state(&self) -> CanonStateNotifications<Self::Primitives> {
        self.canon_state_tx.subscribe()
    }
}

impl<P: WithWalletProvider> NodePrimitivesProvider for AnvilStateProvider<P> {
    type Primitives = EthPrimitives;
}
