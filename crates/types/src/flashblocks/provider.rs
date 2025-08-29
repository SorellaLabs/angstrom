//! Provider for pending Flashblocks state.
use alloy::{
    providers::{Provider, ProviderCall, RootProvider, RpcWithBlock},
    rpc::client::NoParams
};
use alloy_primitives::{Address, Bytes, StorageValue, U64, U256};

use crate::flashblocks::FlashblocksRx;

pub struct PendingStateProvider<P: Provider> {
    provider: P,
    pending:  FlashblocksRx
}

impl<P> PendingStateProvider<P> where P: Provider {}

impl<P> Provider for PendingStateProvider<P>
where
    P: Provider + Clone + 'static
{
    fn root(&self) -> &RootProvider {
        self.provider.root()
    }

    /// Override the `get_block_number` method to fetch the latest block number.
    fn get_block_number(&self) -> ProviderCall<NoParams, U64, u64> {
        // Block number remains the same.
        self.provider.get_block_number()
    }

    fn get_transaction_count(&self, address: Address) -> RpcWithBlock<Address, U64, u64> {
        tracing::debug!(%address, "get_transaction_count");
        let pending = self.pending.clone();
        let provider = self.provider.clone();

        RpcWithBlock::new_provider(move |block_id| {
            let call = provider.get_transaction_count(address).block_id(block_id);
            let pending = pending.clone();

            // If the tag is pending, we use the pending state.
            ProviderCall::BoxedFuture(Box::pin(async move {
                let nonce = if let Some(pending) = pending.borrow().as_ref()
                    && block_id.is_pending()
                {
                    pending
                        .executed_block
                        .execution_outcome()
                        .account(&address)
                        .flatten()
                        .map(|a| a.nonce)
                } else {
                    None
                };

                Ok(nonce.unwrap_or(call.await?))
            }))
        })
    }

    fn get_storage_at(
        &self,
        address: Address,
        key: U256
    ) -> RpcWithBlock<(Address, U256), StorageValue> {
        tracing::debug!(%address, %key, "get_storage_at");
        let pending = self.pending.clone();
        let provider = self.provider.clone();

        RpcWithBlock::new_provider(move |block_id| {
            let call = provider.get_storage_at(address, key).block_id(block_id);
            let pending = pending.clone();

            // If the tag is pending, we use the pending state.
            ProviderCall::BoxedFuture(Box::pin(async move {
                let value = if let Some(pending) = pending.borrow().as_ref()
                    && block_id.is_pending()
                {
                    pending
                        .executed_block
                        .execution_outcome()
                        .storage(&address, key)
                } else {
                    None
                };

                Ok(value.unwrap_or(call.await?))
            }))
        })
    }

    fn get_code_at(&self, address: Address) -> RpcWithBlock<Address, Bytes> {
        tracing::debug!(%address, "get_code_at");
        let pending = self.pending.clone();
        let provider = self.provider.clone();

        RpcWithBlock::new_provider(move |block_id| {
            let call = provider.get_code_at(address).block_id(block_id);
            let pending = pending.clone();

            ProviderCall::BoxedFuture(Box::pin(async move {
                let code = if let Some(pending) = pending.borrow().as_ref()
                    && block_id.is_pending()
                {
                    // Abomination
                    pending
                        .executed_block
                        .execution_outcome()
                        .state()
                        .account(&address)
                        .map(|info| {
                            info.info
                                .as_ref()
                                .map(|info| info.code.as_ref().map(|code| code.original_bytes()))
                                .flatten()
                        })
                        .flatten()
                } else {
                    None
                };

                Ok(code.unwrap_or(call.await?))
            }))
        })
    }
}
