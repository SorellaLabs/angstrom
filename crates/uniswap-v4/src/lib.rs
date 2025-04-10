use std::{collections::HashSet, pin::Pin, sync::Arc};

use alloy::{
    consensus::TxReceipt, primitives::aliases::I24, providers::Provider, sol_types::SolEvent
};
use alloy_primitives::{Address, BlockNumber, FixedBytes};
use angstrom_eth::manager::EthEvent;
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    contract_bindings::{
        angstrom::Angstrom::PoolKey,
        controller_v_1::ControllerV1::{PoolConfigured, PoolRemoved}
    },
    primitive::UniswapPoolRegistry
};
use futures::Stream;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use reth_provider::{
    CanonStateNotifications, DatabaseProviderFactory, ReceiptProvider, StateProvider,
    TryIntoHistoricalStateProvider
};
use uniswap::pool_factory::V4PoolFactory;

use crate::uniswap::{
    pool_data_loader::DataLoader, pool_manager::UniswapPoolManager,
    pool_providers::canonical_state_adapter::CanonicalStateAdapter
};

/// This module should have information on all the Constant Function Market
/// Makers that we work with.  Right now that's only Uniswap, but if there are
/// ever any others they will be added here
pub mod uniswap;

///  name           type    slot   offset  bytes    contract
/// _controller | address| 0    | 0      | 20    | src/Angstrom.sol:Angstrom |
/// We use this so that we are able to historically go back from the current
/// block of angstrom and fetch all the new pool events. We look at the angstrom
/// contract for controller address as the control address is upgradable. This
/// means that this will be secure in the case of new deployments
const CONTROLLER_ADDRESS_SLOT: FixedBytes<32> = FixedBytes::<32>::ZERO;

/// Goes from the deploy block to the current block fetching all pools.
pub async fn fetch_angstrom_pools<DB>(
    // the block angstrom was deployed at
    deploy_block: usize,
    end_block: usize,
    angstrom_address: Address,
    db: &DB
) -> Vec<PoolKey>
where
    DB: DatabaseProviderFactory + ReceiptProvider,
    <DB as DatabaseProviderFactory>::Provider: TryIntoHistoricalStateProvider
{
    let logs = (deploy_block..=end_block)
        .into_par_iter()
        .flat_map(|block| {
            let storage_provider = db
                .database_provider_ro()
                .unwrap()
                .try_into_history_at_block(block as u64)
                .unwrap();

            let controller_addr = Address::from_word(FixedBytes::new(
                storage_provider
                    .storage(angstrom_address, CONTROLLER_ADDRESS_SLOT)
                    .unwrap()
                    .unwrap()
                    .to_be_bytes::<32>()
            ));

            db.receipts_by_block((block as u64).into())
                .unwrap()
                .unwrap_or_default()
                .into_iter()
                .flat_map(|receipt| receipt.logs().to_vec())
                .filter(move |log| log.address == controller_addr)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    logs.into_iter()
        .fold(HashSet::new(), |mut set, log| {
            if let Ok(pool) = PoolConfigured::decode_log(&log, true) {
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

            if let Ok(pool) = PoolRemoved::decode_log(&log, true) {
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

pub async fn configure_uniswap_manager<BlockSync: BlockSyncConsumer>(
    provider: Arc<impl Provider + 'static>,
    state_notification: CanonStateNotifications,
    uniswap_pool_registry: UniswapPoolRegistry,
    current_block: BlockNumber,
    block_sync: BlockSync,
    pool_manager_address: Address,
    update_stream: Pin<Box<dyn Stream<Item = EthEvent> + Send + Sync>>
) -> UniswapPoolManager<
    CanonicalStateAdapter<impl Provider + 'static>,
    impl Provider + 'static,
    BlockSync
> {
    let factory = V4PoolFactory::new(provider.clone(), uniswap_pool_registry, pool_manager_address);

    let notifier =
        Arc::new(CanonicalStateAdapter::new(state_notification, provider.clone(), current_block));

    UniswapPoolManager::new(factory, current_block, notifier, block_sync, update_stream).await
}

// #[cfg(all(test, feature = "testnet-sepolia"))]
#[cfg(test)]
pub mod fuzz_uniswap {
    use std::{collections::HashSet, ops::Deref, sync::Arc};

    use alloy::{
        primitives::{
            Address, Bytes, U256, address,
            aliases::{I24, U24}
        },
        providers::{
            Identity, Provider, ProviderBuilder, RootProvider,
            fillers::{
                BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller
            }
        },
        rpc::types::{BlockNumberOrTag, Filter},
        sol_types::{SolCall, SolEvent, SolValue}
    };
    use alloy_primitives::keccak256;
    use angstrom_types::{
        CHAIN_ID,
        matching::uniswap::{PoolSnapshot, Quantity},
        primitive::{TESTNET_ANGSTROM_ADDRESS, TESTNET_POOL_MANAGER_ADDRESS},
        reth_db_wrapper::DBError
    };
    use futures::StreamExt;
    use rand::Rng;
    use revm::{
        Context, DatabaseRef, ExecuteEvm, Journal, MainBuilder,
        context::{BlockEnv, CfgEnv, TxEnv},
        database::CacheDB,
        primitives::{TxKind, hardfork::SpecId}
    };
    use revm_bytecode::Bytecode;
    use revm_primitives::I256;
    use validation::{find_slot_offset_for_approval, find_slot_offset_for_balance};

    use crate::{
        DataLoader, PoolConfigured, PoolKey, PoolRemoved, UniswapPoolRegistry,
        uniswap::pool::{EnhancedUniswapPool, U256_1}
    };

    pub type ProviderType = FillProvider<
        JoinFill<
            Identity,
            JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>
        >,
        RootProvider
    >;

    alloy::sol!(
        function executeExactInput(bytes calldata args) public;

        /// @dev equivalent to: abi.decode(params, (IV4Router.ExactInputSingleParams))
        struct ExactInputSingleParams {
            PoolKey poolKey;
            bool zeroForOne;
            uint128 amountIn;
            uint128 amountOutMinimum;
            bytes hookData;
        }

        /// @dev equivalent to: abi.decode(params, (IV4Router.ExactOutputSingleParams))
        struct ExactOutputSingleParams {
            PoolKey poolKey;
            bool zeroForOne;
            uint128 amountOut;
            uint128 amountInMaximum;
            bytes hookData;
        }

        type PoolId is bytes32;

        event Swap(
            PoolId indexed id,
            address indexed sender,
            int128 amount0,
            int128 amount1,
            uint160 sqrtPriceX96,
            uint128 liquidity,
            int24 tick,
            uint24 fee
        );
    );

    const HOOK_EXECUTOR: Address = address!("0xD7d2f2A4E117fAb1611accB599a0717959d83588");
    const LAST_BLOCK_SLOT_ANGSTROM: U256 = U256::from_limbs([3, 0, 0, 0]);
    const TEST_ORDER_ADDR: Address = address!("0x3193C77CD2c0cE208356Dc8B0F96159F8181a3f2");

    #[derive(Clone)]
    #[repr(transparent)]
    pub struct InnerProvider(Arc<ProviderType>);

    impl Deref for InnerProvider {
        type Target = ProviderType;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    // uint256 internal constant SWAP_EXACT_IN_SINGLE = 0x06;
    fn build_exact_in_swap_calldata(pool_key: PoolKey, zfo: bool, amount_in: u128) -> Bytes {
        let arg = ExactInputSingleParams {
            poolKey:          pool_key,
            zeroForOne:       zfo,
            amountIn:         amount_in,
            amountOutMinimum: 1,
            hookData:         Bytes::new()
        };

        let flag = U256::ZERO;
        let data = (flag, arg).abi_encode_params();

        executeExactInputCall::new((data.into(),))
            .abi_encode()
            .into()
    }

    // uint256 internal constant SWAP_EXACT_OUT_SINGLE = 0x08;
    fn build_exact_out_swap_calldata(pool_key: PoolKey, zfo: bool, amount_out: u128) -> Bytes {
        let arg = ExactOutputSingleParams {
            poolKey:         pool_key,
            zeroForOne:      zfo,
            amountOut:       amount_out,
            amountInMaximum: u128::MAX - 1,
            hookData:        Bytes::new()
        };
        let flag = U256_1;
        let data = (flag, arg).abi_encode_params();

        executeExactInputCall::new((data.into(),))
            .abi_encode()
            .into()
    }

    /// sets up revm with the angstrom hook override such that we can execute on
    /// the contract as if it was in a post bundle unlock.
    fn execute_calldata<DB: DatabaseRef>(
        target_block: u64,
        router_calldata: Bytes,
        mut db: CacheDB<Arc<DB>>
    ) -> Swap {
        // override the slot
        let slot = db
            .storage_ref(TESTNET_ANGSTROM_ADDRESS, LAST_BLOCK_SLOT_ANGSTROM)
            .unwrap();

        let mut bytes: [u8; 32] = slot.to_be_bytes();
        let target_bytes = target_block.to_be_bytes();
        bytes[24..].copy_from_slice(&target_bytes);

        db.insert_account_storage(
            TESTNET_ANGSTROM_ADDRESS,
            LAST_BLOCK_SLOT_ANGSTROM,
            U256::from_be_bytes(bytes)
        )
        .unwrap();

        // setup baseline context.
        let mut evm = Context {
            tx:              TxEnv::default(),
            block:           BlockEnv::default(),
            cfg:             CfgEnv::<SpecId>::default().with_chain_id(CHAIN_ID),
            journaled_state: Journal::<CacheDB<Arc<DB>>>::new(SpecId::LATEST, db.clone()),
            chain:           (),
            error:           Ok(())
        }
        .modify_cfg_chained(|cfg| {
            cfg.disable_nonce_check = true;
            cfg.disable_balance_check = true;
            cfg.chain_id = CHAIN_ID;
        })
        .modify_block_chained(|block| {
            block.number = target_block;
        })
        .modify_tx_chained(|tx| {
            tx.kind = TxKind::Call(HOOK_EXECUTOR);
            tx.chain_id = Some(CHAIN_ID);
            tx.caller = TEST_ORDER_ADDR;
            tx.data = router_calldata
        })
        .build_mainnet();
        let result = evm.replay().unwrap();
        if !result.result.is_success() {
            panic!(
                "replay failed {:?} gas: {} logs: {:#?}",
                result.result.output(),
                result.result.gas_used(),
                result.result.logs()
            );
        }

        result
            .result
            .into_logs()
            .into_iter()
            .filter_map(|log| Swap::decode_log(&log, true).ok())
            .collect::<Vec<_>>()[0]
            .clone()
            .data
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_fuzzing_of_uniswap() {
        let node_endpoint =
            std::env::var("NODE_URL").unwrap_or_else(|_| "https://1rpc.io/sepolia".to_string());
        let provider = InnerProvider(Arc::new(
            ProviderBuilder::<_, _, _>::default()
                .with_recommended_fillers()
                .connect(node_endpoint.as_str())
                .await
                .unwrap()
        ));
        println!("starting to look for uinswap pools");
        let database = Arc::new(provider);
        let mut cache_db = CacheDB::new(database.clone());

        let deref = &**database;
        let (block, pools) = init_uniswap_pools(deref, &mut cache_db).await;
        let target_block = block + 1;

        for _ in 0..10_000 {
            for pool in &pools {
                let snapshot = pool.fetch_pool_snapshot().unwrap().2;

                let pool_key = PoolKey {
                    currency0:   pool.token0,
                    currency1:   pool.token1,
                    fee:         U24::from(0x800000),
                    tickSpacing: I24::unchecked_from(pool.tick_spacing),
                    hooks:       TESTNET_ANGSTROM_ADDRESS
                };
                am_check_exact_in(
                    pool,
                    target_block,
                    cache_db.clone(),
                    snapshot.clone(),
                    pool_key.clone(),
                    pool.token0_decimals,
                    pool.token1_decimals
                );
                am_check_exact_out(
                    pool,
                    target_block,
                    cache_db.clone(),
                    snapshot,
                    pool_key,
                    pool.token0_decimals,
                    pool.token1_decimals
                );
            }
        }
    }

    fn am_check_exact_out<DB: DatabaseRef>(
        pool: &EnhancedUniswapPool,
        target_block: u64,
        db: CacheDB<Arc<DB>>,
        snap: PoolSnapshot,
        key: PoolKey,
        t0_dec: u8,
        t1_dec: u8
    ) {
        let mut rng = rand::thread_rng();

        let amount: u128 = rng.gen_range(1..100);
        let zfo: bool = rng.r#gen();
        let amount = if zfo {
            amount * 10u128.pow(t1_dec as u32 / 2)
        } else {
            amount * 10u128.pow(t0_dec as u32 / 2)
        };

        println!("trying to swap with amount {amount} {key:#?} {zfo}");
        let call_bytecode = build_exact_out_swap_calldata(key, zfo, amount);
        let revm_swap_output = execute_calldata(target_block, call_bytecode, db);

        let amount_i = if zfo { Quantity::Token1(amount) } else { Quantity::Token0(amount) };
        let (t0, t1) = pool
            .simulate_swap(
                if zfo { pool.token1 } else { pool.token0 },
                I256::unchecked_from(amount).wrapping_neg(),
                None
            )
            .unwrap();

        let t0 = t0.abs().into_raw().to::<u128>();
        let t1 = t1.abs().into_raw().to::<u128>();

        // local calculations
        let local_swap_output = (snap.current_price() - amount_i).unwrap();
        let t0_delta_local = local_swap_output.d_t0;
        let t1_delta_local = local_swap_output.d_t1;
        let sqrt_price_local = *local_swap_output.end_bound.as_sqrtpricex96();

        assert_eq!(t0, t0_delta_local, "t0 pool.sim_swap != poolsnap sim");
        assert_eq!(t1, t1_delta_local, "t1 pool.sim_swap != poolsnap sim");

        let t0_delta_revm = revm_swap_output.amount0.unsigned_abs();
        let t1_delta_revm = revm_swap_output.amount1.unsigned_abs();
        let sqrt_price_revm = revm_swap_output.sqrtPriceX96;

        assert_eq!(t0_delta_local, t0_delta_revm, "t0 failure amount: {amount} zfo: {zfo}");
        assert_eq!(t1_delta_local, t1_delta_revm, "t1 failure amount: {amount} zfo: {zfo}");
        assert_eq!(sqrt_price_local, sqrt_price_revm, "sqrtprice amount: {amount} zfo: {zfo}");
        println!(
            "{}-{} {}-{} {:?}-{:?}",
            t0_delta_local,
            t0_delta_revm,
            t1_delta_local,
            t1_delta_revm,
            sqrt_price_local,
            sqrt_price_revm
        );

        // let amount_out_rand = rng.
    }
    fn am_check_exact_in<DB: DatabaseRef>(
        pool: &EnhancedUniswapPool,
        target_block: u64,
        db: CacheDB<Arc<DB>>,
        snap: PoolSnapshot,
        key: PoolKey,
        t0_dec: u8,
        t1_dec: u8
    ) {
        let mut rng = rand::thread_rng();

        let amount: u128 = rng.gen_range(1..100);
        let zfo: bool = rng.r#gen();
        let amount = if zfo {
            amount * 10u128.pow(t0_dec as u32 / 2)
        } else {
            amount * 10u128.pow(t1_dec as u32 / 2)
        };
        println!("trying to swap with amount {amount} {key:#?} {zfo}");

        let call_bytecode = build_exact_in_swap_calldata(key, zfo, amount);
        let revm_swap_output = execute_calldata(target_block, call_bytecode, db);

        let amount_i = if zfo { Quantity::Token0(amount) } else { Quantity::Token1(amount) };

        // try different sim
        let (t0, t1) = pool
            .simulate_swap(
                if zfo { pool.token0 } else { pool.token1 },
                I256::unchecked_from(amount),
                None
            )
            .unwrap();

        let t0 = t0.abs().into_raw().to::<u128>();
        let t1 = t1.abs().into_raw().to::<u128>();

        // local calculations
        let local_swap_output = (snap.current_price() + amount_i).unwrap();
        let t0_delta_local = local_swap_output.d_t0;
        let t1_delta_local = local_swap_output.d_t1;

        assert_eq!(t0, t0_delta_local, "t0 pool.sim_swap != poolsnap sim");
        assert_eq!(t1, t1_delta_local, "t1 pool.sim_swap != poolsnap sim");

        let sqrt_price_local = *local_swap_output.end_bound.as_sqrtpricex96();

        let t0_delta_revm = revm_swap_output.amount0.unsigned_abs();
        let t1_delta_revm = revm_swap_output.amount1.unsigned_abs();
        let sqrt_price_revm = revm_swap_output.sqrtPriceX96;

        assert_eq!(t0_delta_local, t0_delta_revm, "t0 failure amount: {amount} zfo: {zfo}");
        assert_eq!(t1_delta_local, t1_delta_revm, "t1 failure amount: {amount} zfo: {zfo}");
        assert_eq!(sqrt_price_local, sqrt_price_revm, "sqrtprice amount: {amount} zfo: {zfo}");
        println!(
            "{}-{} {}-{} {:?}-{:?}",
            t0_delta_local,
            t0_delta_revm,
            t1_delta_local,
            t1_delta_revm,
            sqrt_price_local,
            sqrt_price_revm
        );
    }

    /// initializes the new uniswap pools on most recent sepolia block
    async fn init_uniswap_pools<P: Provider, DB: DatabaseRef>(
        provider: &P,
        db: &mut CacheDB<Arc<DB>>
    ) -> (u64, Vec<EnhancedUniswapPool>) {
        let block = provider.get_block_number().await.unwrap();

        let pools =
            fetch_angstrom_pools(7838402, block as usize, TESTNET_ANGSTROM_ADDRESS, &provider)
                .await;

        let uniswap_registry: UniswapPoolRegistry = pools.into();

        let mut ang_pools = Vec::new();
        let mut tokens = HashSet::new();

        for pool_priv_key in uniswap_registry.private_keys() {
            let data_loader = DataLoader::new_with_registry(
                pool_priv_key,
                uniswap_registry.clone(),
                TESTNET_POOL_MANAGER_ADDRESS
            );
            let mut pool = EnhancedUniswapPool::new(data_loader, 1000);
            pool.initialize(Some(block), provider.root().into())
                .await
                .unwrap();
            tokens.insert(pool.token0);
            tokens.insert(pool.token1);
            ang_pools.push(pool);
        }
        println!("{:#?}", ang_pools);

        for token in tokens {
            let ap_slot: u64 = find_slot_offset_for_approval(db, token).unwrap();
            let approval_location = keccak256(
                (HOOK_EXECUTOR, keccak256((TEST_ORDER_ADDR, ap_slot).abi_encode())).abi_encode()
            );
            db.insert_account_storage(
                token,
                U256::from_be_bytes(*approval_location),
                U256::from(u128::MAX)
            )
            .unwrap();

            let bal_slot: u64 = find_slot_offset_for_balance(db, token).unwrap();
            let bal_location = keccak256((TEST_ORDER_ADDR, bal_slot).abi_encode());
            db.insert_account_storage(
                token,
                U256::from_be_bytes(*bal_location),
                U256::from(u128::MAX)
            )
            .unwrap();
        }

        (block, ang_pools)
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

    impl DatabaseRef for InnerProvider {
        type Error = DBError;

        fn basic_ref(
            &self,
            address: Address
        ) -> Result<Option<revm::state::AccountInfo>, Self::Error> {
            let acc = async_to_sync(self.0.get_account(address).latest().into_future())
                .unwrap_or_default();
            let code = async_to_sync(self.0.get_code_at(address).latest().into_future())
                .unwrap_or_default();
            let code = Some(Bytecode::new_raw(code));

            Ok(Some(revm::state::AccountInfo {
                code_hash: acc.code_hash,
                balance: acc.balance,
                nonce: acc.nonce,
                code
            }))
        }

        fn storage_ref(
            &self,
            address: Address,
            index: alloy::primitives::U256
        ) -> Result<alloy::primitives::U256, Self::Error> {
            let acc = async_to_sync(self.0.get_storage_at(address, index).into_future())?;
            Ok(acc)
        }

        fn block_hash_ref(&self, number: u64) -> Result<alloy::primitives::B256, Self::Error> {
            let acc = async_to_sync(
                self.0
                    .get_block_by_number(BlockNumberOrTag::Number(number))
                    .into_future()
            )?;

            let Some(block) = acc else { return Err(DBError::String("no block".to_string())) };
            Ok(block.header.hash)
        }

        fn code_by_hash_ref(&self, _: alloy::primitives::B256) -> Result<Bytecode, Self::Error> {
            panic!("This should not be called, as the code is already loaded");
        }
    }
    pub fn async_to_sync<F: Future>(f: F) -> F::Output {
        let handle = tokio::runtime::Handle::try_current().expect("No tokio runtime found");
        tokio::task::block_in_place(|| handle.block_on(f))
    }
}
