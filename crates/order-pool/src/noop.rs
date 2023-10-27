//! A transaction pool implementation that does nothing.
//!
//! This is useful for wiring components together that don't require an actual
//! pool but still need to be generic over it.

use std::{collections::HashSet, marker::PhantomData, sync::Arc};

use reth_primitives::{Address, PooledTransactionsElement, TxHash};
use tokio::sync::{mpsc, mpsc::Receiver};

use crate::{
    error::PoolError,
    traits::{GetPooledTransactionLimit, TransactionListenerKind},
    AllPoolTransactions, AllTransactionsEvents, AngstromPooledOrder, BestTransactions, BlockInfo,
    NewTransactionEvent, OrderOrigin, OrderPool, OrderValidator, PoolOrder, PoolResult, PoolSize,
    PropagatedTransactions, TransactionEvents, TransactionValidationOutcome, ValidPoolTransaction
};

/// A [`TransactionPool`] implementation that does nothing.
///
/// All transactions are rejected and no events are emitted.
/// This type will never hold any transactions and is only useful for wiring
/// components together.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct NoopOrderPool;

#[async_trait::async_trait]
impl OrderPool for NoopOrderPool {
    type Order = AngstromPooledOrder;

    fn pool_size(&self) -> PoolSize {
        Default::default()
    }

    fn block_info(&self) -> BlockInfo {
        BlockInfo {
            last_seen_block_hash:   Default::default(),
            last_seen_block_number: 0,
            pending_basefee:        0
        }
    }

    async fn add_transaction_and_subscribe(
        &self,
        _origin: OrderOrigin,
        transaction: Self::Order
    ) -> PoolResult<TransactionEvents> {
        let hash = *transaction.hash();
        Err(PoolError::Other(hash, Box::new(NoopInsertError::new(transaction))))
    }

    async fn add_transaction(
        &self,
        _origin: OrderOrigin,
        transaction: Self::Order
    ) -> PoolResult<TxHash> {
        let hash = *transaction.hash();
        Err(PoolError::Other(hash, Box::new(NoopInsertError::new(transaction))))
    }

    async fn add_orders(
        &self,
        _origin: OrderOrigin,
        transactions: Vec<Self::Order>
    ) -> PoolResult<Vec<PoolResult<TxHash>>> {
        Ok(transactions
            .into_iter()
            .map(|transaction| {
                let hash = *transaction.hash();
                Err(PoolError::Other(hash, Box::new(NoopInsertError::new(transaction))))
            })
            .collect())
    }

    fn transaction_event_listener(&self, _tx_hash: TxHash) -> Option<TransactionEvents> {
        None
    }

    fn all_transactions_event_listener(&self) -> AllTransactionsEvents<Self::Order> {
        AllTransactionsEvents { events: mpsc::channel(1).1 }
    }

    fn pending_transactions_listener_for(
        &self,
        _kind: TransactionListenerKind
    ) -> Receiver<TxHash> {
        mpsc::channel(1).1
    }

    fn new_orders_listener(&self) -> Receiver<NewTransactionEvent<Self::Order>> {
        mpsc::channel(1).1
    }

    fn new_orders_listener_for(
        &self,
        _kind: TransactionListenerKind
    ) -> Receiver<NewTransactionEvent<Self::Order>> {
        mpsc::channel(1).1
    }

    fn pooled_transaction_hashes(&self) -> Vec<TxHash> {
        vec![]
    }

    fn pooled_transaction_hashes_max(&self, _max: usize) -> Vec<TxHash> {
        vec![]
    }

    fn pooled_transactions(&self) -> Vec<Arc<ValidPoolTransaction<Self::Order>>> {
        vec![]
    }

    fn pooled_transactions_max(&self, _max: usize) -> Vec<Arc<ValidPoolTransaction<Self::Order>>> {
        vec![]
    }

    fn get_pooled_transaction_elements(
        &self,
        _tx_hashes: Vec<TxHash>,
        _limit: GetPooledTransactionLimit
    ) -> Vec<PooledTransactionsElement> {
        vec![]
    }

    fn best_transactions(
        &self
    ) -> Box<dyn BestTransactions<Item = Arc<ValidPoolTransaction<Self::Order>>>> {
        Box::new(std::iter::empty())
    }

    fn best_transactions_with_base_fee(
        &self,
        _: u64
    ) -> Box<dyn BestTransactions<Item = Arc<ValidPoolTransaction<Self::Order>>>> {
        Box::new(std::iter::empty())
    }

    fn pending_transactions(&self) -> Vec<Arc<ValidPoolTransaction<Self::Order>>> {
        vec![]
    }

    fn queued_transactions(&self) -> Vec<Arc<ValidPoolTransaction<Self::Order>>> {
        vec![]
    }

    fn all_transactions(&self) -> AllPoolTransactions<Self::Order> {
        AllPoolTransactions::default()
    }

    fn remove_transactions(
        &self,
        _hashes: Vec<TxHash>
    ) -> Vec<Arc<ValidPoolTransaction<Self::Order>>> {
        vec![]
    }

    fn retain_unknown(&self, _hashes: &mut Vec<TxHash>) {}

    fn get(&self, _tx_hash: &TxHash) -> Option<Arc<ValidPoolTransaction<Self::Order>>> {
        None
    }

    fn get_all(&self, _txs: Vec<TxHash>) -> Vec<Arc<ValidPoolTransaction<Self::Order>>> {
        vec![]
    }

    fn on_propagated(&self, _txs: PropagatedTransactions) {}

    fn get_transactions_by_sender(
        &self,
        _sender: Address
    ) -> Vec<Arc<ValidPoolTransaction<Self::Order>>> {
        vec![]
    }

    fn unique_senders(&self) -> HashSet<Address> {
        Default::default()
    }
}

/// A [`OrderValidator`] that does nothing.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MockOrderValidator<T> {
    propagate_local: bool,
    _marker:         PhantomData<T>
}

#[async_trait::async_trait]
impl<T: PoolOrder> OrderValidator for MockOrderValidator<T> {
    type Order = T;

    async fn validate_transaction(
        &self,
        origin: OrderOrigin,
        transaction: Self::Order
    ) -> TransactionValidationOutcome<Self::Order> {
        TransactionValidationOutcome::Valid {
            balance: Default::default(),
            state_nonce: 0,
            transaction,
            propagate: match origin {
                OrderOrigin::External => true,
                OrderOrigin::Local => self.propagate_local,
                OrderOrigin::Private => false
            }
        }
    }
}

impl<T> MockOrderValidator<T> {
    /// Creates a new [`MockOrderValidator`] that does not allow local
    /// transactions to be propagated.
    pub fn no_propagate_local() -> Self {
        Self { propagate_local: false, _marker: Default::default() }
    }
}

impl<T> Default for MockOrderValidator<T> {
    fn default() -> Self {
        MockOrderValidator { propagate_local: true, _marker: Default::default() }
    }
}

/// An error that contains the transaction that failed to be inserted into the
/// noop pool.
#[derive(Debug, Clone, thiserror::Error)]
#[error("Can't insert transaction into the noop pool that does nothing.")]
pub struct NoopInsertError {
    tx: AngstromPooledOrder
}

impl NoopInsertError {
    fn new(tx: AngstromPooledOrder) -> Self {
        Self { tx }
    }

    /// Returns the transaction that failed to be inserted.
    pub fn into_inner(self) -> AngstromPooledOrder {
        self.tx
    }
}
