use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use alloy::{
    primitives::{Address, aliases::U24},
    providers::Provider
};
use angstrom_types::{
    contract_bindings::angstrom::Angstrom::PoolKey,
    primitive::{PoolId, UniswapPoolRegistry},
    provider::{EthNetworkProvider, NetworkProvider}
};
use futures::future::join_all;

use super::{pool::EnhancedUniswapPool, pool_data_loader::PoolDataLoader};
use crate::DataLoader;

pub const INITIAL_TICKS_PER_SIDE: u16 = 400;

#[derive(Clone)]
pub struct V4PoolFactory<P, N = EthNetworkProvider, const TICKS: u16 = INITIAL_TICKS_PER_SIDE> {
    provider:     Arc<P>,
    registry:     UniswapPoolRegistry,
    pool_manager: Address,

    _phantom: PhantomData<N>
}
impl<P, N, const TICKS: u16> V4PoolFactory<P, N, TICKS>
where
    P: Provider<N::Network> + 'static,
    N: NetworkProvider,
    DataLoader: PoolDataLoader
{
    pub fn new(provider: Arc<P>, registry: UniswapPoolRegistry, pool_manager: Address) -> Self {
        Self { provider, registry, pool_manager, _phantom: PhantomData }
    }

    /// inits all uniswap pools found in [`UniswapPoolRegistry`]
    pub async fn init(&self, block: u64) -> Vec<EnhancedUniswapPool<DataLoader>> {
        join_all(self.registry.pools().keys().map(async |pool_id| {
            let internal = self
                .registry
                .conversion_map
                .get(pool_id)
                .expect("factory conversion map failure");

            let mut pool = EnhancedUniswapPool::new(
                DataLoader::new_with_registry(
                    *internal,
                    *pool_id,
                    self.registry.clone(),
                    self.pool_manager
                ),
                TICKS
            );
            pool.initialize::<P, N>(Some(block), self.provider.clone())
                .await
                .expect("failed to init pool");
            pool
        }))
        .await
    }

    pub fn current_pool_keys(&self) -> Vec<PoolKey> {
        self.registry.pools.values().cloned().collect()
    }

    pub fn conversion_map(&self) -> &HashMap<PoolId, PoolId> {
        &self.registry.conversion_map
    }

    pub fn remove_pool(&mut self, pool_key: PoolKey) -> PoolId {
        let id = PoolId::from(pool_key);
        let _ = self.registry.pools.remove(&id);
        self.registry
            .conversion_map
            .remove(&id)
            .expect("failed to remove pool id in factory");
        id
    }

    pub async fn create_new_angstrom_pool(
        &mut self,
        mut pool_key: PoolKey,
        block: u64
    ) -> EnhancedUniswapPool<DataLoader>
    where
        DataLoader: PoolDataLoader
    {
        // add to registry
        let pub_key = PoolId::from(pool_key);
        self.registry.pools.insert(pub_key, pool_key);

        // priv key
        pool_key.fee = U24::from(0x800000);
        let priv_key = PoolId::from(pool_key);
        self.registry.conversion_map.insert(pub_key, priv_key);

        let internal = self
            .registry
            .conversion_map
            .get(&pub_key)
            .expect("new angstrom pool not in conversion map");

        let mut pool = EnhancedUniswapPool::new(
            DataLoader::new_with_registry(
                *internal,
                pub_key,
                self.registry.clone(),
                self.pool_manager
            ),
            TICKS
        );
        pool.initialize::<P, N>(Some(block), self.provider.clone())
            .await
            .expect("failed to init pool");

        pool
    }
}
