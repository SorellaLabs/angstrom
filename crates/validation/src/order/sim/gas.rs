use std::{fmt::Debug, sync::Arc};

use alloy::primitives::Address;
use angstrom_types::sol_bindings::{
    ext::RawPoolOrder,
    grouped_orders::{AllOrders, OrderWithStorageData},
    rpc_orders::TopOfBlockOrder
};
use reth_provider::BlockNumReader;
use revm::database::CacheDB;

// use super::gas_inspector::{GasSimulationInspector, GasUsed};
use super::GasUsed;

// A address we can use to deploy contracts

// internal balance on book should be 27k gas cheaper
pub const BOOK_GAS: u64 = 92_000;
pub const BOOK_GAS_INTERNAL: u64 = 65_000;
pub const TOB_GAS: u64 = 1;
pub const TOB_GAS_INTERNAL: u64 = 1;
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
    _db:               CacheDB<Arc<DB>>,
    // the dkeployed addresses in cache_db
    _angstrom_address: Address,
    /// the address(pubkey) of this node.
    #[allow(unused)]
    node_address:      Option<Address>
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
            Ok(Self {
                _db:               CacheDB::new(db),
                _angstrom_address: angstrom_address,
                node_address:      Some(node_address)
            })
        } else {
            Ok(Self {
                _db:               CacheDB::new(db),
                _angstrom_address: angstrom_address.unwrap_or_default(),
                node_address:      None
            })
        }
    }

    pub fn gas_of_tob_order(
        &self,
        tob: &OrderWithStorageData<TopOfBlockOrder>,
        _block: u64
    ) -> eyre::Result<GasUsed> {
        // need to grab the order hash
        // self.execute_on_revm(
        //     &HashMap::default(),
        //     OverridesForTestAngstrom {
        //         flipped_order: Address::ZERO,
        //         amount_in:     U256::from(tob.amount()),
        //         amount_out:    U256::from(tob.quantity_out),
        //         token_out:     tob.token_out(),
        //         token_in:      tob.token_in(),
        //         user_address:  tob.from()
        //     },
        //     |execution_env| {
        //         let bundle = AngstromBundle::build_dummy_for_tob_gas(tob).unwrap();
        //
        //         let bundle = bundle.pade_encode();
        //         let bundle_bytes: Bytes = bundle.into();
        //         execution_env.block.number = U256::from(block + 1);
        //
        //         let tx = &mut execution_env.tx;
        //         tx.caller = self.node_address.unwrap_or(DEFAULT_FROM);
        //         tx.transact_to = TxKind::Call(self.angstrom_address);
        //         tx.data =
        // angstrom_types::contract_bindings::angstrom::Angstrom::executeCall::new(
        //             (bundle_bytes,)
        //         )
        //         .abi_encode()
        //         .into();
        //     }
        // )
        // .map_err(|e| eyre!("tob order err={} {:?}", e, tob.order_hash()))
        if tob.use_internal() { Ok(TOB_GAS_INTERNAL) } else { Ok(TOB_GAS) }
    }

    pub fn gas_of_book_order(
        &self,
        order: &OrderWithStorageData<AllOrders>,
        _block: u64
    ) -> eyre::Result<GasUsed> {
        // let exact_in = order.exact_in();
        // let bundle = AngstromBundle::build_dummy_for_user_gas(order).unwrap();
        //
        // let bundle = bundle.pade_encode();
        //
        // let (amount_in, amount_out) = if exact_in {
        //     (U256::from(order.amount()), {
        //         let price = order.price_for_book_side(order.is_bid);
        //         price.mul_quantity(U256::from(order.amount()))
        //     })
        // } else {
        //     (
        //         {
        //             let price = order.price_for_book_side(order.is_bid);
        //             price.mul_quantity(U256::from(order.amount()))
        //         },
        //         U256::from(order.amount())
        //     )
        // };
        //
        // self.execute_on_revm(
        //     &HashMap::default(),
        //     OverridesForTestAngstrom {
        //         amount_in,
        //         amount_out,
        //         token_out: order.token_out(),
        //         token_in: order.token_in(),
        //         user_address: order.from(),
        //         flipped_order: Address::default()
        //     },
        //     |execution_env| {
        //         execution_env.block.number = U256::from(block + 1);
        //
        //         let tx = &mut execution_env.tx;
        //         tx.caller = self.node_address.unwrap_or(DEFAULT_FROM);
        //         tx.transact_to = TxKind::Call(self.angstrom_address);
        //         tx.data =
        // angstrom_types::contract_bindings::angstrom::Angstrom::executeCall::new(
        //             (bundle.into(),)
        //         )
        //         .abi_encode()
        //         .into();
        //     }
        // )
        // .map_err(|e| eyre!("user order err={} {:?}", e, order.from()))
        if order.use_internal() { Ok(BOOK_GAS_INTERNAL) } else { Ok(BOOK_GAS) }
    }

    // fn execute_with_db<D: DatabaseRef, F>(db: D, f: F) ->
    // eyre::Result<(ResultAndState, D)> where
    //     F: FnOnce(&mut TxEnv),
    //     <D as revm::DatabaseRef>::Error: Send + Sync + Debug,
    // {
    //     let evm_handler = EnvWithHandlerCfg::default();
    //     let mut revm_sim = revm::Evm::builder()
    //         .with_ref_db(db)
    //         .with_env_with_handler_cfg(evm_handler)
    //         .modify_env(|env| {
    //             env.cfg.disable_balance_check = true;
    //             env.cfg.limit_contract_code_size = Some(usize::MAX - 1);
    //             env.cfg.disable_block_gas_limit = true;
    //         })
    //         .modify_tx_env(f)
    //         .build();
    //
    //     let Ok(out) = revm_sim.transact() else {
    //         return Err(eyre!("failed to transact transaction"));
    //     };
    //     let (cache_db, _) = revm_sim.into_db_and_env_with_handler_cfg();
    //     Ok((out, cache_db.0))
    // }
    //
    // /// deploys angstrom + univ4 and then sets DEFAULT_FROM address as a node in
    // /// the network.
    // fn setup_revm_cache_database_for_simulation(db: Arc<DB>) ->
    // eyre::Result<ConfiguredRevm<DB>> {     let cache_db =
    // CacheDB::new(db.clone());
    //
    //     let (out, cache_db) = Self::execute_with_db(cache_db, |tx| {
    //         tx.transact_to = TxKind::Create;
    //         tx.caller = DEFAULT_FROM;
    //         tx.data =
    //
    // angstrom_types::contract_bindings::pool_manager::PoolManager::BYTECODE.
    // clone();         tx.value = U256::from(0);
    //         tx.nonce = Some(0);
    //     })?;
    //
    //     if !out.result.is_success() {
    //         println!("{:?}", out.result);
    //         eyre::bail!("failed to deploy uniswap v4 pool manager");
    //     }
    //
    //     let v4_address = Address::from_slice(&keccak256((DEFAULT_FROM,
    // 0).abi_encode())[12..]);
    //
    //     // deploy angstrom.
    //     let angstrom_raw_bytecode =
    //         angstrom_types::contract_bindings::angstrom::Angstrom::BYTECODE.
    // clone();
    //
    //     // in solidity when deploying. constructor args are appended to the end
    // of the     // bytecode.
    //     let constructor_args = (v4_address, DEFAULT_FROM,
    // DEFAULT_FROM).abi_encode().into();     let data: Bytes =
    // [angstrom_raw_bytecode, constructor_args].concat().into();
    //
    //     // angstrom deploy is sicko mode.
    //     let flags = UniswapFlags::BeforeSwap
    //         | UniswapFlags::BeforeInitialize
    //         | UniswapFlags::BeforeAddLiquidity
    //         | UniswapFlags::BeforeRemoveLiquidity;
    //
    //     let (angstrom_address, salt) =
    //         mine_address_with_factory(DEFAULT_CREATE2_FACTORY, flags,
    // UniswapFlags::mask(), &data);
    //
    //     let final_mock_initcode = [salt.abi_encode(), data.to_vec()].concat();
    //
    //     let (out, cache_db) = Self::execute_with_db(cache_db, |tx| {
    //         tx.transact_to = TxKind::Call(DEFAULT_CREATE2_FACTORY);
    //         tx.caller = DEFAULT_FROM;
    //         tx.data = final_mock_initcode.into();
    //         tx.value = U256::from(0);
    //     })?;
    //
    //     if !out.result.is_success() {
    //         eyre::bail!("failed to deploy angstrom");
    //     }
    //
    //     // enable default from to call the angstrom contract.
    //     let (out, cache_db) = Self::execute_with_db(cache_db, |tx| {
    //         tx.transact_to = TxKind::Call(angstrom_address);
    //         tx.caller = DEFAULT_FROM;
    //         tx.data =
    // angstrom_types::contract_bindings::angstrom::Angstrom::toggleNodesCall::new(
    //             (vec![DEFAULT_FROM],),
    //         )
    //         .abi_encode()
    //         .into();
    //
    //         tx.value = U256::from(0);
    //     })?;
    //
    //     if !out.result.is_success() {
    //         eyre::bail!("failed to set default from address as node on
    // angstrom");     }
    //
    //     Ok(ConfiguredRevm { db: cache_db, angstrom: angstrom_address })
    // }
}

// struct ConfiguredRevm<DB> {
//     pub angstrom: Address,
//     pub db: CacheDB<Arc<DB>>,
// }

// pub fn mine_address_with_factory(
//     factory: Address,
//     flags: U160,
//     mask: U160,
//     initcode: &Bytes,
// ) -> (Address, U256) {
//     let init_code_hash = keccak256(initcode);
//     let mut salt = U256::ZERO;
//     let mut counter: u128 = 0;
//     loop {
//         let target_address: Address = factory.create2(B256::from(salt),
// init_code_hash);         let u_address: U160 = target_address.into();
//         if (u_address & mask) == flags {
//             break;
//         }
//         salt += U256::from(1_u8);
//         counter += 1;
//         if counter > 100_000 {
//             panic!("We tried this too many times!")
//         }
//     }
//     let final_address = factory.create2(B256::from(salt), init_code_hash);
//     (final_address, salt)
// }
