use std::fmt::Debug;

use alloy::{eips::BlockId, providers::Provider, sol_types::SolCall};
use alloy_primitives::{Address, Bytes, TxKind};
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

pub fn format_call<IC: SolCall + Debug>(call_id: u16, to: Address, call: IC) -> String {
    let call_str = format!("call:\n{call:?}\n\n");
    let to_str = format!("to: {to:?}\n");
    let calldata_str = format!("calldata: {:?}\n", Bytes::from(call.abi_encode()));
    format!("\n\n------------- Call {call_id} -------------\n{call_str}{to_str}{calldata_str}")
}
