use std::collections::HashMap;

use alloy::primitives::{Address, FixedBytes};
use angstrom_types::primitive::{NewInitializedPool, PoolId};
use dashmap::DashMap;

#[derive(Clone)]
pub struct AngstromPools {
    key_to_id: DashMap<FixedBytes<40>, PoolId>,
    id_to_key: DashMap<PoolId, FixedBytes<40>>
}

impl AngstromPools {
    pub fn new(key_to_id: DashMap<FixedBytes<40>, PoolId>) -> Self {
        // Create our reverse index from our forward index
        let id_to_key = key_to_id
            .iter()
            .map(|item| return (*item.value(), *item.key()))
            .collect();
        AngstromPools { key_to_id, id_to_key }
    }

    pub fn get_poolid(&self, addr1: Address, addr2: Address) -> Option<PoolId> {
        let key = Self::build_key(addr1, addr2);
        self.key_to_id.get(&key).map(|inner| *inner)
    }

    pub fn order_info(
        &self,
        currency_in: Address,
        currency_out: Address
    ) -> Option<(bool, PoolId)> {
        tracing::debug!(shit=?self.key_to_id);
        // Uniswap pools are priced as t1/t0 - the order is a bid if it's offering t1 to
        // get t0.   Uniswap standard has the token addresses sorted and t0 is the
        // lower of the two, therefore if the currency_in is the higher of the two we
        // know it's t1 and therefore this order is a bid.
        let is_bid = currency_in > currency_out;
        self.key_to_id
            .get(&Self::build_key(currency_in, currency_out))
            .map(|inner| (is_bid, *inner))
    }

    /// Get the asset addresses of a pool from the Uniswap PoolId.  By Uniswap
    /// convention, these addresses will always be sorted, therefore the return
    /// type here is `Option<(t0, t1)>`
    pub fn get_addresses(&self, poolid: PoolId) -> Option<(Address, Address)> {
        self.id_to_key.get(&poolid).map(|bytes| {
            // We know these sizes so it's OK
            let addr1: [u8; 20] = bytes.0[0..20].try_into().unwrap();
            let addr2: [u8; 20] = bytes.0[20..].try_into().unwrap();

            (Address::from(addr1), Address::from(addr2))
        })
    }

    #[inline(always)]
    pub fn build_key(addr1: Address, addr2: Address) -> FixedBytes<40> {
        match addr1.cmp(&addr2) {
            std::cmp::Ordering::Greater => FixedBytes::concat_const(addr2.0, addr1.0),
            std::cmp::Ordering::Less => FixedBytes::concat_const(addr1.0, addr2.0),
            // No such thing as a pool between the same token, although I suppose we could also
            // still assemble the key and just fail to find it
            std::cmp::Ordering::Equal => FixedBytes::default()
        }
    }

    pub fn new_pool(&mut self, pool: NewInitializedPool) {
        let (key, id) = (AngstromPools::build_key(pool.currency_in, pool.currency_out), pool.id);
        self.key_to_id.insert(key, id);
        self.id_to_key.insert(id, key);
    }
}
