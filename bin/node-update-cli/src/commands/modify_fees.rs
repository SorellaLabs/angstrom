use alloy::{
    eips::BlockId, providers::Provider, rpc::types::TransactionRequest, sol_types::SolCall
};
use alloy_primitives::{
    Address, FixedBytes, TxKind,
    aliases::{I24, U24}
};
use angstrom_types::{
    contract_bindings::{
        angstrom::Angstrom::PoolKey,
        controller_v_1::ControllerV1::{self, getPoolByKeyCall}
    },
    contract_payloads::angstrom::{AngstromPoolConfigStore, AngstromPoolPartialKey},
    primitive::{ANGSTROM_ADDRESS, CONTROLLER_V1_ADDRESS, PoolId}
};
use futures::FutureExt;
use reth::rpc::types::TransactionInput;

#[derive(Debug, Clone, clap::Parser)]
pub struct ModifyPoolFeesCommand {
    /// pool id to modify
    #[clap(short, long)]
    pub pool_id: PoolId,

    /// the new bundle fee value denominated in 100ths of a bip
    #[clap(long)]
    pub bundle_fee_e6: Option<u64>,

    /// the new unlock fee value denominated in 100ths of a bip
    #[clap(long)]
    pub unlock_fee_e6: Option<u64>,

    /// the new protocol unlock fee value denominated in 100ths of a bip
    #[clap(long)]
    pub protocol_unlock_fee_e6: Option<u64>
}

impl ModifyPoolFeesCommand {
    pub async fn run<P: Provider>(self, provider: P) -> eyre::Result<()> {
        let configure_call = self.get_initial_fees(&provider).await?;

        let tx = TransactionRequest::default()
            .input(configure_call.abi_encode().into())
            .to(*CONTROLLER_V1_ADDRESS.get().unwrap());

        let tx_hash = provider.send_transaction(tx).await?.watch().await?;

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

        let unlocked_fees = view_call(
            provider,
            *CONTROLLER_V1_ADDRESS.get().unwrap(),
            _private::unlockedFeeCall {
                _0: *ANGSTROM_ADDRESS.get().unwrap(),
                _1: FixedBytes::from(*store_key)
            }
        )
        .await?;

        let (mut bundle_fee, mut unlock_fee, mut protocol_unlock_fee) = (
            U24::from(searched_pool_config_store.fee_in_e6),
            unlocked_fees.fee,
            unlocked_fees.protocolFee
        );

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

async fn get_token_pairs<P: Provider>(
    provider: &P,
    pool_partial_key: AngstromPoolPartialKey
) -> eyre::Result<(Address, Address)> {
    let out = view_call(
        provider,
        *CONTROLLER_V1_ADDRESS.get().unwrap(),
        getPoolByKeyCall { key: FixedBytes::from(*pool_partial_key) }
    )
    .await?;

    Ok((out.asset0, out.asset1))
}

async fn view_call<P, IC>(provider: &P, contract: Address, call: IC) -> eyre::Result<IC::Return>
where
    P: Provider,
    IC: SolCall
{
    let tx = TransactionRequest {
        to: Some(TxKind::Call(contract)),
        input: TransactionInput::both(call.abi_encode().into()),
        ..Default::default()
    };

    let data = provider.call(tx).block(BlockId::latest()).await?;
    Ok(IC::abi_decode_returns(&data)?)
}

mod _private {
    use alloy::sol;

    sol! {
        function unlockedFee(address, bytes27)
            internal
            view
            returns (uint24 fee, uint24 protocolFee);
    }
}
