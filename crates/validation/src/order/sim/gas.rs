use std::{collections::HashMap, ops::BitOr, sync::Arc};

use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::{Anvil, AnvilInstance},
    primitives::{address, keccak256, Address, TxKind, B256, U160, U256},
    providers::{
        builder,
        fillers::{ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller},
        Identity, IpcConnect, RootProvider
    },
    pubsub::PubSubFrontend,
    rlp::Bytes,
    signers::local::PrivateKeySigner,
    sol_types::{SolCall, SolValue}
};
use angstrom_types::{
    contract_bindings::angstrom::Angstrom::Overflow,
    contract_payloads::angstrom::AngstromBundle,
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder,
        RawPoolOrder
    }
};
use eyre::eyre;
use pade::PadeEncode;
use reth_errors::RethError;
use reth_primitives::transaction::FillTxEnv;
use revm::{
    db::{CacheDB, WrapDatabaseRef},
    handler::register::{EvmHandler, HandleRegister},
    inspector_handle_register,
    interpreter::Gas,
    primitives::{AccountInfo, Bytecode, EnvWithHandlerCfg, ResultAndState, TxEnv},
    DatabaseRef, Evm
};

use super::gas_inspector::{GasSimulationInspector, GasUsed};
use crate::BlockStateProviderFactory;

/// A address we can use to deploy contracts
const DEFAULT_FROM: Address = address!("aa250d5630b4cf539739df2c5dacb4c659f2488d");

const DEFAULT_CREATE2_FACTORY: Address = address!("4e59b44847b379578588920cA78FbF26c0B4956C");

pub enum UniswapFlags {
    BeforeInitialize,
    AfterInitialize,
    BeforeAddLiquidity,
    AfterAddLiquidity,
    BeforeRemoveLiquidity,
    AfterRemoveLiquidity,
    BeforeSwap,
    AfterSwap,
    BeforeDonate,
    AfterDonate,
    BeforeSwapReturnsDelta,
    AfterSwapReturnsDelta,
    AfterAddLiquidityReturnsDelta,
    AfterRemoveLiquidityReturnsDelta
}

impl UniswapFlags {
    pub fn mask() -> U160 {
        (U160::from(1_u8) << 14) - U160::from(1_u8)
    }
}

impl From<UniswapFlags> for U160 {
    fn from(value: UniswapFlags) -> U160 {
        let bitshift: usize = match value {
            UniswapFlags::BeforeInitialize => 13,
            UniswapFlags::AfterInitialize => 12,
            UniswapFlags::BeforeAddLiquidity => 11,
            UniswapFlags::AfterAddLiquidity => 10,
            UniswapFlags::BeforeRemoveLiquidity => 9,
            UniswapFlags::AfterRemoveLiquidity => 8,
            UniswapFlags::BeforeSwap => 7,
            UniswapFlags::AfterSwap => 6,
            UniswapFlags::BeforeDonate => 5,
            UniswapFlags::AfterDonate => 4,
            UniswapFlags::BeforeSwapReturnsDelta => 3,
            UniswapFlags::AfterSwapReturnsDelta => 2,
            UniswapFlags::AfterAddLiquidityReturnsDelta => 1,
            UniswapFlags::AfterRemoveLiquidityReturnsDelta => 0
        };
        U160::from(1_u8) << bitshift
    }
}

impl BitOr for UniswapFlags {
    type Output = U160;

    fn bitor(self, rhs: Self) -> Self::Output {
        Into::<U160>::into(self) | Into::<U160>::into(rhs)
    }
}

impl BitOr<UniswapFlags> for U160 {
    type Output = U160;

    fn bitor(self, rhs: UniswapFlags) -> Self::Output {
        self | Into::<U160>::into(rhs)
    }
}

impl BitOr<U160> for UniswapFlags {
    type Output = U160;

    fn bitor(self, rhs: U160) -> Self::Output {
        Into::<U160>::into(self) | rhs
    }
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
    angstrom_address: Address
}

impl<DB> OrderGasCalculations<DB>
where
    DB: Unpin + Clone + 'static + revm::DatabaseRef,
    <DB as revm::DatabaseRef>::Error: Send + Sync
{
    pub fn new(db: Arc<DB>, angstrom_address: Option<Address>) -> eyre::Result<Self> {
        if let Some(angstrom_address) = angstrom_address {
            Ok(Self { db: CacheDB::new(db), angstrom_address })
        } else {
            let ConfiguredRevm { db, angstrom, .. } =
                Self::setup_revm_cache_database_for_simulation(db)?;
            Ok(Self { db, angstrom_address: angstrom })
        }
    }

    pub fn gas_of_tob_order(
        &self,
        tob: &OrderWithStorageData<TopOfBlockOrder>
    ) -> eyre::Result<GasUsed> {
        self.execute_on_revm(
            &HashMap::default(),
            OverridesForTestAngstrom {
                amount_in:    U256::from(tob.amount_in()),
                amount_out:   U256::from(tob.amount_out_min()),
                token_out:    tob.token_out(),
                token_in:     tob.token_in(),
                user_address: tob.from()
            },
            |execution_env| {
                let bundle = AngstromBundle::build_dummy_for_tob_gas(tob)
                    .unwrap()
                    .pade_encode();

                let tx = &mut execution_env.tx;
                tx.caller = DEFAULT_FROM;
                tx.transact_to = TxKind::Call(self.angstrom_address);
                tx.data = angstrom_types::contract_bindings::angstrom::Angstrom::executeCall::new(
                    (bundle.into(),)
                )
                .abi_encode()
                .into();
                tx.value = U256::from(0);
                tx.nonce = None;
            }
        )
    }

    pub fn gas_of_book_order(
        &self,
        order: &OrderWithStorageData<GroupedVanillaOrder>
    ) -> eyre::Result<GasUsed> {
        self.execute_on_revm(
            &HashMap::default(),
            OverridesForTestAngstrom {
                amount_in:    U256::from(order.amount_in()),
                amount_out:   U256::from(order.amount_out_min()),
                token_out:    order.token_out(),
                token_in:     order.token_in(),
                user_address: order.from()
            },
            |execution_env| {
                let bundle = AngstromBundle::build_dummy_for_user_gas(order)
                    .unwrap()
                    .pade_encode();

                let tx = &mut execution_env.tx;
                tx.caller = DEFAULT_FROM;
                tx.transact_to = TxKind::Call(self.angstrom_address);
                tx.data = angstrom_types::contract_bindings::angstrom::Angstrom::executeCall::new(
                    (bundle.into(),)
                )
                .abi_encode()
                .into();
                tx.value = U256::from(0);
                tx.nonce = None;
            }
        )
    }

    fn execute_with_db<D: DatabaseRef, F>(db: D, f: F) -> eyre::Result<(ResultAndState, D)>
    where
        F: FnOnce(&mut TxEnv),
        <D as revm::DatabaseRef>::Error: Send + Sync
    {
        let evm_handler = EnvWithHandlerCfg::default();
        let mut revm_sim = revm::Evm::builder()
            .with_ref_db(db)
            .with_env_with_handler_cfg(evm_handler)
            .modify_env(|env| {
                env.cfg.disable_balance_check = true;
                env.cfg.limit_contract_code_size = Some(usize::MAX - 1);
                env.cfg.disable_block_gas_limit = true;
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
        let mut cache_db = CacheDB::new(db.clone());

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
        let mut angstrom_raw_bytecode =
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
        let (out, mut cache_db) = Self::execute_with_db(cache_db, |tx| {
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

        Ok(ConfiguredRevm { db: cache_db, angstrom: angstrom_address, uni_swap: v4_address })
    }

    fn fetch_db_with_overrides(
        &self,
        overrides: OverridesForTestAngstrom
    ) -> eyre::Result<CacheDB<Arc<DB>>> {
        // fork db
        let mut cache_db = self.db.clone();

        // change approval of token in and then balance of token out
        let OverridesForTestAngstrom { user_address, amount_in, amount_out, token_in, token_out } =
            overrides;
        // for the first 10 slots, we just force override everything to balance. because
        // of the way storage slots work in solidity. this shouldn't effect
        // anything
        // https://docs.soliditylang.org/en/latest/internals/layout_in_storage.html
        for i in 0..10 {
            let balance_amount_out_slot = keccak256((self.angstrom_address, i).abi_encode());

            //keccak256(angstrom . keccak256(user . idx)))
            let approval_slot = keccak256(
                (self.angstrom_address, keccak256((user_address, i).abi_encode())).abi_encode()
            );

            cache_db
                .insert_account_storage(token_out, balance_amount_out_slot.into(), amount_out)
                .map_err(|_| eyre!("failed to insert account into storage"))?;

            cache_db
                .insert_account_storage(token_in, approval_slot.into(), amount_in)
                .map_err(|_| eyre!("failed to insert account into storage"))?;
        }

        Ok(cache_db)
    }

    fn execute_on_revm<F>(
        &self,
        offsets: &HashMap<usize, usize>,
        overrides: OverridesForTestAngstrom,
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
                .with_ref_db(self.fetch_db_with_overrides(overrides)?)
                .with_external_context(&mut inspector)
                .with_env_with_handler_cfg(evm_handler)
                .append_handler_register(inspector_handle_register)
                .modify_env(|env| {
                    env.cfg.disable_balance_check = true;
                })
                .build();

            let result = evm
                .transact()
                .map_err(|_| eyre!("failed to transact with revm"))?;

            if !result.result.is_success() {
                return Err(eyre::eyre!(
                    "gas simulation had a revert. cannot guarantee the proper gas was estimated"
                ))
            }
        }

        Ok(inspector.into_gas_used())
    }
}

struct ConfiguredRevm<DB> {
    pub uni_swap: Address,
    pub angstrom: Address,
    pub db:       CacheDB<Arc<DB>>
}

struct OverridesForTestAngstrom {
    pub user_address: Address,
    pub amount_in:    U256,
    pub amount_out:   U256,
    pub token_in:     Address,
    pub token_out:    Address
}

#[cfg(test)]
pub mod test {
    // test to see proper gas_calculations
    use std::path::Path;

    use alloy::{
        node_bindings::WEI_IN_ETHER,
        primitives::{hex, U256}
    };
    use angstrom_types::reth_db_wrapper::RethDbWrapper;
    use eyre::eyre;
    use revm::primitives::AccountInfo;
    use testing_tools::load_reth_db;

    use super::*;

    #[test]
    fn ensure_creation_of_mock_works() {
        let db_path = Path::new("/home/data/reth/db/");
        let db = load_reth_db(db_path);
        let res = OrderGasCalculations::new(Arc::new(RethDbWrapper::new(db)));

        if let Err(e) = res.as_ref() {
            eprintln!("{}", e);
        }

        assert!(res.is_ok(), "failed to deploy angstrom structure and v4 to chain");
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

        // setup account with lots of weth
        for i in 0..10 {
            let balance_amount_out_slot = keccak256((user_with_funds, i).abi_encode());
            let approval_slot = keccak256(
                (DEFAULT_FROM, keccak256((user_with_funds, i).abi_encode())).abi_encode()
            );

            cache_db
                .insert_account_storage(weth_contract, balance_amount_out_slot.into(), amount)
                .map_err(|_| eyre!("failed to insert account into storage"))
                .unwrap();

            cache_db
                .insert_account_storage(weth_contract, approval_slot.into(), amount)
                .map_err(|_| eyre!("failed to insert account into storage"))
                .unwrap();
        }

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
        let mut a = AccountInfo {
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
}
