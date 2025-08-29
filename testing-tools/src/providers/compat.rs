use alloy_rpc_types::{Block, TransactionReceipt};
use reth_node_types::NodePrimitives;
use reth_optimism_primitives::OpPrimitives;
use reth_primitives::EthPrimitives;

/// Convert alloy RPC Block to generic NodePrimitives Block using try_into
pub fn rpc_block_to_pr_block<PR: NodePrimitives>(block: &Block) -> eyre::Result<PR::Block>
where
    PR::Block: TryFrom<Block>,
    <PR::Block as TryFrom<Block>>::Error: std::fmt::Debug,
{
    block
        .clone()
        .try_into()
        .map_err(|e| eyre::eyre!("Failed to convert block: {:?}", e))
}

/// Convert alloy RPC receipts to generic NodePrimitives receipts using try_into
pub fn rpc_receipts_to_pr_receipts<PR: NodePrimitives>(
    receipts: Vec<TransactionReceipt>,
) -> eyre::Result<Vec<PR::Receipt>>
where
    PR::Receipt: TryFrom<alloy_rpc_types::ReceiptEnvelope<alloy_rpc_types::Log>>,
    <PR::Receipt as TryFrom<alloy_rpc_types::ReceiptEnvelope<alloy_rpc_types::Log>>>::Error:
        std::fmt::Debug,
{
    receipts
        .into_iter()
        .map(|r| {
            // TransactionReceipt wraps a ReceiptEnvelope
            r.inner
                .try_into()
                .map_err(|e| eyre::eyre!("Failed to convert receipt: {:?}", e))
        })
        .collect()
}

/// Convert alloy RPC Block to EthPrimitives Block
pub fn rpc_block_to_eth_block(
    block: &Block,
) -> eyre::Result<<EthPrimitives as NodePrimitives>::Block> {
    block
        .clone()
        .try_into()
        .map_err(|e| eyre::eyre!("Failed to convert block: {:?}", e))
}

/// Convert alloy RPC receipts to EthPrimitives receipts
pub fn rpc_receipts_to_eth_receipts(
    receipts: Vec<TransactionReceipt>,
) -> eyre::Result<Vec<<EthPrimitives as NodePrimitives>::Receipt>> {
    receipts
        .into_iter()
        .map(|r| {
            // TransactionReceipt has an inner field that contains the actual receipt data
            let inner = r.inner;
            let receipt: reth_primitives::Receipt = inner
                .try_into()
                .map_err(|e| eyre::eyre!("Failed to convert receipt: {:?}", e))?;
            Ok(receipt)
        })
        .collect()
}

/// Convert alloy RPC Block to EthPrimitives Block
pub fn rpc_block_to_op_block(
    block: &Block,
) -> eyre::Result<<OpPrimitives as NodePrimitives>::Block> {
    block
        .clone()
        .try_into()
        .map_err(|e| eyre::eyre!("Failed to convert block: {:?}", e))
}

/// Convert alloy RPC receipts to EthPrimitives receipts
pub fn rpc_receipts_to_op_receipts(
    receipts: Vec<TransactionReceipt>,
) -> eyre::Result<Vec<<OpPrimitives as NodePrimitives>::Receipt>> {
    receipts
        .into_iter()
        .map(|r| {
            // TransactionReceipt has an inner field that contains the actual receipt data
            let inner = r.inner;
            let receipt: reth_primitives::Receipt = inner
                .try_into()
                .map_err(|e| eyre::eyre!("Failed to convert receipt: {:?}", e))?;
            Ok(receipt)
        })
        .collect()
}
