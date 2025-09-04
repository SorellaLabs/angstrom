use alloy::{self, eips::BlockId, network::Network, primitives::Address, providers::Provider};
use alloy_primitives::U256;
use angstrom_types::contract_payloads::{
    CONFIG_STORE_SLOT, POOL_CONFIG_STORE_ENTRY_SIZE,
    angstrom::{AngPoolConfigEntry, AngstromPoolPartialKey}
};
use dashmap::DashMap;
use eyre::eyre;

pub async fn load_poolconfig_from_chain<N, P>(
    angstrom_contract: Address,
    block_id: BlockId,
    provider: &P
) -> eyre::Result<DashMap<AngstromPoolPartialKey, AngPoolConfigEntry>>
where
    N: Network,
    P: Provider<N>
{
    // offset of 6 bytes
    let value = provider
        .get_storage_at(angstrom_contract, U256::from(CONFIG_STORE_SLOT))
        .block_id(block_id)
        .await
        .map_err(|e| eyre!("Error getting storage: {}", e))?;

    let value_bytes: [u8; 32] = value.to_be_bytes();
    tracing::debug!("storage slot of poolkey storage {:?}", value_bytes);
    let config_store_address = Address::from(<[u8; 20]>::try_from(&value_bytes[4..24]).unwrap());
    tracing::info!(?config_store_address);

    let code = provider
        .get_code_at(config_store_address)
        .block_id(block_id)
        .await
        .map_err(|e| eyre!("Error getting code: {}", e))?;

    tracing::info!(len=?code.len(), "bytecode: {:x}", code);

    if code.is_empty() {
        return Ok(DashMap::new());
    }

    if code.first() != Some(&0) {
        return Err(eyre!(
            "Invalid encoded entries: must start with a safety
byte of 0"
        ));
    }
    tracing::info!(bytecode_len=?code.len());
    let adjusted_entries = &code[1..];
    if adjusted_entries.len() % POOL_CONFIG_STORE_ENTRY_SIZE != 0 {
        tracing::info!(bytecode_len=?adjusted_entries.len(),
?POOL_CONFIG_STORE_ENTRY_SIZE);
        return Err(eyre!(
            "Invalid encoded
entries: incorrect length after removing safety byte"
        ));
    }
    let entries = adjusted_entries
        .chunks(POOL_CONFIG_STORE_ENTRY_SIZE)
        .enumerate()
        .map(|(index, chunk)| {
            let pool_partial_key =
                AngstromPoolPartialKey::new(<[u8; 27]>::try_from(&chunk[0..27]).unwrap());
            let tick_spacing = u16::from_be_bytes([chunk[27], chunk[28]]);
            let fee_in_e6 = u32::from_be_bytes([0, chunk[29], chunk[30], chunk[31]]);
            (
                pool_partial_key,
                AngPoolConfigEntry {
                    pool_partial_key,
                    tick_spacing,
                    fee_in_e6,
                    store_index: index
                }
            )
        })
        .collect();
    Ok(entries)
}
