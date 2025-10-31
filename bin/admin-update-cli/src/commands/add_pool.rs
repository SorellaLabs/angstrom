use std::path::PathBuf;

use alloy::{eips::BlockId, providers::Provider};
use alloy_primitives::{Address, FixedBytes, U160, U256, aliases::U24};
use angstrom_types::{
    contract_bindings::{angstrom::Angstrom, controller_v_1::ControllerV1},
    contract_payloads::angstrom::AngstromPoolConfigStore,
    primitive::{ANGSTROM_ADDRESS, CONTROLLER_V1_ADDRESS}
};

use crate::utils::{format_call, view_call};

/// generates the calls/calldata for creating a new pool
#[derive(Debug, Clone, clap::Parser)]
pub struct AddPoolCommand {
    /// token0 of the new pool
    #[clap(long)]
    pub token0: Address,

    /// token1 of the new pool
    #[clap(long)]
    pub token1: Address,

    /// tick_spacing of the new pool
    /// ControllerV1::configurePoolCall requires tick_spacing to be a u16
    #[clap(long)]
    pub tick_spacing: u16,

    /// the bundle fee value denominated in 100ths of a bip
    #[clap(long = "bundle-fee")]
    pub bundle_fee_e6: u32,

    /// the unlock fee value denominated in 100ths of a bip
    #[clap(long = "unlock-fee")]
    pub unlock_fee_e6: u32,

    /// the protocol unlock fee value denominated in 100ths of a bip
    #[clap(long = "protocol-unlock-fee")]
    pub protocol_unlock_fee_e6: u32,

    /// the initial sqrt price of the new pool in X96 units
    #[clap(long = "sqrt-price")]
    pub sqrt_price: U160,

    /// if set, will write the encoded hex to an outfile, otherwise will print
    /// it in the cli
    #[clap(short = 'o', long)]
    pub encoded_data_out_file: Option<PathBuf>
}

impl AddPoolCommand {
    pub async fn run<P: Provider>(self, provider: P) -> eyre::Result<()> {
        if self.token0 >= self.token1 {
            eyre::bail!("token0 cannot be greater than or equal to token 1");
        }

        if self.check_key_exists(&provider, BlockId::latest()).await? {
            eyre::bail!("key already exists for tokens: {:?} - {:?}", self.token0, self.token1);
        }

        let current_store_index = self
            .get_next_store_index(&provider, BlockId::latest())
            .await?;

        let call0 = self.build_configure_pool_calldata();
        let call1 = self.build_initialize_pool_calldata(current_store_index);

        let calldata_str = format!(
            "{}{}",
            format_call(0, *CONTROLLER_V1_ADDRESS.get().unwrap(), call0),
            format_call(1, *ANGSTROM_ADDRESS.get().unwrap(), call1)
        );

        if let Some(path) = self.encoded_data_out_file {
            std::fs::write(&path, calldata_str.as_bytes())?;
            tracing::info!("wrote calldata bytes to {path:?}");
        } else {
            tracing::info!("displaying calldata");
            println!("{calldata_str}")
        }

        Ok(())
    }

    async fn get_next_store_index<P: Provider>(
        &self,
        provider: &P,
        block: BlockId
    ) -> eyre::Result<U256> {
        let pool_index = view_call(
            provider,
            *CONTROLLER_V1_ADDRESS.get().unwrap(),
            block,
            ControllerV1::totalPoolsCall {}
        )
        .await?;

        Ok(pool_index)
    }

    async fn check_key_exists<P: Provider>(
        &self,
        provider: &P,
        block: BlockId
    ) -> eyre::Result<bool> {
        let key = *AngstromPoolConfigStore::derive_store_key(self.token0, self.token1);

        let exists = view_call(
            provider,
            *CONTROLLER_V1_ADDRESS.get().unwrap(),
            block,
            ControllerV1::keyExistsCall { key: FixedBytes::new(key) }
        )
        .await?;

        Ok(exists)
    }

    fn build_configure_pool_calldata(&self) -> ControllerV1::configurePoolCall {
        ControllerV1::configurePoolCall {
            asset0:              self.token0,
            asset1:              self.token1,
            tickSpacing:         self.tick_spacing as u16,
            bundleFee:           U24::from(self.bundle_fee_e6),
            unlockedFee:         U24::from(self.unlock_fee_e6),
            protocolUnlockedFee: U24::from(self.protocol_unlock_fee_e6)
        }
    }

    fn build_initialize_pool_calldata(&self, store_index: U256) -> Angstrom::initializePoolCall {
        Angstrom::initializePoolCall {
            assetA:       self.token0,
            assetB:       self.token1,
            storeIndex:   store_index,
            sqrtPriceX96: self.sqrt_price
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy::{
        alloy_sol_types::SolCall,
        eips::BlockId,
        node_bindings::Anvil,
        providers::{
            Provider, ProviderBuilder, WsConnect,
            ext::{AnvilApi, DebugApi, TraceApi}
        },
        rpc::types::TransactionRequest,
        sol_types::SolValue
    };
    use alloy_primitives::{U160, U256, address, keccak256};
    use angstrom_types::{
        contract_bindings::pool_manager::PoolManager::PoolKey,
        primitive::{CONTROLLER_V1_ADDRESS, POOL_MANAGER_ADDRESS, PoolId, init_with_chain_id}
    };

    use super::*;
    use crate::{cli::init_tracing, utils::view_call};

    impl AddPoolCommand {
        fn pool_id(&self) -> PoolId {
            PoolKey {
                currency0:   self.token0,
                currency1:   self.token1,
                fee:         U24::from(0x800000),
                tickSpacing: I24::unchecked_from(self.tick_spacing),
                hooks:       *ANGSTROM_ADDRESS.get().unwrap()
            }
            .into()
        }
    }

    async fn pool_manager_slot0_value<P: Provider>(
        provider: &P,
        pool_id: U256,
        block: BlockId
    ) -> U256 {
        let slot = keccak256((pool_id, U256::from(6u8)).abi_encode());
        provider
            .get_storage_at(*POOL_MANAGER_ADDRESS.get().unwrap(), slot.into())
            .block_id(block)
            .await
            .unwrap()
    }

    #[tracing_test::traced_test]
    #[tokio::test]
    async fn test_key_exists() {
        dotenv::dotenv().ok();
        let block_number = 23698848;

        let provider = ProviderBuilder::new()
            .connect_ws(WsConnect::new(
                std::env::var("CI_ETH_WS_URL").expect("CI_ETH_WS_URL not found in .env")
            ))
            .await
            .unwrap();

        let mut cmd = AddPoolCommand {
            encoded_data_out_file:  None,
            token0:                 address!("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"),
            token1:                 address!("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),
            tick_spacing:           Default::default(),
            bundle_fee_e6:          Default::default(),
            unlock_fee_e6:          Default::default(),
            protocol_unlock_fee_e6: Default::default(),
            sqrt_price:             Default::default()
        };

        let key_exists = cmd
            .check_key_exists(&provider, block_number.into())
            .await
            .unwrap();
        assert!(key_exists);

        cmd.token1 = address!("0xdac17f958d2ee523a2206206994597c13d831ec7");
        let key_exists = cmd
            .check_key_exists(&provider, block_number.into())
            .await
            .unwrap();
        assert!(!key_exists);
    }

    #[tracing_test::traced_test]
    #[tokio::test]
    async fn test_add_pools() {
        dotenv::dotenv().ok();
        init_with_chain_id(1);
        init_tracing();

        let block_number = 23698848;

        let fork_url = std::env::var("CI_ETH_WS_URL").expect("CI_ETH_WS_URL not found in .env");

        let anvil = Anvil::new()
            .chain_id(1)
            .arg("--host")
            .arg("0.0.0.0")
            .port(rand::random::<u16>())
            .fork(fork_url)
            .fork_block_number(block_number)
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

        let usdc = address!("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");
        let usdt = address!("0xdac17f958d2ee523a2206206994597c13d831ec7");

        let cmd = AddPoolCommand {
            encoded_data_out_file:  None,
            token0:                 usdc,
            token1:                 usdt,
            tick_spacing:           10,
            bundle_fee_e6:          100,
            unlock_fee_e6:          200,
            protocol_unlock_fee_e6: 150,
            sqrt_price:             U160::from_str_radix(
                "217271571724181780572453274713147958",
                10
            )
            .unwrap()
        };

        let num_pools_pre = view_call(
            &provider,
            *CONTROLLER_V1_ADDRESS.get().unwrap(),
            block_number.into(),
            ControllerV1::totalPoolsCall {}
        )
        .await
        .unwrap();

        let pool_slot0_pre =
            pool_manager_slot0_value(&provider, cmd.pool_id().into(), block_number.into()).await;

        assert_eq!(pool_slot0_pre, U256::ZERO);

        let configure_pool_call = cmd.build_configure_pool_calldata();

        let configure_pool_tx = TransactionRequest::default()
            .to(*CONTROLLER_V1_ADDRESS.get().unwrap())
            .from(fast_owner)
            .input(configure_pool_call.abi_encode().into());

        let txs = provider
            .send_transaction(configure_pool_tx)
            .await
            .unwrap()
            .watch()
            .await
            .unwrap();

        let initialize_pool_call = cmd.build_initialize_pool_calldata(U256::from(2u8));
        let initialize_pool_tx = TransactionRequest::default()
            .to(*ANGSTROM_ADDRESS.get().unwrap())
            .from(fast_owner)
            .input(initialize_pool_call.abi_encode().into());

        let _ = provider
            .send_transaction(initialize_pool_tx)
            .await
            .unwrap()
            .watch()
            .await
            .unwrap();

        let num_pools_post = view_call(
            &provider,
            *CONTROLLER_V1_ADDRESS.get().unwrap(),
            BlockId::latest(),
            ControllerV1::totalPoolsCall {}
        )
        .await
        .unwrap();

        let pool_slot0_post =
            pool_manager_slot0_value(&provider, cmd.pool_id().into(), BlockId::latest()).await;

        assert_eq!(num_pools_post - num_pools_pre, U256::ONE);
        assert_ne!(pool_slot0_post, U256::ZERO);
    }
}
