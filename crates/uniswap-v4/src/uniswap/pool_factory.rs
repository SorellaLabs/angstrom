use std::{collections::HashMap, sync::Arc};

use alloy::{
    primitives::{Address, aliases::U24},
    providers::Provider
};
use angstrom_types::{
    contract_bindings::angstrom::Angstrom::PoolKey,
    primitive::{PoolId, UniswapPoolRegistry}
};
use futures::future::join_all;

use super::{pool::EnhancedUniswapPool, pool_data_loader::PoolDataLoader};
use crate::DataLoader;

#[derive(Clone)]
pub struct V4PoolFactory<P, const TICKS: u16 = 400> {
    provider:     Arc<P>,
    registry:     UniswapPoolRegistry,
    pool_manager: Address
}
impl<P: Provider + 'static, const TICKS: u16> V4PoolFactory<P, TICKS>
where
    DataLoader: PoolDataLoader
{
    pub fn new(provider: Arc<P>, registry: UniswapPoolRegistry, pool_manager: Address) -> Self {
        Self { provider, registry, pool_manager }
    }

    /// inits all uniswap pools found in [`UniswapPoolRegistry`]
    pub async fn init(&self, block: u64) -> Vec<EnhancedUniswapPool<DataLoader>> {
        join_all(self.registry.pools().keys().map(async |pool_id| {
            let internal = self.registry.conversion_map.get(pool_id).unwrap();

            let mut pool = EnhancedUniswapPool::new(
                DataLoader::new_with_registry(*internal, self.registry.clone(), self.pool_manager),
                TICKS
            );
            pool.initialize(Some(block), self.provider.clone())
                .await
                .expect("failed to init pool");
            pool
        }))
        .await
    }

    pub fn conversion_map(&self) -> &HashMap<PoolId, PoolId> {
        &self.registry.conversion_map
    }

    pub fn remove_pool(&mut self, pool_key: PoolKey) -> PoolId {
        let id = PoolId::from(pool_key);
        let _ = self.registry.pools.remove(&id);
        self.registry.conversion_map.remove(&id).unwrap()
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
        let pub_key = PoolId::from(pool_key.clone());
        self.registry.pools.insert(pub_key, pool_key.clone());

        // priv key
        pool_key.fee = U24::from(0x800000);
        let priv_key = PoolId::from(pool_key.clone());
        self.registry.conversion_map.insert(pub_key, priv_key);

        let internal = self.registry.conversion_map.get(&pub_key).unwrap();

        let mut pool = EnhancedUniswapPool::new(
            DataLoader::new_with_registry(*internal, self.registry.clone(), self.pool_manager),
            TICKS
        );
        pool.initialize(Some(block), self.provider.clone())
            .await
            .expect("failed to init pool");

        pool
    }
}
