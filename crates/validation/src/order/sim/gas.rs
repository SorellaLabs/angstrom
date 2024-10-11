use std::{collections::HashMap, sync::Arc};

use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::{Anvil, AnvilInstance},
    primitives::Address,
    providers::{
        builder,
        fillers::{ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller},
        Identity, IpcConnect, RootProvider
    },
    pubsub::PubSubFrontend,
    signers::local::PrivateKeySigner,
    sol_types::{SolCall, SolValue}
};
use angstrom_types::sol_bindings::{
    grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
    rpc_orders::TopOfBlockOrder
};
use reth_primitives::{transaction::FillTxEnv, TxKind};
use revm::{
    db::{CacheDB, WrapDatabaseRef},
    handler::register::EvmHandler,
    inspector_handle_register,
    interpreter::Gas,
    primitives::{AccountInfo, EnvWithHandlerCfg, ResultAndState, TxEnv},
    DatabaseRef, Evm
};

use super::gas_inspector::{GasSimulationInspector, GasUsed};
use crate::{BlockStateProviderFactory, RevmLRU};

const DEFAULT_FROM: Address =
    alloy::primitives::address!("aa250d5630b4cf539739df2c5dacb4c659f2488d");

/// deals with the calculation of gas for a given type of order.
/// user orders and tob orders take different paths and are different size and
/// as such, pay different amount of gas in order to execute.
/// The calculation is done by this pc offset inspector which captures the
/// specific PC offsets of the code we want the user to pay for specifically.
/// Once the bundle has been built. We simulate the bundle and then calculate
/// the shared gas by using the simple formula:
/// (Bundle execution cost - Sum(Orders Gas payed)) / len(Orders)
pub struct OrderGasCalculations<DB> {
    db:               Arc<RevmLRU<DB>>,
    angstrom_address: Option<Address>
}

impl<DB> OrderGasCalculations<DB>
where
    DB: BlockStateProviderFactory + Unpin + Clone + 'static
{
    /// if there isn't a address. OrderGasCalculations will spin up a mock env
    /// with revm.
    pub fn new(db: Arc<RevmLRU<DB>>, angstrom_address: Option<Address>) -> Self {
        Self { db, angstrom_address }
    }

    fn execute_with_db<D: DatabaseRef, F>(&self, db: D, f: F) -> (ResultAndState, D)
    where
        F: FnOnce(&mut TxEnv)
    {
        let evm_handler = EnvWithHandlerCfg::default();
        let mut revm_sim = revm::Evm::builder()
            .with_ref_db(db)
            .with_env_with_handler_cfg(evm_handler)
            .append_handler_register(inspector_handle_register)
            .modify_env(|env| {
                env.cfg.disable_balance_check = true;
                env.cfg.disable_block_gas_limit = true;
            })
            .modify_tx_env(f)
            .build();
        let out = revm_sim.transact()?;
        let cache_db = revm_sim.into_context().evm.db.0;
        (out, cache_db)
    }

    fn setup_revm_cache_database_for_simulation(&self) -> eyre::Result<ConfiguredRevm<DB>> {
        let mut cache_db = CacheDB::new(self.db.clone());

        let (out, cache_db) = self.execute_with_db(cache_db, |tx| {
            tx.transact_to = TxKind::Create;
            tx.caller = DEFAULT_FROM;
            tx.data = angstrom_types::contract_bindings::poolmanager::PoolManager::BYTECODE;
            tx.value = U256::from(0);
        });

        if !out.result.is_success() {
            eyre::bail!("failed to deploy uniswap v4 pool manager");
        }
        let v4_address = Address::from_slice(&*out.result.output().unwrap());

        // deploy angstrom.

        let mut angstrom_raw_bytecode =
            angstrom_types::contract_bindings::angstrom::Angstrom::BYTECODE;

        // in solidity when deploying. constructor args are appended to the end of the
        // bytecode.
        let constructor_args = (v4_address, DEFAULT_FROM, DEFAULT_FROM).abi_encode().into();
        let data = [angstrom_raw_bytecode, constructor_args].concat();

        let (out, cache_db) = self.execute_with_db(cache_db, |tx| {
            tx.transact_to = TxKind::Create;
            tx.caller = DEFAULT_FROM;
            tx.data = data;
            tx.value = U256::from(0);
        });

        if !out.result.is_success() {
            eyre::bail!("failed to deploy angstrom");
        }
        let angstrom_address = Address::from_slice(&*out.result.output().unwrap());

        // enable default from to call the angstrom contract.
        let (out, cache_db) = self.execute_with_db(cache_db, |tx| {
            tx.transact_to = TxKind::Call(angstrom_address);
            tx.caller = DEFAULT_FROM;
            tx.data =
                angstrom_types::contract_bindings::angstrom::Angstrom::toggleNodesCall::new(vec![
                    DEFAULT_FROM,
                ])
                .abi_encode()
                .into();

            tx.value = U256::from(0);
        });

        if !out.result.is_success() {
            eyre::bail!("failed to set default from address as node on angstrom");
        }
    }

    fn execute_on_revm<F>(
        &self,
        offsets: &HashMap<usize, usize>,
        f: F
    ) -> Result<GasUsed, GasSimulationError>
    where
        F: FnOnce(&mut EnvWithHandlerCfg)
    {
        let mut inspector = GasSimulationInspector::new(self.angstrom_address, offsets);
        let mut evm_handler = EnvWithHandlerCfg::default();

        f(&mut evm_handler);

        let account_info = cache_db.insert_contract(account);

        // TODO: going to get rid of revm lru so this is why using cache_db here

        let mut evm = revm::Evm::builder()
            .with_ref_db(self.db.clone())
            .with_external_context(&mut inspector)
            .with_env_with_handler_cfg(evm_handler)
            .append_handler_register(inspector_handle_register)
            .modify_env(|env| {
                env.cfg.disable_balance_check = true;
            })
            .build();

        let result = evm.transact()?;

        if !result.result.is_success() {
            return Err(eyre::eyre!(
                "gas simulation had a revert. cannot guarantee the proper gas was estimated"
            ))
        }

        Ok(inspector.into_gas_used())
    }

    pub fn gas_of_tob_order(
        &self,
        tob: &OrderWithStorageData<TopOfBlockOrder>
    ) -> Result<GasUsed, GasSimulationError> {
        self.execute_on_revm(&HashMap::default(), |execution_env| {
            let tx = &mut execution_env.tx;
            tx.caller = from;
            tx.transact_to = TxKind::Call(pool_address);
            tx.data = encoded.into();
            tx.value = U256::from(0);
            tx.nonce = 1;
        })
    }

    pub fn gas_of_book_order(
        &self,
        order: &OrderWithStorageData<GroupedVanillaOrder>
    ) -> Result<GasUsed, GasSimulationError> {
        self.execute_on_revm(&HashMap::default(), |execution_env| {
            // execution_env.env.tx.data =
            // execution_env.env.
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GasSimulationError {
    #[error("Transaction Reverted")]
    TransactionReverted
}

struct ConfiguredRevm<DB> {
    pub uniswap:  Address,
    pub angstrom: Address,
    pub db:       CacheDB<RevmLRU<DB>>
}
