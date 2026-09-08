use alloy_eips::BlockNumHash;
use alloy_primitives::{Address, Bytes, U256};
use alloy_rpc_types::Log;
use alloy_sol_types::{SolCall, SolValue};
use angstrom_types_primitives::{
    ANGSTROM_ADDRESS, ANGSTROM_DEPLOYED_BLOCK, CONTROLLER_V1_ADDRESS,
    contract_bindings::controller_v_1::ControllerV1
};

pub fn angstrom_deployed_block() -> u64 {
    *ANGSTROM_DEPLOYED_BLOCK.get().unwrap()
}

pub fn controller_v1_address() -> Address {
    *CONTROLLER_V1_ADDRESS.get().unwrap()
}

pub fn angstrom_address() -> Address {
    *ANGSTROM_ADDRESS.get().unwrap()
}

pub struct TokenSavings {
    pub asset:       Address,
    pub symbol:      String,
    pub saved_gross: String
}

pub struct BundleFees {
    pub block:  BlockNumHash,
    pub tokens: Vec<TokenSavings>
}

pub fn decode_distribute_fees_logs(logs: Vec<Log>) -> eyre::Result<Vec<Log>> {
    let mut valid_logs = Vec::new();
    for log in logs {
        // Timelock puts id/index in topics; the data contains
        // target/value/calldata.
        let (target, _, calldata) = <(Address, U256, Bytes)>::abi_decode_params(&log.data().data)?;
        if target == controller_v1_address()
            && calldata.starts_with(&ControllerV1::distributeFeesCall::SELECTOR)
        {
            let call = ControllerV1::distributeFeesCall::abi_decode(&calldata)?;
            if call.assets.iter().any(|asset| !asset.total.is_zero()) {
                valid_logs.push(log);
            }
        }
    }
    Ok(valid_logs)
}
