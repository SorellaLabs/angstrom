use std::path::PathBuf;

use alloy::{
    eips::BlockId,
    providers::Provider,
    sol_types::{ SolValue}
};
use alloy_primitives::{
    Address, FixedBytes, U256,
    aliases::{I24, U24},
    keccak256, uint
};
use angstrom_types::{
    contract_bindings::{
        angstrom::Angstrom::{AngstromInstance, PoolKey},
        controller_v_1::ControllerV1::{self, getPoolByKeyCall}
    },
    contract_payloads::angstrom::{AngstromPoolConfigStore, AngstromPoolPartialKey},
    primitive::{ANGSTROM_ADDRESS, CONTROLLER_V1_ADDRESS, PoolId}
};
use futures::FutureExt;

use crate::utils::{format_call, view_call};

/// generates the calldata for the `configurePool` call on the
/// ControllerV1, specifically for modifying fees of an existing pool
#[derive(Debug, Clone, clap::Parser)]
pub struct ModifyPoolFeesCommand {
    /// pool id to modify
    #[clap(short, long)]
    pub pool_id: PoolId,

    /// the new bundle fee value denominated in 100ths of a bip
    #[clap(long = "bundle-fee")]
    pub bundle_fee_e6: Option<u64>,

    /// the new unlock fee value denominated in 100ths of a bip
    #[clap(long = "unlock-fee")]
    pub unlock_fee_e6: Option<u64>,

    /// the new protocol unlock fee value denominated in 100ths of a bip
    #[clap(long = "protocol-unlock-fee")]
    pub protocol_unlock_fee_e6: Option<u64>,

    /// if set, will write the encoded hex to an outfile, otherwise will print
    /// it in the cli
    #[clap(short = 'o', long)]
    pub encoded_data_out_file: Option<PathBuf>
}

impl ModifyPoolFeesCommand {
    pub async fn run<P: Provider>(self, provider: P) -> eyre::Result<()> {
        let call = self.get_initial_fees(&provider).await?;

        let calldata_str =
            format!("{}", format_call(0, *CONTROLLER_V1_ADDRESS.get().unwrap(), call));

        if let Some(path) = self.encoded_data_out_file {
            std::fs::write(&path, calldata_str.as_bytes())?;
            tracing::info!("wrote calldata bytes to {path:?}");
        } else {
            tracing::info!("displaying calldata");
            println!("{calldata_str}")
        }

        Ok(())
    }

    async fn get_initial_fees<P: Provider>(
        &self,
        provider: &P
    ) -> eyre::Result<ControllerV1::configurePoolCall> {
        let config_store = AngstromPoolConfigStore::load_from_chain(
            *ANGSTROM_ADDRESS.get().unwrap(),
            BlockId::latest(),
            provider
        )
        .await
        .map_err(|e| eyre::eyre!("{e:?}"))?;

        let token_pairs =
            futures::future::try_join_all(config_store.all_entries().iter().map(|key| {
                get_token_pairs(provider, key.pool_partial_key)
                    .then(async move |x| x.map(|t| (key.pool_partial_key, t)))
            }))
            .await?;

        let (searched_pool_config_store, store_key, (token0, token1)) = token_pairs
            .into_iter()
            .find_map(|(store_key, (t0, t1))| {
                config_store
                    .get_entry(t0, t1)
                    .map(|pool_store| {
                        let pool_key = PoolKey {
                            currency0:   t0,
                            currency1:   t1,
                            fee:         U24::from(0x800000),
                            tickSpacing: I24::unchecked_from(pool_store.tick_spacing),
                            hooks:       *ANGSTROM_ADDRESS.get().unwrap()
                        };
                        (PoolId::from(pool_key) == self.pool_id).then_some((
                            pool_store,
                            store_key,
                            (t0, t1)
                        ))
                    })
                    .flatten()
            })
            .ok_or(eyre::eyre!("no pool config found for pool id {:?}", self.pool_id))?;

        let unlocked_fees = get_unlocked_fee(provider, store_key).await?;

        let (mut bundle_fee, mut unlock_fee, mut protocol_unlock_fee) =
            (U24::from(searched_pool_config_store.fee_in_e6), unlocked_fees.0, unlocked_fees.1);

        tracing::info!(pool_id = ?self.pool_id, ?bundle_fee, ?unlock_fee, ?protocol_unlock_fee, "CURRENT fees set");

        if let Some(new_bundle_fee) = self.bundle_fee_e6.map(U24::from) {
            if bundle_fee == new_bundle_fee {
                eyre::bail!("new bundle fee cannot be the same as the current bundle fee");
            }
            bundle_fee = new_bundle_fee;
        }

        if let Some(new_unlock_fee) = self.unlock_fee_e6.map(U24::from) {
            if unlock_fee == new_unlock_fee {
                eyre::bail!("new unlock fee cannot be the same as the current unlock fee");
            }
            unlock_fee = new_unlock_fee;
        }

        if let Some(new_protocol_unlock_fee) = self.protocol_unlock_fee_e6.map(U24::from) {
            if protocol_unlock_fee == new_protocol_unlock_fee {
                eyre::bail!(
                    "new protocol unlock fee cannot be the same as the current protocol unlock fee"
                );
            }
            protocol_unlock_fee = new_protocol_unlock_fee;
        }

        tracing::info!(pool_id = ?self.pool_id, ?bundle_fee, ?unlock_fee, ?protocol_unlock_fee, "NEW fees set");

        Ok(ControllerV1::configurePoolCall {
            asset0:              token0,
            asset1:              token1,
            tickSpacing:         searched_pool_config_store.tick_spacing,
            bundleFee:           bundle_fee,
            unlockedFee:         unlock_fee,
            protocolUnlockedFee: protocol_unlock_fee
        })
    }
}

const UNLOCKED_FEE_PACKED_SET_SLOT: U256 = uint!(2_U256);

async fn get_unlocked_fee<P: Provider>(
    provider: &P,
    store_key: AngstromPoolPartialKey
) -> eyre::Result<(U24, U24)> {
    let angstrom = AngstromInstance::new(*ANGSTROM_ADDRESS.get().unwrap(), provider);

    let unlocked_fee_slot = keccak256((*store_key, UNLOCKED_FEE_PACKED_SET_SLOT).abi_encode());
    let unlocked_packed_is_set_bytes = angstrom.extsload(unlocked_fee_slot.into()).call().await?;

    let unlocked_fee = U24::from_be_slice(&unlocked_packed_is_set_bytes.to_be_bytes::<32>()[30..]);
    let protocol_unlocked_fee = U24::from_be_slice(
        &((unlocked_packed_is_set_bytes >> 24) as U256).to_be_bytes::<32>()[30..]
    );

    Ok((unlocked_fee, protocol_unlocked_fee))
}

async fn get_token_pairs<P: Provider>(
    provider: &P,
    pool_partial_key: AngstromPoolPartialKey
) -> eyre::Result<(Address, Address)> {
    let out = view_call(
        provider,
        *CONTROLLER_V1_ADDRESS.get().unwrap(),
        BlockId::latest(),
        getPoolByKeyCall { key: FixedBytes::from(*pool_partial_key) }
    )
    .await?;

    Ok((out.asset0, out.asset1))
}

#[cfg(test)]
mod tests {
    use alloy::{
        node_bindings::Anvil,
        providers::{ProviderBuilder, WsConnect, ext::AnvilApi},
        rpc::types::TransactionRequest
    };
    use alloy_primitives::{address, b256};
    use angstrom_types::primitive::init_with_chain_id;

    use super::*;
    use crate::cli::init_tracing;

    #[tracing_test::traced_test]
    #[tokio::test]
    async fn test_get_initial_fees() {
        dotenv::dotenv().ok();
        init_with_chain_id(1);
        init_tracing();

        let provider = ProviderBuilder::new()
            .connect(&std::env::var("CI_ETH_WS_URL").expect("CI_ETH_WS_URL not found in .env"))
            .await
            .unwrap();

        let cmd = ModifyPoolFeesCommand {
            pool_id:                b256!(
                "0x90078845bceb849b171873cfbc92db8540e9c803ff57d9d21b1215ec158e79b3"
            ),
            bundle_fee_e6:          Some(1),
            unlock_fee_e6:          Some(2),
            protocol_unlock_fee_e6: Some(3),
            encoded_data_out_file:  None
        };

        let fees = cmd.get_initial_fees(&provider).await.unwrap();
        assert_eq!(fees.bundleFee, U24::from(1u8));
        assert_eq!(fees.unlockedFee, U24::from(2u8));
        assert_eq!(fees.protocolUnlockedFee, U24::from(3u8));
    }

    #[tracing_test::traced_test]
    #[tokio::test]
    async fn test_change_fees() {
        dotenv::dotenv().ok();
        init_with_chain_id(1);
        init_tracing();

        let fork_url = std::env::var("CI_ETH_WS_URL").expect("CI_ETH_WS_URL not found in .env");

        let anvil = Anvil::new()
            .chain_id(1)
            .arg("--host")
            .arg("0.0.0.0")
            .port(rand::random::<u16>())
            .fork(fork_url)
            .fork_block_number(23231623)
            .arg("--code-size-limit")
            .arg("393216")
            .arg("--disable-block-gas-limit")
            .block_time(2)
            .try_spawn()
            .unwrap();

        let provider = ProviderBuilder::new()
            .connect_ws(WsConnect::new(anvil.ws_endpoint_url()))
            .await
            .unwrap();

        let fast_owner = address!("0xD31C82069da3013fdB16B731AD19076Af9b93105");

        provider
            .anvil_impersonate_account(fast_owner)
            .await
            .unwrap();

        let mut cmd = ModifyPoolFeesCommand {
            pool_id:                b256!(
                "0x90078845bceb849b171873cfbc92db8540e9c803ff57d9d21b1215ec158e79b3"
            ),
            bundle_fee_e6:          Some(1),
            unlock_fee_e6:          Some(2),
            protocol_unlock_fee_e6: Some(3),
            encoded_data_out_file:  None
        };

        let fees = cmd.get_initial_fees(&provider).await.unwrap();

        let tx = TransactionRequest::default()
            .to(*CONTROLLER_V1_ADDRESS.get().unwrap())
            .from(fast_owner)
            .input(fees.abi_encode().into());

        let _ = provider
            .send_transaction(tx)
            .await
            .unwrap()
            .watch()
            .await
            .unwrap();

        cmd.bundle_fee_e6 = Some(4);
        cmd.bundle_fee_e6 = Some(5);
        cmd.bundle_fee_e6 = Some(6);
        let fees = cmd.get_initial_fees(&provider).await.unwrap();

        assert_eq!(fees.bundleFee, U24::from(4u8));
        assert_eq!(fees.unlockedFee, U24::from(5u8));
        assert_eq!(fees.protocolUnlockedFee, U24::from(6u8));
    }
}
