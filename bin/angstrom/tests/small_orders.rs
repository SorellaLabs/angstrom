use std::{
    collections::{HashMap, HashSet},
    sync::Arc
};

use alloy::{
    network::Ethereum,
    primitives::Address,
    providers::{Provider, ProviderBuilder},
    rpc::types::{BlockId, BlockNumberOrTag, Filter},
    sol_types::SolEvent
};
use alloy_primitives::aliases::I24;
use angstrom::components::initialize_strom_handles;
use angstrom_types::{
    contract_bindings::{angstrom::Angstrom::PoolKey, controller_v_1::ControllerV1::*},
    contract_payloads::angstrom::AngstromPoolConfigStore,
    primitive::*
};
use futures::StreamExt;
use validation::validator::ValidationClient;
// The goal of this test is to spinup matching engine and assert that our
// uniswap v4 pool swaps match against all critical points,
// 1) matching engine output <> bundle building endpoint
// 2) bundle_bundling <> revm sim
#[tokio::test]
pub async fn test_small_orders_matching_engine() {
    init_with_chain_id(1);
    let channels = initialize_strom_handles();
    let validation_client = ValidationClient(channels.validator_tx.clone());
    let url = "";

    let querying_provider: Arc<_> = ProviderBuilder::<_, _, Ethereum>::default()
        .with_recommended_fillers()
        // backup
        .connect(&url)
        .await
        .unwrap()
        .into();

    let block_id = querying_provider.get_block_number().await.unwrap();

    let angstrom_address = *ANGSTROM_ADDRESS.get().unwrap();
    let controller = *CONTROLLER_V1_ADDRESS.get().unwrap();
    let deploy_block = *ANGSTROM_DEPLOYED_BLOCK.get().unwrap();
    let gas_token = *GAS_TOKEN_ADDRESS.get().unwrap();
    let pool_manager = *POOL_MANAGER_ADDRESS.get().unwrap();

    let pool_config_store = Arc::new(
        AngstromPoolConfigStore::load_from_chain(
            angstrom_address,
            BlockId::Number(BlockNumberOrTag::Latest),
            &querying_provider
        )
        .await
        .unwrap()
    );

    // load the angstrom pools;
    tracing::info!("starting search for pools");
    let pools = fetch_angstrom_pools(
        deploy_block as usize,
        block_id as usize,
        angstrom_address,
        &querying_provider
    )
    .await;
    tracing::info!("found pools");
    let angstrom_tokens = pools
        .iter()
        .flat_map(|pool| [pool.currency0, pool.currency1])
        .fold(HashMap::<Address, usize>::new(), |mut acc, x| {
            *acc.entry(x).or_default() += 1;
            acc
        });

    let uniswap_registry: UniswapPoolRegistry = pools.into();
    let uni_ang_registry =
        UniswapAngstromRegistry::new(uniswap_registry.clone(), pool_config_store.clone());

    let uniswap_pool_manager = configure_uniswap_manager::<_, DEFAULT_TICKS>(
        querying_provider.clone(),
        eth_handle.subscribe_cannon_state_notifications().await,
        uniswap_registry,
        block_id,
        global_block_sync.clone(),
        pool_manager,
        network_stream
    )
    .await;

    assert!(true);
}

async fn fetch_angstrom_pools<P>(
    // the block angstrom was deployed at
    mut deploy_block: usize,
    end_block: usize,
    angstrom_address: Address,
    db: &P
) -> Vec<PoolKey>
where
    P: Provider
{
    let mut filters = vec![];
    let controller_address = *CONTROLLER_V1_ADDRESS.get().unwrap();

    loop {
        let this_end_block = std::cmp::min(deploy_block + 99_999, end_block);

        if this_end_block == deploy_block {
            break;
        }

        println!("{:?} {:?}", deploy_block, this_end_block);
        let filter = Filter::new()
            .from_block(deploy_block as u64)
            .to_block(this_end_block as u64)
            .address(controller_address);

        filters.push(filter);

        deploy_block = std::cmp::min(end_block, this_end_block);
    }

    let logs = futures::stream::iter(filters)
        .map(|filter| async move {
            db.get_logs(&filter)
                .await
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>()
        })
        .buffered(10)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    logs.into_iter()
        .fold(HashSet::new(), |mut set, log| {
            if let Ok(pool) = PoolConfigured::decode_log(&log.clone().into_inner()) {
                let pool_key = PoolKey {
                    currency0:   pool.asset0,
                    currency1:   pool.asset1,
                    fee:         pool.bundleFee,
                    tickSpacing: I24::try_from_be_slice(&{
                        let bytes = pool.tickSpacing.to_be_bytes();
                        let mut a = [0u8; 3];
                        a[1..3].copy_from_slice(&bytes);
                        a
                    })
                    .unwrap(),
                    hooks:       angstrom_address
                };

                set.insert(pool_key);
                return set;
            }

            if let Ok(pool) = PoolRemoved::decode_log(&log.clone().into_inner()) {
                let pool_key = PoolKey {
                    currency0:   pool.asset0,
                    currency1:   pool.asset1,
                    fee:         pool.feeInE6,
                    tickSpacing: pool.tickSpacing,
                    hooks:       angstrom_address
                };

                set.remove(&pool_key);
                return set;
            }
            set
        })
        .into_iter()
        .collect::<Vec<_>>()
}
