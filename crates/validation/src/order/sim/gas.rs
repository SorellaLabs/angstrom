use std::{collections::HashMap, fmt::Debug, sync::Arc};

use alloy::{
    primitives::{address, keccak256, Address, Bytes, TxKind, B256, U160, U256},
    sol_types::{SolCall, SolValue}
};
use angstrom_types::{
    contract_payloads::angstrom::AngstromBundle,
    matching::{uniswap::UniswapFlags, Ray},
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder,
        RawPoolOrder
    }
};
use eyre::eyre;
use pade::PadeEncode;
use reth_provider::BlockNumReader;
use revm::{
    db::CacheDB,
    inspector_handle_register,
    primitives::{EnvWithHandlerCfg, ResultAndState, TxEnv},
    DatabaseRef
};

use super::gas_inspector::{GasSimulationInspector, GasUsed};

/// A address we can use to deploy contracts
const DEFAULT_FROM: Address = address!("aa250d5630b4cf539739df2c5dacb4c659f2488d");
const DEFAULT_CREATE2_FACTORY: Address = address!("4e59b44847b379578588920cA78FbF26c0B4956C");
// const SETUP_BYTECODE: FixedBytes<32> =
//     fixed_bytes!("
// 907ea7ad6d1fbded0236f040aea693e2c9711b62b065fc95c4262972aca03996");

/// deals with the calculation of gas for a given type of order.
/// user orders and tob orders take different paths and are different size and
/// as such, pay different amount of gas in order to execute.
/// The calculation is done by this pc offset inspector which captures the
/// specific PC offsets of the code we want the user to pay for specifically.
/// Once the bundle has been built. We simulate the bundle and then calculate
/// the shared gas by using the simple formula:
/// (Bundle execution cost - Sum(Orders Gas payed)) / len(Orders)
#[derive(Clone)]
pub struct OrderGasCalculations<DB> {
    db:               CacheDB<Arc<DB>>,
    // the deployed addresses in cache_db
    angstrom_address: Address,
    /// the address(pubkey) of this node.
    node_address:     Option<Address>
}

impl<DB> OrderGasCalculations<DB>
where
    DB: Unpin + Clone + 'static + revm::DatabaseRef + BlockNumReader,
    <DB as revm::DatabaseRef>::Error: Send + Sync + Debug
{
    pub fn new(
        db: Arc<DB>,
        angstrom_address: Option<Address>,
        node_address: Address
    ) -> eyre::Result<Self> {
        // let bytecode = keccak256(&Angstrom::BYTECODE);
        // assert!(
        //     SETUP_BYTECODE == bytecode,
        //     "setup bytecode doesn't match bytecode we got. This can mean that the
        // offsets for gas \      could be miss-set and lead to errors"
        // );

        if let Some(angstrom_address) = angstrom_address {
            Ok(Self { db: CacheDB::new(db), angstrom_address, node_address: Some(node_address) })
        } else {
            let ConfiguredRevm { db, angstrom } =
                Self::setup_revm_cache_database_for_simulation(db)?;

            Ok(Self { db, angstrom_address: angstrom, node_address: None })
        }
    }

    pub fn gas_of_tob_order(
        &self,
        tob: &OrderWithStorageData<TopOfBlockOrder>,
        block: u64
    ) -> eyre::Result<GasUsed> {
        self.execute_on_revm(
            &HashMap::default(),
            OverridesForTestAngstrom {
                flipped_order: Address::ZERO,
                amount_in:     U256::from(tob.amount_in()),
                amount_out:    U256::from(tob.quantity_out),
                token_out:     tob.token_out(),
                token_in:      tob.token_in(),
                user_address:  tob.from()
            },
            |execution_env| {
                let bundle = AngstromBundle::build_dummy_for_tob_gas(tob)
                    .unwrap()
                    .pade_encode();
                let bundle_bytes: Bytes = bundle.into();
                execution_env.block.number = U256::from(block + 1);

                let tx = &mut execution_env.tx;
                tx.caller = self.node_address.unwrap_or(DEFAULT_FROM);
                tx.transact_to = TxKind::Call(self.angstrom_address);
                tx.data = angstrom_types::contract_bindings::angstrom::Angstrom::executeCall::new(
                    (bundle_bytes,)
                )
                .abi_encode()
                .into();
            }
        )
        .map_err(|e| eyre!("tob order err={}", e))
    }

    pub fn gas_of_book_order(
        &self,
        order: &OrderWithStorageData<GroupedVanillaOrder>,
        block: u64
    ) -> eyre::Result<GasUsed> {
        let bundle = AngstromBundle::build_dummy_for_user_gas(order).unwrap();
        let bundle = bundle.pade_encode();
        self.execute_on_revm(
            &HashMap::default(),
            OverridesForTestAngstrom {
                amount_in:     U256::from(order.amount_in()),
                amount_out:    {
                    let price = Ray::from(U256::from(order.limit_price()));

                    price.mul_quantity(U256::from(order.amount_in()))
                },
                token_out:     order.token_out(),
                token_in:      order.token_in(),
                user_address:  order.from(),
                flipped_order: Address::default()
            },
            |execution_env| {
                execution_env.block.number = U256::from(block + 1);

                let tx = &mut execution_env.tx;
                tx.caller = self.node_address.unwrap_or(DEFAULT_FROM);
                tx.transact_to = TxKind::Call(self.angstrom_address);
                tx.data = angstrom_types::contract_bindings::angstrom::Angstrom::executeCall::new(
                    (bundle.into(),)
                )
                .abi_encode()
                .into();
            }
        )
        .map_err(|e| eyre!("user order err={}", e))
    }

    fn execute_with_db<D: DatabaseRef, F>(db: D, f: F) -> eyre::Result<(ResultAndState, D)>
    where
        F: FnOnce(&mut TxEnv),
        <D as revm::DatabaseRef>::Error: Send + Sync + Debug
    {
        let evm_handler = EnvWithHandlerCfg::default();
        let mut revm_sim = revm::Evm::builder()
            .with_ref_db(db)
            .with_env_with_handler_cfg(evm_handler)
            .modify_env(|env| {
                env.cfg.disable_balance_check = true;
                env.cfg.limit_contract_code_size = Some(usize::MAX - 1);
                env.cfg.disable_block_gas_limit = true;
            })
            .modify_tx_env(f)
            .build();

        let Ok(out) = revm_sim.transact() else {
            return Err(eyre!("failed to transact transaction"))
        };
        let (cache_db, _) = revm_sim.into_db_and_env_with_handler_cfg();
        Ok((out, cache_db.0))
    }

    /// deploys angstrom + univ4 and then sets DEFAULT_FROM address as a node in
    /// the network.
    fn setup_revm_cache_database_for_simulation(db: Arc<DB>) -> eyre::Result<ConfiguredRevm<DB>> {
        let cache_db = CacheDB::new(db.clone());

        let (out, cache_db) = Self::execute_with_db(cache_db, |tx| {
            tx.transact_to = TxKind::Create;
            tx.caller = DEFAULT_FROM;
            tx.data =
                angstrom_types::contract_bindings::pool_manager::PoolManager::BYTECODE.clone();
            tx.value = U256::from(0);
            tx.nonce = Some(0);
        })?;

        if !out.result.is_success() {
            println!("{:?}", out.result);
            eyre::bail!("failed to deploy uniswap v4 pool manager");
        }

        let v4_address = Address::from_slice(&keccak256((DEFAULT_FROM, 0).abi_encode())[12..]);

        // deploy angstrom.
        let angstrom_raw_bytecode =
            angstrom_types::contract_bindings::angstrom::Angstrom::BYTECODE.clone();

        // in solidity when deploying. constructor args are appended to the end of the
        // bytecode.
        let constructor_args = (v4_address, DEFAULT_FROM, DEFAULT_FROM).abi_encode().into();
        let data: Bytes = [angstrom_raw_bytecode, constructor_args].concat().into();

        // angstrom deploy is sicko mode.
        let flags = UniswapFlags::BeforeSwap
            | UniswapFlags::BeforeInitialize
            | UniswapFlags::BeforeAddLiquidity
            | UniswapFlags::BeforeRemoveLiquidity;

        let (angstrom_address, salt) =
            mine_address_with_factory(DEFAULT_CREATE2_FACTORY, flags, UniswapFlags::mask(), &data);

        let final_mock_initcode = [salt.abi_encode(), data.to_vec()].concat();

        let (out, cache_db) = Self::execute_with_db(cache_db, |tx| {
            tx.transact_to = TxKind::Call(DEFAULT_CREATE2_FACTORY);
            tx.caller = DEFAULT_FROM;
            tx.data = final_mock_initcode.into();
            tx.value = U256::from(0);
        })?;

        if !out.result.is_success() {
            eyre::bail!("failed to deploy angstrom");
        }

        // enable default from to call the angstrom contract.
        let (out, cache_db) = Self::execute_with_db(cache_db, |tx| {
            tx.transact_to = TxKind::Call(angstrom_address);
            tx.caller = DEFAULT_FROM;
            tx.data = angstrom_types::contract_bindings::angstrom::Angstrom::toggleNodesCall::new(
                (vec![DEFAULT_FROM],)
            )
            .abi_encode()
            .into();

            tx.value = U256::from(0);
        })?;

        if !out.result.is_success() {
            eyre::bail!("failed to set default from address as node on angstrom");
        }

        Ok(ConfiguredRevm { db: cache_db, angstrom: angstrom_address })
    }

    fn execute_on_revm<F>(
        &self,
        offsets: &HashMap<usize, usize>,
        _overrides: OverridesForTestAngstrom,
        f: F
    ) -> eyre::Result<GasUsed>
    where
        F: FnOnce(&mut EnvWithHandlerCfg)
    {
        let mut inspector = GasSimulationInspector::new(self.angstrom_address, offsets);
        let mut evm_handler = EnvWithHandlerCfg::default();

        f(&mut evm_handler);

        {
            let mut evm = revm::Evm::builder()
                .with_ref_db(self.db.clone())
                .with_external_context(&mut inspector)
                .with_env_with_handler_cfg(evm_handler)
                .append_handler_register(inspector_handle_register)
                .modify_env(|env| {
                    env.cfg.disable_balance_check = true;
                })
                .build();

            let result = evm
                .transact()
                .map_err(|e| eyre!("failed to transact with revm: {e:?}"))?;

            if !result.result.is_success() {
                let output = result.result.output().unwrap().to_vec();
                let allowed_revert = alloy::primitives::hex!("cc67af53");
                if output[0..4] != allowed_revert {
                    return Err(eyre::eyre!(
                        "gas simulation had a revert. cannot guarantee the proper gas was \
                         estimated err={:?}",
                        result.result
                    ))
                }
            }
        }

        Ok(inspector.into_gas_used())
    }
}

struct ConfiguredRevm<DB> {
    pub angstrom: Address,
    pub db:       CacheDB<Arc<DB>>
}

#[allow(dead_code)]
struct OverridesForTestAngstrom {
    pub flipped_order: Address,
    pub user_address:  Address,
    pub amount_in:     U256,
    pub amount_out:    U256,
    pub token_in:      Address,
    pub token_out:     Address
}

pub fn mine_address_with_factory(
    factory: Address,
    flags: U160,
    mask: U160,
    initcode: &Bytes
) -> (Address, U256) {
    let init_code_hash = keccak256(initcode);
    let mut salt = U256::ZERO;
    let mut counter: u128 = 0;
    loop {
        let target_address: Address = factory.create2(B256::from(salt), init_code_hash);
        let u_address: U160 = target_address.into();
        if (u_address & mask) == flags {
            break
        }
        salt += U256::from(1_u8);
        counter += 1;
        if counter > 100_000 {
            panic!("We tried this too many times!")
        }
    }
    let final_address = factory.create2(B256::from(salt), init_code_hash);
    (final_address, salt)
}

// need a reth connection for this
#[cfg(all(test, feature = "reth-db-dep-tests"))]
pub mod test {
    // test to see proper gas_calculations
    use std::{
        path::Path,
        time::{Duration, SystemTime, UNIX_EPOCH}
    };

    use alloy::{
        node_bindings::WEI_IN_ETHER,
        primitives::{hex, Uint, U256},
        signers::{local::LocalSigner, SignerSync}
    };
    use angstrom_types::{
        reth_db_wrapper::RethDbWrapper,
        sol_bindings::{
            grouped_orders::StandingVariants,
            rpc_orders::{ExactStandingOrder, OmitOrderMeta}
        }
    };
    use eyre::eyre;
    use reth_provider::BlockNumReader;
    use reth_revm::primitives::Bytecode;
    use revm::primitives::AccountInfo;
    use testing_tools::utils::load_reth_db;

    use super::*;

    const WETH_ADDRESS: Address = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
    const USER_WITH_FUNDS: Address = address!("d02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");

    const ANGSTROM_DOMAIN: alloy::sol_types::Eip712Domain = alloy::sol_types::eip712_domain! {
        name: "angstrom",
        version: "1",
        chain_id: 1,
    };

    #[test]
    fn ensure_creation_of_mock_works() {
        let db_path = Path::new("/home/data/reth/db/");
        let db = load_reth_db(db_path);
        let res = OrderGasCalculations::new(Arc::new(RethDbWrapper::new(db)), None, Address::ZERO);

        if let Err(e) = res.as_ref() {
            eprintln!("{}", e);
        }

        assert!(res.is_ok(), "failed to deploy angstrom structure and v4 to chain");
    }

    fn signed_tob_order(block: u64) -> (Address, TopOfBlockOrder) {
        let user = LocalSigner::random();
        let address = user.address();

        let mut default = TopOfBlockOrder {
            use_internal: false,
            asset_in: WETH_ADDRESS,
            asset_out: WETH_ADDRESS,
            recipient: address,
            quantity_in: WEI_IN_ETHER.to(),
            quantity_out: WEI_IN_ETHER.to(),
            valid_for_block: block,
            ..Default::default()
        };

        let hash = default.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
        let sig = user.sign_hash_sync(&hash).unwrap();

        default.meta.isEcdsa = true;
        default.meta.from = address;
        default.meta.signature = sig.pade_encode().into();

        (address, default)
    }

    fn signed_exact_order() -> (Address, ExactStandingOrder) {
        let user = LocalSigner::random();
        let address = user.address();

        let mut default = angstrom_types::sol_bindings::rpc_orders::ExactStandingOrder {
            ref_id:               0,
            max_extra_fee_asset0: 0,
            exact_in:             true,
            amount:               WEI_IN_ETHER.to(),
            min_price:            U256::from(1u128),
            use_internal:         false,
            asset_in:             WETH_ADDRESS,
            asset_out:            WETH_ADDRESS,
            recipient:            USER_WITH_FUNDS,
            hook_data:            alloy::primitives::Bytes::new(),
            nonce:                0,
            deadline:             Uint::<40, 1>::from_be_slice(
                &(SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
                    + Duration::from_secs(1000))
                .as_secs()
                .to_be_bytes()[3..]
            ),

            meta: Default::default()
        };
        let hash = default.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
        let sig = user.sign_hash_sync(&hash).unwrap();

        default.meta.isEcdsa = true;
        default.meta.from = address;
        default.meta.signature = sig.pade_encode().into();

        (address, default)
    }

    #[test]
    fn test_tob_gas_calculations_work() {
        let db_path = Path::new("/home/data/reth/db/");
        let db = Arc::new(RethDbWrapper::new(load_reth_db(db_path)));

        let gas_calculations =
            OrderGasCalculations::new(Arc::new(RethDbWrapper::new(db)), None, Address::ZERO);

        assert!(gas_calculations.is_ok(), "failed to deploy angstrom structure and v4 to chain");
        let mut gas_calculations = gas_calculations.unwrap();

        let block = gas_calculations.db.db.last_block_number().unwrap() + 1;
        let (swapper, order) = signed_tob_order(block);

        // ensure we give the proper approvals of token in as this is
        // baseline assumed by this module
        set_balances_and_approvals(
            &mut gas_calculations.db,
            gas_calculations.angstrom_address,
            swapper,
            WETH_ADDRESS,
            WEI_IN_ETHER
        );

        let tob_order = OrderWithStorageData {
            order,
            is_currently_valid: true,
            is_bid: true,
            ..Default::default()
        };

        // ensure user address has proper funds
        let order_gas = gas_calculations
            .gas_of_tob_order(&tob_order, 0)
            .expect("failed_to execute tob order");

        // have not set offsets
        assert_eq!(order_gas, 0);
    }

    #[test]
    fn test_user_gas_calculations_work() {
        let db_path = Path::new("/home/data/reth/db/");
        let db = Arc::new(RethDbWrapper::new(load_reth_db(db_path)));

        let gas_calculations =
            OrderGasCalculations::new(Arc::new(RethDbWrapper::new(db)), None, Address::ZERO);

        assert!(gas_calculations.is_ok(), "failed to deploy angstrom structure and v4 to chain");
        let mut gas_calculations = gas_calculations.unwrap();

        let (swapper, order) = signed_exact_order();

        // ensure we give the proper approvals of token in as this is
        // baseline assumed by this module
        set_balances_and_approvals(
            &mut gas_calculations.db,
            gas_calculations.angstrom_address,
            swapper,
            WETH_ADDRESS,
            WEI_IN_ETHER
        );

        let user_order = OrderWithStorageData {
            order: GroupedVanillaOrder::Standing(StandingVariants::Exact(order)),
            is_currently_valid: true,
            is_bid: true,
            ..Default::default()
        };

        // ensure user address has proper funds
        let order_gas = gas_calculations
            .gas_of_book_order(&user_order, 0)
            .expect("failed_to execute user order");

        // have not set offsets
        assert_eq!(order_gas, 0);
    }

    alloy::sol!(
        function name() public view returns (string);
        function symbol() public view returns (string);
        function decimals() public view returns (uint8);
        function totalSupply() public view returns (uint256);
        function balanceOf(address _owner) public view returns (uint256 balance);
        function transfer(address _to, uint256 _value) public returns (bool success);
        function transferFrom(address _from, address _to, uint256 _value) public returns (bool success);
        function approve(address _spender, uint256 _value) public returns (bool success);
        function allowance(address _owner, address _spender) public view returns (uint256 remaining);
    );

    #[test]
    fn test_simple_gas_calculations_on_a_transfer() {
        let weth_contract = address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");

        let user_with_funds = address!("d02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");

        let db_path = Path::new("/home/data/reth/db/");
        let db = Arc::new(RethDbWrapper::new(load_reth_db(db_path)));
        let amount = U256::from(50) * WEI_IN_ETHER;
        let mut cache_db = CacheDB::new(db);

        set_balances_and_approvals(
            &mut cache_db,
            DEFAULT_FROM,
            user_with_funds,
            weth_contract,
            amount
        );

        let mut offsets = std::collections::HashMap::default();
        // its important to note that the end pc needs to be +1 the wante
        offsets.insert(5, 8);

        let mut inspector = GasSimulationInspector::new(weth_contract, &offsets);

        let mut evm_handler = EnvWithHandlerCfg::default();
        let tx = &mut evm_handler.tx;

        tx.transact_to = TxKind::Call(weth_contract);
        tx.caller = DEFAULT_FROM;
        tx.data = transferFromCall::new((user_with_funds, DEFAULT_FROM, WEI_IN_ETHER))
            .abi_encode()
            .into();

        tx.value = U256::from(0);

        let mut evm = revm::Evm::builder()
            .with_ref_db(cache_db)
            .with_external_context(&mut inspector)
            .with_env_with_handler_cfg(evm_handler)
            .append_handler_register(inspector_handle_register)
            .modify_env(|env| {
                env.cfg.disable_balance_check = true;
            })
            .build();

        let result = evm
            .transact()
            .map_err(|_| eyre!("failed to transact with revm"))
            .unwrap();

        if !result.result.is_success() {
            panic!(
                "gas simulation had a revert. cannot guarantee the proper gas
        was estimated"
            )
        }
        let total_gas = result.result.gas_used();
        println!("{total_gas} -- total gas used");
        drop(evm);

        let gas_used = inspector.into_gas_used();
        // this is the expected codes.
        // 	0005    60  PUSH1 0x04 (3)
        // 	0007    36  CALLDATASIZE (2)

        assert_eq!(gas_used, 5);
    }

    #[test]
    fn test_simple_gas_calculations_on_raw_bytecode() {
        let rand = address!("d02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");

        let db_path = Path::new("/home/data/reth/db/");
        let db = Arc::new(RethDbWrapper::new(load_reth_db(db_path)));
        let mut cache_db = CacheDB::new(db);
        let a = AccountInfo {
            balance:   U256::ZERO,
            code:      Some(Bytecode::new_raw(alloy::primitives::Bytes::from_static(&hex!(
                "6042604260425860005260206000F3"
            )))),
            nonce:     0,
            code_hash: keccak256(hex!("604260005260206000F3"))
        };

        cache_db.insert_account_info(rand, a);

        let mut offsets = std::collections::HashMap::default();
        offsets.insert(0, 9);

        let mut inspector = GasSimulationInspector::new(rand, &offsets);

        let mut evm_handler = EnvWithHandlerCfg::default();
        let tx = &mut evm_handler.tx;

        tx.transact_to = TxKind::Call(rand);
        tx.caller = DEFAULT_FROM;
        tx.data = vec![].into();
        tx.value = U256::from(0);

        let mut evm = revm::Evm::builder()
            .with_ref_db(cache_db)
            .with_external_context(&mut inspector)
            .with_env_with_handler_cfg(evm_handler)
            .append_handler_register(inspector_handle_register)
            .modify_env(|env| {
                env.cfg.disable_balance_check = true;
            })
            .build();

        let result = evm
            .transact()
            .map_err(|_| eyre!("failed to transact with revm"))
            .unwrap();

        if !result.result.is_success() {
            panic!(
                "gas simulation had a revert. cannot guarantee the proper gas
        was estimated"
            )
        }
        drop(evm);

        let gas_used = inspector.into_gas_used();
        // this is the bytecode.
        // PUSH1 0x42 (3)
        // PUSH1 0x42 (3)
        // PUSH1 0x42 (3)
        // PC (2)
        // PUSH1 0x00 (3)
        // MSTORE (6)
        // PUSH1 0x20 (3)
        // PUSH1 0x00 (3)
        // RETURN (0)

        let total_gas = result.result.gas_used();
        assert_eq!(total_gas, 26 + 21000);

        assert_eq!(gas_used, 14);
    }

    fn set_balances_and_approvals<DB: DatabaseRef + Unpin>(
        cache_db: &mut CacheDB<Arc<DB>>,
        calle_address: Address,
        user_address: Address,
        token: Address,
        amount: U256
    ) {
        for i in 0..10 {
            let balance_amount_out_slot = keccak256((user_address, i).abi_encode());
            let approval_slot =
                keccak256((calle_address, keccak256((user_address, i).abi_encode())).abi_encode());

            cache_db
                .insert_account_storage(token, balance_amount_out_slot.into(), amount)
                .map_err(|_| eyre!("failed to insert account into storage"))
                .unwrap();

            cache_db
                .insert_account_storage(token, approval_slot.into(), amount)
                .map_err(|_| eyre!("failed to insert account into storage"))
                .unwrap();
        }
    }
}
