//! A provider that uses the pending state.

use alloy::{
    providers::{Provider, ProviderCall, RootProvider, RpcWithBlock},
    rpc::client::NoParams
};
use alloy_primitives::{Address, Bytes, StorageValue, U64, U256};

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
                let base = call.await?;

                let pending = pending.read();

                // If there are overrides, add them to the base nonce.
                if let Some(overrides) = pending.state_overrides()
                    && block_id.is_pending()
                {
                    // Return either the override nonce or the base nonce if it doesn't exist.
                    let nonce = overrides
                        .get(&address)
                        .map(|acc| acc.nonce)
                        .flatten()
                        .unwrap_or(base);

                    Ok(nonce)
                } else {
                    Ok(base)
                }
            }))
        })
    }

    fn get_storage_at(
        &self,
        address: Address,
        key: U256
    ) -> RpcWithBlock<(Address, U256), StorageValue> {
        todo!()
    }

    fn get_code_at(&self, address: Address) -> RpcWithBlock<Address, Bytes> {
        todo!()
    }
}
