use std::sync::Arc;

use alloy_provider::Provider;
use alloy_rpc_types::Filter;
use angstrom_types::contract_bindings::controller_v_1::ControllerV1::{self, ControllerV1Events};

use crate::types::*;

pub struct ProtocolFeeFetcher<P: Provider> {
    provider:  Arc<P>,
    max_block: u64
}

impl<P: Provider> ProtocolFeeFetcher<P> {
    pub async fn new(provider: P) -> eyre::Result<Self> {
        let max_block = provider.get_block_number().await?;
        Ok(Self { provider: Arc::new(provider), max_block })
    }

    /// gets the block and the tx index of the last time the fees were pulled
    pub async fn get_last_fee_pull(&self) -> eyre::Result<BlockAndTxIndex> {
        let filter = Filter::new()
            .from_block(angstrom_deployed_block())
            .to_block(self.max_block)
            .address(controller_v1_address());
        todo!("must have the correct filter");

        let logs = self.provider.get_logs(&filter).await?;

        let block_and_idx = logs
            .into_iter()
            .max_by_key(|log| log.block_number.unwrap())
            .map(|log| BlockAndTxIndex {
                block_number: log.block_number.unwrap(),
                tx_index:     log.transaction_index.unwrap()
            })
            .unwrap_or_else(|| BlockAndTxIndex {
                block_number: angstrom_deployed_block(),
                tx_index:     0
            });

        Ok(block_and_idx)
    }

    pub async fn get_fee_changes(
        &self,
        starting_point: BlockAndTxIndex
    ) -> eyre::Result<Vec<BlockAndTxIndex>> {
        let filter = Filter::new()
            .from_block(starting_point.block_number)
            .to_block(self.max_block)
            .address(controller_v1_address());

        // .event_signature(ControllerV1::);

        let logs = self.provider.get_logs(&filter).await?;

        todo!()
    }

    pub async fn get_bundles_in_range(&self, range: ProtocolFeeBlockRange) -> eyre::Result<()> {
        let filter = Filter::new()
            .from_block(angstrom_deployed_block())
            .to_block(self.max_block)
            .address(angstrom_address());

        let logs = self.provider.get_logs(&filter).await?;

        Ok(())
    }
}
