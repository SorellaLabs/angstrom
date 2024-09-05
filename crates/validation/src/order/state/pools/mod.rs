use alloy_primitives::{Address, U256};
use angstrom_pools::AngstromPools;
use angstrom_types::sol_bindings::grouped_orders::{PoolOrder, RawPoolOrder};
use index_to_address::{AssetIndexToAddress, AssetIndexToAddressWrapper};

use super::config::ValidationConfig;

pub mod angstrom_pools;
pub mod index_to_address;

pub trait PoolsTracker: Clone + Send + Unpin {
    /// Returns None if no pool is found
    fn fetch_pool_info_for_order<O: RawPoolOrder>(
        &self,
        order: O
    ) -> Option<(UserOrderPoolInfo, AssetIndexToAddressWrapper<O>)>;
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
    pub asset_index_to_address: AssetIndexToAddress,
    pub pools:                  AngstromPools
}

impl PoolsTracker for AngstromPoolsTracker {
    fn fetch_pool_info_for_order<O: RawPoolOrder>(
        &self,
        order: O
    ) -> Option<(UserOrderPoolInfo, AssetIndexToAddressWrapper<O>)> {
        let wrapped = self.asset_index_to_address.wrap(order)?;
        let (is_bid, pool_id) = self
            .pools
            .order_info(wrapped.token_in(), wrapped.token_out())?;

        let user_info = UserOrderPoolInfo { pool_id, is_bid, token: wrapped.token_in() };

        Some((user_info, wrapped))
    }
}

impl AngstromPoolsTracker {
    pub fn new(config: ValidationConfig) -> Self {
        todo!()
    }
}

#[cfg(test)]
pub mod pool_tracker_mock {
    use alloy_primitives::{Address, FixedBytes};
    use dashmap::DashMap;

    use super::{angstrom_pools::PoolIdWithDirection, *};

    #[derive(Clone)]
    pub struct MockPoolTracker {
        asset_index_to_address: DashMap<u16, Address>,
        pools:                  DashMap<(Address, Address), PoolIdWithDirection>
    }

    impl MockPoolTracker {
        pub fn add_asset(&self, index: u16, address: Address) {
            self.asset_index_to_address.insert(index, address);
        }

        pub fn add_pool(&self, token0: Address, token1: Address, info: PoolIdWithDirection) {
            self.pools.insert((token0, token1), info);
        }
    }

    impl PoolsTracker for MockPoolTracker {
        fn fetch_pool_info_for_order<O: RawPoolOrder>(
            &self,
            order: O
        ) -> Option<(UserOrderPoolInfo, AssetIndexToAddressWrapper<O>)> {
            let token_in = *self
                .asset_index_to_address
                .get(&order.get_token_in())?
                .value();
            let token_out = *self
                .asset_index_to_address
                .get(&order.get_token_out())?
                .value();

            let (is_bid, pool_id) = self.pools.get(&(token_in, token_out))?.value();
            let wrapped = AssetIndexToAddressWrapper { token_out, token_in, order };
            let info = UserOrderPoolInfo { pool_id, is_bid, token: token_in };

            Some((info, wrapped))
        }
    }
}
