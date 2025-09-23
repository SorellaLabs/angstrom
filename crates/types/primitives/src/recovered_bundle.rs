use alloy_primitives::Address;
use serde::{Deserialize, Serialize};

use crate::{
    contract_payloads::{angstrom::AngstromBundle, rewards::RewardsUpdate},
    matching::Ray,
    sol_bindings::grouped_orders::AllOrders
};

/// A Angstrom Bundle with All Orders Reconstructed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RecoveredAngstromBundle {
    pub orders: Vec<AllOrders>,
    pub pairs:  Vec<RecoveredPair>
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RecoveredPair {
    pub token0:        Address,
    pub token0_save:   u128,
    pub token0_take:   u128,
    pub token0_settle: u128,

    pub token1:        Address,
    pub token1_save:   u128,
    pub token1_take:   u128,
    pub token1_settle: u128,

    pub price_1_over_0: Ray,

    // swap data
    pub zero_for_one: bool,
    pub swap_in_qty:  u128,
    pub update:       RewardsUpdate
}

impl RecoveredAngstromBundle {
    pub fn recover_bundle(angstrom_bundle: &AngstromBundle, execution_block: u64) -> Self {
        let pairs = RecoveredPair::recover_from_angstrom_bundle(angstrom_bundle);
        let orders = angstrom_bundle
            .top_of_block_orders
            .iter()
            .map(|order| {
                AllOrders::TOB(order.recover_order(
                    &angstrom_bundle.pairs,
                    &angstrom_bundle.assets,
                    execution_block
                ))
            })
            .chain(angstrom_bundle.user_orders.iter().map(|user_order| {
                user_order.recover_order(
                    &angstrom_bundle.pairs,
                    &angstrom_bundle.assets,
                    execution_block
                )
            }))
            .collect::<Vec<_>>();

        Self { orders, pairs }
    }
}

impl RecoveredPair {
    pub fn recover_from_angstrom_bundle(angstrom_bundle: &AngstromBundle) -> Vec<Self> {
        angstrom_bundle
            .pairs
            .iter()
            .enumerate()
            .map(|(i, pair)| {
                let token0_data = &angstrom_bundle.assets[pair.index0 as usize];
                let token1_data = &angstrom_bundle.assets[pair.index1 as usize];
                let price = Ray::from(pair.price_1over0);
                let pool_update = angstrom_bundle
                    .pool_updates
                    .iter()
                    .find(|pool_update| pool_update.pair_index == i as u16)
                    .unwrap();

                Self {
                    token0:         token0_data.addr,
                    token0_save:    token0_data.save,
                    token0_take:    token0_data.take,
                    token0_settle:  token0_data.settle,
                    token1:         token1_data.addr,
                    token1_save:    token1_data.save,
                    token1_take:    token1_data.take,
                    token1_settle:  token1_data.settle,
                    price_1_over_0: price,
                    zero_for_one:   pool_update.zero_for_one,
                    swap_in_qty:    pool_update.swap_in_quantity,
                    update:         pool_update.rewards_update.clone()
                }
            })
            .collect()
    }
}
