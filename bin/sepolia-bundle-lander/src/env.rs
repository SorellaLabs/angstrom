use std::{collections::HashSet, sync::Arc};

use alloy::{
    eips::Encodable2718,
    network::TransactionBuilder,
    primitives::{Address, Bytes, U256, address, aliases::I24},
    providers::{
        Identity, Provider, ProviderBuilder, RootProvider,
        fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller}
    },
    sol_types::{SolCall, SolEvent}
};
use alloy_rpc_types::{Filter, TransactionRequest};
use angstrom_types::{
    contract_bindings::{
        angstrom::Angstrom::PoolKey,
        controller_v_1::ControllerV1::{PoolConfigured, PoolRemoved}
    },
    primitive::{ANGSTROM_DOMAIN, AngstromSigner, UniswapPoolRegistry},
    sol_bindings::{grouped_orders::GroupedVanillaOrder, rpc_orders::OmitOrderMeta}
};
use futures::{StreamExt, stream::FuturesUnordered};
use itertools::Itertools;
use testing_tools::type_generator::orders::UserOrderBuilder;
use uniswap_v4::uniswap::{pool::EnhancedUniswapPool, pool_data_loader::DataLoader};

use crate::{
    SEPOLIA_SIG_CHECK, approveCall,
    cli::{BundleLander, JsonPKs},
    isValidSignatureNowCall
};

pub type ProviderType = FillProvider<
    JoinFill<
        Identity,
        JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>
    >,
    RootProvider
>;

pub struct BundleWashTraderEnv {
    pub keys:     Vec<AngstromSigner>,
    pub provider: Arc<ProviderType>,
    pub pools:    Vec<EnhancedUniswapPool>
}

impl BundleWashTraderEnv {
    pub async fn init(cli: &BundleLander, keys: JsonPKs) -> eyre::Result<Self> {
        let provider = Arc::new(
            ProviderBuilder::<_, _, _>::default()
                .with_recommended_fillers()
                // backup
                .connect(cli.node_endpoint.as_str())
                .await
                .unwrap()
        );

        let block = provider.get_block_number().await.unwrap();

        let pools =
            fetch_angstrom_pools(7838402, block as usize, cli.angstrom_address, &provider).await;

        let uniswap_registry: UniswapPoolRegistry = pools.into();

        let mut ang_pools = Vec::new();

        for pool_priv_key in uniswap_registry.private_keys() {
            let data_loader = DataLoader::new_with_registry(
                pool_priv_key,
                uniswap_registry.clone(),
                cli.pool_manager_address
            );
            let mut pool = EnhancedUniswapPool::new(data_loader, 400);
            pool.initialize(None, provider.root().into()).await?;
            tracing::info!("{:#?}", pool);
            ang_pools.push(pool);
        }

        let keys = keys
            .keys
            .iter()
            .map(|key| AngstromSigner::new(key.parse().unwrap()))
            .collect::<Vec<_>>();

        let all_tokens = ang_pools
            .iter()
            .flat_map(|p| [p.token0, p.token1])
            .unique()
            .collect::<Vec<_>>();

        // Self::verify_proper_signing_angstrom(&keys, provider.clone()).await?;
        Self::approve_max_tokens_to_angstrom(cli.angstrom_address, all_tokens, &keys, &provider)
            .await?;

        Ok(Self { keys, provider, pools: ang_pools })
    }

    async fn verify_proper_signing_angstrom(
        users: &[AngstromSigner],
        provider: Arc<ProviderType>
    ) -> eyre::Result<()> {
        let basic_order = UserOrderBuilder::default().bid().exact().amount(100);
        let fees = provider.estimate_eip1559_fees().await.unwrap();
        let chain_id = provider.get_chain_id().await.unwrap();

        for user in users {
            let order = basic_order.clone().signing_key(Some(user.clone())).build();
            let GroupedVanillaOrder::KillOrFill(
                angstrom_types::sol_bindings::grouped_orders::FlashVariants::Exact(order)
            ) = order
            else {
                panic!()
            };

            let signature = order.meta.signature.clone();
            let signer = user.address();
            let digest = order.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);

            let call = isValidSignatureNowCall::new((signer, digest, signature)).abi_encode();

            let mut tx = TransactionRequest::default()
                .with_from(user.address())
                .with_kind(alloy_primitives::TxKind::Call(SEPOLIA_SIG_CHECK))
                .with_input(call);

            let next_nonce = provider
                .get_transaction_count(user.address())
                .await
                .unwrap();

            tx.set_nonce(next_nonce);
            tx.set_gas_limit(30_000_000);
            tx.set_max_fee_per_gas(fees.max_fee_per_gas + 1e9 as u128);
            tx.set_max_priority_fee_per_gas(fees.max_priority_fee_per_gas + 1e9 as u128);

            tx.set_chain_id(chain_id);
            let res = provider.call(tx).await.unwrap();
            assert!(
                isValidSignatureNowCall::abi_decode_returns(&res, true)
                    .unwrap()
                    ._0
            );
        }

        Ok(())
    }

    #[allow(unused)]
    async fn approve_max_tokens_to_angstrom(
        angstrom: Address,
        tokens: Vec<Address>,
        users: &[AngstromSigner],
        provider: &ProviderType
    ) -> eyre::Result<()> {
        return Ok(());
        let mut nonce_offset = 0;
        let mut pending_orders = FuturesUnordered::new();

        let fees = provider.estimate_eip1559_fees().await.unwrap();
        let chain_id = provider.get_chain_id().await.unwrap();

        for token in tokens {
            for user in users {
                let call: Bytes = approveCall::new((angstrom, U256::MAX)).abi_encode().into();

                let mut tx = TransactionRequest::default()
                    .with_from(user.address())
                    .with_kind(alloy_primitives::TxKind::Call(token))
                    .with_input(call);

                let next_nonce = provider
                    .get_transaction_count(user.address())
                    .await
                    .unwrap();

                tx.set_nonce(next_nonce + nonce_offset);
                tx.set_gas_limit(30_000_000);
                tx.set_max_fee_per_gas(fees.max_fee_per_gas + 1e9 as u128);
                tx.set_max_priority_fee_per_gas(fees.max_priority_fee_per_gas + 1e9 as u128);

                tx.set_chain_id(chain_id);
                let signed_tx = tx.build(user).await.unwrap();

                let encoded_tx = signed_tx.encoded_2718();
                pending_orders.push(
                    provider
                        .send_raw_transaction(&encoded_tx)
                        .await
                        .unwrap()
                        .watch()
                );
            }
            // with a new token, the nonce will increase
            nonce_offset += 1;
        }
        tracing::info!("placed approval txes");

        // wait and unwrap all orders.
        while let Some(next) = pending_orders.next().await {
            tracing::info!("got tx");
            next?;
        }

        Ok(())
    }
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
    let controller_address = address!("0x4De4326613020a00F5545074bC578C87761295c7");

    loop {
        let this_end_block = std::cmp::min(deploy_block + 99_999, end_block);

        if this_end_block == deploy_block {
            break;
        }

        tracing::info!(?deploy_block, ?this_end_block);
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
    tracing::info!(?logs);

    logs.into_iter()
        .fold(HashSet::new(), |mut set, log| {
            if let Ok(pool) = PoolConfigured::decode_log(&log.clone().into_inner(), true) {
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

            if let Ok(pool) = PoolRemoved::decode_log(&log.clone().into_inner(), true) {
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
