use std::{collections::HashMap, hash::Hash};

use alloy::{
    eips::BlockId,
    network::Network,
    primitives::{keccak256, Address, Bytes, B256, U256},
    providers::Provider,
    sol_types::SolValue,
    transports::Transport
};
use pade_macro::{PadeDecode, PadeEncode};
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::{
    asset::builder::{AssetBuilder, AssetBuilderStage},
    rewards::PoolUpdate,
    tob::ToBOutcome,
    Asset, Pair, POOL_CONFIG_STORE_ENTRY_SIZE
};
use crate::{
    consensus::{PreProposal, Proposal},
    matching::{uniswap::PoolSnapshot, Ray},
    orders::{OrderFillState, OrderOutcome},
    primitive::{PoolId, PoolKey, UniswapPoolRegistry},
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder as RpcTopOfBlockOrder,
        RawPoolOrder
    }
};

// This currently exists in types::sol_bindings as well, but that one is
// outdated so I'm building a new one here for now and then migrating
#[derive(
    PadeEncode, PadeDecode, Clone, Default, Debug, Hash, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct TopOfBlockOrder {
    pub use_internal:    bool,
    pub quantity_in:     u128,
    pub quantity_out:    u128,
    pub asset_in_index:  u16,
    pub asset_out_index: u16,
    pub recipient:       Option<Address>,
    pub hook_data:       Option<Bytes>,
    pub signature:       Bytes
}

impl TopOfBlockOrder {
    // eip-712 hash_struct
    pub fn order_hash(&self) -> B256 {
        keccak256(&self.signature)
    }

    pub fn of(
        internal: &OrderWithStorageData<RpcTopOfBlockOrder>,
        asset_in_index: u16,
        asset_out_index: u16
    ) -> Self {
        let quantity_in = internal.quantityIn;
        let quantity_out = internal.quantityOut;
        let recipient = Some(internal.recipient);
        let hook_data = Some(internal.hookPayload.clone());
        let signature = internal.meta.signature.clone();
        Self {
            use_internal: false,
            quantity_in,
            quantity_out,
            asset_in_index,
            asset_out_index,
            recipient,
            hook_data,
            signature
        }
    }
}

#[derive(Debug, PadeEncode, PadeDecode)]
pub struct StandingValidation {
    nonce:    u64,
    // 40 bits wide in reality
    #[pade_width(5)]
    deadline: u64
}

#[derive(Debug, PadeEncode, PadeDecode)]
pub enum OrderQuantities {
    Exact { quantity: u128 },
    Partial { min_quantity_in: u128, max_quantity_in: u128, filled_quantity: u128 }
}

#[derive(Debug, PadeEncode, PadeDecode)]
pub struct UserOrder {
    pub use_internal:        bool,
    pub pair_index:          u16,
    pub min_price:           alloy::primitives::U256,
    pub recipient:           Option<Address>,
    pub hook_data:           Option<Bytes>,
    pub a_to_b:              bool,
    pub standing_validation: Option<StandingValidation>,
    pub order_quantities:    OrderQuantities,
    pub exact_in:            bool,
    pub signature:           Bytes
}

impl UserOrder {
    pub fn order_hash(&self) -> B256 {
        keccak256(&self.signature)
    }

    pub fn from_internal_order(
        order: &OrderWithStorageData<GroupedVanillaOrder>,
        outcome: &OrderOutcome,
        pair_index: u16
    ) -> Self {
        let order_quantities = match order.order {
            GroupedVanillaOrder::KillOrFill(_) => {
                OrderQuantities::Exact { quantity: order.quantity().to() }
            }
            GroupedVanillaOrder::Standing(_) => {
                let max_quantity_in: u128 = order.quantity().to();
                let filled_quantity = match outcome.outcome {
                    OrderFillState::CompleteFill => max_quantity_in,
                    OrderFillState::PartialFill(fill) => fill.to(),
                    _ => 0
                };
                OrderQuantities::Partial { min_quantity_in: 0, max_quantity_in, filled_quantity }
            }
        };
        let hook_data = match order.order {
            GroupedVanillaOrder::KillOrFill(ref o) => o.hook_data().clone(),
            GroupedVanillaOrder::Standing(ref o) => o.hook_data().clone()
        };
        Self {
            a_to_b: order.is_bid,
            exact_in: false,
            hook_data: Some(hook_data),
            min_price: *order.price(),
            order_quantities,
            pair_index,
            recipient: None,
            signature: order.signature().clone(),
            standing_validation: None,
            use_internal: false
        }
    }
}

#[derive(Debug, PadeEncode, PadeDecode)]
pub struct AngstromBundle {
    pub assets:              Vec<Asset>,
    pub pairs:               Vec<Pair>,
    pub pool_updates:        Vec<PoolUpdate>,
    pub top_of_block_orders: Vec<TopOfBlockOrder>,
    pub user_orders:         Vec<UserOrder>
}

impl AngstromBundle {
    pub fn get_prices_per_pair(&self) -> &[Pair] {
        &self.pairs
    }

    pub fn get_order_hashes(&self) -> impl Iterator<Item = B256> + '_ {
        self.top_of_block_orders
            .iter()
            .map(|order| order.order_hash())
            .chain(self.user_orders.iter().map(|order| order.order_hash()))
    }

    pub fn build_dummy_for_tob_gas(
        user_order: &OrderWithStorageData<RpcTopOfBlockOrder>
    ) -> eyre::Result<Self> {
        let mut top_of_block_orders = Vec::new();
        let pool_updates = Vec::new();
        let mut pairs = Vec::new();
        let user_orders = Vec::new();
        let mut asset_builder = AssetBuilder::new();

        // Get the information for the pool or skip this solution if we can't find a
        // pool for it
        let (t0, t1) = {
            let token_in = user_order.token_in();
            let token_out = user_order.token_out();
            if token_in < token_out {
                (token_in, token_out)
            } else {
                (token_out, token_in)
            }
        };
        // Make sure the involved assets are in our assets array and we have the
        // appropriate asset index for them
        let t0_idx = asset_builder.add_or_get_asset(t0) as u16;
        let t1_idx = asset_builder.add_or_get_asset(t1) as u16;

        // TODO this wasn't done when pulled from davids branch.
        let pair = Pair {
            index0:       t0_idx,
            index1:       t1_idx,
            store_index:  0,
            price_1over0: U256::from(1)
        };
        pairs.push(pair);

        // Get our list of user orders, if we have any
        top_of_block_orders.push(TopOfBlockOrder::of(user_order, t0_idx, t1_idx));

        Ok(Self::new(
            asset_builder.get_asset_array(),
            pairs,
            pool_updates,
            top_of_block_orders,
            user_orders
        ))
    }

    pub fn build_dummy_for_user_gas(
        user_order: &OrderWithStorageData<GroupedVanillaOrder>
    ) -> eyre::Result<Self> {
        let top_of_block_orders = Vec::new();
        let pool_updates = Vec::new();
        let mut pairs = Vec::new();
        let mut user_orders = Vec::new();
        let mut asset_builder = AssetBuilder::new();

        // Get the information for the pool or skip this solution if we can't find a
        // pool for it
        let (t0, t1) = {
            let token_in = user_order.token_in();
            let token_out = user_order.token_out();
            if token_in < token_out {
                (token_in, token_out)
            } else {
                (token_out, token_in)
            }
        };
        // Make sure the involved assets are in our assets array and we have the
        // appropriate asset index for them
        let t0_idx = asset_builder.add_or_get_asset(t0) as u16;
        let t1_idx = asset_builder.add_or_get_asset(t1) as u16;

        // TODO this wasn't done when pulled from davids branch.
        let pair = Pair {
            index0:       t0_idx,
            index1:       t1_idx,
            store_index:  0,
            price_1over0: U256::from(1)
        };
        pairs.push(pair);

        let pair_idx = pairs.len() - 1;

        let outcome =
            OrderOutcome { id: user_order.order_id, outcome: OrderFillState::CompleteFill };
        // Get our list of user orders, if we have any
        user_orders.push(UserOrder::from_internal_order(user_order, &outcome, pair_idx as u16));

        Ok(Self::new(
            asset_builder.get_asset_array(),
            pairs,
            pool_updates,
            top_of_block_orders,
            user_orders
        ))
    }

    pub fn from_proposal(
        proposal: &Proposal,
        pools: &HashMap<PoolId, (Address, Address, PoolSnapshot, u16)>
    ) -> eyre::Result<Self> {
        let mut top_of_block_orders = Vec::new();
        let mut pool_updates = Vec::new();
        let mut pairs = Vec::new();
        let mut user_orders = Vec::new();
        let mut asset_builder = AssetBuilder::new();

        // Break out our input orders into lists of orders by pool
        let orders_by_pool = PreProposal::orders_by_pool_id(&proposal.preproposals);

        // Walk through our solutions to add them to the structure
        for solution in proposal.solutions.iter() {
            // Get the information for the pool or skip this solution if we can't find a
            // pool for it
            let Some((t0, t1, snapshot, store_index)) = pools.get(&solution.id) else {
                // This should never happen but let's handle it as gracefully as possible -
                // right now will skip the pool, not produce an error
                warn!("Skipped a solution as we couldn't find a pool for it: {:?}", solution);
                continue;
            };
            // Make sure the involved assets are in our assets array and we have the
            // appropriate asset index for them
            let t0_idx = asset_builder.add_or_get_asset(*t0) as u16;
            let t1_idx = asset_builder.add_or_get_asset(*t1) as u16;
            // Build our Pair featuring our uniform clearing price
            // This price is in Ray format as requested.
            // TODO:  Get the store index so this can be correct
            let ucp: U256 = *solution.ucp;
            let pair = Pair {
                index0:       t0_idx,
                index1:       t1_idx,
                store_index:  *store_index,
                price_1over0: ucp
            };
            pairs.push(pair);
            let pair_idx = pairs.len() - 1;

            // Pull out our net AMM order
            let net_amm_order = solution
                .amm_quantity
                .as_ref()
                .map(|amm_o| amm_o.to_order_tuple(t0_idx, t1_idx));
            // Pull out our TOB swap and TOB reward
            let (tob_swap, tob_rewards) = solution
                .searcher
                .as_ref()
                .map(|tob| {
                    let swap = if tob.is_bid {
                        (t1_idx, t0_idx, tob.quantityIn, tob.quantityOut)
                    } else {
                        (t0_idx, t1_idx, tob.quantityIn, tob.quantityOut)
                    };
                    // We swallow an error here
                    let outcome = ToBOutcome::from_tob_and_snapshot(tob, snapshot).ok();
                    (Some(swap), outcome)
                })
                .unwrap_or_default();
            // Merge our net AMM order with the TOB swap
            let merged_amm_swap = match (net_amm_order, tob_swap) {
                (Some(amm), Some(tob)) => {
                    if amm.0 == tob.0 {
                        // If they're in the same direction we just sum them
                        Some((amm.0, amm.1, (amm.2 + tob.2), (amm.3 + tob.3)))
                    } else {
                        // If they're in opposite directions then we see if we have to flip them
                        if tob.2 > amm.3 {
                            Some((tob.0, tob.1, tob.2 - amm.2, tob.3 - amm.3))
                        } else {
                            Some((amm.0, amm.1, amm.2 - tob.3, amm.3 - tob.2))
                        }
                    }
                }
                (net_amm_order, tob_swap) => net_amm_order.or(tob_swap)
            };
            // Unwrap our merged amm order or provide a zero default
            let (asset_in_index, asset_out_index, quantity_in, quantity_out) =
                merged_amm_swap.unwrap_or((t0_idx, t1_idx, 0_u128, 0_u128));
            // If we don't have a rewards update, we insert a default "empty" struct
            let tob_outcome = tob_rewards.unwrap_or_default();

            // Account for our net AMM Order
            asset_builder.uniswap_swap(
                AssetBuilderStage::Swap,
                asset_in_index as usize,
                asset_out_index as usize,
                quantity_in,
                quantity_out
            );
            // Account for our reward
            asset_builder.allocate(AssetBuilderStage::Reward, *t0, tob_outcome.total_reward.to());
            let rewards_update = tob_outcome.to_rewards_update();
            // Push the pool update
            pool_updates.push(PoolUpdate {
                zero_for_one: false,
                pair_index: pair_idx as u16,
                swap_in_quantity: quantity_in,
                rewards_update
            });
            // Add the ToB order to our tob order list - This is currently converting
            // between two ToB order formats
            if let Some(tob) = solution.searcher.as_ref() {
                // Account for our ToB order
                let (asset_in, asset_out) = if tob.is_bid { (*t1, *t0) } else { (*t0, *t1) };
                asset_builder.external_swap(
                    AssetBuilderStage::TopOfBlock,
                    asset_in,
                    asset_out,
                    tob.quantityIn,
                    tob.quantityOut
                );
                let contract_tob = TopOfBlockOrder::of(tob, asset_in_index, asset_out_index);
                top_of_block_orders.push(contract_tob);
            }

            // Get our list of user orders, if we have any
            let mut order_list: Vec<&OrderWithStorageData<GroupedVanillaOrder>> = orders_by_pool
                .get(&solution.id)
                .map(|order_set| order_set.iter().collect())
                .unwrap_or_default();
            // Sort the user order list so we can properly associate it with our
            // OrderOutcomes.  First bids by price then asks by price.
            order_list.sort_by(|a, b| match (a.is_bid, b.is_bid) {
                (true, true) => b.priority_data.cmp(&a.priority_data),
                (false, false) => a.priority_data.cmp(&b.priority_data),
                (..) => b.is_bid.cmp(&a.is_bid)
            });
            // Loop through our filled user orders, do accounting, and add them to our user
            // order list
            for (outcome, order) in solution
                .limit
                .iter()
                .zip(order_list.iter())
                .filter(|(outcome, _)| outcome.is_filled())
            {
                let quantity_out = match outcome.outcome {
                    OrderFillState::PartialFill(p) => p,
                    _ => order.quantity()
                };
                // Calculate the price of this order given the amount filled and the UCP
                let quantity_in = if order.is_bid {
                    Ray::from(ucp).mul_quantity(quantity_out)
                } else {
                    Ray::from(ucp).inverse_quantity(quantity_out)
                };
                // Account for our user order
                let (asset_in, asset_out) = if order.is_bid { (*t1, *t0) } else { (*t0, *t1) };
                asset_builder.external_swap(
                    AssetBuilderStage::UserOrder,
                    asset_in,
                    asset_out,
                    quantity_in.to(),
                    quantity_out.to()
                );
                user_orders.push(UserOrder::from_internal_order(order, outcome, pair_idx as u16));
            }
        }
        Ok(Self::new(
            asset_builder.get_asset_array(),
            pairs,
            pool_updates,
            top_of_block_orders,
            user_orders
        ))
    }
}

impl AngstromBundle {
    pub fn new(
        assets: Vec<Asset>,
        pairs: Vec<Pair>,
        pool_updates: Vec<PoolUpdate>,
        top_of_block_orders: Vec<TopOfBlockOrder>,
        user_orders: Vec<UserOrder>
    ) -> Self {
        Self { assets, pairs, pool_updates, top_of_block_orders, user_orders }
    }
}

#[derive(Hash, Eq, PartialEq, Copy, Clone)]
pub struct AngstromPoolPartialKey([u8; 27]);

#[derive(Copy, Clone)]
pub struct AngPoolConfigEntry {
    pub pool_partial_key: AngstromPoolPartialKey,
    pub tick_spacing:     u16,
    pub fee_in_e6:        u32,
    pub store_index:      usize
}

#[derive(Default, Clone)]
pub struct AngstromPoolConfigStore {
    entries: HashMap<AngstromPoolPartialKey, AngPoolConfigEntry>
}

impl AngstromPoolConfigStore {
    pub async fn load_from_chain<T, N, P>(
        angstrom_contract: Address,
        block_id: BlockId,
        provider: &P
    ) -> Result<AngstromPoolConfigStore, String>
    where
        T: Transport + Clone,
        N: Network,
        P: Provider<T, N>
    {
        let value = provider
            .get_storage_at(angstrom_contract, U256::from(4))
            .await
            .map_err(|e| format!("Error getting storage: {}", e))?;

        let value_bytes: [u8; 32] = value.to_be_bytes();
        let config_store_address =
            Address::from(<[u8; 20]>::try_from(&value_bytes[4..24]).unwrap());

        let code = provider
            .get_code_at(config_store_address)
            .block_id(block_id)
            .await
            .map_err(|e| format!("Error getting code: {}", e))?;

        AngstromPoolConfigStore::try_from(code.as_ref())
            .map_err(|e| format!("Failed to deserialize code into AngstromPoolConfigStore: {}", e))
    }

    pub fn derive_store_key(asset0: Address, asset1: Address) -> AngstromPoolPartialKey {
        let hash = keccak256((asset0, asset1).abi_encode());
        let mut store_key = [0u8; 27];
        store_key.copy_from_slice(&hash[5..32]);
        AngstromPoolPartialKey(store_key)
    }

    pub fn get_entry(&self, asset0: Address, asset1: Address) -> Option<AngPoolConfigEntry> {
        let store_key = Self::derive_store_key(asset0, asset1);
        self.entries.get(&store_key).cloned()
    }
}

impl TryFrom<&[u8]> for AngstromPoolConfigStore {
    type Error = String;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.first() != Some(&0) {
            return Err("Invalid encoded entries: must start with a safety byte of 0".to_string())
        }
        let adjusted_entries = &value[1..];
        if adjusted_entries.len() % POOL_CONFIG_STORE_ENTRY_SIZE != 0 {
            return Err(
                "Invalid encoded entries: incorrect length after removing safety byte".to_string()
            )
        }
        let entries = adjusted_entries
            .chunks(POOL_CONFIG_STORE_ENTRY_SIZE)
            .enumerate()
            .map(|(index, chunk)| {
                let pool_partial_key =
                    AngstromPoolPartialKey(<[u8; 27]>::try_from(&chunk[0..27]).unwrap());
                let tick_spacing = u16::from_be_bytes([chunk[27], chunk[28]]);
                let fee_in_e6 = u32::from_be_bytes([0, chunk[29], chunk[30], chunk[31]]);
                (
                    pool_partial_key,
                    AngPoolConfigEntry {
                        pool_partial_key,
                        tick_spacing,
                        fee_in_e6,
                        store_index: index
                    }
                )
            })
            .collect();

        Ok(AngstromPoolConfigStore { entries })
    }
}

#[derive(Default, Clone)]
pub struct UniswapAngstromRegistry {
    uniswap_pools:         UniswapPoolRegistry,
    angstrom_config_store: AngstromPoolConfigStore
}

impl UniswapAngstromRegistry {
    pub fn new(
        uniswap_pools: UniswapPoolRegistry,
        angstrom_config_store: AngstromPoolConfigStore
    ) -> Self {
        UniswapAngstromRegistry { uniswap_pools, angstrom_config_store }
    }

    pub fn get_uni_pool(&self, pool_id: &PoolId) -> Option<PoolKey> {
        self.uniswap_pools.get(pool_id).cloned()
    }

    pub fn get_ang_entry(&self, pool_id: &PoolId) -> Option<AngPoolConfigEntry> {
        let uni_entry = self.get_uni_pool(pool_id)?;
        self.angstrom_config_store
            .get_entry(uni_entry.currency0, uni_entry.currency1)
    }
}

#[cfg(test)]
mod test {
    use super::AngstromBundle;

    #[test]
    fn can_be_constructed() {
        let _result = AngstromBundle::new(vec![], vec![], vec![], vec![], vec![]);
    }

    #[test]
    fn can_be_cretaed_from_proposal() {
        // AngstromBundle::from_proposal(proposal, pools);
    }
}
