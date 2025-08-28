//! A provider that uses the pending state.

use alloy::{
    providers::{Provider, ProviderCall, RootProvider, RpcWithBlock},
    rpc::client::NoParams
};
use alloy_primitives::{Address, Bytes, StorageKey, StorageValue, U64, U256};

use crate::PendingStateReader;

impl<P: Provider + Clone + 'static> Provider for PendingStateReader<P> {
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
                // The base state
                let mut nonce = call.await?;

                let pending = pending.read();

                if let Some(overrides) = pending.state_overrides()
                    && block_id.is_pending()
                {
                    // If an override exists, return the override nonce.
                    // Return either the override nonce or the base nonce if it doesn't exist.
                    nonce = overrides
                        .get(&address)
                        .map(|acc| acc.nonce)
                        .flatten()
                        .unwrap_or(nonce);
                }

                Ok(nonce)
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
                let mut value: StorageValue = call.await?;

                let pending = pending.read();

                if let Some(overrides) = pending.state_overrides()
                    && block_id.is_pending()
                {
                    let key: StorageKey = key.into();
                    // Return either the override nonce or the base nonce if it doesn't exist.
                    value = overrides
                        .get(&address)
                        .map(|acc| {
                            acc.state_diff
                                .as_ref()
                                .map(|diff| diff.get(&key).cloned())
                                .flatten()
                        })
                        .flatten()
                        .unwrap_or(value.into())
                        .into();
                }

                Ok(value)
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
                let mut code = call.await?;

                let pending = pending.read();

                if let Some(overrides) = pending.state_overrides()
                    && block_id.is_pending()
                {
                    code = overrides
                        .get(&address)
                        .map(|acc| acc.code.clone())
                        .flatten()
                        .unwrap_or(code);
                }

                Ok(code)
            }))
        })
    }
}
