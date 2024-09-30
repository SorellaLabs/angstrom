use alloy::primitives::Address;
use angstrom_pools::AngstromPools;
use angstrom_types::{
    primitive::{NewInitializedPool, PoolId},
    sol_bindings::ext::RawPoolOrder
};
use dashmap::DashMap;

use super::config::ValidationConfig;

pub mod angstrom_pools;

pub trait PoolsTracker: Clone + Send + Unpin {
    /// Returns None if no pool is found
    fn fetch_pool_info_for_order<O: RawPoolOrder>(&self, order: &O) -> Option<UserOrderPoolInfo>;

    /// indexes a new pool into the tracker
    fn index_new_pool(&mut self, pool: NewInitializedPool);
}

#[derive(Debug, Clone)]
pub struct UserOrderPoolInfo {
    // token in for pool
    pub token:   Address,
    pub is_bid:  bool,
    pub pool_id: PoolId
}

#[derive(Clone)]
/// keeps track of all valid pools and the mappings of asset id to pool id
pub struct AngstromPoolsTracker {
    /// TODO: we can most likely flatten this but will circle back
    pub pools: AngstromPools
}

impl AngstromPoolsTracker {
    pub fn new(config: ValidationConfig) -> Self {
        let pools = config
            .pools
            .iter()
            .map(|pool| (AngstromPools::build_key(pool.token0, pool.token1), pool.pool_id))
            .collect::<DashMap<_, _>>();
        let angstrom_pools = AngstromPools::new(pools);

        Self { pools: angstrom_pools }
    }

    /// Get the token addresses for a pool specified by Uniswap PoolId.  By
    /// Uniswap convention, these token addresses will be sorted, therefore the
    /// return from this method is `Option<(t0, t1)>`
    pub fn get_pool_addresses(&self, poolid: PoolId) -> Option<(Address, Address)> {
        self.pools.get_addresses(poolid)
    }
}

impl PoolsTracker for AngstromPoolsTracker {
    /// None if no pool was found
    fn fetch_pool_info_for_order<O: RawPoolOrder>(&self, order: &O) -> Option<UserOrderPoolInfo> {
        let (is_bid, pool_id) = self.pools.order_info(order.token_in(), order.token_out())?;

        let user_info = UserOrderPoolInfo { pool_id, is_bid, token: order.token_in() };

        Some(user_info)
    }

    fn index_new_pool(&mut self, pool: NewInitializedPool) {
        self.pools.new_pool(pool);
    }
}

#[cfg(test)]
pub mod pool_tracker_mock {
    use alloy::primitives::{Address, FixedBytes};
    use angstrom_types::primitive::PoolId;
    use dashmap::DashMap;

    use super::*;

    #[derive(Clone, Default)]
    pub struct MockPoolTracker {
        pools: DashMap<(Address, Address), PoolId>
    }

    impl MockPoolTracker {
        pub fn add_pool(&self, token0: Address, token1: Address, pool: PoolId) {
            self.pools.insert((token0, token1), pool);
            self.pools.insert((token1, token0), pool);
        }
    }

    impl PoolsTracker for MockPoolTracker {
        fn fetch_pool_info_for_order<O: RawPoolOrder>(
            &self,
            order: &O
        ) -> Option<UserOrderPoolInfo> {
            let pool_id = self.pools.get(&(order.token_in(), order.token_out()))?;

            let user_info = UserOrderPoolInfo {
                pool_id: *pool_id,
                is_bid:  order.token_in() > order.token_out(),
                token:   order.token_in()
            };

            Some(user_info)
        }

        fn index_new_pool(&mut self, pool: NewInitializedPool) {
            self.pools
                .insert((pool.currency_in, pool.currency_out), pool.id);
        }
    }
}
