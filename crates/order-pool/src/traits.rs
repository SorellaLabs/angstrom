use std::{
    collections::{HashMap, HashSet},
    fmt,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use alloy_rlp::Bytes;
use futures_util::{ready, Stream};
use reth_primitives::{
    AccessList, Address, BlobTransactionSidecar, FromRecoveredPooledTransaction,
    FromRecoveredTransaction, IntoRecoveredTransaction, PeerId, PooledTransactionsElement,
    PooledTransactionsElementEcRecovered, SealedBlock, Transaction, TransactionKind,
    TransactionSignedEcRecovered, TxHash, B256, U256
};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::Receiver;

use crate::{
    error::PoolResult,
    pool::{state::SubPool, TransactionEvents},
    validate::ValidPoolTransaction,
    AllTransactionsEvents
};

/// General purpose abstraction of a transaction-pool.
///
/// This is intended to be used by API-consumers such as RPC that need inject
/// new incoming, unverified transactions. And by block production that needs to
/// get transactions to execute in a new block.
///
/// Note: This requires `Clone` for convenience, since it is assumed that this
/// will be implemented for a wrapped `Arc` type, see also
/// [`Pool`](crate::Pool).
#[async_trait::async_trait]
#[auto_impl::auto_impl(Arc)]
pub trait OrderPool: Send + Sync + Clone {
    /// The regular limit order type
    type Order: PoolOrder;
    /// A composable limit order type that accesses external state through pre
    /// or post hooks
    type ComposableOrder: ComposablePoolOrder;
    /// A searcher order
    type SearcherOrder: SearcherPoolOrder;
    /// A composable limit order type that accesses external state through pre
    /// or post hooks
    type SearcherComposableOrder: SearcherComposablePoolOrder;

    /// Returns stats about the pool and all sub-pools.
    fn pool_size(&self) -> PoolSize;

    /// Returns the block the pool is currently tracking.
    ///
    /// This tracks the block that the pool has last seen.
    fn block_info(&self) -> BlockInfo;

    /// Imports an _external_ transaction.
    ///
    /// This is intended to be used by the network to insert incoming
    /// transactions received over the p2p network.
    ///
    /// Consumer: P2P
    async fn add_external_order(&self, transaction: Self::Order) -> PoolResult<TxHash> {
        self.add_order(OrderOrigin::External, transaction).await
    }

    async fn add_external_searcher_order(
        &self,
        transaction: Self::SearcherOrder
    ) -> PoolResult<TxHash> {
        self.add_searcher_order(OrderOrigin::External, transaction)
            .await
    }

    /// Imports all _external_ transactions
    ///
    ///
    /// Consumer: Utility
    async fn add_external_orders(
        &self,
        orders: Vec<Self::Order>
    ) -> PoolResult<Vec<PoolResult<TxHash>>> {
        self.add_transactions(OrderOrigin::External, transactions)
            .await
    }

    /// Adds an _unvalidated_ transaction into the pool and subscribe to state
    /// changes.
    ///
    /// This is the same as [TransactionPool::add_transaction] but returns an
    /// event stream for the given transaction.
    ///
    /// Consumer: Custom
    async fn add_order_and_subscribe(
        &self,
        origin: OrderOrigin,
        order: Self::Order
    ) -> PoolResult<TransactionEvents>;

    /// Adds an _unvalidated_ limit order into the pool.
    ///
    /// Consumer: RPC
    async fn add_order(&self, origin: OrderOrigin, order: Self::Order) -> PoolResult<TxHash>;

    /// Adds the given _unvalidated_ limit orders into the pool.
    ///
    /// Returns a list of results.
    ///
    /// Consumer: RPC
    async fn add_orders(
        &self,
        origin: OrderOrigin,
        orders: Vec<Self::Order>
    ) -> PoolResult<Vec<PoolResult<TxHash>>>;

    /// Adds the given _unvalidated_ searcher orders into the pool.
    ///
    /// Returns a list of results.
    ///
    /// Consumer: RPC
    async fn add_searcher_orders(
        &self,
        origin: OrderOrigin,
        transactions: Vec<Self::Order>
    ) -> PoolResult<Vec<PoolResult<TxHash>>>;

    /// Adds an _unvalidated_ searcher order into the pool and subscribe to
    /// state changes.
    ///
    /// This is the same as [TransactionPool::add_transaction] but returns an
    /// event stream for the given transaction.
    ///
    /// Consumer: Custom
    async fn add_searcher_order_and_subscribe(
        &self,
        origin: OrderOrigin,
        order: Self::Order
    ) -> PoolResult<TransactionEvents>;

    /// Returns a new order change event stream for the given order.
    ///
    /// Returns `None` if the transaction is not in the pool.
    fn order_event_listener(&self, tx_hash: TxHash) -> Option<TransactionEvents>;

    /// Returns a new order change event stream for _all_  orders in
    /// the pool.
    fn all_orders_event_listener(&self) -> AllTransactionsEvents<Self::Order>;

    /// Returns a new Stream that yields order hashes for new __pending__
    /// transactions inserted into the pool that are allowed to be
    /// propagated.
    ///
    /// Note: This is intended for networking and will __only__ yield
    /// transactions that are allowed to be propagated over the network.
    ///
    /// Consumer: RPC/P2P
    fn pending_orders_listener(&self) -> Receiver<TxHash> {
        self.pending_orders_listener_for(TransactionListenerKind::PropagateOnly)
    }

    /// Returns a new Stream that yields order hashes for new __pending__
    /// transactions inserted into the pool depending on the given
    /// [TransactionListenerKind] argument.
    fn pending_orders_listener_for(&self, kind: TransactionListenerKind) -> Receiver<TxHash>;

    /// Returns a new stream that yields new valid orders added to the
    /// pool.
    fn new_orders_listener(&self) -> Receiver<NewTransactionEvent<Self::Order>> {
        self.new_transactions_listener_for(TransactionListenerKind::PropagateOnly)
    }

    /// Returns a new stream that yields new valid transactions added to the
    /// pool depending on the given [TransactionListenerKind] argument.
    fn new_orders_listener_for(
        &self,
        kind: TransactionListenerKind
    ) -> Receiver<NewTransactionEvent<Self::Order>>;

    /// Returns a new Stream that yields new transactions added to the pending
    /// sub-pool.
    ///
    /// This is a convenience wrapper around [Self::new_transactions_listener]
    /// that filters for [SubPool::Pending](crate::SubPool).
    fn new_pending_pool_orders_listener(&self) -> NewSubpoolTransactionStream<Self::Order> {
        NewSubpoolTransactionStream::new(
            self.new_transactions_listener_for(TransactionListenerKind::PropagateOnly),
            SubPool::Pending
        )
    }

    /// Returns a new Stream that yields new transactions added to the basefee
    /// sub-pool.
    ///
    /// This is a convenience wrapper around [Self::new_transactions_listener]
    /// that filters for [SubPool::BaseFee](crate::SubPool).
    fn new_basefee_pool_orders_listener(&self) -> NewSubpoolTransactionStream<Self::Order> {
        NewSubpoolTransactionStream::new(self.new_transactions_listener(), SubPool::BaseFee)
    }

    /// Returns a new Stream that yields new transactions added to the
    /// queued-pool.
    ///
    /// This is a convenience wrapper around [Self::new_transactions_listener]
    /// that filters for [SubPool::Queued](crate::SubPool).
    fn new_queued_orders_listener(&self) -> NewSubpoolTransactionStream<Self::Order> {
        NewSubpoolTransactionStream::new(self.new_transactions_listener(), SubPool::Queued)
    }

    /// Returns the _hashes_ of all transactions in the pool.
    ///
    /// Note: This returns a `Vec` but should guarantee that all hashes are
    /// unique.
    ///
    /// Consumer: P2P
    fn pooled_order_hashes(&self) -> Vec<TxHash>;

    /// Returns only the first `max` hashes of transactions in the pool.
    ///
    /// Consumer: P2P
    fn pooled_order_hashes_max(&self, max: usize) -> Vec<TxHash>;

    /// Returns the _full_ transaction objects all transactions in the pool.
    ///
    /// This is intended to be used by the network for the initial exchange of
    /// pooled transaction _hashes_
    ///
    /// Note: This returns a `Vec` but should guarantee that all transactions
    /// are unique.
    ///
    /// Caution: In case of blob transactions, this does not include the
    /// sidecar.
    ///
    /// Consumer: P2P
    fn pooled_orders(&self) -> Vec<Arc<ValidPoolTransaction<Self::Order>>>;

    /// Returns only the first `max` transactions in the pool.
    ///
    /// Consumer: P2P
    fn pooled_orders_max(&self, max: usize) -> Vec<Arc<ValidPoolTransaction<Self::Order>>>;

    /// Returns converted [PooledTransactionsElement] for the given transaction
    /// hashes.
    ///
    /// This adheres to the expected behavior of [`GetPooledTransactions`](https://github.com/ethereum/devp2p/blob/master/caps/eth.md#getpooledtransactions-0x09):
    /// The transactions must be in same order as in the request, but it is OK
    /// to skip transactions which are not available.
    ///
    /// If the transaction is a blob transaction, the sidecar will be included.
    ///
    /// Consumer: P2P
    fn get_pooled_orders_elements(
        &self,
        tx_hashes: Vec<TxHash>,
        limit: GetPooledTransactionLimit
    ) -> Vec<PooledTransactionsElement>;

    /// Returns an iterator that yields transactions that are ready for block
    /// production.
    ///
    /// Consumer: Block production
    fn best_orders(
        &self
    ) -> Box<dyn BestTransactions<Item = Arc<ValidPoolTransaction<Self::Order>>>>;

    /// Returns an iterator that yields transactions that are ready for block
    /// production with the given base fee.
    ///
    /// Consumer: Block production
    fn best_orders_with_base_fee(
        &self,
        base_fee: u64
    ) -> Box<dyn BestTransactions<Item = Arc<ValidPoolTransaction<Self::Order>>>>;

    /// Returns all transactions that can be included in the next block.
    ///
    /// This is primarily used for the `txpool_` RPC namespace: <https://geth.ethereum.org/docs/interacting-with-geth/rpc/ns-txpool> which distinguishes between `pending` and `queued` transactions, where `pending` are transactions ready for inclusion in the next block and `queued` are transactions that are ready for inclusion in future blocks.
    ///
    /// Consumer: RPC
    fn pending_orders(&self) -> Vec<Arc<ValidPoolTransaction<Self::Order>>>;

    /// Returns all transactions that can be included in _future_ blocks.
    ///
    /// This and [Self::pending_transactions] are mutually exclusive.
    ///
    /// Consumer: RPC
    fn queued_orders(&self) -> Vec<Arc<ValidPoolTransaction<Self::Order>>>;

    /// Returns all transactions that are currently in the pool grouped by
    /// whether they are ready for inclusion in the next block or not.
    ///
    /// This is primarily used for the `txpool_` namespace: <https://geth.ethereum.org/docs/interacting-with-geth/rpc/ns-txpool>
    ///
    /// Consumer: RPC
    fn all_orders(&self) -> AllPoolTransactions<Self::Order>;

    /// Removes all transactions corresponding to the given hashes.
    ///
    /// Also removes all _dependent_ transactions.
    ///
    /// Consumer: Block production
    fn remove_orders(&self, hashes: Vec<TxHash>) -> Vec<Arc<ValidPoolTransaction<Self::Order>>>;

    /// Retains only those hashes that are unknown to the pool.
    /// In other words, removes all transactions from the given set that are
    /// currently present in the pool.
    ///
    /// Consumer: P2P
    fn retain_unknown(&self, hashes: &mut Vec<TxHash>);

    /// Returns if the transaction for the given hash is already included in
    /// this pool.
    fn contains(&self, tx_hash: &TxHash) -> bool {
        self.get(tx_hash).is_some()
    }

    /// Returns the transaction for the given hash.
    fn get(&self, tx_hash: &TxHash) -> Option<Arc<ValidPoolTransaction<Self::Order>>>;

    /// Returns all transactions objects for the given hashes.
    ///
    /// Caution: This in case of blob transactions, this does not include the
    /// sidecar.
    fn get_all(&self, txs: Vec<TxHash>) -> Vec<Arc<ValidPoolTransaction<Self::Order>>>;

    /// Notify the pool about transactions that are propagated to peers.
    ///
    /// Consumer: P2P
    fn on_propagated(&self, txs: PropagatedTransactions);

    /// Returns all transactions sent by a given user
    fn get_orders_by_sender(&self, sender: Address) -> Vec<Arc<ValidPoolTransaction<Self::Order>>>;

    /// Returns a set of all senders of transactions in the pool
    fn unique_senders(&self) -> HashSet<Address>;
}

/// Extension for [TransactionPool] trait that allows to set the current block
/// info.
#[auto_impl::auto_impl(Arc)]
pub trait OrderPoolExt: OrderPool {
    /// Sets the current block info for the pool.
    fn set_block_info(&self, info: BlockInfo);

    /// Event listener for when the pool needs to be updated
    ///
    /// Implementers need to update the pool accordingly.
    /// For example the base fee of the pending block is determined after a
    /// block is mined which affects the dynamic fee requirement of pending
    /// transactions in the pool.
    fn on_canonical_state_change(&self, update: CanonicalStateUpdate<'_>);

    /// Updates the accounts in the pool
    fn update_accounts(&self, accounts: Vec<ChangedAccount>);
}

/// Determines what kind of new transactions should be emitted by a stream of
/// transactions.
///
/// This gives control whether to include transactions that are allowed to be
/// propagated.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TransactionListenerKind {
    /// Any new pending transactions
    All,
    /// Only transactions that are allowed to be propagated.
    ///
    /// See also [ValidPoolTransaction]
    PropagateOnly
}

impl TransactionListenerKind {
    /// Returns true if we're only interested in transactions that are allowed
    /// to be propagated.
    #[inline]
    pub fn is_propagate_only(&self) -> bool {
        matches!(self, Self::PropagateOnly)
    }
}

/// A Helper type that bundles all transactions in the pool.
#[derive(Debug, Clone)]
pub struct AllPoolTransactions<T: PoolOrder> {
    /// Transactions that are ready for inclusion in the next block.
    pub pending: Vec<Arc<ValidPoolTransaction<T>>>,
    /// Transactions that are ready for inclusion in _future_ blocks, but are
    /// currently parked, because they depend on other transactions that are
    /// not yet included in the pool (nonce gap) or otherwise blocked.
    pub queued:  Vec<Arc<ValidPoolTransaction<T>>>
}

// === impl AllPoolTransactions ===

impl<T: PoolOrder> AllPoolTransactions<T> {
    /// Returns an iterator over all pending [TransactionSignedEcRecovered]
    /// transactions.
    pub fn pending_recovered(&self) -> impl Iterator<Item = TransactionSignedEcRecovered> + '_ {
        self.pending
            .iter()
            .map(|tx| tx.transaction.to_recovered_transaction())
    }

    /// Returns an iterator over all queued [TransactionSignedEcRecovered]
    /// transactions.
    pub fn queued_recovered(&self) -> impl Iterator<Item = TransactionSignedEcRecovered> + '_ {
        self.queued
            .iter()
            .map(|tx| tx.transaction.to_recovered_transaction())
    }
}

impl<T: PoolOrder> Default for AllPoolTransactions<T> {
    fn default() -> Self {
        Self { pending: Default::default(), queued: Default::default() }
    }
}

/// Represents a transaction that was propagated over the network.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct PropagatedTransactions(pub HashMap<TxHash, Vec<PropagateKind>>);

/// Represents how a transaction was propagated over the network.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PropagateKind {
    /// The full transaction object was sent to the peer.
    ///
    /// This is equivalent to the `Transaction` message
    Full(PeerId),
    /// Only the Hash was propagated to the peer.
    Hash(PeerId)
}

// === impl PropagateKind ===

impl PropagateKind {
    /// Returns the peer the transaction was sent to
    pub fn peer(&self) -> &PeerId {
        match self {
            PropagateKind::Full(peer) => peer,
            PropagateKind::Hash(peer) => peer
        }
    }
}

impl From<PropagateKind> for PeerId {
    fn from(value: PropagateKind) -> Self {
        match value {
            PropagateKind::Full(peer) => peer,
            PropagateKind::Hash(peer) => peer
        }
    }
}

/// Represents a new transaction
#[derive(Debug)]
pub struct NewTransactionEvent<T: PoolOrder> {
    /// The pool which the transaction was moved to.
    pub subpool:     SubPool,
    /// Actual transaction
    pub transaction: Arc<ValidPoolTransaction<T>>
}

impl<T: PoolOrder> Clone for NewTransactionEvent<T> {
    fn clone(&self) -> Self {
        Self { subpool: self.subpool, transaction: self.transaction.clone() }
    }
}

/// Where the transaction originates from.
///
/// Depending on where the transaction was picked up, it affects how the
/// transaction is handled internally, e.g. limits for simultaneous transaction
/// of one sender.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum OrderOrigin {
    /// Transaction is coming from a local source.
    Local,
    /// Transaction has been received externally.
    ///
    /// This is usually considered an "untrusted" source, for example received
    /// from another in the network.
    External,
    /// Transaction is originated locally and is intended to remain private.
    ///
    /// This type of transaction should not be propagated to the network. It's
    /// meant for private usage within the local node only.
    //TODO: Implement only send to leader if non leader for composable txs
    Private
}

// === impl OrderOrigin ===

impl OrderOrigin {
    /// Whether the transaction originates from a local source.
    pub fn is_local(&self) -> bool {
        matches!(self, OrderOrigin::Local)
    }
}

/// Represents changes after a new canonical block or range of canonical blocks
/// was added to the chain.
///
/// It is expected that this is only used if the added blocks are canonical to
/// the pool's last known block hash. In other words, the first added block of
/// the range must be the child of the last known block hash.
///
/// This is used to update the pool state accordingly.
#[derive(Debug, Clone)]
pub struct CanonicalStateUpdate<'a> {
    /// Hash of the tip block.
    pub new_tip:                &'a SealedBlock,
    /// EIP-1559 Base fee of the _next_ (pending) block
    ///
    /// The base fee of a block depends on the utilization of the last block and
    /// its base fee.
    pub pending_block_base_fee: u64,

    /// A set of changed accounts across a range of blocks.
    pub changed_accounts:   Vec<ChangedAccount>,
    /// All mined transactions in the block range.
    pub mined_transactions: Vec<B256>
}

impl<'a> CanonicalStateUpdate<'a> {
    /// Returns the number of the tip block.
    pub fn number(&self) -> u64 {
        self.new_tip.number
    }

    /// Returns the hash of the tip block.
    pub fn hash(&self) -> B256 {
        self.new_tip.hash
    }

    /// Timestamp of the latest chain update
    pub fn timestamp(&self) -> u64 {
        self.new_tip.timestamp
    }

    /// Returns the block info for the tip block.
    pub fn block_info(&self) -> BlockInfo {
        BlockInfo {
            last_seen_block_hash:   self.hash(),
            last_seen_block_number: self.number(),
            pending_basefee:        self.pending_block_base_fee
        }
    }
}

impl<'a> fmt::Display for CanonicalStateUpdate<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{{ hash: {}, number: {}, pending_block_base_fee: {}, changed_accounts: {}, \
             mined_transactions: {} }}",
            self.hash(),
            self.number(),
            self.pending_block_base_fee,
            self.changed_accounts.len(),
            self.mined_transactions.len()
        )
    }
}

/// Represents a changed account
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ChangedAccount {
    /// The address of the account.
    pub address: Address,
    /// Account nonce.
    pub nonce:   u64,
    /// Account balance.
    pub balance: U256
}

// === impl ChangedAccount ===

impl ChangedAccount {
    /// Creates a new `ChangedAccount` with the given address and 0 balance and
    /// nonce.
    pub(crate) fn empty(address: Address) -> Self {
        Self { address, nonce: 0, balance: U256::ZERO }
    }
}

/// An `Iterator` that only returns transactions that are ready to be executed.
///
/// This makes no assumptions about the order of the transactions, but expects
/// that _all_ transactions are valid (no nonce gaps.) for the tracked state of
/// the pool.
///
/// Note: this iterator will always return the best transaction that it
/// currently knows. There is no guarantee transactions will be returned
/// sequentially in decreasing priority order.
pub trait BestTransactions: Iterator + Send {
    /// Mark the transaction as invalid.
    ///
    /// Implementers must ensure all subsequent transaction _don't_ depend on
    /// this transaction. In other words, this must remove the given
    /// transaction _and_ drain all transaction that depend on it.
    fn mark_invalid(&mut self, transaction: &Self::Item);

    /// An iterator may be able to receive additional pending transactions that
    /// weren't present it the pool when it was created.
    ///
    /// This ensures that iterator will return the best transaction that it
    /// currently knows and not listen to pool updates.
    fn no_updates(&mut self);
}

/// A no-op implementation that yields no transactions.
impl<T> BestTransactions for std::iter::Empty<T> {
    fn mark_invalid(&mut self, _tx: &T) {}

    fn no_updates(&mut self) {}
}

/// A Helper type that bundles best transactions attributes together.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct BestTransactionsAttributes {
    /// The base fee attribute for best transactions.
    pub basefee: u64
}

// === impl BestTransactionsAttributes ===

impl BestTransactionsAttributes {
    /// Creates a new `BestTransactionsAttributes` with the given basefee and
    /// blob fee.
    pub fn new(basefee: u64) -> Self {
        Self { basefee }
    }

    /// Creates a new `BestTransactionsAttributes` with the given basefee.
    pub fn base_fee(basefee: u64) -> Self {
        Self::new(basefee)
    }
}

pub trait ComposablePoolOrder: PoolOrder {}
pub trait SearcherPoolOrder: PoolOrder {
    /// Value of the searcher bid
    fn lvr_bid(&self) -> u64;

    /// Proportion
    fn bid_proportion(&self) -> u64;

    /// The pool's price post arbitrage
    fn pool_price(&self) -> u64;
}

pub trait SearcherComposablePoolOrder: SearcherPoolOrder {
    /// Builder Bribe
    fn coinbase_tip(&self) -> u64;

    fn pre_hook_access_list(&self) -> AccessList;

    fn post_hook_access_list(&self) -> AccessList;
}

/// Trait for transaction types used inside the pool
pub trait PoolOrder:
    fmt::Debug
    + Send
    + Sync
    + FromRecoveredPooledTransaction
    + FromRecoveredTransaction
    + IntoRecoveredTransaction
{
    /// Hash of the Order
    fn hash(&self) -> &TxHash;

    /// The Sender of the Order
    fn sender(&self) -> Address;

    /// Returns the nonce for this order
    fn nonce(&self) -> u64;

    /// Returns the cost that this transaction is allowed to consume:
    ///
    /// For EIP-1559 transactions: `max_fee_per_gas * gas_limit + tx_value`.
    /// For legacy transactions: `gas_price * gas_limit + tx_value`.
    /// For EIP-4844 blob transactions: `max_fee_per_gas * gas_limit + tx_value
    /// + max_blob_fee_per_gas * blob_gas_used`.
    fn cost(&self) -> U256;

    /// Amount of gas that should be used in executing this order. This is
    /// paid up-front.
    fn gas_limit(&self) -> u64;

    /// Returns the maximum fee per gas the caller is willing to
    /// pay.
    ///
    /// For legacy transactions this is gas_price.
    ///
    /// This is also commonly referred to as the "Gas Fee Cap" (`GasFeeCap`).
    // TODO: We will reimplement this but as a max_fee_per_value where they can
    // comnunicate their max gas per value cleared based in the numeraire of the
    // pair which seems especially useful for limit orders
    fn max_fee_per_gas(&self) -> u128;

    /// Returns the access_list for the particular transaction type.
    /// For Legacy transactions, returns default.
    fn access_list(&self) -> Option<&AccessList>;

    /// Returns the EIP-1559 Priority fee the caller is paying to the block
    /// author.
    ///
    /// This will return `None` for non-EIP1559 transactions
    fn max_priority_fee_per_gas(&self) -> Option<u128>;

    /// Returns the effective tip for this transaction.
    ///
    /// For EIP-1559 transactions: `min(max_fee_per_gas - base_fee,
    /// max_priority_fee_per_gas)`. For legacy transactions: `gas_price -
    /// base_fee`.
    fn effective_tip_per_gas(&self, base_fee: u64) -> Option<u128>;

    /// Returns the max priority fee per gas if the transaction is an EIP-1559
    /// transaction, and otherwise returns the gas price.
    fn priority_fee_or_price(&self) -> u128;

    /// Returns the transaction's [`TransactionKind`], which is the address of
    /// the recipient or [`TransactionKind::Create`] if the transaction is a
    /// contract creation.
    fn kind(&self) -> &TransactionKind;

    /// Returns the recipient of the transaction if it is not a
    /// [TransactionKind::Create] transaction.
    fn to(&self) -> Option<Address> {
        (*self.kind()).to()
    }

    /// Returns the input data of this transaction.
    fn input(&self) -> &[u8];

    /// Returns a measurement of the heap usage of this type and all its
    /// internals.
    fn size(&self) -> usize;

    /// Returns the transaction type
    fn tx_type(&self) -> u8;

    /// Returns the length of the rlp encoded transaction object
    ///
    /// Note: Implementations should cache this value.
    fn encoded_length(&self) -> usize;

    /// Returns chain_id
    fn chain_id(&self) -> Option<u64>;
}

/// The default [PoolTransaction] for the [Pool](crate::Pool) for Ethereum.
///
/// This type is essentially a wrapper around [TransactionSignedEcRecovered]
/// with additional fields derived from the transaction that are frequently used
/// by the pools for ordering.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AngstromPooledOrder {
    /// EcRecovered transaction info
    pub(crate) transaction: TransactionSignedEcRecovered,

    /// For EIP-1559 transactions: `max_fee_per_gas * gas_limit + tx_value`.
    /// For legacy transactions: `gas_price * gas_limit + tx_value`.
    /// For EIP-4844 blob transactions: `max_fee_per_gas * gas_limit + tx_value
    /// + max_blob_fee_per_gas * blob_gas_used`.
    pub(crate) cost: U256,

    /// This is the RLP length of the transaction, computed when the transaction
    /// is added to the pool.
    pub(crate) encoded_length: usize
}

impl AngstromPooledOrder {
    /// Create new instance of [Self].
    //TODO: modify order types
    pub fn new(transaction: TransactionSignedEcRecovered, encoded_length: usize) -> Self {
        let gas_cost = match &transaction.transaction {
            Transaction::Legacy(t) => U256::from(t.gas_price) * U256::from(t.gas_limit),
            Transaction::Eip2930(t) => U256::from(t.gas_price) * U256::from(t.gas_limit),
            Transaction::Eip1559(t) => U256::from(t.max_fee_per_gas) * U256::from(t.gas_limit),
            _ => todo!()
        };
        let mut cost: U256 = transaction.value().into();
        cost += gas_cost;

        if let Some(blob_tx) = transaction.as_eip4844() {
            // add max blob cost
            cost += U256::from(blob_tx.max_fee_per_blob_gas * blob_tx.blob_gas() as u128);
        }

        Self { transaction, cost, encoded_length }
    }

    /// Return the reference to the underlying transaction.
    pub fn transaction(&self) -> &TransactionSignedEcRecovered {
        &self.transaction
    }
}

/// Conversion from the network transaction type to the pool transaction type.
impl From<PooledTransactionsElementEcRecovered> for AngstromPooledOrder {
    fn from(tx: PooledTransactionsElementEcRecovered) -> Self {
        let encoded_length = tx.length_without_header();
        let (tx, signer) = tx.into_components();
        AngstromPooledOrder::new(tx.into_ecrecovered_transaction(signer), encoded_length)
    }
}

impl PoolOrder for AngstromPooledOrder {
    /// Returns hash of the transaction.
    fn hash(&self) -> &TxHash {
        self.transaction.hash_ref()
    }

    /// Returns the Sender of the transaction.
    fn sender(&self) -> Address {
        self.transaction.signer()
    }

    /// Returns the nonce for this transaction.
    fn nonce(&self) -> u64 {
        self.transaction.nonce()
    }

    /// Returns the cost that this transaction is allowed to consume:
    ///
    /// For EIP-1559 transactions: `max_fee_per_gas * gas_limit + tx_value`.
    /// For legacy transactions: `gas_price * gas_limit + tx_value`.
    /// For EIP-4844 blob transactions: `max_fee_per_gas * gas_limit + tx_value
    /// + max_blob_fee_per_gas * blob_gas_used`.
    fn cost(&self) -> U256 {
        self.cost
    }

    /// Amount of gas that should be used in executing this transaction. This is
    /// paid up-front.
    fn gas_limit(&self) -> u64 {
        self.transaction.gas_limit()
    }

    /// Returns the EIP-1559 Max base fee the caller is willing to pay.
    ///
    /// For legacy transactions this is gas_price.
    ///
    /// This is also commonly referred to as the "Gas Fee Cap" (`GasFeeCap`).
    fn max_fee_per_gas(&self) -> u128 {
        match &self.transaction.transaction {
            Transaction::Legacy(tx) => tx.gas_price,
            Transaction::Eip2930(tx) => tx.gas_price,
            Transaction::Eip1559(tx) => tx.max_fee_per_gas,
            Transaction::Eip4844(tx) => tx.max_fee_per_gas
        }
    }

    /// Returns the EIP-1559 Priority fee the caller is paying to the block
    /// author.
    ///
    /// This will return `None` for non-EIP1559 transactions
    fn max_priority_fee_per_gas(&self) -> Option<u128> {
        match &self.transaction.transaction {
            Transaction::Legacy(_) => None,
            Transaction::Eip2930(_) => None,
            Transaction::Eip1559(tx) => Some(tx.max_priority_fee_per_gas),
            Transaction::Eip4844(tx) => Some(tx.max_priority_fee_per_gas)
        }
    }

    fn access_list(&self) -> Option<&AccessList> {
        self.transaction.access_list()
    }

    /// Returns the effective tip for this transaction.
    ///
    /// For EIP-1559 transactions: `min(max_fee_per_gas - base_fee,
    /// max_priority_fee_per_gas)`. For legacy transactions: `gas_price -
    /// base_fee`.
    fn effective_tip_per_gas(&self, base_fee: u64) -> Option<u128> {
        self.transaction.effective_tip_per_gas(base_fee)
    }

    /// Returns the max priority fee per gas if the transaction is an EIP-1559
    /// transaction, and otherwise returns the gas price.
    fn priority_fee_or_price(&self) -> u128 {
        self.transaction.priority_fee_or_price()
    }

    /// Returns the transaction's [`TransactionKind`], which is the address of
    /// the recipient or [`TransactionKind::Create`] if the transaction is a
    /// contract creation.
    fn kind(&self) -> &TransactionKind {
        self.transaction.kind()
    }

    fn input(&self) -> &[u8] {
        self.transaction.input().as_ref()
    }

    /// Returns a measurement of the heap usage of this type and all its
    /// internals.
    fn size(&self) -> usize {
        self.transaction.transaction.input().len()
    }

    /// Returns the transaction type
    fn tx_type(&self) -> u8 {
        self.transaction.tx_type().into()
    }

    /// Returns the length of the rlp encoded object
    fn encoded_length(&self) -> usize {
        self.encoded_length
    }

    /// Returns chain_id
    fn chain_id(&self) -> Option<u64> {
        self.transaction.chain_id()
    }
}

impl FromRecoveredTransaction for AngstromPooledOrder {
    fn from_recovered_transaction(tx: TransactionSignedEcRecovered) -> Self {
        // CAUTION: this should not be done for EIP-4844 transactions, as the blob
        // sidecar is missing.
        let encoded_length = tx.length_without_header();
        AngstromPooledOrder::new(tx, encoded_length)
    }
}

impl FromRecoveredPooledTransaction for AngstromPooledOrder {
    fn from_recovered_transaction(tx: PooledTransactionsElementEcRecovered) -> Self {
        AngstromPooledOrder::from(tx)
    }
}

impl IntoRecoveredTransaction for AngstromPooledOrder {
    fn to_recovered_transaction(&self) -> TransactionSignedEcRecovered {
        self.transaction.clone()
    }
}

/// Represents the current status of the pool.
#[derive(Debug, Clone, Default)]
pub struct PoolSize {
    /// Number of transactions in the _pending_ sub-pool.
    pub pending:      usize,
    /// Reported size of transactions in the _pending_ sub-pool.
    pub pending_size: usize,
    /// Number of transactions in the _basefee_ pool.
    pub basefee:      usize,
    /// Reported size of transactions in the _basefee_ sub-pool.
    pub basefee_size: usize,
    /// Number of transactions in the _queued_ sub-pool.
    pub queued:       usize,
    /// Reported size of transactions in the _queued_ sub-pool.
    pub queued_size:  usize,
    /// Number of all transactions of all sub-pools
    ///
    /// Note: this is the sum of ```pending + basefee + queued```
    pub total:        usize
}

// === impl PoolSize ===

impl PoolSize {
    /// Asserts that the invariants of the pool size are met.
    #[cfg(test)]
    pub(crate) fn assert_invariants(&self) {
        assert_eq!(self.total, self.pending + self.basefee + self.queued);
    }
}

/// Represents the current status of the pool.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BlockInfo {
    /// Hash for the currently tracked block.
    pub last_seen_block_hash:   B256,
    /// Current the currently tracked block.
    pub last_seen_block_number: u64,
    /// Currently enforced base fee: the threshold for the basefee sub-pool.
    ///
    /// Note: this is the derived base fee of the _next_ block that builds on
    /// the block the pool is currently tracking.
    pub pending_basefee:        u64
}

/// The limit to enforce for [TransactionPool::get_pooled_transaction_elements].
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GetPooledTransactionLimit {
    /// No limit, return all transactions.
    None,
    /// Enforce a size limit on the returned transactions, for example 2MB
    SizeSoftLimit(usize)
}

impl GetPooledTransactionLimit {
    /// Returns true if the given size exceeds the limit.
    #[inline]
    pub fn exceeds(&self, size: usize) -> bool {
        match self {
            GetPooledTransactionLimit::None => false,
            GetPooledTransactionLimit::SizeSoftLimit(limit) => size > *limit
        }
    }
}

/// A Stream that yields full transactions the subpool
#[must_use = "streams do nothing unless polled"]
#[derive(Debug)]
pub struct NewSubpoolTransactionStream<Tx: PoolOrder> {
    st:      Receiver<NewTransactionEvent<Tx>>,
    subpool: SubPool
}

// === impl NewSubpoolTransactionStream ===

impl<Tx: PoolOrder> NewSubpoolTransactionStream<Tx> {
    /// Create a new stream that yields full transactions from the subpool
    pub fn new(st: Receiver<NewTransactionEvent<Tx>>, subpool: SubPool) -> Self {
        Self { st, subpool }
    }
}

impl<Tx: PoolOrder> Stream for NewSubpoolTransactionStream<Tx> {
    type Item = NewTransactionEvent<Tx>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match ready!(self.st.poll_recv(cx)) {
                Some(event) => {
                    if event.subpool == self.subpool {
                        return Poll::Ready(Some(event))
                    }
                }
                None => return Poll::Ready(None)
            }
        }
    }
}
