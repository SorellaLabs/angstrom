use alloy_primitives::{Address, TxHash};
use angstrom_types::contract_bindings::{
    angstrom::Angstrom::AngstromInstance, controller_v_1::ControllerV1,
    pool_gate::PoolGate::PoolGateInstance, position_fetcher::PositionFetcher
};
use tracing::{debug, info};

use super::{TestAnvilEnvironment, uniswap::TestUniswapEnv};
use crate::contracts::{DebugTransaction, deploy::angstrom::deploy_angstrom_create3};

pub trait TestAngstromEnv: TestAnvilEnvironment + TestUniswapEnv {
    fn angstrom(&self) -> Address;
}

#[derive(Clone)]
pub struct AngstromEnv<E: TestUniswapEnv> {
    #[allow(dead_code)]
    inner:            E,
    angstrom:         Address,
    controller_v1:    Address,
    position_fetcher: Address
}

impl<E> AngstromEnv<E>
where
    E: TestUniswapEnv
{
    pub async fn new(inner: E, nodes: Vec<Address>) -> eyre::Result<Self> {
        let angstrom = Self::deploy_angstrom(&inner, nodes).await?;
        let controller_v1 = Self::deploy_controller_v1(&inner, angstrom).await?;
        let position_fetcher = Self::deploy_position_fetcher(&inner, angstrom).await?;

        info!("Environment deploy complete!");

        Ok(Self { inner, angstrom, controller_v1, position_fetcher })
    }

    async fn deploy_angstrom(inner: &E, nodes: Vec<Address>) -> eyre::Result<Address> {
        let provider = inner.provider();
        debug!("Deploying Angstrom...");

        let angstrom_addr = inner
            .execute_then_mine(deploy_angstrom_create3(
                provider,
                inner.pool_manager(),
                inner.controller()
            ))
            .await?;
        debug!("Angstrom deployed at: {}", angstrom_addr);

        // gotta toggle nodes
        let ang_i = AngstromInstance::new(angstrom_addr, &provider);
        let _ = inner
            .execute_then_mine(ang_i.toggleNodes(nodes).from(inner.controller()).run_safe())
            .await?;

        Ok(angstrom_addr)
    }

    async fn deploy_controller_v1(inner: &E, angstrom_addr: Address) -> eyre::Result<Address> {
        debug!("Deploying ControllerV1...");
        let controller_v1_addr = *inner
            .execute_then_mine(ControllerV1::deploy(
                inner.provider(),
                angstrom_addr,
                inner.controller()
            ))
            .await?
            .address();
        debug!("ControllerV1 deployed at: {}", controller_v1_addr);

        let angstrom = AngstromInstance::new(angstrom_addr, inner.provider());
        let _ = inner
            .execute_then_mine(
                angstrom
                    .setController(controller_v1_addr)
                    .from(inner.controller())
                    .run_safe()
            )
            .await?;

        // Set the PoolGate's hook to be our Mock
        debug!("Setting PoolGate hook...");
        let pool_gate_instance = PoolGateInstance::new(inner.pool_gate(), inner.provider());
        inner
            .execute_then_mine(
                pool_gate_instance
                    .setHook(angstrom_addr)
                    .from(inner.controller())
                    .run_safe()
            )
            .await?;

        Ok(controller_v1_addr)
    }

    async fn deploy_position_fetcher(inner: &E, angstrom: Address) -> eyre::Result<Address> {
        debug!("Deploying PositionFetcher...");
        let position_fetcher_addr = *inner
            .execute_then_mine(PositionFetcher::deploy(
                inner.provider(),
                inner.position_manager(),
                angstrom
            ))
            .await?
            .address();

        debug!("PositionFetcher deployed at: {}", position_fetcher_addr);

        Ok(position_fetcher_addr)
    }

    pub fn angstrom(&self) -> Address {
        self.angstrom
    }

    pub fn controller_v1(&self) -> Address {
        self.controller_v1
    }

    pub fn position_fetcher(&self) -> Address {
        self.position_fetcher
    }
}

impl<E> TestUniswapEnv for AngstromEnv<E>
where
    E: TestUniswapEnv
{
    fn pool_manager(&self) -> Address {
        self.inner.pool_manager()
    }

    fn pool_gate(&self) -> Address {
        self.inner.pool_gate()
    }

    fn position_manager(&self) -> Address {
        self.inner.position_manager()
    }

    async fn add_liquidity_position(
        &self,
        asset0: Address,
        asset1: Address,
        lower_tick: alloy_primitives::aliases::I24,
        upper_tick: alloy_primitives::aliases::I24,
        liquidity: alloy_primitives::U256
    ) -> eyre::Result<TxHash> {
        self.inner
            .add_liquidity_position(asset0, asset1, lower_tick, upper_tick, liquidity)
            .await
    }
}

impl<E> TestAnvilEnvironment for AngstromEnv<E>
where
    E: TestUniswapEnv
{
    type P = E::P;

    fn provider(&self) -> &Self::P {
        self.inner.provider()
    }

    fn controller(&self) -> Address {
        self.inner.controller()
    }
}

#[cfg(all(test, feature = "reth-db-dep-tests"))]
mod tests {
    use std::{
        collections::HashMap,
        time::{Duration, SystemTime, UNIX_EPOCH}
    };

    use alloy::{
        primitives::{
            Address, Bytes, U256, Uint,
            aliases::{I24, U24}
        },
        providers::Provider,
        signers::{
            SignerSync,
            local::{LocalSigner, PrivateKeySigner}
        }
    };
    use alloy_primitives::FixedBytes;
    use angstrom_types::{
        block_sync::GlobalBlockSync,
        contract_bindings::{
            angstrom::Angstrom::{AngstromInstance, PoolKey},
            mintable_mock_erc_20::MintableMockERC20,
            pool_gate::PoolGate::PoolGateInstance
        },
        contract_payloads::angstrom::{AngstromBundle, BundleGasDetails, UserOrder},
        matching::{SqrtPriceX96, uniswap::LiqRange},
        orders::{OrderFillState, OrderOutcome},
        primitive::{ANGSTROM_DOMAIN, AngstromSigner},
        sol_bindings::{
            grouped_orders::{GroupedVanillaOrder, OrderWithStorageData, StandingVariants},
            rpc_orders::OmitOrderMeta
        }
    };
    use pade::PadeEncode;

    use super::{AngstromEnv, DebugTransaction};
    use crate::{
        contracts::environment::{
            LocalAnvil, SpawnedAnvil, TestAnvilEnvironment,
            uniswap::{TestUniswapEnv, UniswapEnv}
        },
        providers::AnvilProvider,
        type_generator::{
            amm::AMMSnapshotBuilder,
            consensus::{pool::Pool, proposal::ProposalBuilder}
        }
    };

    #[tokio::test]
    async fn can_be_constructed() {
        let anvil = AnvilProvider::spawn_new_isolated(GlobalBlockSync::new(0))
            .await
            .unwrap();
        let uniswap = UniswapEnv::new(anvil.wallet_provider()).await.unwrap();
        AngstromEnv::new(uniswap, vec![]).await.unwrap();
    }

    #[test]
    fn do_a_thing() {
        let user = LocalSigner::random();
        let address = user.address();
        let mut default = angstrom_types::sol_bindings::rpc_orders::ExactStandingOrder {
            ref_id:               0,
            max_extra_fee_asset0: 0,
            exact_in:             true,
            amount:               10,
            min_price:            U256::from(1u128),
            use_internal:         false,
            asset_in:             Address::random(),
            asset_out:            Address::random(),
            recipient:            Address::random(),
            hook_data:            alloy::primitives::Bytes::new(),
            nonce:                0,
            deadline:             Uint::<40, 1>::from_be_slice(
                &(SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
                    + Duration::from_secs(1000))
                .as_secs()
                .to_be_bytes()[3..]
            ),
            meta:                 Default::default()
        };
        let hash = default.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
        let sig = user.sign_hash_sync(&hash).unwrap();
        default.meta.isEcdsa = true;
        default.meta.from = address;
        default.meta.signature = sig.pade_encode().into();

        let user_order = OrderWithStorageData {
            order: GroupedVanillaOrder::Standing(StandingVariants::Exact(default)),
            is_currently_valid: None,
            is_bid: true,
            ..Default::default()
        };
        let outcome =
            OrderOutcome { id: user_order.order_id, outcome: OrderFillState::CompleteFill };
        let _encode =
            UserOrder::from_internal_order_max_gas(&user_order, &outcome, 0).pade_encode();
    }

    #[tokio::test]
    async fn accepts_payload() {
        // Create our anvil environment and grab the nodes and provider
        let anvil = LocalAnvil::new("http://localhost:8545".to_owned())
            .await
            .unwrap();
        // Some tricks since they're the same
        let spawned_anvil = SpawnedAnvil::new().await.unwrap();

        let nodes: Vec<Address> = spawned_anvil.anvil.addresses().to_vec();
        let controller = nodes[7];

        let controller_signing_key = AngstromSigner::new(
            PrivateKeySigner::from_slice(&spawned_anvil.anvil.keys()[7].clone().to_bytes())
                .unwrap()
        );

        let uniswap = UniswapEnv::new(anvil).await.unwrap();
        let env = AngstromEnv::new(uniswap, vec![]).await.unwrap();
        let angstrom = AngstromInstance::new(env.angstrom(), env.provider());
        println!("Angstrom: {}", angstrom.address());
        println!("Controller: {}", controller);
        println!("Uniswap: {}", env.pool_manager());
        println!("PoolGate: {}", env.pool_gate());

        let pool_gate = PoolGateInstance::new(env.pool_gate(), env.provider());
        let raw_c0 = MintableMockERC20::deploy(env.provider()).await.unwrap();

        let raw_c1 = MintableMockERC20::deploy(env.provider()).await.unwrap();
        let (currency0, currency1) = match raw_c0.address().cmp(raw_c1.address()) {
            std::cmp::Ordering::Greater => (*raw_c1.address(), *raw_c0.address()),
            _ => (*raw_c0.address(), *raw_c1.address())
        };
        // Setup our pool
        let pool = PoolKey {
            currency0,
            currency1,
            fee: U24::ZERO,
            tickSpacing: I24::unchecked_from(10),
            hooks: Address::default()
        };
        let liquidity = 1_000_000_000_000_000_u128;
        let price = SqrtPriceX96::at_tick(100000).unwrap();
        let amm = AMMSnapshotBuilder::new(price)
            .with_positions(vec![LiqRange::new(99000, 101000, liquidity).unwrap()])
            .build();
        // Configure our pool that we just made
        angstrom
            .configurePool(pool.currency0, pool.currency1, 10, U24::ZERO, U24::ZERO)
            .from(controller)
            .run_safe()
            .await
            .unwrap();
        angstrom
            .initializePool(pool.currency0, pool.currency1, U256::ZERO, *price)
            .run_safe()
            .await
            .unwrap();
        let salt: FixedBytes<32> = FixedBytes::default();
        pool_gate
            .addLiquidity(
                pool.currency0,
                pool.currency1,
                I24::unchecked_from(99000),
                I24::unchecked_from(101000),
                U256::from(liquidity),
                salt
            )
            .from(controller)
            .run_safe()
            .await
            .unwrap();

        // Get our ToB address and money it up
        // let tob_address = Address::random();
        // println!("TOB address: {:?}", tob_address);
        raw_c0
            .mint(env.angstrom(), U256::from(1_000_000_000_000_000_000_u128))
            .run_safe()
            .await
            .unwrap();
        raw_c1
            .mint(controller, U256::from(1_000_000_000_000_000_000_u128))
            .run_safe()
            .await
            .unwrap();
        raw_c1
            .approve(env.angstrom(), U256::from(2201872310000_u128))
            .from(controller)
            .run_safe()
            .await
            .unwrap();
        let pool = Pool::new(pool, amm.clone(), controller);
        let pools = vec![pool.clone()];
        let current_block = env.provider().get_block_number().await.unwrap();
        let proposal = ProposalBuilder::new()
            .for_pools(pools)
            .order_count(10)
            .preproposal_count(1)
            .with_secret_key(controller_signing_key)
            .for_block(current_block + 2)
            .build();
        println!("Proposal solutions:\n{:?}", proposal.solutions);
        let pools = HashMap::from([(pool.id(), (pool.token0(), pool.token1(), amm, 0))]);
        let bundle =
            AngstromBundle::from_proposal(&proposal, BundleGasDetails::default(), &pools).unwrap();
        println!("Bundle: {:?}", bundle);
        let encoded = bundle.pade_encode();

        angstrom.toggleNodes(nodes).run_safe().await.unwrap();
        angstrom
            .execute(Bytes::from(encoded))
            .from(controller)
            .run_safe()
            .await
            .unwrap();
        // angstrom.execute(encoded)
    }
}

/*

initial pool there are


*/
