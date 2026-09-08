use alloy_eips::BlockNumHash;
use alloy_primitives::{Address, Bytes, TxHash, U256, aliases::U24};
use alloy_rpc_types::Log;
use alloy_sol_types::{SolCall, SolEvent, SolValue};
use angstrom_types_primitives::{
    ANGSTROM_ADDRESS, ANGSTROM_DEPLOYED_BLOCK, CONTROLLER_V1_ADDRESS, POOL_MANAGER_ADDRESS,
    contract_bindings::{controller_v_1::ControllerV1, pool_manager::PoolManager},
    contract_payloads::Asset
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

pub fn pool_manager_address() -> Address {
    *POOL_MANAGER_ADDRESS.get().unwrap()
}

pub struct TokenMeta {
    pub asset:    Address,
    pub symbol:   String,
    pub decimals: u8
}

pub struct ProtocolFeeCalculationBuilder {
    pub blocks: Vec<ProtocolFeeBlockCalculationBuilder>,
    pub tokens: Vec<TokenMeta>
}

pub struct ProtocolFeeBlockCalculationBuilder {
    pub block_number:     u64,
    pub saves:            Vec<Asset>,
    pub distribute_calls: Vec<DecodedLogWithMeta<ControllerV1::distributeFeesCall>>
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedLogWithMeta<T> {
    pub block_number: u64,
    pub tx_hash:      TxHash,
    pub tx_index:     u64,
    pub log_index:    u64,
    pub data:         T
}

impl<T: PartialEq + PartialOrd> PartialOrd for DecodedLogWithMeta<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some((self.block_number, self.tx_index, self.log_index).cmp(&(
            other.block_number,
            other.tx_index,
            other.log_index
        )))
    }
}

impl<T: PartialEq + Eq + PartialOrd> Ord for DecodedLogWithMeta<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.block_number, self.tx_index, self.log_index).cmp(&(
            other.block_number,
            other.tx_index,
            other.log_index
        ))
    }
}

pub fn decode_distribute_fees_logs(
    logs: Vec<Log>
) -> eyre::Result<Vec<DecodedLogWithMeta<ControllerV1::distributeFeesCall>>> {
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
                let meta_log = DecodedLogWithMeta {
                    block_number: log.block_number.unwrap(),
                    tx_hash:      log.transaction_hash.unwrap(),
                    tx_index:     log.transaction_index.unwrap(),
                    log_index:    log.log_index.unwrap(),
                    data:         call
                };
                valid_logs.push(meta_log);
            }
        }
    }
    Ok(valid_logs)
}

pub fn decode_angstrom_pool_manager_logs(
    logs: Vec<Log>
) -> eyre::Result<Vec<DecodedLogWithMeta<()>>> {
    let mut valid_logs = Vec::new();
    for log in logs {
        if let Some(swap_log) = PoolManager::Swap::decode_log(&log.inner).ok() {
            if swap_log.fee == U24::ZERO {
                let meta_log = DecodedLogWithMeta {
                    block_number: log.block_number.unwrap(),
                    tx_hash:      log.transaction_hash.unwrap(),
                    tx_index:     log.transaction_index.unwrap(),
                    log_index:    log.log_index.unwrap(),
                    data:         ()
                };
                valid_logs.push(meta_log);
            }
        }
    }
    Ok(valid_logs)
}
