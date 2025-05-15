use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    ops::Deref,
    sync::Arc
};

use alloy::{
    eips::BlockId,
    network::Network,
    primitives::{Address, B256, FixedBytes, U256, keccak256},
    providers::Provider,
    sol_types::SolValue
};
use alloy_primitives::I256;
use base64::Engine;
use dashmap::DashMap;
use itertools::Itertools;
use pade_macro::{PadeDecode, PadeEncode};
use serde::{Deserialize, Serialize};
use tracing::{Level, debug, error, trace, warn};

use super::{
    Asset, CONFIG_STORE_SLOT, POOL_CONFIG_STORE_ENTRY_SIZE, Pair,
    asset::builder::{AssetBuilder, AssetBuilderStage},
    rewards::PoolUpdate
};
#[cfg(all(feature = "testnet", not(feature = "testnet-sepolia")))]
use crate::testnet::TestnetStateOverrides;
use crate::{
    consensus::{PreProposal, Proposal},
    contract_bindings::angstrom::Angstrom::PoolKey,
    contract_payloads::rewards::RewardsUpdate,
    matching::{Ray, SqrtPriceX96, get_quantities_at_price, uniswap::Direction},
    orders::{OrderFillState, OrderId, OrderOutcome, OrderSet, PoolSolution},
    primitive::{PoolId, UniswapPoolRegistry},
    sol_bindings::{
        RawPoolOrder,
        grouped_orders::{AllOrders, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder as RpcTopOfBlockOrder
    },
    uni_structure::{BaselinePoolState, donation::DonationCalculation}
};

mod order;
mod tob;
pub use order::{OrderQuantities, StandingValidation, UserOrder};
pub use tob::*;

const LP_DONATION_SPLIT: f64 = 0.75;

#[derive(
    Debug, PadeEncode, PadeDecode, Clone, PartialEq, Serialize, Deserialize, Eq, PartialOrd, Ord,
)]
pub struct AngstromBundle {
    pub assets:              Vec<Asset>,
    pub pairs:               Vec<Pair>,
    pub pool_updates:        Vec<PoolUpdate>,
    pub top_of_block_orders: Vec<TopOfBlockOrder>,
    pub user_orders:         Vec<UserOrder>
}

impl AngstromBundle {
    pub fn has_book(&self) -> bool {
        !self.user_orders.is_empty()
    }

    pub fn get_prices_per_pair(&self) -> &[Pair] {
        &self.pairs
    }

    #[cfg(all(feature = "testnet", not(feature = "testnet-sepolia")))]
    pub fn fetch_needed_overrides(&self, block_number: u64) -> TestnetStateOverrides {
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
            let hash = order.signing_hash(&self.pairs, &self.assets, block_number);
            let address = order.signature.recover_signer(hash);

            // Grab the price because we need it most of the time
            let price = Ray::from(self.pairs[order.pair_index as usize].price_1over0);
            let qty = match (order.zero_for_one, order.exact_in) {
                // Zero for one, exact in -> quantity on the order (t0).  Extra fee is deducted from
                // this
                (true, true) => order.order_quantities.fetch_max_amount(),
                // Zero for one, exact out -> quantity needed to produce output amount (t0) + extra
                // fee (t0)
                (true, false) => {
                    price.inverse_quantity(order.order_quantities.fetch_max_amount(), true)
                        + order.extra_fee_asset0
                }
                // One for zero, exact in -> quantity on the order (t1) and fee is taken from the
                // ouptut
                (false, true) => order.order_quantities.fetch_max_amount(),
                // One for zero, exact out -> quantity needed to produce the output amount + fee
                // (t1)
                (false, false) => price.quantity(
                    order.order_quantities.fetch_max_amount() + order.extra_fee_asset0,
                    true
                )
            };

            approvals
                .entry(token)
                .or_default()
                .entry(address)
                .and_modify(|q| {
                    *q = q.saturating_add(qty);
                })
                .or_insert(qty);
            balances
                .entry(token)
                .or_default()
                .entry(address)
                .and_modify(|q| {
                    *q = q.saturating_add(qty);
                })
                .or_insert(qty);
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
            let hash = order.signing_hash(&self.pairs, &self.assets, block_number);
            let address = order.signature.recover_signer(hash);

            let mut qty = order.quantity_in;
            if order.zero_for_1 {
                qty += order.gas_used_asset_0;
            }

            approvals
                .entry(token)
                .or_default()
                .entry(address)
                .and_modify(|q| {
                    *q = q.saturating_add(qty);
                })
                .or_insert(qty);
            balances
                .entry(token)
                .or_default()
                .entry(address)
                .and_modify(|q| {
                    *q = q.saturating_add(qty);
                })
                .or_insert(qty);
        });

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

    pub fn get_accounts(&self, block_number: u64) -> impl Iterator<Item = Address> + '_ {
        self.top_of_block_orders
            .iter()
            .map(move |order| order.user_address(&self.pairs, &self.assets, block_number))
            .chain(
                self.user_orders.iter().map(move |order| {
                    order.recover_signer(&self.pairs, &self.assets, block_number)
                })
            )
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
        let pool_updates = Vec::new();
        let mut pairs = Vec::new();
        let user_orders = Vec::new();
        let mut asset_builder = AssetBuilder::new();

        // Get the information for the pool or skip this solution if we can't find a
        // pool for it
        let (t0, t1) = {
            let token_in = user_order.token_in();
            let token_out = user_order.token_out();

            if token_in < token_out { (token_in, token_out) } else { (token_out, token_in) }
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
        user_order: &OrderWithStorageData<AllOrders>
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
                if token_in < token_out { (token_in, token_out) } else { (token_out, token_in) }
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

    fn fetch_total_orders_and_gas_delegated_to_orders(
        orders_by_pool: &HashMap<FixedBytes<32>, HashSet<OrderWithStorageData<AllOrders>>>,
        solutions: &[PoolSolution]
    ) -> (u64, u64) {
        solutions
            .iter()
            .map(|s| (s, orders_by_pool.get(&s.id).cloned()))
            .filter_map(|(solution, order_list)| {
                let Some(mut order_list) = order_list.map(|i| i.into_iter().collect::<Vec<_>>())
                else {
                    return solution
                        .searcher
                        .as_ref()
                        .map(|searcher| (1, searcher.priority_data.gas_units));
                };

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

    pub fn process_solution(
        pairs: &mut Vec<Pair>,
        asset_builder: &mut AssetBuilder,
        user_orders: &mut Vec<UserOrder>,
        orders_by_pool: &HashMap<FixedBytes<32>, HashSet<OrderWithStorageData<AllOrders>>>,
        top_of_block_orders: &mut Vec<TopOfBlockOrder>,
        pool_updates: &mut Vec<PoolUpdate>,
        solution: &PoolSolution,
        snapshot: &BaselinePoolState,
        t0: Address,
        t1: Address,
        store_index: u16,
        shared_gas: Option<U256>
    ) -> eyre::Result<()> {
        let process_solution_span =
            tracing::debug_span!("process_solution", t0 = ?t0, t1 = ?t1, pool_id = ?solution.id)
                .entered();
        // Dump the solution if solution dumps are enabled
        if tracing::event_enabled!(target: "dump::solution", Level::TRACE, pool_id = ?solution.id) {
            // Dump the solution
            let json = serde_json::to_string(&(
                solution,
                orders_by_pool,
                snapshot,
                t0,
                t1,
                store_index,
                shared_gas
            ))
            .unwrap();
            let b64_output = base64::prelude::BASE64_STANDARD.encode(json.as_bytes());
            trace!(target: "dump::solution", data = b64_output, "Raw solution data");
        }

        debug!(t0 = ?t0, t1 = ?t1, pool_id = ?solution.id, "Processing solution for pool");

        // Make sure the involved assets are in our assets array and we have the
        // appropriate asset index for them
        let t0_idx = asset_builder.add_or_get_asset(t0) as u16;
        let t1_idx = asset_builder.add_or_get_asset(t1) as u16;
        tracing::info!(?t0, ?t1, ?t0_idx, ?t1_idx);

        // Build our Pair featuring our uniform clearing price
        // This price is in Ray format as requested.
        pairs.push(Pair {
            index0: t0_idx,
            index1: t1_idx,
            store_index,
            price_1over0: *solution.ucp
        });
        let pair_idx = pairs.len() - 1;

        // Log the indexes we found for the assets and pair of this solution
        trace!(t0_idx, t1_idx, pair_idx, "Found asset and pair indexes");

        // Add the ToB order to our tob order list - This is currently converting
        // between two ToB order formats
        if let Some(tob) = solution.searcher.as_ref() {
            // Account for our ToB order
            asset_builder.external_swap(
                AssetBuilderStage::TopOfBlock,
                tob.asset_in,
                tob.asset_out,
                tob.quantity_in,
                tob.quantity_out
            );
            let contract_tob = if let Some(g) = shared_gas {
                let order = TopOfBlockOrder::of(tob, g, pair_idx as u16)?;
                asset_builder.add_gas_fee(
                    AssetBuilderStage::TopOfBlock,
                    t0,
                    order.gas_used_asset_0
                );
                order
            } else {
                asset_builder.add_gas_fee(
                    AssetBuilderStage::TopOfBlock,
                    t0,
                    tob.priority_data.gas.to()
                );
                TopOfBlockOrder::of_max_gas(tob, pair_idx as u16)
            };
            let is_bid = tob.asset_in == t1;
            trace!(quantity_in = ?contract_tob.quantity_in, quantity_out = ?contract_tob.quantity_out, is_bid, "Processing searcher (ToB) order");
            top_of_block_orders.push(contract_tob);
        }

        ///////////////////////////////
        //// handling user orders ////
        /////////////////////////////

        let default = HashMap::new();
        // Get our list of user orders, if we have any
        let mut order_list: HashMap<OrderId, &OrderWithStorageData<AllOrders>> = orders_by_pool
            .get(&solution.id)
            .map(|o| o.iter().map(|order| (order.order_id, order)).collect())
            .unwrap_or_else(|| default);

        // Loop through our filled user orders, do accounting, and add them to our user
        // order list
        let mut total_user_fees: u128 = 0;
        for (outcome, order) in solution
            .limit
            .iter()
            .map(|order| (order, order_list.remove(&order.id)))
            .filter(|(outcome, o)| {
                if o.is_none() && outcome.is_filled() {
                    tracing::error!(?outcome, "lost a order");
                    return false;
                }
                outcome.is_filled()
            })
        {
            total_user_fees = total_user_fees.saturating_add(Self::apply_user_order(
                outcome,
                order,
                solution.ucp,
                solution.fee,
                shared_gas,
                pair_idx,
                asset_builder,
                user_orders
            )?);
        }
        tracing::info!(?solution.fee, total_user_fees, "fees for book");

        ///////////////////////////////
        //// handling donate merge ////
        //////////////////////////////

        // Now it's time to figure out what's happening with our AMM swap and pool
        // rewards
        // Let's get our swap and reward data out of our ToB order, if it exists
        let tob_swap_info = if let Some(ref tob) = solution.searcher {
            match TopOfBlockOrder::calc_vec_and_reward(tob, snapshot) {
                Ok((v, reward_q)) => {
                    trace!(
                        net_t0 = v.total_d_t0,
                        net_t1 = v.total_d_t1,
                        end_price = ?v.end_price,
                        is_bid = !v.zero_for_one(),
                        reward_q,
                        "Processing ToB swap vs AMM"
                    );
                    Some((v, reward_q))
                }
                Err(error) => {
                    error!(?error, "Error in ToB swap vs AMM");
                    None
                }
            }
        } else {
            trace!("No ToB Swap vs AMM");
            None
        };

        // If we have a ToB swap, our post-tob-price is the price at the end of that
        // swap, otherwise we're starting from the snapshot's current price
        let post_tob_price = tob_swap_info
            .clone()
            .map(|(v, _)| v)
            .unwrap_or_else(|| snapshot.noop());

        // NOTE: if we have no books, its a zero swap from exact price to exact price.
        // optimally we have these separate branches but this is just a patch fix
        let book_swap_vec = if solution.ucp.is_zero() {
            // snapshot.noop()
            trace!("No book swap, UCP was zero");
            None
        } else {
            let post_tob = post_tob_price.end_price;
            let ucp: SqrtPriceX96 = solution.ucp.into();
            // grab amount in when swap to price, then from there, calculate
            // actual values.
            let book_swap_vec =
                post_tob_price.swap_to_price(Direction::from_prices(post_tob, ucp), ucp)?;
            trace!(
                net_t0 = book_swap_vec.total_d_t0,
                net_t1 = book_swap_vec.total_d_t1,
                end_price = ?book_swap_vec.end_price,
                is_bid = !book_swap_vec.zero_for_one(),
                reward_q = solution.reward_t0,
                "Processing Book swap vs AMM"
            );
            Some(book_swap_vec)
        };

        // add user donation split
        let total_lp_user_donate = (total_user_fees as f64 * LP_DONATION_SPLIT) as u128;
        let save_amount = total_user_fees - total_lp_user_donate;

        // We then use `post_tob_price` as the start price for our book swap, just as
        // our matcher did.  We want to use the representation of the book swap
        // (`book_swap_vec`) to distribute any extra rewards from our book matching.

        // We're making an assumption here that's valid for the Delta validator (that
        // the AMM was swapped during matching from the post_tob_price to the UCP)
        // let book_swap_vec = PoolPriceVec::from_price_range(post_tob_price,
        // book_end_price)?;

        // We need to do our donations in the right order - first the ToB and then the
        // book.  So let's do that
        let book_donation_vec = book_swap_vec
            .as_ref()
            .map(|bsv| bsv.t0_donation_vec(solution.reward_t0 + total_lp_user_donate));

        let tob_donation_vec = tob_swap_info
            .as_ref()
            .map(|(tob_vec, tob_d)| tob_vec.t0_donation_vec(*tob_d));

        let donation = match (book_donation_vec, tob_donation_vec) {
            (Some(bsv), Some(tob)) => {
                Some(&(DonationCalculation::from_vec(&tob))? + bsv.as_slice())
            }
            (Some(bsv), None) => Some(DonationCalculation::from_vec(&bsv)?),
            (None, Some(tob)) => Some(DonationCalculation::from_vec(&tob)?),
            (None, None) => None
        };
        let total_donation = donation
            .as_ref()
            .map(|d| d.total_donated)
            .unwrap_or(solution.reward_t0 + total_lp_user_donate);

        // Find our net AMM vec by combining T0s.  There's not a specific reason we use
        // T0 for this, we might want to make this a bit more robust or careful
        let net_pool_vec = if let Some((tob_vec, _)) = tob_swap_info {
            // zero for 1 is neg

            // if zero for 1 is neg
            let net_t0 = book_swap_vec
                .as_ref()
                .map(|b| b.t0_signed())
                .unwrap_or(I256::ZERO)
                + tob_vec.t0_signed();

            let net_direction =
                if net_t0.is_negative() { Direction::SellingT0 } else { Direction::BuyingT0 };

            let amount_in = if net_t0.is_negative() {
                net_t0.unsigned_abs()
            } else {
                (book_swap_vec
                    .as_ref()
                    .map(|b| b.t1_signed())
                    .unwrap_or(I256::ZERO)
                    + tob_vec.t1_signed())
                .unsigned_abs()
            };

            snapshot
                .swap_current_with_amount(I256::from_raw(amount_in), net_direction)
                .unwrap()
                .clone()
            // snap
        } else {
            book_swap_vec.unwrap_or_else(|| snapshot.noop())
        };

        trace!(
            net_t0 = net_pool_vec.total_d_t0,
            net_t1 = net_pool_vec.total_d_t1,
            end_price = ?net_pool_vec.end_price,
            is_bid = !net_pool_vec.zero_for_one(),
            reward_q = total_donation,
            "Built net swap vec"
        );

        // Account for our net_pool_vec
        let (asset_in_index, asset_out_index) =
            if net_pool_vec.zero_for_one() { (t0_idx, t1_idx) } else { (t1_idx, t0_idx) };
        let (quantity_in, quantity_out) = (net_pool_vec.input(), net_pool_vec.output());

        asset_builder.uniswap_swap(
            AssetBuilderStage::Swap,
            asset_in_index as usize,
            asset_out_index as usize,
            quantity_in,
            quantity_out
        );

        // Now that we have our net pool vec, we know what the "current tick" is
        // for the uniswap pool after our swap. Now we want to merge our t0
        // rewards

        // Account for our total reward and fees
        // We might want to split this in some way in the future

        // Allocate the reward quantity
        asset_builder.allocate(AssetBuilderStage::Reward, t0, total_donation);
        asset_builder.allocate(AssetBuilderStage::Reward, t0, save_amount);
        asset_builder.add_gas_fee(AssetBuilderStage::Reward, t0, save_amount);
        // Account for our tribute

        let (rewards_update, optional_reward) = donation
            .map(|d| d.into_reward_updates())
            .unwrap_or_else(|| {
                (
                    RewardsUpdate::CurrentOnly {
                        amount:             total_donation,
                        expected_liquidity: snapshot.current_liquidity()
                    },
                    None
                )
            });

        // The first PoolUpdate is the actual net pool swap and associated rewards
        pool_updates.push(PoolUpdate {
            zero_for_one: net_pool_vec.zero_for_one(),
            pair_index: pair_idx as u16,
            swap_in_quantity: net_pool_vec.input(),
            rewards_update
        });

        if let Some(optional_reward) = optional_reward {
            pool_updates.push(PoolUpdate {
                zero_for_one:     false,
                pair_index:       pair_idx as u16,
                swap_in_quantity: 0,
                rewards_update:   optional_reward
            });
        }

        // Drop our span to give it purpose in life
        drop(process_solution_span);

        // And we're done
        Ok(())
    }

    fn apply_user_order(
        outcome: &OrderOutcome,
        order: Option<&OrderWithStorageData<AllOrders>>,
        ucp: Ray,
        fee: u32,
        shared_gas: Option<U256>,
        pair_idx: usize,
        asset_builder: &mut AssetBuilder,
        user_orders: &mut Vec<UserOrder>
    ) -> eyre::Result<u128> {
        let order = order.unwrap();
        // Calculate our final amounts based on whether the order is in T0 or T1 context
        assert_eq!(outcome.id.hash, order.order_id.hash, "Order and outcome mismatched");

        let fill_amount = outcome.fill_amount(order.amount());

        // TODO: this needs to be properly set

        let gas = order.priority_data.gas.to::<u128>();
        let (t1, t0_net, t0_fee) = get_quantities_at_price(
            order.is_bid(),
            order.exact_in(),
            fill_amount,
            gas,
            fee as u128,
            ucp
        );

        // we don't account for the gas here in these quantites as the order
        let (quantity_in, quantity_out) = if order.is_bid() {
            // one for zero

            // If the order is a bid, we're getting all our T1 in and we're sending t0_net
            // back to the contract
            (t1, t0_net)
        } else {
            // If the order is an ask, we're getting t0_net + t0_fee + gas in and we're
            // sending t1 back to the contract
            // zero for one
            (t0_net + t0_fee + gas, t1)
        };

        trace!(quantity_in = ?quantity_in, quantity_out = ?quantity_out, is_bid = order.is_bid, exact_in = order.exact_in(), "Processing user order");
        let token_in = order.token_in();
        let token_out = order.token_out();

        let user_order = if let Some(g) = shared_gas {
            UserOrder::from_internal_order(order, outcome, g, pair_idx as u16)?
        } else {
            UserOrder::from_internal_order_max_gas(order, outcome, pair_idx as u16)
        };

        // we add once we are past anything that can error.
        asset_builder.external_swap(
            AssetBuilderStage::UserOrder,
            token_in,
            token_out,
            quantity_in,
            quantity_out
        );
        user_orders.push(user_order);
        Ok(t0_fee)
    }

    pub fn from_proposal(
        proposal: &Proposal,
        orders: OrderSet<OrderWithStorageData<AllOrders>, OrderWithStorageData<RpcTopOfBlockOrder>>,
        _gas_details: BundleGasDetails,
        pools: &HashMap<PoolId, (Address, Address, BaselinePoolState, u16)>
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
        let (total_swaps, _) = Self::fetch_total_orders_and_gas_delegated_to_orders(
            &orders_by_pool,
            &proposal.solutions
        );
        // this should never underflow. if it does. means that there is underlying
        // problem with the gas delegation module

        if total_swaps == 0 {
            return Err(eyre::eyre!("have a total swaps count of 0"));
        }

        // what we need to do is go through and first add all the tokens,
        // then sort them and change the offests before we index all orders
        for solution in proposal.solutions.iter() {
            let Some((t0, t1, ..)) = pools.get(&solution.id) else {
                // This should never happen but let's handle it as gracefully as possible -
                // right now will skip the pool, not produce an error
                warn!(
                    "Skipped a solution as we couldn't find a pool for it: {:?}, {:?}",
                    pools, solution.id
                );
                continue;
            };
            asset_builder.add_or_get_asset(*t0);
            asset_builder.add_or_get_asset(*t1);
        }
        asset_builder.order_assets_properly();

        // fetch gas used
        // Walk through our solutions to add them to the structure
        for solution in proposal.solutions.iter().sorted_unstable_by_key(|k| {
            let Some((t0, t1, ..)) = pools.get(&k.id) else {
                // This should never happen but let's handle it as gracefully as possible -
                // right now will skip the pool, not produce an error
                return 0usize;
            };
            let t0_idx = asset_builder.add_or_get_asset(*t0);
            let t1_idx = asset_builder.add_or_get_asset(*t1);
            (t0_idx << 16) | t1_idx
        }) {
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

            // Call our processing function with a fixed amount of shared gas
            Self::process_solution(
                &mut pairs,
                &mut asset_builder,
                &mut user_orders,
                &orders_by_pool,
                &mut top_of_block_orders,
                &mut pool_updates,
                solution,
                snapshot,
                *t0,
                *t1,
                *store_index,
                Some(U256::ZERO)
            )?;
        }

        Ok(Self::new(
            asset_builder.get_asset_array(),
            pairs,
            pool_updates,
            top_of_block_orders,
            user_orders
        ))
    }

    /// builds a bundle where orders are set to max allocated gas to ensure a
    /// fully passing env. with the gas details from the response, can
    /// properly allocate order gas amounts.
    pub fn for_gas_finalization(
        limit: Vec<OrderWithStorageData<AllOrders>>,
        solutions: Vec<PoolSolution>,
        pools: &HashMap<PoolId, (Address, Address, BaselinePoolState, u16)>
    ) -> eyre::Result<Self> {
        let mut top_of_block_orders = Vec::new();
        let mut pool_updates = Vec::new();
        let mut pairs = Vec::new();
        let mut user_orders = Vec::new();
        let mut asset_builder = AssetBuilder::new();

        let orders_by_pool: HashMap<
            alloy_primitives::FixedBytes<32>,
            HashSet<OrderWithStorageData<AllOrders>>
        > = limit.iter().fold(HashMap::new(), |mut acc, x| {
            acc.entry(x.pool_id).or_default().insert(x.clone());
            acc
        });

        // what we need to do is go through and first add all the tokens,
        // then sort them and change the offests before we index all orders
        for solution in solutions.iter() {
            let Some((t0, t1, ..)) = pools.get(&solution.id) else {
                // This should never happen but let's handle it as gracefully as possible -
                // right now will skip the pool, not produce an error
                warn!(
                    "Skipped a solution as we couldn't find a pool for it: {:?}, {:?}",
                    pools, solution.id
                );
                continue;
            };
            asset_builder.add_or_get_asset(*t0);
            asset_builder.add_or_get_asset(*t1);
        }
        asset_builder.order_assets_properly();

        // Walk through our solutions to add them to the structure
        for solution in solutions.iter().sorted_unstable_by_key(|k| {
            let Some((t0, t1, ..)) = pools.get(&k.id) else {
                // This should never happen but let's handle it as gracefully as possible -
                // right now will skip the pool, not produce an error
                return 0usize;
            };
            let t0_idx = asset_builder.add_or_get_asset(*t0);
            let t1_idx = asset_builder.add_or_get_asset(*t1);
            (t0_idx << 16) | t1_idx
        }) {
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
            // Call our processing function with a fixed amount of shared gas
            Self::process_solution(
                &mut pairs,
                &mut asset_builder,
                &mut user_orders,
                &orders_by_pool,
                &mut top_of_block_orders,
                &mut pool_updates,
                solution,
                snapshot,
                *t0,
                *t1,
                *store_index,
                None
            )?;
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

#[allow(unused)]
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
    pub async fn load_from_chain<N, P>(
        angstrom_contract: Address,
        block_id: BlockId,
        provider: &P
    ) -> Result<AngstromPoolConfigStore, String>
    where
        N: Network,
        P: Provider<N>
    {
        // offset of 6 bytes
        let value = provider
            .get_storage_at(angstrom_contract, U256::from(CONFIG_STORE_SLOT))
            .block_id(block_id)
            .await
            .map_err(|e| format!("Error getting storage: {}", e))?;

        let value_bytes: [u8; 32] = value.to_be_bytes();
        tracing::debug!("storage slot of poolkey storage {:?}", value_bytes);
        let config_store_address =
            Address::from(<[u8; 20]>::try_from(&value_bytes[4..24]).unwrap());
        tracing::info!(?config_store_address);

        let code = provider
            .get_code_at(config_store_address)
            .block_id(block_id)
            .await
            .map_err(|e| format!("Error getting code: {}", e))?;

        tracing::info!(len=?code.len(), "bytecode: {:x}", code);

        AngstromPoolConfigStore::try_from(code.as_ref())
            .map_err(|e| format!("Failed to deserialize code into AngstromPoolConfigStore: {}", e))
    }

    pub fn length(&self) -> usize {
        self.entries.len()
    }

    pub fn remove_pair(&self, asset0: Address, asset1: Address) {
        let key = Self::derive_store_key(asset0, asset1);

        let Some((_, entry)) = self.entries.remove(&key) else { return };
        let index = entry.store_index;

        // if we have any indexes that are GT the index we remove, we subtract 1 from it
        self.entries.iter_mut().for_each(|mut f| {
            let v = f.value_mut();
            if v.store_index > index {
                v.store_index -= 1;
            }
        })
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
        if value.is_empty() {
            return Ok(Self::default());
        }

        if value.first() != Some(&0) {
            return Err("Invalid encoded entries: must start with a safety byte of 0".to_string());
        }
        tracing::info!(bytecode_len=?value.len());
        let adjusted_entries = &value[1..];
        if adjusted_entries.len() % POOL_CONFIG_STORE_ENTRY_SIZE != 0 {
            tracing::info!(bytecode_len=?adjusted_entries.len(), ?POOL_CONFIG_STORE_ENTRY_SIZE);
            return Err(
                "Invalid encoded entries: incorrect length after removing safety byte".to_string()
            );
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
