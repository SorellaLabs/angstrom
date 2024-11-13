use alloy::primitives::Address;
use angstrom_pools::AngstromPools;
use angstrom_types::{
    contract_payloads::angstrom::AngstromPoolConfigStore,
    primitive::{NewInitializedPool, PoolId},
    sol_bindings::ext::RawPoolOrder
};
use dashmap::DashMap;

use super::config::PoolConfig;

pub trait PoolsTracker: Send + Unpin {
    /// Returns None if no pool is found
    fn fetch_pool_info_for_order<O: RawPoolOrder>(&self, order: &O) -> Option<UserOrderPoolInfo>;
}

#[derive(Debug, Clone)]
pub struct UserOrderPoolInfo {
    // token in for pool
    pub token:   Address,
    pub is_bid:  bool,
    pub pool_id: PoolId
}

/// keeps track of all valid pools and the mappings of asset id to pool id
pub struct AngstromPoolsTracker {
    angstrom_address: Address,
    pool_store:       AngstromPoolConfigStore
}

impl AngstromPoolsTracker {
    pub fn new(angstrom_address: Address, pool_store: AngstromPoolConfigStore) -> Self {
        Self { angstrom_address, pool_store }
    }

    pub fn get_poolid(&self, mut addr1: Address, mut addr2: Address) -> Option<PoolId> {
        let store = self.pool_store.get_entry(addr1, addr2)?;
        if addr2 < addr1 {
            std::mem::swap(&mut addr1, &mut addr2)
        };

        Some(PoolId::from(PoolKey {
            currency0:   addr1,
            currency1:   addr2,
            tickSpacing: store.tick_spacing,
            hooks:       self.angstrom_address,
            fee:         store.fee_in_e6
        }))
    }

    pub fn order_info(
        &self,
        currency_in: Address,
        currency_out: Address
    ) -> Option<(bool, PoolId)> {
        // Uniswap pools are priced as t1/t0 - the order is a bid if it's offering t1 to
        // get t0.   Uniswap standard has the token addresses sorted and t0 is the
        // lower of the two, therefore if the currency_in is the higher of the two we
        // know it's t1 and therefore this order is a bid.
        let is_bid = currency_in > currency_out;
        let key = self.get_poolid(currency_in, currency_out)?;

        Some((is_bid, key))
    }
}

impl PoolsTracker for AngstromPoolsTracker {
    /// None if no pool was found
    fn fetch_pool_info_for_order<O: RawPoolOrder>(&self, order: &O) -> Option<UserOrderPoolInfo> {
        let (is_bid, pool_id) = self.pools.order_info(order.token_in(), order.token_out())?;

        let user_info = UserOrderPoolInfo { pool_id, is_bid, token: order.token_in() };

        Some(user_info)
    }
}

#[cfg(test)]
pub mod pool_tracker_mock {
    use alloy::primitives::Address;
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
    }
}
