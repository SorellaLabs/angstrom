use alloy::{
    eips::{BlockId, BlockNumberOrTag},
    primitives::{Address, Bytes, FixedBytes, StorageValue, U64, U256},
    providers::{Provider, ProviderCall, ProviderLayer, RootProvider, RpcWithBlock},
    rpc::client::NoParams,
    transports::TransportErrorKind
};
use eyre::Result;
use reth_provider::{
    BlockNumReader, DatabaseProviderFactory, ProviderError, StateProvider,
    TryIntoHistoricalStateProvider
};

pub struct RethDbLayer<DB>
where
    DB: DatabaseProviderFactory<Provider: TryIntoHistoricalStateProvider + BlockNumReader> + Clone
{
    db: DB
}

impl<DB> RethDbLayer<DB>
where
    DB: DatabaseProviderFactory<Provider: TryIntoHistoricalStateProvider + BlockNumReader> + Clone
{
    pub const fn new(db: DB) -> Self {
        Self { db }
    }

    pub(crate) fn db(&self) -> DB {
        self.db.clone()
    }
}

impl<P, DB> ProviderLayer<P> for RethDbLayer<DB>
where
    P: Provider,
    DB: DatabaseProviderFactory<Provider: TryIntoHistoricalStateProvider + BlockNumReader>
        + Clone
        + 'static
{
    type Provider = RethDbProvider<P, DB>;

    fn layer(&self, inner: P) -> Self::Provider {
        RethDbProvider::new(inner, self.db())
    }
}

/// A provider that overrides the vanilla `Provider` trait to get results from
/// the reth-db.
///
/// It holds the `reth_provider::ProviderFactory` that enables read-only access
/// to the database tables and static files.
#[derive(Clone)]
pub struct RethDbProvider<P, DB>
where
    P: Provider,
    DB: DatabaseProviderFactory<Provider: TryIntoHistoricalStateProvider + BlockNumReader> + Clone
{
    inner:            P,
    provider_factory: DbAccessor<DB>
}

impl<P, DB> RethDbProvider<P, DB>
where
    P: Provider,
    DB: DatabaseProviderFactory<Provider: TryIntoHistoricalStateProvider + BlockNumReader> + Clone
{
    /// Create a new `RethDbProvider` instance.
    pub fn new(inner: P, db: DB) -> Self
    where
        DB: DatabaseProviderFactory
    {
        let db_accessor: DbAccessor<DB> = DbAccessor::new(db);
        Self { inner, provider_factory: db_accessor }
    }

    const fn factory(&self) -> &DbAccessor<DB> {
        &self.provider_factory
    }
}

/// Implement the `Provider` trait for the `RethDbProvider` struct.
///
/// This is where we override specific RPC methods to fetch from the reth-db.
impl<P, DB> Provider for RethDbProvider<P, DB>
where
    P: Provider,
    DB: DatabaseProviderFactory<Provider: TryIntoHistoricalStateProvider + BlockNumReader>
        + Clone
        + 'static
{
    fn root(&self) -> &RootProvider {
        self.inner.root()
    }

    /// Override the `get_block_number` method to fetch the latest block number
    /// from the reth-db.
    fn get_block_number(&self) -> ProviderCall<NoParams, U64, u64> {
        let provider = self
            .factory()
            .provider()
            .map_err(TransportErrorKind::custom)
            .unwrap();

        let best = provider
            .best_block_number()
            .map_err(TransportErrorKind::custom);

        ProviderCall::ready(best)
    }

    /// Override the `get_transaction_count` method to fetch the transaction
    /// count of an address.
    ///
    /// `RpcWithBlock` uses `ProviderCall` under the hood.
    fn get_transaction_count(&self, address: Address) -> RpcWithBlock<Address, U64, u64> {
        let this = self.factory().clone();
        RpcWithBlock::new_provider(move |block_id| {
            let provider = this
                .provider_at(block_id)
                .map_err(TransportErrorKind::custom)
                .unwrap();

            let maybe_acc = provider
                .basic_account(&address)
                .map_err(TransportErrorKind::custom)
                .unwrap();

            let nonce = maybe_acc.map(|acc| acc.nonce).unwrap_or_default();

            ProviderCall::ready(Ok(nonce))
        })
    }

    fn get_storage_at(
        &self,
        address: Address,
        key: U256
    ) -> RpcWithBlock<(Address, U256), StorageValue> {
        let this = self.factory().clone();

        RpcWithBlock::new_provider(move |block_id| {
            let provider = this
                .provider_at(block_id)
                .map_err(TransportErrorKind::custom)
                .unwrap();

            ProviderCall::ready(
                provider
                    .storage(address, FixedBytes::from(key))
                    .map(|v| v.unwrap_or_default())
                    .map_err(TransportErrorKind::custom)
            )
        })
    }

    fn get_code_at(&self, address: Address) -> RpcWithBlock<Address, Bytes> {
        let this = self.factory().clone();

        RpcWithBlock::new_provider(move |block_id| {
            let provider = this
                .provider_at(block_id)
                .map_err(TransportErrorKind::custom)
                .unwrap();

            ProviderCall::ready(
                provider
                    .account_code(&address)
                    .map(|f| f.unwrap_or_default().original_bytes())
                    .map_err(TransportErrorKind::custom)
            )
        })
    }

    // TODO: eth_call, raw_call
}

/// A helper type to get the appropriate DB provider.
#[derive(Clone)]
struct DbAccessor<DB>
where
    DB: DatabaseProviderFactory<Provider: TryIntoHistoricalStateProvider + BlockNumReader> + Clone
{
    inner: DB
}

impl<DB> DbAccessor<DB>
where
    DB: DatabaseProviderFactory<Provider: TryIntoHistoricalStateProvider + BlockNumReader> + Clone
{
    const fn new(inner: DB) -> Self {
        Self { inner }
    }

    fn provider(&self) -> Result<DB::Provider, ProviderError> {
        self.inner.database_provider_ro()
    }

    fn provider_at(&self, block_id: BlockId) -> Result<Box<dyn StateProvider>, ProviderError> {
        let provider = self.inner.database_provider_ro()?;

        let block_number = match block_id {
            BlockId::Hash(hash) => {
                if let Some(num) = provider.block_number(hash.into())? {
                    num
                } else {
                    return Err(ProviderError::BlockHashNotFound(hash.into()));
                }
            }
            BlockId::Number(BlockNumberOrTag::Number(num)) => num,
            _ => provider.best_block_number()?
        };

        provider.try_into_history_at_block(block_number)
    }
}
