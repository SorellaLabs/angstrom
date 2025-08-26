use std::collections::HashMap;

use alloy_primitives::{Address, U256};
use angstrom_types::{matching::SqrtPriceX96, primitive::PoolId};
use itertools::Itertools;
use reth_provider::test_utils::NoopProvider;
use reth_tasks::TaskExecutor;
use testing_tools::{
    controllers::enviroments::AngstromTestnet,
    types::{
        GlobalTestingConfig,
        actions::WithAction,
        checked_actions::WithCheckedAction,
        checks::WithCheck,
        config::DevnetConfig,
        initial_state::{Erc20ToDeploy, PartialConfigPoolKey}
    }
};
use tracing::{debug, info};
use validation::common::TokenPriceGenerator;

use crate::cli::devnet::DevnetCli;

pub(crate) async fn run_devnet(executor: TaskExecutor, cli: DevnetCli) -> eyre::Result<()> {
    let config = cli.make_config()?;
    let initial_state = config.initial_state_config();

    let mut testnet =
        AngstromTestnet::spawn_devnet(NoopProvider::default(), config, executor.clone())
            .await?
            .as_state_machine();

    info!("deployed state machine");

    let peer = testnet.testnet.random_peer();

    let token_gen = peer.strom_validation(|v| v.underlying.token_price_generator());
    let mut pairs_to_pools = token_gen.pairs_to_pools();

    let new_pool_key =
        PartialConfigPoolKey::new(50, 60, 34028236692, SqrtPriceX96::at_tick(0).unwrap());

    testnet.advance_block();
    testnet.check_token_price_gen_has_pools(pairs_to_pools.clone());
    testnet.deploy_new_pool(
        new_pool_key,
        Erc20ToDeploy::new("UNO", "1", None),
        Erc20ToDeploy::new("DOS", "2", None),
        U256::from(4u8)
    );
    testnet.advance_block();

    testnet.check_token_price_gen_has_pools(pairs_to_pools.clone());

    testnet.run().await;

    Ok(())
}
