use alloy::{eips::BlockId, providers::Provider, sol_types::SolCall};
use alloy_primitives::{Address, TxKind};
use reth::rpc::types::{TransactionInput, TransactionRequest};

pub async fn view_call<P, IC>(
    provider: &P,
    contract: Address,
    block: BlockId,
    call: IC
) -> eyre::Result<IC::Return>
where
    P: Provider,
    IC: SolCall
{
    let tx = TransactionRequest {
        to: Some(TxKind::Call(contract)),
        input: TransactionInput::both(call.abi_encode().into()),
        ..Default::default()
    };

    let data = provider.call(tx).block(block).await?;
    Ok(IC::abi_decode_returns(&data)?)
}
