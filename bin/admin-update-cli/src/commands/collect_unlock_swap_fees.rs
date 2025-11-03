use std::path::PathBuf;

use alloy_primitives::Address;
use angstrom_types::{
    contract_bindings::controller_v_1::ControllerV1, primitive::CONTROLLER_V1_ADDRESS
};

use crate::utils::format_call;

/// generates the calldata for the `collect_unlock_swap_fees` call on the
/// ControllerV1
#[derive(Debug, Clone, clap::Parser)]
pub struct CollectUnlockSwapFeesCommand {
    /// the address to withdrawal the assets to
    #[clap(long = "to")]
    pub withdraw_to: Address,

    /// assets to withdraw
    #[clap(long = "assets", value_delimiter = ',')]
    pub assets: Vec<Address>,

    /// if set, will write the encoded hex to an outfile, otherwise will print
    /// it in the cli
    #[clap(short = 'o', long)]
    pub encoded_data_out_file: Option<PathBuf>
}

impl CollectUnlockSwapFeesCommand {
    pub fn run(&self) -> eyre::Result<()> {
        let call = self.build_calldata();

        let calldata_str =
            format!("{}", format_call(0, *CONTROLLER_V1_ADDRESS.get().unwrap(), call));

        if let Some(path) = self.encoded_data_out_file.as_ref() {
            std::fs::write(&path, calldata_str.as_bytes())?;
            tracing::info!("wrote calldata bytes to {path:?}");
        } else {
            tracing::info!("displaying calldata");
            println!("{calldata_str}")
        }

        Ok(())
    }

    fn build_calldata(&self) -> ControllerV1::collect_unlock_swap_feesCall {
        ControllerV1::collect_unlock_swap_feesCall {
            to:            self.withdraw_to,
            packed_assets: self
                .assets
                .iter()
                .flat_map(|a| **a)
                .collect::<Vec<_>>()
                .into()
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy::{
        eips::BlockId,
        node_bindings::Anvil,
        providers::{Provider, ProviderBuilder, WsConnect, ext::AnvilApi},
        rpc::types::TransactionRequest,
        sol_types::SolCall
    };
    use alloy_primitives::{U256, address, bytes};
    use angstrom_types::primitive::{CONTROLLER_V1_ADDRESS, ERC20, init_with_chain_id};

    use super::*;
    use crate::{cli::init_tracing, utils::view_call};

    async fn get_balance_of<P: Provider>(
        provider: &P,
        owner: Address,
        token: Address,
        block: BlockId
    ) -> U256 {
        view_call(provider, token, block, ERC20::balanceOfCall { _owner: owner })
            .await
            .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    fn test_build_calldata() {
        let withdraw_to = address!("0x1746484ea5e11c75e009252c102c8c33e0315fd4");

        let cmd = CollectUnlockSwapFeesCommand {
            withdraw_to,
            assets: vec![
                address!("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"),
                address!("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),
                address!("0xdac17f958d2ee523a2206206994597c13d831ec7"),
            ],
            encoded_data_out_file: None
        };

        let call = cmd.build_calldata();

        assert_eq!(call.to, withdraw_to);
        assert_eq!(call.packed_assets, bytes!("0x33830e480000000000000000000000001746484ea5e11c75e009252c102c8c33e0315fd40000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000003ca0b86991c6218b36c1d19d4a2e9eb0ce3606eb48c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2dac17f958d2ee523a2206206994597c13d831ec700000000"));
    }

    #[tracing_test::traced_test]
    #[tokio::test]
    async fn test_collect_unlock_swap_fees() {
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
        let weth = address!("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
        let usdt = address!("0xdac17f958d2ee523a2206206994597c13d831ec7");

        let cmd = CollectUnlockSwapFeesCommand {
            withdraw_to:           fast_owner,
            assets:                vec![usdc, weth, usdt],
            encoded_data_out_file: None
        };

        let call = cmd.build_calldata();

        let usdc_before = get_balance_of(&provider, fast_owner, usdc, block_number.into()).await;
        let weth_before = get_balance_of(&provider, fast_owner, weth, block_number.into()).await;
        let usdt_before = get_balance_of(&provider, fast_owner, usdt, block_number.into()).await;

        let tx = TransactionRequest::default()
            .to(*CONTROLLER_V1_ADDRESS.get().unwrap())
            .from(fast_owner)
            .input(call.abi_encode().into());

        let _ = provider
            .send_transaction(tx)
            .await
            .unwrap()
            .watch()
            .await
            .unwrap();

        let usdc_after = get_balance_of(&provider, fast_owner, usdc, BlockId::latest()).await;
        let weth_after = get_balance_of(&provider, fast_owner, weth, BlockId::latest()).await;
        let usdt_after = get_balance_of(&provider, fast_owner, usdt, BlockId::latest()).await;

        assert_eq!(usdc_after - usdc_before, U256::from(847893643_u128));
        assert_eq!(weth_after - weth_before, U256::from(332801990541928268_u128));
        assert_eq!(usdt_after - usdt_before, U256::from(150865625_u128));
    }
}
