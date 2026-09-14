use std::collections::{HashMap, HashSet};

use alloy_primitives::{Address, FixedBytes, I256, U256};
use angstrom_types_primitives::{
    contract_payloads::{
        Pair,
        angstrom::{AngstromBundle, BundleGasDetails, TopOfBlockOrder, UserOrder},
        asset::builder::*,
        protocol_fees::DonationSplits,
        rewards::{PoolUpdate, RewardsUpdate}
    },
    primitive::{OrderId, PoolId, Ray, SqrtPriceX96},
    sol_bindings::{
        RawPoolOrder,
        grouped_orders::{AllOrders, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder as RpcTopOfBlockOrder
    }
};
use base64::{Engine, prelude::BASE64_STANDARD};
use itertools::Itertools;
use tracing::{Level, debug, error, trace, warn};

use crate::{
    consensus::{PreProposal, Proposal},
    matching::get_quantities_at_price,
    orders::{OrderFillState, OrderOutcome, OrderSet, PoolSolution},
    testnet::TestnetStateOverrides,
    traits::{tob::TopOfBlockOrderRewardCalc, user_orders::UserOrderFromInternal},
    uni_structure::{
        BaselinePoolState,
        donation::{DonationCalculation, DonationResidual, check_conservation, sum_donations}
    }
};

pub trait BundleProcessing: Sized {
    fn fetch_needed_overrides(&self, block_number: u64) -> TestnetStateOverrides;

    fn process_solution(
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
        shared_gas: Option<U256>,
        splits: DonationSplits
    ) -> eyre::Result<()>;

    fn build_dummy_for_tob_gas(
        user_order: &OrderWithStorageData<RpcTopOfBlockOrder>
    ) -> eyre::Result<Self>;

    fn build_dummy_for_user_gas(user_order: &OrderWithStorageData<AllOrders>)
    -> eyre::Result<Self>;

    fn fetch_total_orders_and_gas_delegated_to_orders(
        orders_by_pool: &HashMap<FixedBytes<32>, HashSet<OrderWithStorageData<AllOrders>>>,
        solutions: &[PoolSolution]
    ) -> (u64, u64);

    fn apply_user_order(
        outcome: &OrderOutcome,
        order: Option<&OrderWithStorageData<AllOrders>>,
        ucp: Ray,
        fee: u32,
        shared_gas: Option<U256>,
        pair_idx: usize,
        asset_builder: &mut AssetBuilder,
        user_orders: &mut Vec<UserOrder>
    ) -> eyre::Result<u128>;

    fn orders_by_pool_from_pre_proposals(
        pre_proposals: &[PreProposal],
        orders: OrderSet<AllOrders, RpcTopOfBlockOrder>
    ) -> HashMap<PoolId, HashSet<OrderWithStorageData<AllOrders>>>;

    fn from_proposal(
        proposal: &Proposal,
        orders: OrderSet<AllOrders, RpcTopOfBlockOrder>,
        _gas_details: BundleGasDetails,
        pools: &HashMap<PoolId, (Address, Address, BaselinePoolState, u16)>,
        splits: DonationSplits
    ) -> eyre::Result<Self>;

    fn for_gas_finalization(
        limit: Vec<OrderWithStorageData<AllOrders>>,
        solutions: Vec<PoolSolution>,
        pools: &HashMap<PoolId, (Address, Address, BaselinePoolState, u16)>,
        splits: DonationSplits
    ) -> eyre::Result<Self>;
}

impl BundleProcessing for AngstromBundle {
    fn fetch_needed_overrides(&self, block_number: u64) -> TestnetStateOverrides {
        let mut approvals: HashMap<Address, HashMap<Address, u128>> = HashMap::new();
        let mut balances: HashMap<Address, HashMap<Address, u128>> = HashMap::new();
        let mut angstrom_balances: HashMap<Address, HashMap<Address, u128>> = HashMap::new();

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

            if order.use_internal {
                // For internal balance orders, add to angstrom_balances
                angstrom_balances
                    .entry(token)
                    .or_default()
                    .entry(address)
                    .and_modify(|q| {
                        *q = q.saturating_add(qty);
                    })
                    .or_insert(qty);
            } else {
                // For external balance orders, add to approvals and balances
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
            }
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

            if order.use_internal {
                // For internal balance orders, add to angstrom_balances
                angstrom_balances
                    .entry(token)
                    .or_default()
                    .entry(address)
                    .and_modify(|q| {
                        *q = q.saturating_add(qty);
                    })
                    .or_insert(qty);
            } else {
                // For external balance orders, add to approvals and balances
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
            }
        });

        TestnetStateOverrides { approvals, balances, angstrom_balances }
    }

    fn process_solution(
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
        shared_gas: Option<U256>,
        splits: DonationSplits
    ) -> eyre::Result<()> {
        tracing::info!(?solution);
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
            let b64_output = BASE64_STANDARD.encode(json.as_bytes());
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
        let pair_idx = pairs.len();

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

        let mut ucp = solution.ucp;

        // Now it's time to figure out what's happening with our AMM swap and pool
        // rewards
        // Let's get our swap and reward data out of our ToB order, if it exists
        let tob_swap_info = if let Some(ref tob) = solution.searcher {
            match TopOfBlockOrder::calc_vec_and_reward(tob, snapshot) {
                Ok((v, reward_q)) => {
                    // if we have a tob swap, and ucp == 0, we are going to want to update it
                    if ucp.is_zero() {
                        ucp = Ray::from(v.end_price);
                    }

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

        pairs.push(Pair { index0: t0_idx, index1: t1_idx, store_index, price_1over0: *ucp });

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
            let ucp: SqrtPriceX96 = solution.ucp.into();
            tracing::info!(?solution);
            // grab amount in when swap to price, then from there, calculate
            // actual values.
            let book_swap_vec = post_tob_price.swap_to_price(ucp).unwrap();
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
        let (total_lp_user_donate, user_protocol_fee) = splits.split_user(total_user_fees);

        // We then use `post_tob_price` as the start price for our book swap, just as
        // our matcher did.  We want to use the representation of the book swap
        // (`book_swap_vec`) to distribute any extra rewards from our book matching.

        // We're making an assumption here that's valid for the Delta validator (that
        // the AMM was swapped during matching from the post_tob_price to the UCP)
        // let book_swap_vec = PoolPriceVec::from_price_range(post_tob_price,
        // book_end_price)?;

        // The budget the book allocator is handed: book surplus plus the LP share of
        // the user fees.
        let book_budget = solution
            .reward_t0
            .checked_add(total_lp_user_donate)
            .ok_or_else(|| eyre::eyre!("book donation budget exceeds u128"))?;

        // We need to do our donations in the right order - first the ToB and then the
        // book.  So let's do that
        let (book_donation_vec, book_residual) = match book_swap_vec.as_ref() {
            Some(bsv) => {
                let (vec, residual) = bsv.t0_donation_vec(book_budget)?;
                (Some(vec), residual)
            }
            // A zero UCP hands this budget to no allocator. Where it lands then
            // depends on the ToB side: with no ToB vector either, the `CurrentOnly`
            // fallback below donates the whole budget to the current tick, so it is
            // placed; after a ToB move it stays in `contract_liquid` and
            // `collect_extra` sweeps it into `save`.
            None if tob_swap_info.is_none() => (None, DonationResidual::default()),
            None => (None, DonationResidual { rounding: 0, unplaced: book_budget })
        };

        let (tob_donation_vec, tob_protocol_fee, tob_residual) = match tob_swap_info.as_ref() {
            Some((tob_vec, gross_tob_reward)) => {
                let (tob_lp_budget, protocol) = splits.split_tob(*gross_tob_reward);
                let (vec, residual) = tob_vec.t0_donation_vec(tob_lp_budget)?;
                (Some(vec), protocol, residual)
            }
            None => (None, 0u128, DonationResidual::default())
        };

        // Conservation, per pool and per source. The ToB gross splits three ways - LPs,
        // the configured fee, and what the allocator did not place - and the book
        // budget two.
        let gross_tob = tob_swap_info.as_ref().map(|(_, gross)| *gross).unwrap_or(0);
        check_conservation(
            "ToB",
            tob_donation_vec
                .as_deref()
                .map(sum_donations)
                .transpose()?
                .unwrap_or(0),
            tob_protocol_fee,
            tob_residual,
            gross_tob
        )?;
        // `placed` comes from the vectors and the residual from the swap info on
        // purpose: swapping the residual arms above fails this check in both cases.
        let book_placed = match book_donation_vec.as_deref() {
            Some(vec) => sum_donations(vec)?,
            // No vector from either source: the `CurrentOnly` fallback places the
            // whole budget.
            None if tob_donation_vec.is_none() => book_budget,
            None => 0
        };
        check_conservation("book", book_placed, 0, book_residual, book_budget)?;

        // Both retained portions settle together through `save`. The residuals above
        // reach `save` through `collect_extra` instead, so adding them here would
        // count them twice.
        let save_amount = user_protocol_fee
            .checked_add(tob_protocol_fee)
            .ok_or_else(|| eyre::eyre!("retained fees exceed u128"))?;

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
            .unwrap_or(book_budget);

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

            let net_direction = net_t0.is_negative();

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

    fn build_dummy_for_tob_gas(
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

    fn build_dummy_for_user_gas(
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

        tracing::info!(quantity_in = ?quantity_in, quantity_out = ?quantity_out, is_bid = order.is_bid, exact_in = order.exact_in(), "Processing user order");
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

    fn orders_by_pool_from_pre_proposals(
        pre_proposals: &[PreProposal],
        orders: OrderSet<AllOrders, RpcTopOfBlockOrder>
    ) -> HashMap<PoolId, HashSet<OrderWithStorageData<AllOrders>>> {
        let valid_limit = pre_proposals
            .iter()
            .flat_map(|pre| pre.limit.clone())
            .collect();
        let (limit_orders, _) = orders.into_book_and_searcher(valid_limit, vec![]);

        limit_orders.into_iter().fold(HashMap::new(), |mut acc, x| {
            acc.entry(x.pool_id).or_default().insert(x);
            acc
        })
    }

    fn from_proposal(
        proposal: &Proposal,
        orders: OrderSet<AllOrders, RpcTopOfBlockOrder>,
        _gas_details: BundleGasDetails,
        pools: &HashMap<PoolId, (Address, Address, BaselinePoolState, u16)>,
        splits: DonationSplits
    ) -> eyre::Result<Self> {
        trace!("Starting from_proposal");
        let mut top_of_block_orders = Vec::new();
        let mut pool_updates = Vec::new();
        let mut pairs = Vec::new();
        let mut user_orders = Vec::new();
        let mut asset_builder = AssetBuilder::new();

        // Break out our input orders into lists of orders by pool
        let preproposals = proposal.flattened_pre_proposals();
        let orders_by_pool = Self::orders_by_pool_from_pre_proposals(&preproposals, orders);

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
                Some(U256::ZERO),
                splits
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
    fn for_gas_finalization(
        limit: Vec<OrderWithStorageData<AllOrders>>,
        solutions: Vec<PoolSolution>,
        pools: &HashMap<PoolId, (Address, Address, BaselinePoolState, u16)>,
        splits: DonationSplits
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
                None,
                splits
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

#[cfg(test)]
mod tests {
    use alloy_primitives::address;
    use angstrom_types_primitives::{
        AngstromAddressConfig,
        orders::builders::{StoredOrderBuilder, ToBOrderBuilder, UserOrderBuilder},
        primitive::{AngstromSigner, SqrtPriceX96}
    };

    use super::*;
    use crate::uni_structure::liquidity_base::BaselineLiquidity;

    const T0: Address = address!("0x0000000000000000000000000000000000000001");
    const T1: Address = address!("0x0000000000000000000000000000000000000010");
    /// A second token1 for the two-pool case, so both pools share T0 only.
    const T1_B: Address = address!("0x0000000000000000000000000000000000000011");

    /// A single liquidity range with no initialized ticks, sitting at tick 0.
    fn pool(liquidity: u128) -> BaselinePoolState {
        BaselinePoolState::new(
            BaselineLiquidity::new(
                10,
                0,
                SqrtPriceX96::at_tick(0).unwrap(),
                liquidity,
                HashMap::new(),
                HashMap::new()
            ),
            0,
            0
        )
    }

    fn pool_id(n: u8) -> PoolId {
        PoolId::with_last_byte(n)
    }

    fn user_order(
        pool_id: PoolId,
        is_bid: bool,
        amount: u128,
        price: Ray
    ) -> OrderWithStorageData<AllOrders> {
        UserOrderBuilder::new()
            .exact()
            .is_bid(is_bid)
            .amount(amount)
            .exact_in(true)
            .min_price(price)
            .signing_key(Some(AngstromSigner::random()))
            .with_storage()
            .pool_id(pool_id)
            .is_bid(is_bid)
            .build()
    }

    /// A ToB bid paying `quantity_in` of T1 for `quantity_out` of T0. The gross
    /// surplus `calc_vec_and_reward` reports is whatever the AMM returns over
    /// `quantity_out`, so callers pick the gross by choosing `quantity_out`.
    fn tob_order(
        pool_id: PoolId,
        quantity_in: u128,
        quantity_out: u128
    ) -> OrderWithStorageData<RpcTopOfBlockOrder> {
        let order = ToBOrderBuilder::new()
            .asset_in(T1)
            .asset_out(T0)
            .quantity_in(quantity_in)
            .quantity_out(quantity_out)
            .signing_key(Some(AngstromSigner::random()))
            .build();
        StoredOrderBuilder::new(AllOrders::TOB(order.clone()))
            .pool_id(pool_id)
            .bid()
            .build()
            .try_map_inner(|_| Ok(order.clone()))
            .unwrap()
    }

    /// Builds a ToB bid whose gross surplus is exactly `gross`.
    fn tob_with_gross(
        snap: &BaselinePoolState,
        id: PoolId,
        gross: u128
    ) -> (OrderWithStorageData<RpcTopOfBlockOrder>, u128) {
        let quantity_in = 1_000_000u128;
        let out = snap
            .swap_current_with_amount(I256::unchecked_from(quantity_in), false)
            .unwrap()
            .total_d_t0;
        (tob_order(id, quantity_in, out - gross), gross)
    }

    fn filled(order: &OrderWithStorageData<AllOrders>) -> OrderOutcome {
        OrderOutcome { id: order.order_id, outcome: OrderFillState::CompleteFill }
    }

    fn orders_by_pool(
        orders: &[OrderWithStorageData<AllOrders>]
    ) -> HashMap<FixedBytes<32>, HashSet<OrderWithStorageData<AllOrders>>> {
        orders.iter().fold(HashMap::new(), |mut acc, o| {
            acc.entry(o.pool_id).or_default().insert(o.clone());
            acc
        })
    }

    /// Everything `process_solution` writes out, for one or more pools driven
    /// through a single `AssetBuilder`.
    struct Solved {
        pairs:        Vec<Pair>,
        assets:       Vec<angstrom_types_primitives::contract_payloads::Asset>,
        tob_orders:   Vec<TopOfBlockOrder>,
        pool_updates: Vec<PoolUpdate>
    }

    impl Solved {
        fn save(&self, token: Address) -> u128 {
            self.assets.iter().find(|a| a.addr == token).unwrap().save
        }

        fn asset(&self, token: Address) -> &angstrom_types_primitives::contract_payloads::Asset {
            self.assets.iter().find(|a| a.addr == token).unwrap()
        }

        /// Total T0 the bundle hands to LPs for `pair`.
        fn rewarded(&self, pair: u16) -> u128 {
            self.pool_updates
                .iter()
                .filter(|u| u.pair_index == pair)
                .flat_map(|u| u.rewards_update.quantities())
                .sum()
        }
    }

    fn solve(
        pools: &[(PoolSolution, BaselinePoolState, Address, Address)],
        orders: &[OrderWithStorageData<AllOrders>],
        splits: DonationSplits
    ) -> eyre::Result<Solved> {
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();
        let by_pool = orders_by_pool(orders);
        let mut asset_builder = AssetBuilder::new();
        for (_, _, t0, t1) in pools {
            asset_builder.add_or_get_asset(*t0);
            asset_builder.add_or_get_asset(*t1);
        }
        asset_builder.order_assets_properly();

        let mut pairs = Vec::new();
        let mut user_orders = Vec::new();
        let mut tob_orders = Vec::new();
        let mut pool_updates = Vec::new();
        for (solution, snapshot, t0, t1) in pools {
            AngstromBundle::process_solution(
                &mut pairs,
                &mut asset_builder,
                &mut user_orders,
                &by_pool,
                &mut tob_orders,
                &mut pool_updates,
                solution,
                snapshot,
                *t0,
                *t1,
                0,
                None,
                splits
            )?;
        }
        Ok(Solved { pairs, assets: asset_builder.get_asset_array(), tob_orders, pool_updates })
    }

    /// A batch the book clears on its own: bid and ask net out in T1, the AMM
    /// never moves, and the only T0 surplus is the user fee. Both halves of the
    /// split have to be accounted even though no donation is ever placed.
    #[test]
    fn book_only_exact_match_with_user_fees() {
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();
        let snap = pool(1_000_000_000_000_000);
        let ucp = Ray::from(snap.current_price());
        let id = pool_id(1);
        let fee = 2_000u32;

        let ask = user_order(id, false, 1_000_000, ucp);
        let (t1_out, _, ask_fee) =
            get_quantities_at_price(false, true, 1_000_000, 0, fee as u128, ucp);
        let bid = user_order(id, true, t1_out, ucp);
        let (_, _, bid_fee) = get_quantities_at_price(true, true, t1_out, 0, fee as u128, ucp);

        let total_user_fees = ask_fee + bid_fee;
        assert!(total_user_fees > 0, "the case needs positive user fees");

        let splits = DonationSplits::new(750_000, 1_000_000).unwrap();
        let (lp_share, protocol_fee) = splits.split_user(total_user_fees);
        assert!(protocol_fee > 0, "the case needs a nonzero protocol share");

        let solution = PoolSolution {
            id,
            ucp,
            fee,
            limit: vec![filled(&ask), filled(&bid)],
            ..Default::default()
        };
        let solved = solve(&[(solution, snap, T0, T1)], &[ask, bid], splits).unwrap();

        // No searcher: nothing encoded, nothing rewarded, and the ToB side of
        // the conservation check ran on a gross of zero.
        assert!(solved.tob_orders.is_empty());

        // The AMM did not move, so the allocator had no range to place into and
        // the LP share is retained rather than donated.
        assert_eq!(solved.rewarded(0), 0);
        assert_eq!(
            solved.save(T0),
            protocol_fee + lp_share,
            "both halves of the user fee must reach `save`"
        );

        // The configured fee is reserved rather than swept: `allocate` spends it
        // out of contract liquidity, borrowing from Uniswap because the reward
        // stage starts empty, which is what keeps `collect_extra` from counting
        // it a second time.
        let t0 = solved.asset(T0);
        assert_eq!((t0.take, t0.settle), (protocol_fee, protocol_fee));
    }

    /// `ucp == 0` sends the book down the branch that never reaches an
    /// allocator. Conservation still has to balance, which it only does because
    /// that branch reports its budget as `unplaced`. This pins the retained
    /// `(None, Some(tob))` arm: swapping the two residual arms fails it.
    #[test]
    fn book_noop_after_tob_move() {
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();
        let snap = pool(1_000_000_000_000_000);
        let id = pool_id(1);
        let (searcher, gross) = tob_with_gross(&snap, id, 1_001);

        let splits = DonationSplits::new(750_000, 750_000).unwrap();
        let (lp_budget, protocol_fee) = splits.split_tob(gross);

        let solution = PoolSolution {
            id,
            ucp: Ray::ZERO,
            searcher: Some(searcher.clone()),
            reward_t0: 5_000,
            ..Default::default()
        };
        let solved = solve(&[(solution, snap.clone(), T0, T1)], &[], splits).unwrap();

        // The book budget reached no allocator and is retained; the call only
        // returns `Ok` because that retention is reported rather than dropped.
        assert_eq!(solved.rewarded(0), lp_budget);
        assert_eq!(solved.save(T0), protocol_fee);

        // The pair is priced at the end of the ToB swap, not at the pool's
        // pre-ToB price.
        let (tob_vec, _) = TopOfBlockOrder::calc_vec_and_reward(&searcher, &snap).unwrap();
        assert_eq!(solved.pairs[0].price_1over0, *Ray::from(tob_vec.end_price));
        assert_ne!(solved.pairs[0].price_1over0, *Ray::from(snap.current_price()));
    }

    /// The `(None, None)` arm: `ucp == 0` and no ToB, so neither source
    /// produces a vector. The `CurrentOnly` fallback donates the whole book
    /// budget to the current tick, so it must be reported as placed, not
    /// `unplaced`.
    #[test]
    fn book_budget_with_no_vectors_is_placed_at_the_current_tick() {
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();
        let snap = pool(1_000_000_000_000_000);
        let solution = PoolSolution {
            id: pool_id(1),
            ucp: Ray::ZERO,
            searcher: None,
            reward_t0: 5_000,
            ..Default::default()
        };
        let solved = solve(
            &[(solution, snap.clone(), T0, T1)],
            &[],
            DonationSplits::new(750_000, 1_000_000).unwrap()
        )
        .unwrap();

        assert_eq!(
            solved.pool_updates[0].rewards_update,
            RewardsUpdate::CurrentOnly {
                amount:             5_000,
                expected_liquidity: snap.current_liquidity()
            }
        );
        assert_eq!(solved.rewarded(0), 5_000);
        assert_eq!(solved.save(T0), 0);
    }

    /// A swap vector that exists but holds no steps still goes to the
    /// allocator. Skipping it would report no residual and fail
    /// conservation; falling back to `None` would reward the whole budget
    /// to the current tick instead.
    #[test]
    fn some_empty_still_allocates() {
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();
        let snap = pool(1_000_000_000_000_000);
        let ucp = Ray::from(snap.current_price());
        let solution = PoolSolution { id: pool_id(1), ucp, reward_t0: 5_000, ..Default::default() };

        let solved = solve(
            &[(solution, snap, T0, T1)],
            &[],
            DonationSplits::new(750_000, 1_000_000).unwrap()
        )
        .unwrap();

        // An empty `DonationCalculation` reports zero expected liquidity. The
        // skipped-allocator path would have produced the pool's live liquidity
        // and the whole budget as the reward.
        assert_eq!(
            solved.pool_updates[0].rewards_update,
            RewardsUpdate::CurrentOnly { amount: 0, expected_liquidity: 0 }
        );
    }

    /// Each pool's gross is split on its own. A single split of the aggregate
    /// rounds differently, so the two answers disagree by a unit here.
    #[test]
    fn two_pools_sharing_token0() {
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();
        let snap = pool(1_000_000_000_000_000);
        let splits = DonationSplits::new(1_000_000, 750_000).unwrap();

        let (searcher_a, gross_a) = tob_with_gross(&snap, pool_id(1), 1_001);
        let (searcher_b, gross_b) = tob_with_gross(&snap, pool_id(2), 2_002);

        let (lp_a, fee_a) = splits.split_tob(gross_a);
        let (lp_b, fee_b) = splits.split_tob(gross_b);
        let per_pool = fee_a + fee_b;
        let aggregated = splits.split_tob(gross_a + gross_b).1;
        assert_ne!(
            per_pool, aggregated,
            "the grosses must be chosen so per-pool and aggregated splits differ"
        );

        let solved = solve(
            &[
                (
                    PoolSolution {
                        id: pool_id(1),
                        ucp: Ray::ZERO,
                        searcher: Some(searcher_a),
                        ..Default::default()
                    },
                    snap.clone(),
                    T0,
                    T1
                ),
                (
                    PoolSolution {
                        id: pool_id(2),
                        ucp: Ray::ZERO,
                        searcher: Some(searcher_b),
                        ..Default::default()
                    },
                    snap.clone(),
                    T0,
                    T1_B
                )
            ],
            &[],
            splits
        )
        .unwrap();

        // Each pool rewarded its own LP budget...
        assert_eq!(solved.rewarded(0), lp_a);
        assert_eq!(solved.rewarded(1), lp_b);
        // ...and the shared T0 `save` is the sum of the two fees, not one split
        // of the combined gross.
        assert_eq!(solved.save(T0), per_pool);
        assert_ne!(solved.save(T0), aggregated);
    }
}
