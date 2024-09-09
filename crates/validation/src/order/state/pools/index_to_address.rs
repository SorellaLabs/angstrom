use std::{
    collections::HashMap,
    ops::{Deref, DerefMut}
};

use alloy_primitives::{Address, TxHash, U256};
use angstrom_types::{
    orders::OrderLocation,
    sol_bindings::{
        grouped_orders::{OrderWithStorageData, PoolOrder, RawPoolOrder},
        sol::AssetIndex,
        FetchAssetIndexes
    }
};
use dashmap::DashMap;
use reth_primitives::B256;

use super::UserOrderPoolInfo;

#[derive(Clone, Debug, Default)]
pub struct AssetIndexToAddress(DashMap<u16, Address>);

#[derive(Debug, Clone)]
pub struct AssetIndexToAddressWrapper<Order: RawPoolOrder> {
    pub token_in:  Address,
    pub token_out: Address,
    pub order:     Order
}

impl<Order: RawPoolOrder> AssetIndexToAddressWrapper<Order> {
    pub fn into_order_storage_with_data(
        self,
        block: u64,
        is_cur_valid: bool,
        is_valid: bool,
        is_limit: bool,
        pool_info: UserOrderPoolInfo,
        invalidates: Vec<B256>
    ) -> OrderWithStorageData<Order> {
        OrderWithStorageData {
            priority_data: angstrom_types::orders::OrderPriorityData {
                price:  self.limit_price(),
                volume: self.amount_in(),
                gas:    0
            },
            pool_id: pool_info.pool_id,
            is_currently_valid: is_cur_valid,
            is_bid: pool_info.is_bid,
            is_valid,
            valid_block: block,
            order_id: angstrom_types::orders::OrderId {
                address:  self.from(),
                pool_id:  pool_info.pool_id,
                hash:     self.hash(),
                nonce:    self.nonce(),
                deadline: self.deadline(),
                location: if is_limit { OrderLocation::Limit } else { OrderLocation::Searcher }
            },
            invalidates,
            order: self.order
        }
    }
}

impl<Order: RawPoolOrder> PoolOrder for AssetIndexToAddressWrapper<Order> {
    fn from(&self) -> Address {
        self.order.from()
    }

    fn token_out(&self) -> Address {
        self.token_out
    }

    fn token_in(&self) -> Address {
        self.token_in
    }

    fn hash(&self) -> TxHash {
        self.order.hash()
    }

    fn nonce(&self) -> U256 {
        self.order.nonce()
    }

    fn deadline(&self) -> U256 {
        self.order.deadline()
    }

    fn amount_in(&self) -> u128 {
        self.order.amount_in()
    }

    fn limit_price(&self) -> u128 {
        self.order.limit_price()
    }

    fn amount_out_min(&self) -> u128 {
        self.order.amount_out_min()
    }
}

impl<Order: RawPoolOrder> Deref for AssetIndexToAddressWrapper<Order> {
    type Target = Order;

    fn deref(&self) -> &Self::Target {
        &self.order
    }
}
impl<Order: RawPoolOrder> DerefMut for AssetIndexToAddressWrapper<Order> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.order
    }
}

impl AssetIndexToAddress {
    pub fn new(map: DashMap<u16, Address>) -> Self {
        Self(map)
    }

    pub fn get_address(&self, asset_index: &u16) -> Option<Address> {
        self.0.get(asset_index).map(|f| *f)
    }

    pub fn wrap<Order: RawPoolOrder>(
        &self,
        order: Order
    ) -> Option<AssetIndexToAddressWrapper<Order>> {
        let token_in = self.get_address(&order.get_token_in())?;
        let token_out = self.get_address(&order.get_token_out())?;

        Some(AssetIndexToAddressWrapper { token_in, token_out, order })
    }
}
