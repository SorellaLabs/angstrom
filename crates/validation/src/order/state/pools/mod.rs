use alloy_primitives::Address;
use angstrom_pools::AngstromPools;
use angstrom_types::sol_bindings::ext::RawPoolOrder;
use dashmap::DashMap;

use super::config::ValidationConfig;

pub mod angstrom_pools;

pub trait PoolsTracker: Clone + Send + Unpin {
    /// Returns None if no pool is found
    fn fetch_pool_info_for_order<O: RawPoolOrder>(
        &self,
        order: O
    ) -> Option<(UserOrderPoolInfo, O)>;
}

#[derive(Debug, Clone)]
pub struct UserOrderPoolInfo {
    // token in for pool
    pub token:   Address,
    pub is_bid:  bool,
    pub pool_id: usize
}

#[derive(Clone)]
/// keeps track of all valid pools and the mappings of asset id to pool id
pub struct AngstromPoolsTracker {
    /// TODO: we can most likely flatten this but will circle back
    pub pools: AngstromPools
}

impl PoolsTracker for AngstromPoolsTracker {
    fn fetch_pool_info_for_order<O: RawPoolOrder>(
        &self,
        order: O
    ) -> Option<(UserOrderPoolInfo, O)> {
        let (is_bid, pool_id) = self.pools.order_info(order.token_in(), order.token_out())?;

        let user_info = UserOrderPoolInfo { pool_id, is_bid, token: order.token_in() };

        Some((user_info, order))
    }
}

impl AngstromPoolsTracker {
    pub fn new(config: ValidationConfig) -> Self {
        let pools = config
            .pools
            .iter()
            .flat_map(|pool| {
                let key0 = AngstromPools::get_key(pool.token0, pool.token1);
                let key1 = AngstromPools::get_key(pool.token1, pool.token0);
                [(key0, (true, pool.pool_id)), (key1, (false, pool.pool_id))]
            })
            .collect::<DashMap<_, _>>();
        let angstrom_pools = AngstromPools::new(pools);

        Self { pools: angstrom_pools }
    }
}

#[cfg(test)]
pub mod pool_tracker_mock {
    use alloy_primitives::Address;
    use dashmap::DashMap;

    use super::{angstrom_pools::PoolIdWithDirection, *};

    #[derive(Clone, Default)]
    pub struct MockPoolTracker {
        asset_index_to_address: DashMap<u16, Address>,
        pools:                  DashMap<(Address, Address), PoolIdWithDirection>
    }

    impl MockPoolTracker {
        pub fn add_asset(&self, index: u16, address: Address) {
            self.asset_index_to_address.insert(index, address);
        }

        pub fn add_pool(&self, token0: Address, token1: Address, pool: u16) {
            self.pools.insert((token0, token1), (true, pool as usize));
            self.pools.insert((token1, token0), (false, pool as usize));
        }
    }

    impl PoolsTracker for MockPoolTracker {
        fn fetch_pool_info_for_order<O: RawPoolOrder>(
            &self,
            order: O
        ) -> Option<(UserOrderPoolInfo, AssetIndexToAddressWrapper<O>)> {
            let asset_in = *self.asset_index_to_address.get(&order.token_in())?.value();
            let asset_out = *self.asset_index_to_address.get(&order.token_out())?.value();

            let value = self.pools.get(&(token_in, token_out))?;
            let (is_bid, pool_id) = value.value();
            let wrapped = AssetIndexToAddressWrapper { asset_out, asset_in, order };
            let info = UserOrderPoolInfo { pool_id: *pool_id, is_bid: *is_bid, token: token_in };

            Some((info, wrapped))
        }
    }
}
