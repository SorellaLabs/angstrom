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
    use validation::{find_slot_offset_for_approval, find_slot_offset_for_balance};

    use crate::{
        DataLoader, PoolConfigured, PoolKey, PoolRemoved, UniswapPoolRegistry,
        uniswap::pool::EnhancedUniswapPool
    };

    const UNIVERSAL_ROUTER_SEPOLIA: Address =
        address!("0x3a9d48ab9751398bbfa63ad67599bb04e4bdf98b");
    pub type ProviderType = FillProvider<
        JoinFill<
            Identity,
            JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>
        >,
        RootProvider
    >;

    const SETTLE: u8 = 0x0b;

    alloy::sol!(
        function execute(bytes calldata commands, bytes[] calldata inputs) public payable override;

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

    // const LAST_BLOCK_SLOT_ANGSTROM: U256 = U256[;
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

    // uint256 internal constant SWAP_EXACT_OUT_SINGLE = 0x08;
    fn build_exact_out_swap_calldata(pool_key: PoolKey, zfo: bool, amount_out: u128) -> Bytes {
        let token_out = if zfo { pool_key.currency1 } else { pool_key.currency0 };
        let arg = ExactOutputSingleParams {
            poolKey:         pool_key,
            zeroForOne:      zfo,
            amountOut:       amount_out,
            amountInMaximum: u128::MAX - 1,
            hookData:        Bytes::new()
        };
        let params = vec![
            Bytes::from_iter(arg.abi_encode()),
            Bytes::from_iter((token_out, amount_out, true).abi_encode()),
        ];
        let argument = Bytes::from_iter(vec![0x08u8, SETTLE]);

        // abi.decode(data, (bytes, bytes[]));
        let unlock_calldata = Bytes::from_iter((argument, params).abi_encode());

        let v4_swap_param = Bytes::from_iter(vec![0x10]);
        executeCall::new((v4_swap_param, vec![unlock_calldata]))
            .abi_encode()
            .into()
    }

    // uint256 internal constant SWAP_EXACT_IN_SINGLE = 0x06;
    fn build_exact_in_swap_calldata(pool_key: PoolKey, zfo: bool, amount_in: u128) -> Bytes {
        let token_in = if zfo { pool_key.currency0 } else { pool_key.currency1 };
        let arg = ExactInputSingleParams {
            poolKey:          pool_key,
            zeroForOne:       zfo,
            amountIn:         amount_in,
            amountOutMinimum: 0,
            hookData:         Bytes::new()
        };
        let params = vec![
            Bytes::from_iter(arg.abi_encode()),
            Bytes::from_iter((token_in, amount_in, true).abi_encode()),
        ];
        let argument = Bytes::from_iter(vec![0x06u8, SETTLE]);

        // abi.decode(data, (bytes, bytes[]));
        let unlock_calldata = Bytes::from_iter((argument, params).abi_encode());

        let v4_swap_param = Bytes::from_iter(vec![0x10]);
        executeCall::new((v4_swap_param, vec![unlock_calldata]))
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
        })
        .modify_block_chained(|block| {
            block.number = target_block;
            tracing::info!(?block.number, "simulating block on");
        })
        .modify_tx_chained(|tx| {
            tx.kind = TxKind::Call(UNIVERSAL_ROUTER_SEPOLIA);
            tx.chain_id = Some(CHAIN_ID);
            tx.caller = TESTNET_ANGSTROM_ADDRESS;
            tx.data = router_calldata
        })
        .build_mainnet();
        let result = evm.replay().unwrap();
        if !result.result.is_success() {
            tracing::info!("replay failed");
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
                    target_block,
                    cache_db.clone(),
                    snapshot.clone(),
                    pool_key.clone(),
                    pool.token0_decimals,
                    pool.token1_decimals
                );
                am_check_exact_out(
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
        target_block: u64,
        db: CacheDB<Arc<DB>>,
        snap: PoolSnapshot,
        key: PoolKey,
        t0_dec: u8,
        t1_dec: u8
    ) {
        let mut rng = rand::thread_rng();

        let amount: u128 = rng.gen_range(1..10000);
        let zfo: bool = rng.r#gen();
        let amount = if zfo { amount.pow(t1_dec as u32) } else { amount.pow(t0_dec as u32) };

        let call_bytecode = build_exact_out_swap_calldata(key, zfo, amount);
        let revm_swap_output = execute_calldata(target_block, call_bytecode, db);

        let amount_i = if zfo { Quantity::Token1(amount) } else { Quantity::Token0(amount) };

        // local calculations
        let local_swap_output = (snap.current_price() - amount_i).unwrap();
        let t0_delta_local = local_swap_output.d_t0;
        let t1_delta_local = local_swap_output.d_t1;
        let sqrt_price_local = *local_swap_output.end_bound.as_sqrtpricex96();

        let t0_delta_revm = revm_swap_output.amount0.abs() as u128;
        let t1_delta_revm = revm_swap_output.amount1.abs() as u128;
        let sqrt_price_revm = revm_swap_output.sqrtPriceX96;

        assert_eq!(t0_delta_local, t0_delta_revm, "amount: {amount} zfo: {zfo}");
        assert_eq!(t1_delta_local, t1_delta_revm, "amount: {amount} zfo: {zfo}");
        assert_eq!(sqrt_price_local, sqrt_price_revm, "amount: {amount} zfo: {zfo}");
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
        target_block: u64,
        db: CacheDB<Arc<DB>>,
        snap: PoolSnapshot,
        key: PoolKey,
        t0_dec: u8,
        t1_dec: u8
    ) {
        let mut rng = rand::thread_rng();

        let amount: u128 = rng.gen_range(1..10000);
        let zfo: bool = rng.r#gen();
        let amount = if zfo { amount.pow(t0_dec as u32) } else { amount.pow(t1_dec as u32) };

        let call_bytecode = build_exact_in_swap_calldata(key, zfo, amount);
        let revm_swap_output = execute_calldata(target_block, call_bytecode, db);

        let amount_i = if zfo { Quantity::Token0(amount) } else { Quantity::Token1(amount) };

        // local calculations
        let local_swap_output = (snap.current_price() + amount_i).unwrap();
        let t0_delta_local = local_swap_output.d_t0;
        let t1_delta_local = local_swap_output.d_t1;
        let sqrt_price_local = *local_swap_output.end_bound.as_sqrtpricex96();

        let t0_delta_revm = revm_swap_output.amount0.abs() as u128;
        let t1_delta_revm = revm_swap_output.amount1.abs() as u128;
        let sqrt_price_revm = revm_swap_output.sqrtPriceX96;

        assert_eq!(t0_delta_local, t0_delta_revm, "amount: {amount} zfo: {zfo}");
        assert_eq!(t1_delta_local, t1_delta_revm, "amount: {amount} zfo: {zfo}");
        assert_eq!(sqrt_price_local, sqrt_price_revm, "amount: {amount} zfo: {zfo}");
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
            let mut pool = EnhancedUniswapPool::new(data_loader, 100);
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
                (UNIVERSAL_ROUTER_SEPOLIA, keccak256((TEST_ORDER_ADDR, ap_slot).abi_encode()))
                    .abi_encode()
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
            let acc = async_to_sync(self.0.get_account(address).latest().into_future())?;
            let code = async_to_sync(self.0.get_code_at(address).latest().into_future())?;
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
