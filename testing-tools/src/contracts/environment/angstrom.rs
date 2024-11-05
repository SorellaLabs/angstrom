use alloy::primitives::Address;
use angstrom_types::contract_bindings::pool_gate::PoolGate::PoolGateInstance;
use tracing::debug;

use super::{uniswap::TestUniswapEnv, TestAnvilEnvironment};
use crate::contracts::{deploy::angstrom::deploy_angstrom, DebugTransaction};

pub trait TestAngstromEnv: TestAnvilEnvironment + TestUniswapEnv {
    fn angstrom(&self) -> Address;
}

pub struct AngstromEnv<E: TestUniswapEnv> {
    #[allow(dead_code)]
    inner:    E,
    angstrom: Address
}

impl<E> AngstromEnv<E>
where
    E: TestUniswapEnv
{
    pub async fn new(inner: E) -> eyre::Result<Self> {
        let provider = inner.provider();
        debug!("Deploying mock rewards manager...");
        let angstrom = deploy_angstrom(
            &provider,
            inner.pool_manager(),
            inner.controller(),
            Address::default()
        )
        .await;
        debug!("Angstrom deployed at: {}", angstrom);
        // Set the PoolGate's hook to be our Mock
        debug!("Setting PoolGate hook...");
        let pool_gate_instance = PoolGateInstance::new(inner.pool_gate(), &provider);
        pool_gate_instance
            .setHook(angstrom)
            .from(inner.controller())
            .run_safe()
            .await?;
        debug!("Environment deploy complete!");
        Ok(Self { inner, angstrom })
    }

    pub fn angstrom(&self) -> Address {
        self.angstrom
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

    async fn add_liquidity_position(
        &self,
        asset0: Address,
        asset1: Address,
        lower_tick: alloy_primitives::aliases::I24,
        upper_tick: alloy_primitives::aliases::I24,
        liquidity: alloy_primitives::U256
    ) -> eyre::Result<()> {
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
    type T = E::T;

    fn provider(&self) -> &Self::P {
        self.inner.provider()
    }

    fn controller(&self) -> Address {
        self.inner.controller()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        time::{Duration, SystemTime, UNIX_EPOCH}
    };

    use alloy::{
        primitives::{
            aliases::{I24, U24},
            Address, Bytes, Uint, U256
        },
        providers::Provider,
        signers::{local::LocalSigner, SignerSync}
    };
    use alloy_primitives::FixedBytes;
    use alloy_sol_types::{eip712_domain, Eip712Domain};
    use angstrom_types::{
        contract_bindings::{
            angstrom::Angstrom::{AngstromInstance, PoolKey},
            mintable_mock_erc_20::MintableMockERC20,
            pool_gate::PoolGate::PoolGateInstance
        },
        contract_payloads::angstrom::{AngstromBundle, UserOrder},
        matching::{uniswap::LiqRange, SqrtPriceX96},
        orders::{OrderFillState, OrderOutcome},
        primitive::ANGSTROM_DOMAIN,
        sol_bindings::{
            grouped_orders::{GroupedVanillaOrder, OrderWithStorageData, StandingVariants},
            rpc_orders::OmitOrderMeta
        }
    };
    use enr::k256::ecdsa::SigningKey;
    use pade::PadeEncode;

    use super::{AngstromEnv, DebugTransaction};
    use crate::{
        contracts::environment::{
            uniswap::{TestUniswapEnv, UniswapEnv},
            LocalAnvil, SpawnedAnvil, TestAnvilEnvironment
        },
        type_generator::{
            amm::AMMSnapshotBuilder,
            consensus::{pool::Pool, proposal::ProposalBuilder},
            orders::SigningInfo
        }
    };

    #[tokio::test]
    async fn can_be_constructed() {
        let anvil = SpawnedAnvil::new().await.unwrap();
        let uniswap = UniswapEnv::new(anvil).await.unwrap();
        AngstromEnv::new(uniswap).await.unwrap();
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
        // (address, default)

        let user_order = OrderWithStorageData {
            order: GroupedVanillaOrder::Standing(StandingVariants::Exact(default)),
            is_currently_valid: true,
            is_bid: true,
            ..Default::default()
        };
        let outcome =
            OrderOutcome { id: user_order.order_id, outcome: OrderFillState::CompleteFill };
        let _encode = UserOrder::from_internal_order(&user_order, &outcome, 0).pade_encode();
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
        let controller_signing_key: SigningKey = spawned_anvil.anvil.keys()[7].clone().into();
        let uniswap = UniswapEnv::new(anvil).await.unwrap();
        let env = AngstromEnv::new(uniswap).await.unwrap();
        let angstrom = AngstromInstance::new(env.angstrom(), env.provider());
        let angstrom_addr = env.angstrom;
        println!("Angstrom: {}", angstrom.address());
        println!("Controller: {}", controller);
        println!("Uniswap: {}", env.pool_manager());
        println!("PoolGate: {}", env.pool_gate());

        let domain: Eip712Domain = eip712_domain!(
           name: "Angstrom",
           version: "v1",
           chain_id: 1,
           verifying_contract: angstrom_addr,
        );
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
            .configurePool(pool.currency0, pool.currency1, 10, U24::ZERO)
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

        let order_key = SigningInfo { domain, address: controller, key: controller_signing_key };

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
            .order_key(Some(order_key))
            .for_block(current_block + 2)
            .build();
        println!("Proposal solutions:\n{:?}", proposal.solutions);
        let pools = HashMap::from([(pool.id(), (pool.token0(), pool.token1(), amm, 0))]);
        let bundle = AngstromBundle::from_proposal(&proposal, &pools).unwrap();
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
