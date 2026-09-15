//! Ticket 32 — builder-produced bundles settled against unchanged Angstrom.
#![cfg(feature = "anvil")]

use std::collections::HashMap;

use alloy::{
    providers::{Provider, ext::AnvilApi},
    sol_types::SolValue
};
use alloy_primitives::{
    Address, FixedBytes, I256, U256,
    aliases::{I24, U24}
};
use angstrom_types::{
    contract_bindings::{
        angstrom::Angstrom::{AngstromInstance, PoolKey},
        controller_v_1::ControllerV1::ControllerV1Instance,
        mintable_mock_erc_20::MintableMockERC20,
        pool_gate::PoolGate::PoolGateInstance
    },
    contract_payloads::{
        angstrom::{AngstromBundle, TopOfBlockOrder},
        protocol_fees::DonationSplits
    },
    orders::{
        OrderFillState, OrderOutcome, PoolSolution,
        builders::{StoredOrderBuilder, ToBOrderBuilder, UserOrderBuilder}
    },
    primitive::{
        AngstromAddressBuilder, AngstromSigner, PoolId, Ray, SqrtPriceX96, get_quantities_at_price
    },
    sol_bindings::{
        grouped_orders::{AllOrders, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder as RpcTopOfBlockOrder
    },
    submission::{ChainSubmitter, TxFeatureInfo},
    traits::{BundleProcessing, TopOfBlockOrderRewardCalc},
    uni_structure::{BaselinePoolState, liquidity_base::BaselineLiquidity}
};
use itertools::izip;
use testing_tools::{
    contracts::{
        DebugTransaction,
        anvil::WalletProviderRpc,
        environment::{
            SpawnedAnvil, TestAnvilEnvironment,
            angstrom::AngstromEnv,
            uniswap::{TestUniswapEnv, UniswapEnv}
        }
    },
    providers::AnvilSubmissionProvider
};

const CHAIN: u64 = 1;
const TICK_SPACING: i32 = 10;
const BUNDLE_FEE: u32 = 2_000;
const LIQUIDITY: u128 = 1_000_000_000_000_000;
/// What the first ToB bid in a bundle pays in, in its pool's t1; each later
/// pool pays one multiple more, so the pools' t1 deltas are told apart.
const TOB_QUANTITY_IN: u128 = 1_000_000;
/// `poolRewards` lives at slot 7 and its `rewardGrowthOutside` array is
/// `REWARD_GROWTH_SIZE` words long, so `globalGrowth` sits just past it.
/// `contracts/src/periphery/AngstromView.sol:34`.
const POOL_REWARDS_SLOT: u64 = 7;
const REWARD_GROWTH_SIZE: u64 = 16_777_216;

struct Harness {
    env:        AngstromEnv<UniswapEnv<SpawnedAnvil>>,
    angstrom:   AngstromInstance<WalletProviderRpc>,
    controller: Address,
    /// A toggled node, which is who `execute` has to come from.
    node:       AngstromSigner<alloy::signers::local::PrivateKeySigner>
}

/// A configured, funded pool. Each settlement gets its own so that it is
/// always built from a snapshot that matches the chain.
struct Pool {
    t0:          Address,
    t1:          Address,
    /// The id Uniswap knows the pool by, which is what `poolRewards` is keyed
    /// on. Angstrom initializes with the dynamic-fee flag, not the bundle fee.
    uni_id:      PoolId,
    store_index: u16,
    /// The single liquidity range the pool was seeded with.
    range:       (i32, i32),
    snapshot:    BaselinePoolState
}

impl Harness {
    async fn new() -> eyre::Result<Self> {
        // `spawn_anvil` reads CHAIN_ID, so it has to be set before the node comes
        // up; the rest of the addresses are only known once they are deployed.
        AngstromAddressBuilder::default()
            .with_chain_id(CHAIN)
            .build()
            .try_init();

        let fork_url = std::env::var("ETH_WS_URL")
            .ok()
            .filter(|url| !url.is_empty())
            .unwrap_or_else(|| "https://ethereum-rpc.publicnode.com".to_string());
        let anvil = SpawnedAnvil::new_forked(&fork_url).await?;
        // Key 7 is the account `SpawnedAnvil` makes the controller, and the
        // controller is the node `AngstromEnv` toggles below.
        let node = AngstromSigner::new(anvil.anvil.keys()[7].clone().into());
        let controller = anvil.controller();
        let uniswap = UniswapEnv::new(anvil).await?;
        let env = AngstromEnv::new(uniswap, vec![controller]).await?;

        // Orders are signed against the deployment's own EIP-712 domain, so the
        // config has to name the Angstrom that was actually deployed.
        AngstromAddressBuilder::default()
            .with_chain_id(CHAIN)
            .with_angstrom_address(env.angstrom())
            .with_pool_manager(env.pool_manager())
            .with_position_manager(env.position_manager())
            .with_controller(env.controller_v1())
            .build()
            .try_init();

        let angstrom = AngstromInstance::new(env.angstrom(), env.provider().clone());
        Ok(Self { env, angstrom, controller, node })
    }

    /// Deploys `N` tokens, sorted so the lowest address is token0 of any pair
    /// it is put in.
    async fn deploy_tokens<const N: usize>(&self) -> eyre::Result<[Address; N]> {
        let mut tokens = [Address::ZERO; N];
        for token in &mut tokens {
            *token = *MintableMockERC20::deploy(self.provider()).await?.address();
        }
        tokens.sort();
        Ok(tokens)
    }

    /// Deploys a token pair and sets the pool up.
    async fn deploy_pool(&self, store_index: u16) -> eyre::Result<Pool> {
        let [t0, t1] = self.deploy_tokens().await?;
        self.setup_pool(t0, t1, store_index).await
    }

    /// Two pools that share token0 and nothing else, at `store_index` and the
    /// one after it.
    async fn deploy_pools_sharing_token0(&self, store_index: u16) -> eyre::Result<[Pool; 2]> {
        let [t0, t1, t1_b] = self.deploy_tokens().await?;
        Ok([
            self.setup_pool(t0, t1, store_index).await?,
            self.setup_pool(t0, t1_b, store_index + 1).await?
        ])
    }

    /// Configures and initializes the pool, and seeds it with one wide
    /// liquidity range around tick 0.
    async fn setup_pool(&self, t0: Address, t1: Address, store_index: u16) -> eyre::Result<Pool> {
        let controller_v1 =
            ControllerV1Instance::new(self.env.controller_v1(), self.provider().clone());
        let pool_gate = PoolGateInstance::new(self.env.pool_gate(), self.provider().clone());

        let start_tick = 0;
        let price = SqrtPriceX96::at_tick(start_tick)?;

        controller_v1
            .configurePool(t0, t1, TICK_SPACING as u16, U24::from(BUNDLE_FEE), U24::ZERO, U24::ZERO)
            .from(self.controller)
            .run_safe()
            .await?;
        self.angstrom
            .initializePool(t0, t1, U256::from(store_index), *price)
            .from(self.controller)
            .run_safe()
            .await?;
        pool_gate
            .tickSpacing(I24::unchecked_from(TICK_SPACING))
            .from(self.controller)
            .run_safe()
            .await?;
        // One wide range around the start tick, so a small swap never leaves it
        // and the node's view of liquidity matches the pool's.
        let range = (start_tick - TICK_SPACING * 1000, start_tick + TICK_SPACING * 1000);
        pool_gate
            .addLiquidity(
                t0,
                t1,
                I24::unchecked_from(range.0),
                I24::unchecked_from(range.1),
                U256::from(LIQUIDITY),
                FixedBytes::<32>::default()
            )
            .from(self.controller)
            .run_safe()
            .await?;

        let uni_id = PoolId::from(PoolKey {
            currency0:   t0,
            currency1:   t1,
            // `ANGSTROM_INIT_HOOK_FEE`, the dynamic-fee flag Angstrom initializes
            // the Uniswap pool with (`contracts/src/modules/UniConsumer.sol:9`).
            fee:         U24::from(0x800000),
            tickSpacing: I24::unchecked_from(TICK_SPACING),
            hooks:       self.env.angstrom()
        });

        let snapshot = BaselinePoolState::new(
            BaselineLiquidity::new(
                TICK_SPACING,
                start_tick,
                price,
                LIQUIDITY,
                HashMap::new(),
                HashMap::new()
            ),
            0,
            0
        );

        Ok(Pool { t0, t1, uni_id, store_index, range, snapshot })
    }

    fn provider(&self) -> &WalletProviderRpc {
        self.env.provider()
    }

    async fn erc20_balance(&self, token: Address, owner: Address) -> eyre::Result<U256> {
        Ok(MintableMockERC20::new(token, self.provider())
            .balanceOf(owner)
            .call()
            .await?)
    }

    async fn extsload(&self, slot: U256) -> eyre::Result<U256> {
        Ok(self.angstrom.extsload(slot).call().await?)
    }

    fn rewards_base(pool: &Pool) -> U256 {
        U256::from_be_bytes(
            alloy_primitives::keccak256((pool.uni_id, U256::from(POOL_REWARDS_SLOT)).abi_encode())
                .0
        )
    }

    async fn global_growth(&self, pool: &Pool) -> eyre::Result<U256> {
        self.extsload(Self::rewards_base(pool) + U256::from(REWARD_GROWTH_SIZE))
            .await
    }

    /// `poolRewards[id].rewardGrowthOutside[tick]` at each end of the pool's
    /// range - the per-tick half a `MultiTick` update moves and a `CurrentOnly`
    /// one does not.
    async fn range_tick_growth(&self, pool: &Pool) -> eyre::Result<(U256, U256)> {
        let base = Self::rewards_base(pool);
        Ok((
            self.extsload(base + U256::from(pool.range.0 as u32 & 0xff_ffff))
                .await?,
            self.extsload(base + U256::from(pool.range.1 as u32 & 0xff_ffff))
                .await?
        ))
    }

    async fn pool_state(&self, pool: &Pool) -> eyre::Result<PoolState> {
        Ok(PoolState {
            growth:          self.global_growth(pool).await?,
            tick_growth:     self.range_tick_growth(pool).await?,
            pool_manager_t1: self.erc20_balance(pool.t1, self.env.pool_manager()).await?,
            angstrom_t1:     self.erc20_balance(pool.t1, self.env.angstrom()).await?
        })
    }
}

/// The chain-side state of one pool, read either side of the tx.
struct PoolState {
    growth:          U256,
    tick_growth:     (U256, U256),
    pool_manager_t1: U256,
    angstrom_t1:     U256
}

/// Everything the bundle is checked against, read either side of the tx.
struct Settled {
    /// `save` for t0 out of the submitted bundle's own `Asset` array.
    encoded_save:  u128,
    /// How much t0 the Angstrom contract actually kept.
    balance_delta: u128,
    /// One entry per pool the bundle spanned, in the order they were given.
    pools:         Vec<PoolSettled>
}

struct PoolSettled {
    /// The LP allocation the bundle encoded for this pool, summed over its
    /// `RewardsUpdate`s.
    rewarded:          u128,
    /// LP budget handed to the allocators: the ToB share plus the book budget.
    lp_budget:         u128,
    user_protocol_fee: u128,
    tob_protocol_fee:  u128,
    /// What the searcher paid in, which is all the t1 the bundle moves net.
    tob_quantity_in:   u128,
    /// `save` for this pool's t1 out of the bundle's `Asset` array.
    t1_save:           u128,
    /// t1 balance deltas of the PoolManager and of Angstrom.
    pool_manager_t1:   I256,
    angstrom_t1:       I256,
    growth_delta:      U256,
    tick_growth_moved: bool
}

impl Harness {
    /// Builds one bundle over `pools` - for each, a ToB bid sized to its gross
    /// and a book that clears against itself at the post-ToB price - submits
    /// it, and reports what the chain did.
    async fn settle(
        &self,
        pools: &[(&Pool, u128)],
        splits: DonationSplits
    ) -> eyre::Result<Settled> {
        let target_block = self.provider().get_block_number().await? + 1;
        let t0 = pools[0].0.t0;
        assert!(pools.iter().all(|(pool, _)| pool.t0 == t0), "the pools must share token0");

        let mut orders = Vec::new();
        let mut solutions = Vec::new();
        let mut pool_map = HashMap::new();
        let mut fees = Vec::new();
        for (i, (pool, gross_tob)) in pools.iter().enumerate() {
            let quantity_in = TOB_QUANTITY_IN * (i as u128 + 1);
            let searcher = self.tob_bid(pool, quantity_in, *gross_tob, target_block)?;
            // The book prices at the end of the ToB swap, so it clears against
            // itself and leaves the AMM where the searcher left it.
            let (tob_vec, gross) = TopOfBlockOrder::calc_vec_and_reward(&searcher, &pool.snapshot)?;
            assert_eq!(gross, *gross_tob, "ToB order was not sized to the requested gross");
            let ucp = Ray::from(tob_vec.end_price);

            let ask = self.user_order(pool, false, 1_000_000, ucp, target_block);
            let (t1_out, _, ask_fee) =
                get_quantities_at_price(false, true, 1_000_000, 0, BUNDLE_FEE as u128, ucp);
            let bid = self.user_order(pool, true, t1_out, ucp, target_block);
            let (_, _, bid_fee) =
                get_quantities_at_price(true, true, t1_out, 0, BUNDLE_FEE as u128, ucp);

            let (lp_user_fees, user_protocol_fee) = splits.split_user(ask_fee + bid_fee);
            let (tob_lp_budget, tob_protocol_fee) = splits.split_tob(*gross_tob);
            fees.push((
                tob_lp_budget + lp_user_fees,
                user_protocol_fee,
                tob_protocol_fee,
                quantity_in
            ));

            solutions.push(PoolSolution {
                id: pool.uni_id,
                ucp,
                fee: BUNDLE_FEE,
                searcher: Some(searcher),
                limit: vec![filled(&ask), filled(&bid)],
                ..Default::default()
            });
            pool_map
                .insert(pool.uni_id, (pool.t0, pool.t1, pool.snapshot.clone(), pool.store_index));
            orders.extend([ask, bid]);
        }
        let bundle = AngstromBundle::for_gas_finalization(orders, solutions, &pool_map, splits)?;

        // The harness funds the orders the same way bundle validation does.
        for (token, slot, value) in bundle
            .fetch_needed_overrides(target_block)
            .into_slots_with_overrides(self.env.angstrom())
        {
            self.provider()
                .anvil_set_storage_at(token, slot.into(), value.into())
                .await?;
        }

        let mut before = Vec::new();
        for (pool, _) in pools {
            before.push(self.pool_state(pool).await?);
        }
        let balance_before = self.erc20_balance(t0, self.env.angstrom()).await?;

        let submitter = AnvilSubmissionProvider {
            provider:         self.provider().clone(),
            angstrom_address: self.env.angstrom()
        };
        let features = TxFeatureInfo {
            nonce: self
                .provider()
                .get_transaction_count(self.node.address())
                .await?,
            fees: self.provider().estimate_eip1559_fees().await?,
            chain_id: CHAIN,
            target_block,
            bundle_gas_used: Box::new(|_| Box::pin(async { Ok(30_000_000u64) })),
            cancel: angstrom_types::submission::CancellationToken::new()
        };
        let tx_hash = submitter
            .submit(&self.node, Some(&bundle), &features)
            .await?[0]
            .tx_hash
            .expect("no transaction was sent");

        // `AnvilSubmissionProvider` fires and forgets, so wait for the mine.
        let mut receipt = None;
        for _ in 0..50 {
            receipt = self.provider().get_transaction_receipt(tx_hash).await?;
            if receipt.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let receipt = receipt.expect("bundle transaction was never mined");

        // `Settlement._saveAndSettle` (`contracts/src/modules/Settlement.sol:82`)
        // computes `bundleDeltas.sub(addr, saving + settle)` and reverts
        // `BundlDeltaUnresolved(addr)` on any nonzero remainder. A successful
        // receipt therefore *is* the zero-unresolved-delta assertion; there is
        // no separate quantity to read back for it.
        assert!(receipt.status(), "bundle reverted: unresolved deltas or a rejected order");

        let balance_after = self.erc20_balance(t0, self.env.angstrom()).await?;
        let encoded_save = |token| {
            bundle
                .assets
                .iter()
                .find(|a| a.addr == token)
                .expect("token missing from the bundle")
                .save
        };
        let mut settled = Vec::new();
        for (
            (pool, _),
            (lp_budget, user_protocol_fee, tob_protocol_fee, tob_quantity_in),
            before
        ) in izip!(pools, fees, before)
        {
            let after = self.pool_state(pool).await?;
            settled.push(PoolSettled {
                rewarded: bundle
                    .pool_updates
                    .iter()
                    .filter(|u| bundle.pairs[u.pair_index as usize].store_index == pool.store_index)
                    .flat_map(|u| u.rewards_update.quantities())
                    .sum(),
                lp_budget,
                user_protocol_fee,
                tob_protocol_fee,
                tob_quantity_in,
                t1_save: encoded_save(pool.t1),
                pool_manager_t1: I256::from_raw(after.pool_manager_t1)
                    - I256::from_raw(before.pool_manager_t1),
                angstrom_t1: I256::from_raw(after.angstrom_t1) - I256::from_raw(before.angstrom_t1),
                growth_delta: after.growth - before.growth,
                tick_growth_moved: after.tick_growth != before.tick_growth
            });
        }
        Ok(Settled {
            encoded_save:  encoded_save(t0),
            balance_delta: (balance_after - balance_before).to::<u128>(),
            pools:         settled
        })
    }

    /// A ToB bid paying `quantity_in` of T1 for T0, sized so the surplus the
    /// AMM leaves over `quantity_out` is exactly `gross`.
    fn tob_bid(
        &self,
        pool: &Pool,
        quantity_in: u128,
        gross: u128,
        block: u64
    ) -> eyre::Result<OrderWithStorageData<RpcTopOfBlockOrder>> {
        let out = pool
            .snapshot
            .swap_current_with_amount(I256::unchecked_from(quantity_in), false)?
            .total_d_t0;
        let order = ToBOrderBuilder::new()
            .asset_in(pool.t1)
            .asset_out(pool.t0)
            .quantity_in(quantity_in)
            .quantity_out(out - gross)
            .valid_block(block)
            .signing_key(Some(AngstromSigner::random()))
            .build();
        StoredOrderBuilder::new(AllOrders::TOB(order.clone()))
            .pool_id(pool.uni_id)
            .bid()
            .build()
            .try_map_inner(|_| Ok(order.clone()))
    }

    fn user_order(
        &self,
        pool: &Pool,
        is_bid: bool,
        amount: u128,
        price: Ray,
        block: u64
    ) -> OrderWithStorageData<AllOrders> {
        let (asset_in, asset_out) = if is_bid { (pool.t1, pool.t0) } else { (pool.t0, pool.t1) };
        // The clearing price is worse than the raw UCP once the bundle fee is
        // taken, so the limits have to leave room or the contract rejects the
        // fill with `LimitViolated`.
        let builder = UserOrderBuilder::new()
            .exact()
            .kill_or_fill()
            .asset_in(asset_in)
            .asset_out(asset_out)
            .amount(amount)
            .exact_in(true)
            .block(block);
        let builder = if is_bid {
            builder.bid_min_price(Ray::from(*price * U256::from(2)))
        } else {
            builder.min_price(Ray::from(*price / U256::from(2)))
        };
        builder
            .signing_key(Some(AngstromSigner::random()))
            .with_storage()
            .pool_id(pool.uni_id)
            .is_bid(is_bid)
            .build()
    }
}

fn filled(order: &OrderWithStorageData<AllOrders>) -> OrderOutcome {
    OrderOutcome { id: order.order_id, outcome: OrderFillState::CompleteFill }
}

/// The per-pool half of every scenario: the searcher's t1 is split between
/// the PoolManager and Angstrom and nowhere else, Angstrom keeping exactly
/// the t1 `save` the bundle encoded, and the pool's reward growth is its own
/// `RewardsUpdate`.
fn assert_pool_settled(label: &str, pool: &PoolSettled) {
    // The book nets to zero in t1. It is priced at the ToB end price after a
    // `Ray` round-trip, so the net swap the bundle encodes comes up a unit
    // short of `quantity_in`; that unit is what `collect_extra` sweeps into
    // t1's `save`.
    assert_eq!(
        pool.pool_manager_t1 + pool.angstrom_t1,
        I256::unchecked_from(pool.tob_quantity_in),
        "{label}: the searcher's t1 did not all reach the PoolManager or Angstrom"
    );
    assert_eq!(
        pool.angstrom_t1,
        I256::unchecked_from(pool.t1_save),
        "{label}: Angstrom did not keep exactly the t1 the bundle saves"
    );

    // Reward growth: `PoolUpdates._updatePool` adds `amount * 2^128 /
    // liquidity` to `globalGrowth` for a `CurrentOnly` update, and leaves
    // the per-tick growth alone.
    assert_eq!(
        pool.growth_delta,
        U256::from(pool.rewarded) * (U256::from(1u8) << 128) / U256::from(LIQUIDITY),
        "{label}: reward growth does not match the bundle's RewardsUpdate"
    );
    assert!(!pool.tick_growth_moved, "{label}: a CurrentOnly update moved per-tick growth");
}

/// The fourth acceptance criterion: a bundle the builder produced, settled by
/// unchanged Angstrom. Run at the rates the contract is deployed with and at a
/// nonzero ToB share, so the ToB fee path is exercised before rollout step 5
/// turns it on for real; then once more as a single bundle over two pools
/// that share token0.
#[tokio::test]
async fn builder_bundles_settle_against_unchanged_angstrom() {
    let harness = Harness::new().await.unwrap();
    // A pool per scenario, so every bundle is built from a snapshot that still
    // matches the chain. All are deployed before the first bundle: submission
    // signs with an explicit nonce, which leaves the provider's nonce filler
    // behind and breaks any later deploy from the same account.
    let pools = [harness.deploy_pool(0).await.unwrap(), harness.deploy_pool(1).await.unwrap()];
    let shared = harness.deploy_pools_sharing_token0(2).await.unwrap();

    for (pool, (label, splits)) in pools.iter().zip([
        ("deployed rates", DonationSplits::new(750_000, 1_000_000).unwrap()),
        ("nonzero tob share", DonationSplits::new(750_000, 750_000).unwrap())
    ]) {
        let settled = harness.settle(&[(pool, 1_001)], splits).await.unwrap();
        let p = &settled.pools[0];

        // Exact `save`: the configured fees plus whatever the allocators could
        // not place, which `collect_extra` sweeps into `save` rather than
        // `save_amount`.
        let residual = p.lp_budget - p.rewarded;
        assert_eq!(
            settled.encoded_save,
            p.user_protocol_fee + p.tob_protocol_fee + residual,
            "{label}: encoded save is not the configured fees plus the swept residual"
        );

        // Nothing on chain accumulates `save` - `pullFee`
        // (`contracts/src/modules/TopLevelAuth.sol:180`) transfers straight out
        // of the raw ERC20 balance - so the second half of "exact save" is what
        // the contract's balance actually did. It keeps the LP rewards too,
        // unclaimed, so the retained total is `save` plus what was donated.
        assert_eq!(
            settled.balance_delta,
            settled.encoded_save + p.rewarded,
            "{label}: the contract did not retain exactly save plus the donation"
        );
        assert_pool_settled(label, p);

        // The second run is the one that makes the ToB fee path non-inert.
        if label == "nonzero tob share" {
            assert!(p.tob_protocol_fee > 0, "the ToB fee path was not exercised");
        }
    }

    // One bundle over two pools sharing token0. The grosses are the unit
    // test's: at a 75% ToB share the per-pool fees (251 + 501) and a single
    // split of the aggregate (751) differ by a unit, so `save` accumulating
    // per pool is observable on chain.
    let label = "two pools sharing token0";
    let splits = DonationSplits::new(750_000, 750_000).unwrap();
    let (gross_a, gross_b) = (1_001, 2_002);
    let fee_a = splits.split_tob(gross_a).1;
    let fee_b = splits.split_tob(gross_b).1;
    let aggregated = splits.split_tob(gross_a + gross_b).1;
    assert_ne!(
        fee_a + fee_b,
        aggregated,
        "the grosses must be chosen so per-pool and aggregated splits differ"
    );

    let [a, b] = &shared;
    let settled = harness
        .settle(&[(a, gross_a), (b, gross_b)], splits)
        .await
        .unwrap();
    let user_fees: u128 = settled.pools.iter().map(|p| p.user_protocol_fee).sum();
    let residual: u128 = settled.pools.iter().map(|p| p.lp_budget - p.rewarded).sum();
    let rewarded: u128 = settled.pools.iter().map(|p| p.rewarded).sum();

    // `save` on t0 is the checked sum of each pool's own fee, plus the user
    // fees and the residuals each pool's allocators left to sweep.
    assert_eq!(
        settled.encoded_save,
        fee_a + fee_b + user_fees + residual,
        "{label}: save on t0 is not the sum of the two pools' own fees"
    );
    assert_ne!(
        settled.encoded_save,
        aggregated + user_fees + residual,
        "{label}: save on t0 is a single split of the aggregate gross"
    );
    assert_eq!(
        settled.balance_delta,
        settled.encoded_save + rewarded,
        "{label}: the contract did not retain exactly save plus both donations"
    );

    // Each pool's t1 and reward growth are its own. The pools reward different
    // amounts, so growth following the wrong pool's update would show.
    assert_ne!(settled.pools[0].rewarded, settled.pools[1].rewarded);
    for (pool, name) in settled.pools.iter().zip(["pool A", "pool B"]) {
        assert_pool_settled(&format!("{label}, {name}"), pool);
    }
}
