use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    ops::Deref,
    sync::Arc
};

use alloy::{
    eips::BlockId,
    network::Network,
    primitives::{keccak256, Address, FixedBytes, B256, U256},
    providers::Provider,
    sol_types::SolValue,
    transports::Transport
};
use alloy_primitives::I256;
use base64::Engine;
use dashmap::DashMap;
use pade_macro::{PadeDecode, PadeEncode};
use tracing::{debug, trace, warn};

use super::{
    asset::builder::{AssetBuilder, AssetBuilderStage},
    rewards::PoolUpdate,
    tob::ToBOutcome,
    Asset, Pair, CONFIG_STORE_SLOT, POOL_CONFIG_STORE_ENTRY_SIZE
};
use crate::{
    consensus::{PreProposal, Proposal},
    contract_bindings::angstrom::Angstrom::PoolKey,
    matching::{uniswap::PoolSnapshot, Ray},
    orders::{OrderFillState, OrderOutcome, PoolSolution},
    primitive::{PoolId, UniswapPoolRegistry},
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder as RpcTopOfBlockOrder,
        RawPoolOrder
    },
    testnet::TestnetStateOverrides
};

mod order;
mod tob;
pub use order::{OrderQuantities, UserOrder};
pub use tob::*;

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

    #[cfg(feature = "testnet")]
    pub fn fetch_needed_overrides(&self, block_number: u64) -> TestnetStateOverrides {
        use std::u128;

        use crate::primitive::TESTNET_ANGSTROM_ADDRESS;

        let mut approvals: HashMap<Address, HashMap<Address, u128>> = HashMap::new();
        let mut balances: HashMap<Address, HashMap<Address, u128>> = HashMap::new();

        // user orders
        self.user_orders.iter().for_each(|order| {
            let token = if order.zero_for_one {
                // token0
                self.assets[self.pairs[order.pair_index as usize].index0 as usize].addr
            } else {
                self.assets[self.pairs[order.pair_index as usize].index1 as usize].addr
            };

            // need to recover sender from signature
            let hash = order.order_hash(&self.pairs, &self.assets, block_number);
            let address = order.signature.recover_signer(hash);

            let qty = order.order_quantities.fetch_max_amount();
            approvals.entry(token).or_default().insert(address, qty);
            balances.entry(token).or_default().insert(address, qty);
        });

        // tob
        self.top_of_block_orders.iter().for_each(|order| {
            let token = if order.zero_for_1 {
                // token0
                self.assets[self.pairs[order.pairs_index as usize].index0 as usize].addr
            } else {
                self.assets[self.pairs[order.pairs_index as usize].index1 as usize].addr
            };

            // need to recover sender from signature
            let hash = order.order_hash(&self.pairs, &self.assets, block_number);
            let address = order.signature.recover_signer(hash);

            let qty = order.quantity_in;
            approvals.entry(token).or_default().insert(address, qty);
            balances.entry(token).or_default().insert(address, qty);
        });

        // approvals cuz diff map but same
        for token in approvals.keys() {
            balances
                .entry(*token)
                .or_default()
                .insert(TESTNET_ANGSTROM_ADDRESS, u128::MAX - 1);
        }

        TestnetStateOverrides { approvals, balances }
    }

    pub fn assert_book_matches(&self) {
        let map = self
            .user_orders
            .iter()
            .fold(HashMap::<Address, I256>::new(), |mut acc, user| {
                let pair = &self.pairs[user.pair_index as usize];
                let asset_in = &self.assets
                    [if user.zero_for_one { pair.index0 } else { pair.index1 } as usize];
                let asset_out = &self.assets
                    [if user.zero_for_one { pair.index1 } else { pair.index0 } as usize];

                let price = Ray::from(user.min_price);
                // if we are exact in, then we can attribute amoutn
                let amount_in = if user.exact_in {
                    U256::from(user.order_quantities.fetch_max_amount())
                } else {
                    price.mul_quantity(U256::from(user.order_quantities.fetch_max_amount()))
                };

                let amount_out = if user.exact_in {
                    price.mul_quantity(U256::from(user.order_quantities.fetch_max_amount()))
                } else {
                    U256::from(user.order_quantities.fetch_max_amount())
                };

                *acc.entry(asset_in.addr).or_default() += I256::from_raw(amount_in);
                *acc.entry(asset_out.addr).or_default() -= I256::from_raw(amount_out);

                acc
            });

        for (address, delta) in map {
            if !delta.is_zero() {
                tracing::error!(?address, ?delta, "user orders don't cancel out");
            } else {
                tracing::info!(?address, "solid delta");
            }
        }
    }

    /// the block number is the block that this bundle was executed at.
    pub fn get_order_hashes(&self, block_number: u64) -> impl Iterator<Item = B256> + '_ {
        self.top_of_block_orders
            .iter()
            .map(move |order| order.order_hash(&self.pairs, &self.assets, block_number))
            .chain(
                self.user_orders
                    .iter()
                    .map(move |order| order.order_hash(&self.pairs, &self.assets, block_number))
            )
    }

    pub fn build_dummy_for_tob_gas(
        user_order: &OrderWithStorageData<RpcTopOfBlockOrder>
    ) -> eyre::Result<Self> {
        let mut top_of_block_orders = Vec::new();
        let mut pool_updates = Vec::new();
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

        let pair = Pair {
            index0:       t0_idx,
            index1:       t1_idx,
            store_index:  0,
            price_1over0: U256::from(1)
        };
        pairs.push(pair);

        asset_builder.external_swap(
            AssetBuilderStage::TopOfBlock,
            user_order.token_in(),
            user_order.token_out(),
            user_order.quantity_in,
            user_order.quantity_out
        );

        pool_updates.push(PoolUpdate {
            zero_for_one:     false,
            pair_index:       0,
            swap_in_quantity: user_order.quantity_out,
            rewards_update:   super::rewards::RewardsUpdate::CurrentOnly { amount: 0 }
        });

        // Get our list of user orders, if we have any
        top_of_block_orders.push(TopOfBlockOrder::of_max_gas(user_order, 0));

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
        // in order to properly build this. we will create a fake order with the
        // amount's flipped going the other way so we have a direct match and
        // don't have to worry about balance deltas

        let top_of_block_orders = Vec::new();
        let pool_updates = Vec::new();
        let mut pairs = Vec::new();
        let mut user_orders = Vec::new();
        let mut asset_builder = AssetBuilder::new();

        {
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

            // hacky but works
            if pairs.is_empty() {
                let pair = Pair {
                    index0:       t0_idx,
                    index1:       t1_idx,
                    store_index:  0,
                    price_1over0: user_order.limit_price()
                };
                pairs.push(pair);
            }

            let pair_idx = pairs.len() - 1;

            let outcome = OrderOutcome {
                id:      user_order.order_id,
                outcome: OrderFillState::CompleteFill
            };
            // Get our list of user orders, if we have any
            user_orders.push(UserOrder::from_internal_order_max_gas(
                user_order,
                &outcome,
                pair_idx as u16
            ));
        }

        Ok(Self::new(
            asset_builder.get_asset_array(),
            pairs,
            pool_updates,
            top_of_block_orders,
            user_orders
        ))
    }

    // builds a bundle where orders are set to max allocated gas to ensure a fully
    // passing env. with the gas details from the response, can properly
    // allocate order gas amounts.
    pub fn for_gas_finalization(
        limit: Vec<OrderWithStorageData<GroupedVanillaOrder>>,
        solutions: Vec<PoolSolution>,
        pools: &HashMap<PoolId, (Address, Address, PoolSnapshot, u16)>
    ) -> eyre::Result<Self> {
        trace!("Starting for_gas_finalization");
        let mut top_of_block_orders = Vec::new();
        let mut pool_updates = Vec::new();
        let mut pairs = Vec::new();
        let mut user_orders = Vec::new();
        let mut asset_builder = AssetBuilder::new();

        let orders_by_pool: HashMap<
            alloy_primitives::FixedBytes<32>,
            Vec<OrderWithStorageData<GroupedVanillaOrder>>
        > = limit.iter().fold(HashMap::new(), |mut acc, x| {
            acc.entry(x.pool_id).or_default().push(x.clone());
            acc
        });

        // Break out our input orders into lists of orders by pool

        // Walk through our solutions to add them to the structure
        for solution in solutions.iter() {
            println!("Processing solution");
            // Get the information for the pool or skip this solution if we can't find a
            // pool for it
            let Some((t0, t1, snapshot, store_index)) = pools.get(&solution.id) else {
                // This should never happen but let's handle it as gracefully as possible -
                // right now will skip the pool, not produce an error
                warn!(
                    "Skipped a solution as we couldn't find a pool for it: {:?}, {:?}",
                    pools, solution.id
                );
                continue;
            };
            println!("Processing pair {} - {}", t0, t1);
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
                        (t1_idx, t0_idx, tob.quantity_in, tob.quantity_out)
                    } else {
                        (t0_idx, t1_idx, tob.quantity_in, tob.quantity_out)
                    };
                    // We swallow an error here
                    let outcome = ToBOutcome::from_tob_and_snapshot(tob, snapshot).ok();
                    (Some(swap), outcome)
                })
                .unwrap_or_default();
            // Merge our net AMM order with the TOB swap
            trace!(tob_swap = ?tob_swap, net_amm_order = ?net_amm_order, "Merging Net AMM with TOB Swap");
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
            trace!(merged_swap = ?merged_amm_swap, "Merged AMM/TOB swap");
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
                    tob.quantity_in,
                    tob.quantity_out
                );
                let contract_tob = TopOfBlockOrder::of_max_gas(tob, pair_idx as u16);
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
                (true, true) => a.priority_data.cmp(&b.priority_data),
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
                assert_eq!(outcome.id.hash, order.order_id.hash);

                let quantity_out = match outcome.outcome {
                    OrderFillState::PartialFill(p) => p,
                    _ => order.max_q()
                };
                // Calculate the price of this order given the amount filled and the UCP
                let quantity_in = if order.is_bid {
                    Ray::from(ucp).mul_quantity(U256::from(quantity_out))
                } else {
                    U256::from(Ray::from(ucp).inverse_quantity(quantity_out, true))
                };
                // Account for our user order
                let (asset_in, asset_out) = if order.is_bid { (*t1, *t0) } else { (*t0, *t1) };
                asset_builder.external_swap(
                    AssetBuilderStage::UserOrder,
                    asset_in,
                    asset_out,
                    quantity_in.to(),
                    quantity_out
                );
                user_orders.push(UserOrder::from_internal_order_max_gas(
                    order,
                    outcome,
                    pair_idx as u16
                ));
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

    fn fetch_total_orders_and_gas_delegated_to_orders(
        orders_by_pool: &HashMap<
            FixedBytes<32>,
            HashSet<OrderWithStorageData<GroupedVanillaOrder>>
        >,
        solutions: &[PoolSolution]
    ) -> (u64, u64) {
        solutions
            .iter()
            .map(|s| (s, orders_by_pool.get(&s.id).cloned()))
            .filter_map(|(solution, order_list)| {
                let mut order_list = order_list?.into_iter().collect::<Vec<_>>();
                // Sort the user order list so we can properly associate it with our
                // OrderOutcomes.  First bids by price then asks by price.
                order_list.sort_by(|a, b| match (a.is_bid, b.is_bid) {
                    (true, true) => b.priority_data.cmp(&a.priority_data),
                    (false, false) => a.priority_data.cmp(&b.priority_data),
                    (..) => b.is_bid.cmp(&a.is_bid)
                });
                let mut cnt = 0;
                let mut total_gas = 0;
                for (_, order) in solution
                    .limit
                    .iter()
                    .zip(order_list.iter())
                    .filter(|(outcome, _)| outcome.is_filled())
                {
                    cnt += 1;
                    total_gas += order.priority_data.gas_units;
                }

                solution.searcher.as_ref().inspect(|searcher| {
                    cnt += 1;
                    total_gas += searcher.priority_data.gas_units;
                });

                Some((cnt, total_gas))
            })
            .fold((0u64, 0u64), |(mut cnt, mut tg), x| {
                cnt += x.0;
                tg += x.1;
                (cnt, tg)
            })
    }

    pub fn from_proposal(
        proposal: &Proposal,
        gas_details: BundleGasDetails,
        pools: &HashMap<PoolId, (Address, Address, PoolSnapshot, u16)>
    ) -> eyre::Result<Self> {
        trace!("Starting from_proposal");
        let mut top_of_block_orders = Vec::new();
        let mut pool_updates = Vec::new();
        let mut pairs = Vec::new();
        let mut user_orders = Vec::new();
        let mut asset_builder = AssetBuilder::new();

        // Break out our input orders into lists of orders by pool
        let preproposals = proposal.flattened_pre_proposals();
        let orders_by_pool = PreProposal::orders_by_pool_id(&preproposals);

        // fetch the accumulated amount of gas delegated to the users
        let (total_swaps, total_gas) = Self::fetch_total_orders_and_gas_delegated_to_orders(
            &orders_by_pool,
            &proposal.solutions
        );

        // this should never underflow. if it does. means that there is underlying
        // problem with the gas delegation module
        assert!(
            gas_details.total_gas_cost_wei > total_gas,
            "Total gas cost '{}' greater than total gas '{}'",
            gas_details.total_gas_cost_wei,
            total_gas
        );
        if total_swaps == 0 {
            return Err(eyre::eyre!("have a total swaps count of 0"));
        }
        let shared_gas_in_wei = (gas_details.total_gas_cost_wei - total_gas) / total_swaps;

        // fetch gas used
        // Walk through our solutions to add them to the structure
        for solution in proposal.solutions.iter() {
            // Output our book data so we can do stuff with it
            let json = serde_json::to_string(&solution).unwrap();
            let b64_output = base64::prelude::BASE64_STANDARD.encode(json.as_bytes());
            trace!(data = b64_output, "Raw solution data");
            // Get the information for the pool or skip this solution if we can't find a
            // pool for it
            let Some((t0, t1, snapshot, store_index)) = pools.get(&solution.id) else {
                // This should never happen but let's handle it as gracefully as possible -
                // right now will skip the pool, not produce an error
                warn!(
                    "Skipped a solution as we couldn't find a pool for it: {:?}, {:?}",
                    pools, solution.id
                );
                continue;
            };
            debug!(t0 = ?t0, t1 = ?t1, pool_id = ?solution.id, "Starting processing of solution");

            let conversion_rate_to_token0 =
                gas_details.token_price_per_wei.get(&(*t0, *t1)).expect(
                    "don't have price for a critical pair. should be unreachable since no orders \
                     would get validated. this would always be skipped"
                );

            // Make sure the involved assets are in our assets array and we have the
            // appropriate asset index for them
            let t0_idx = asset_builder.add_or_get_asset(*t0) as u16;
            let t1_idx = asset_builder.add_or_get_asset(*t1) as u16;

            // Build our Pair featuring our uniform clearing price
            // This price is in Ray format as requested.
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
                        (t1_idx, t0_idx, tob.quantity_in, tob.quantity_out)
                    } else {
                        (t0_idx, t1_idx, tob.quantity_in, tob.quantity_out)
                    };
                    // We swallow an error here
                    let outcome = ToBOutcome::from_tob_and_snapshot(tob, snapshot).ok();
                    (Some(swap), outcome)
                })
                .unwrap_or_default();
            // Merge our net AMM order with the TOB swap
            trace!(tob_swap = ?tob_swap, net_amm_order = ?net_amm_order, "Merging Net AMM with TOB Swap");
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
            trace!(merged_swap = ?merged_amm_swap, "Merged AMM/TOB swap");
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
            // calculate the shared amount of gas in token 0 to share over this pool
            let delegated_amount_in_token_0 =
                (*conversion_rate_to_token0 * U256::from(shared_gas_in_wei)).scale_out_of_ray();

            // Add the ToB order to our tob order list - This is currently converting
            // between two ToB order formats
            if let Some(tob) = solution.searcher.as_ref() {
                // Account for our ToB order
                let (asset_in, asset_out) = if tob.is_bid { (*t1, *t0) } else { (*t0, *t1) };

                asset_builder.external_swap(
                    AssetBuilderStage::TopOfBlock,
                    asset_in,
                    asset_out,
                    tob.quantity_in,
                    tob.quantity_out
                );
                let contract_tob =
                    TopOfBlockOrder::of(tob, delegated_amount_in_token_0, pair_idx as u16)?;
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
                (true, true) => a.priority_data.cmp(&b.priority_data),
                (false, false) => a.priority_data.cmp(&b.priority_data),
                (..) => b.is_bid.cmp(&a.is_bid)
            });
            // Loop through our filled user orders, do accounting, and add them to our user
            // order list
            let ray_ucp = Ray::from(ucp);
            for (outcome, order) in solution
                .limit
                .iter()
                .zip(order_list.iter())
                .filter(|(outcome, _)| outcome.is_filled())
            {
                // Calculate our final amounts based on whether the order is in T0 or T1 context
                let inverse_order = order.is_bid() == order.exact_in();
                assert_eq!(outcome.id.hash, order.order_id.hash);
                let (t0_moving, t1_moving) = if inverse_order {
                    let t1_moving = outcome.fill_amount(order.max_q());
                    let t0_moving = ray_ucp.inverse_quantity(t1_moving, !order.is_bid());
                    (U256::from(t0_moving), U256::from(t1_moving))
                } else {
                    let t0_moving = U256::from(outcome.fill_amount(order.max_q()));
                    let t1_moving = Ray::from(ucp).mul_quantity(t0_moving);
                    (t0_moving, t1_moving)
                };

                let (quantity_in, quantity_out) =
                    if order.is_bid { (t1_moving, t0_moving) } else { (t0_moving, t1_moving) };
                // Account for our user order
                let (asset_in, asset_out) = if order.is_bid { (*t1, *t0) } else { (*t0, *t1) };
                asset_builder.external_swap(
                    AssetBuilderStage::UserOrder,
                    asset_in,
                    asset_out,
                    quantity_in.to(),
                    quantity_out.to()
                );
                user_orders.push(UserOrder::from_internal_order(
                    order,
                    outcome,
                    delegated_amount_in_token_0,
                    pair_idx as u16
                )?);
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

#[derive(Debug, Clone, Default)]
pub struct BundleGasDetails {
    /// a map (sorted tokens) of how much of token0 in gas is needed per unit of
    /// gas
    token_price_per_wei: HashMap<(Address, Address), Ray>,
    /// total gas to execute the bundle on angstrom
    total_gas_cost_wei:  u64
}

impl BundleGasDetails {
    pub fn new(
        token_price_per_wei: HashMap<(Address, Address), Ray>,
        total_gas_cost_wei: u64
    ) -> Self {
        Self { token_price_per_wei, total_gas_cost_wei }
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

#[derive(Debug, Hash, Eq, PartialEq, Copy, Clone)]
pub struct AngstromPoolPartialKey([u8; 27]);

impl Deref for AngstromPoolPartialKey {
    type Target = [u8; 27];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Copy, Clone)]
pub struct AngPoolConfigEntry {
    pub pool_partial_key: AngstromPoolPartialKey,
    pub tick_spacing:     u16,
    pub fee_in_e6:        u32,
    pub store_index:      usize
}

#[derive(Debug, Default, Clone)]
pub struct AngstromPoolConfigStore {
    entries: DashMap<AngstromPoolPartialKey, AngPoolConfigEntry>
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
        // offset of 6 bytes
        let value = provider
            .get_storage_at(angstrom_contract, U256::from(CONFIG_STORE_SLOT))
            .await
            .map_err(|e| format!("Error getting storage: {}", e))?;

        let value_bytes: [u8; 32] = value.to_be_bytes();
        tracing::debug!("storage slot of poolkey storage {:?}", value_bytes);
        let config_store_address =
            Address::from(<[u8; 20]>::try_from(&value_bytes[4..24]).unwrap());

        let code = provider
            .get_code_at(config_store_address)
            .block_id(block_id)
            .await
            .map_err(|e| format!("Error getting code: {}", e))?;

        tracing::debug!("bytecode: {:x}", code);

        AngstromPoolConfigStore::try_from(code.as_ref())
            .map_err(|e| format!("Failed to deserialize code into AngstromPoolConfigStore: {}", e))
    }

    pub fn length(&self) -> usize {
        self.entries.len()
    }

    pub fn remove_pair(&self, asset0: Address, asset1: Address) {
        let key = Self::derive_store_key(asset0, asset1);

        self.entries.remove(&key);
    }

    pub fn new_pool(&self, asset0: Address, asset1: Address, pool: AngPoolConfigEntry) {
        let key = Self::derive_store_key(asset0, asset1);

        self.entries.insert(key, pool);
    }

    pub fn derive_store_key(asset0: Address, asset1: Address) -> AngstromPoolPartialKey {
        let hash = keccak256((asset0, asset1).abi_encode());
        let mut store_key = [0u8; 27];
        store_key.copy_from_slice(&hash[5..32]);
        AngstromPoolPartialKey(store_key)
    }

    pub fn get_entry(&self, asset0: Address, asset1: Address) -> Option<AngPoolConfigEntry> {
        let store_key = Self::derive_store_key(asset0, asset1);
        self.entries.get(&store_key).map(|i| *i)
    }

    pub fn all_entries(&self) -> &DashMap<AngstromPoolPartialKey, AngPoolConfigEntry> {
        &self.entries
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
    angstrom_config_store: Arc<AngstromPoolConfigStore>
}

impl UniswapAngstromRegistry {
    pub fn new(
        uniswap_pools: UniswapPoolRegistry,
        angstrom_config_store: Arc<AngstromPoolConfigStore>
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
    fn decode_tob_angstrom_bundle() {
        let bundle: [u8; 376] = [
            0, 0, 136, 122, 185, 133, 215, 244, 70, 250, 54, 98, 245, 212, 171, 94, 242, 10, 107,
            160, 94, 237, 29, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 192, 42,
            170, 57, 178, 35, 254, 141, 10, 14, 92, 79, 39, 234, 217, 8, 60, 117, 108, 194, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 237, 67, 85, 63, 95, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 237, 67, 85, 63, 95, 0, 0, 38, 0, 0, 0, 1, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 1, 0, 0, 35, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 237, 67, 85, 63, 95, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 152, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 3, 183, 17, 221, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 237, 67, 85, 63, 95, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 12, 193, 120, 139, 238, 5, 82, 51, 29, 109, 124, 113, 245, 142, 31, 6, 216,
            47, 227, 99, 27, 110, 150, 112, 234, 129, 56, 107, 225, 163, 117, 76, 121, 246, 253,
            249, 39, 68, 131, 150, 103, 127, 217, 176, 52, 185, 222, 70, 255, 251, 186, 8, 243,
            112, 12, 12, 247, 87, 89, 190, 161, 56, 9, 90, 204, 75, 252, 28, 228, 93, 15, 115, 133,
            106, 184, 0, 241, 21, 160, 212, 52, 123, 21, 16, 129, 0, 0, 0
        ];
        let slice = &mut bundle.as_slice();

        let mut bundle: AngstromBundle = pade::PadeDecode::pade_decode(slice, None).unwrap();
        println!("{bundle:?}");
        let tob = bundle.top_of_block_orders.remove(0);
        println!("{tob:?}");
    }

    #[test]
    fn decode_user_angstrom_bundle() {
        let bundle: [u8; 373] = [
            0, 0, 136, 57, 251, 60, 242, 199, 91, 76, 34, 70, 86, 22, 254, 22, 128, 255, 34, 164,
            166, 244, 51, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 204, 100, 109, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 204, 100, 109,
            192, 42, 170, 57, 178, 35, 254, 141, 10, 14, 92, 79, 39, 234, 217, 8, 60, 117, 108,
            194, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 64, 15, 29, 48, 25, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 64, 15, 29, 48, 25, 0, 0, 38, 0, 0,
            0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 16, 67, 96, 206,
            21, 193, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 184, 168, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 16, 67, 96, 206, 21, 193, 48, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 3, 204, 100, 109, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 204, 100, 109, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 204, 100, 109, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            3, 204, 100, 109, 27, 173, 77, 129, 8, 3, 181, 255, 66, 55, 66, 206, 216, 73, 59, 189,
            66, 160, 50, 207, 190, 202, 63, 115, 71, 92, 14, 98, 123, 109, 168, 226, 241, 91, 144,
            45, 255, 160, 52, 65, 145, 173, 31, 90, 90, 206, 232, 240, 156, 123, 216, 158, 62, 155,
            36, 55, 255, 111, 67, 204, 109, 84, 52, 115, 11
        ];
        let slice = &mut bundle.as_slice();

        let mut bundle: AngstromBundle = pade::PadeDecode::pade_decode(slice, None).unwrap();
        println!("{bundle:?}");
        let user = bundle.user_orders.remove(0);
        println!("{user:?}");
    }
}
