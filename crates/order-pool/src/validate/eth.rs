//! Angstrom Order Validator

use std::{
    marker::PhantomData,
    sync::{atomic::AtomicBool, Arc}
};

use reth_primitives::{
    constants::ETHEREUM_BLOCK_GAS_LIMIT, ChainSpec, InvalidTransactionError, LEGACY_TX_TYPE_ID
};
use reth_provider::{AccountReader, StateProviderFactory};
use reth_tasks::TaskSpawner;
use tokio::sync::Mutex;

use crate::{
    error::InValidPoolOrderError,
    traits::OrderOrigin,
    validate::{ValidationTask, TX_MAX_SIZE},
    OrderValidator, PoolOrder, TransactionValidationOutcome, TransactionValidationTaskExecutor
};

/// Validator for Angstrom Orders.
#[derive(Debug, Clone)]
pub struct AngstromOrderValidator<Client, T> {
    /// The type that performs the actual validation.
    inner: Arc<OrderValidatorInner<Client, T>>
}

impl<Client, Tx> AngstromOrderValidator<Client, Tx>
where
    Client: StateProviderFactory,
    Tx: PoolOrder
{
    /// Validates a single order
    ///
    /// See also [OrderValidator::validate_transaction]
    pub fn validate_one(
        &self,
        origin: OrderOrigin,
        transaction: Tx
    ) -> TransactionValidationOutcome<Tx> {
        self.inner.validate_one(origin, transaction)
    }

    /// Validates all given orders
    ///
    /// Returns all outcomes for the given orders, in the same order
    ///
    /// See also [Self::validate_one]
    pub fn validate_all(
        &self,
        transactions: Vec<(OrderOrigin, Tx)>
    ) -> Vec<TransactionValidationOutcome<Tx>> {
        transactions
            .into_iter()
            .map(|(origin, tx)| self.validate_one(origin, tx))
            .collect()
    }
}

#[async_trait::async_trait]
impl<Client, Tx> OrderValidator for AngstromOrderValidator<Client, Tx>
where
    Client: StateProviderFactory,
    Tx: PoolOrder
{
    type Order = Tx;

    async fn validate_transaction(
        &self,
        origin: OrderOrigin,
        transaction: Self::Order
    ) -> TransactionValidationOutcome<Self::Order> {
        self.validate_one(origin, transaction)
    }

    async fn validate_transactions(
        &self,
        transactions: Vec<(OrderOrigin, Self::Order)>
    ) -> Vec<TransactionValidationOutcome<Self::Order>> {
        self.validate_all(transactions)
    }
}

/// A [OrderValidator] implementation that validates ethereum transaction.
#[derive(Debug)]
pub(crate) struct OrderValidatorInner<Client, T> {
    /// Spec of the chain
    chain_spec: Arc<ChainSpec>,
    /// This type fetches account info from the db
    client: Client,
    /// tracks activated forks relevant for transaction validation
    /// The current max gas limit
    block_gas_limit: u64,
    /// Minimum priority fee to enforce for acceptance into the pool.
    minimum_priority_fee: Option<u128>,
    /// Toggle to determine if a local transaction should be propagated
    propagate_local_transactions: bool,
    /// Marker for the transaction type
    _marker: PhantomData<T>
}

// === impl OrderValidatorInner ===

impl<Client, Tx> OrderValidatorInner<Client, Tx> {
    /// Returns the configured chain id
    pub(crate) fn chain_id(&self) -> u64 {
        self.chain_spec.chain().id()
    }
}

//TODO(Ludwig): Implement specific order types and validity conditions
impl<Client, Tx> OrderValidatorInner<Client, Tx>
where
    Client: StateProviderFactory,
    Tx: PoolOrder
{
    /// Validates a single transaction.
    fn validate_one(
        &self,
        origin: OrderOrigin,
        transaction: Tx
    ) -> TransactionValidationOutcome<Tx> {
        // Reject transactions over defined size to prevent DOS attacks
        if transaction.size() > TX_MAX_SIZE {
            let size = transaction.size();
            return TransactionValidationOutcome::Invalid(
                transaction,
                InValidPoolOrderError::OversizedData(size, TX_MAX_SIZE)
            )
        }

        // Checks for chain, we keep this check here in preparation of our multichain
        // future. We could impl this in the domain separator, but that seems like
        // unecessary complexity
        if let Some(chain_id) = transaction.chain_id() {
            if chain_id != self.chain_id() {
                return TransactionValidationOutcome::Invalid(
                    transaction,
                    InvalidTransactionError::ChainIdMismatch.into()
                )
            }
        }

        //TODO: Implement min gas cost for order types
        /*
        let _access_list = transaction
            .access_list()
            .map(|list| list.flattened())
            .unwrap_or_default();


        if transaction.gas_limit() < calculate_intrinsic_gas(transaction.kind()) {
            return TransactionValidationOutcome::Invalid(
                transaction,
                InValidPoolOrderError::IntrinsicGasTooLow
            )
        }*/

        let account = match self
            .client
            .latest()
            .and_then(|state| state.basic_account(transaction.sender()))
        {
            Ok(account) => account.unwrap_or_default(),
            Err(err) => {
                return TransactionValidationOutcome::Error(*transaction.hash(), Box::new(err))
            }
        };

        // Signer account shouldn't have bytecode. Presence of bytecode means this is a
        // smartcontract.
        if account.has_bytecode() {
            return TransactionValidationOutcome::Invalid(
                transaction,
                InvalidTransactionError::SignerAccountHasBytecode.into()
            )
        }

        //TODO: Implement this for eip 712 order nonce
        if transaction.nonce() < account.nonce {
            return TransactionValidationOutcome::Invalid(
                transaction,
                InvalidTransactionError::NonceNotConsistent.into()
            )
        }

        //TODO: Implement balance check for trade

        // Checks for max cost
        let cost = transaction.cost();
        if cost > account.balance {
            return TransactionValidationOutcome::Invalid(
                transaction,
                InvalidTransactionError::InsufficientFunds {
                    cost,
                    available_funds: account.balance
                }
                .into()
            )
        }

        // Return the valid transaction
        TransactionValidationOutcome::Valid {
            balance: account.balance,
            state_nonce: account.nonce,
            transaction,
            // by this point assume all external transactions should be propagated
            propagate: match origin {
                OrderOrigin::External => true,
                OrderOrigin::Local => self.propagate_local_transactions,
                OrderOrigin::Private => false
            }
        }
    }
}

/// A builder for [TransactionValidationTaskExecutor]
#[derive(Debug, Clone)]
pub struct AngstromOrderValidatorBuilder {
    chain_spec:                   Arc<ChainSpec>,
    /// The current max gas limit
    block_gas_limit:              u64,
    /// Minimum priority fee to enforce for acceptance into the pool.
    minimum_priority_fee:         Option<u128>,
    /// Determines how many additional tasks to spawn
    ///
    /// Default is 1
    additional_tasks:             usize,
    /// Toggle to determine if a local transaction should be propagated
    propagate_local_transactions: bool
}

impl AngstromOrderValidatorBuilder {
    /// Creates a new builder for the given [ChainSpec]
    pub fn new(chain_spec: Arc<ChainSpec>) -> Self {
        Self {
            chain_spec,
            block_gas_limit: ETHEREUM_BLOCK_GAS_LIMIT,
            minimum_priority_fee: None,
            additional_tasks: 1,
            // default to true, can potentially take this as a param in the future
            propagate_local_transactions: true
        }
    }

    /// Sets toggle to propagate transactions received locally by this client
    /// (e.g transactions from eth_sendTransaction to this nodes' RPC
    /// server)
    ///
    ///  If set to false, only transactions received by network peers (via
    /// p2p) will be marked as propagated in the local transaction pool and
    /// returned on a GetPooledTransactions p2p request
    pub fn set_propagate_local_transactions(mut self, propagate_local_txs: bool) -> Self {
        self.propagate_local_transactions = propagate_local_txs;
        self
    }

    /// Disables propagating transactions received locally by this client
    ///
    /// For more information, check docs for set_propagate_local_transactions
    pub fn no_local_transaction_propagation(mut self) -> Self {
        self.propagate_local_transactions = false;
        self
    }

    /// Sets a minimum priority fee that's enforced for acceptance into the
    /// pool.
    pub fn with_minimum_priority_fee(mut self, minimum_priority_fee: u128) -> Self {
        self.minimum_priority_fee = Some(minimum_priority_fee);
        self
    }

    /// Sets the number of additional tasks to spawn.
    pub fn with_additional_tasks(mut self, additional_tasks: usize) -> Self {
        self.additional_tasks = additional_tasks;
        self
    }

    /// Builds a the [AngstromOrderValidator] and spawns validation tasks via
    /// the [TransactionValidationTaskExecutor]
    ///
    /// The validator will spawn `additional_tasks` additional tasks for
    /// validation.
    ///
    /// By default this will spawn 1 additional task.
    pub fn build_with_tasks<Client, Tx, T>(
        self,
        client: Client,
        tasks: T
    ) -> TransactionValidationTaskExecutor<AngstromOrderValidator<Client, Tx>>
    where
        T: TaskSpawner
    {
        let Self {
            chain_spec,
            block_gas_limit,
            minimum_priority_fee,
            additional_tasks,
            propagate_local_transactions
        } = self;

        let inner = OrderValidatorInner {
            chain_spec,
            client,
            block_gas_limit,
            minimum_priority_fee,
            propagate_local_transactions,
            _marker: Default::default()
        };

        let (tx, task) = ValidationTask::new();

        // Spawn validation tasks, they are blocking because they perform db lookups
        for _ in 0..additional_tasks {
            let task = task.clone();
            tasks.spawn_blocking(Box::pin(async move {
                task.run().await;
            }));
        }

        tasks.spawn_critical_blocking(
            "transaction-validation-service",
            Box::pin(async move {
                task.run().await;
            })
        );

        let to_validation_task = Arc::new(Mutex::new(tx));

        TransactionValidationTaskExecutor {
            validator: AngstromOrderValidator { inner: Arc::new(inner) },
            to_validation_task
        }
    }
}

/// Keeps track of whether certain forks are activated
#[derive(Debug)]
pub(crate) struct ForkTracker {
    /// Tracks if shanghai is activated at the block's timestamp.
    pub(crate) shanghai: AtomicBool,
    /// Tracks if cancun is activated at the block's timestamp.
    pub(crate) cancun:   AtomicBool
}

impl ForkTracker {
    /// Returns true if the Shanghai fork is activated.
    pub(crate) fn is_shanghai_activated(&self) -> bool {
        self.shanghai.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Returns true if the Shanghai fork is activated.
    pub(crate) fn is_cancun_activated(&self) -> bool {
        self.cancun.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Ensure that the code size is not greater than `max_init_code_size`.
/// `max_init_code_size` should be configurable so this will take it as an
/// argument.
pub fn ensure_max_init_code_size<T: PoolOrder>(
    transaction: &T,
    max_init_code_size: usize
) -> Result<(), InValidPoolOrderError> {
    if transaction.kind().is_create() && transaction.input().len() > max_init_code_size {
        Err(InValidPoolOrderError::ExceedsMaxInitCodeSize(transaction.size(), max_init_code_size))
    } else {
        Ok(())
    }
}
