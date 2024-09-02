use std::{
    collections::HashMap,
    ops::{Deref, DerefMut}
};

use alloy_primitives::{Address, TxHash, U256};
use angstrom_types::sol_bindings::{
    grouped_orders::{PoolOrder, RawPoolOrder},
    sol::AssetIndex,
    FetchAssetIndexes
};
use dashmap::DashMap;

#[derive(Debug, Default)]
pub struct AssetIndexToAddress(DashMap<u16, Address>);

#[derive(Debug, Clone)]
pub struct AssetIndexToAddressWrapper<Order: RawPoolOrder> {
    pub token_in:  Address,
    pub token_out: Address,
    pub order:     Order
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
