mod assetarray;
mod builder;
mod state;

use angstrom_types::{
    consensus::Proposal,
    contract_payloads::{
        angstrom::{AngstromBundle, OrderQuantities, TopOfBlockOrder, UserOrder},
        rewards::PoolUpdate
    },
    matching::Ray,
    orders::{OrderFillState, OrderOutcome},
    sol_bindings::grouped_orders::{GroupedVanillaOrder, OrderWithStorageData}
};
use builder::{AssetBuilder, AssetBuilderStage};
use matching_engine::{cfmm::uniswap::tob::ToBOutcome, MatchingManager};
use reth_primitives::U256;
use tracing::warn;
use validation::order::state::pools::AngstromPoolsTracker;

// Note that .to() on Uint is a try_into with an unwrap, so we should be careful
// about safety here

pub fn to_contract_format(
    proposal: &Proposal,
    pools: &AngstromPoolsTracker
) -> Result<AngstromBundle, eyre::Error> {
    let mut top_of_block_orders = Vec::new();
    let mut pool_updates = Vec::new();
    let pairs = Vec::new();
    let mut user_orders = Vec::new();
    let mut asset_builder = AssetBuilder::new();

    // Break out our input orders into lists of orders by pool
    let orders_by_pool = MatchingManager::orders_by_pool_id(&proposal.preproposals);

    // Walk through our solutions to add them to the structure
    for solution in proposal.solutions.iter() {
        // Get the information for the pool or skip this solution if we can't find a
        // pool for it
        let Some((t0, t1)) = pools.get_pool_addresses(solution.id) else {
            // This should never happen but let's handle it as gracefully as possible -
            // right now will skip the pool, not produce an error
            warn!("Skipped a solution as we couldn't find a pool for it: {:?}", solution);
            continue;
        };
        // Make sure the involved assets are in our assets array and we have the
        // appropriate asset index for them
        let t0_idx = asset_builder.add_or_get_asset(t0) as u16;
        let t1_idx = asset_builder.add_or_get_asset(t1) as u16;
        // Build our Pair featuring our uniform clearing price
        // This price is in Ray format as requested.
        let uniswap_price: U256 = *solution.ucp;
        // pairs.push(Pair { t0_idx, t1_idx, uniswap_price });
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
                // let amm = MarketSnapshot::new(vec![], SqrtPriceX96::default()).unwrap();
                //let rewards = calculate_reward(tob, amm).unwrap();
                let rewards = ToBOutcome::default();
                (Some(swap), Some(rewards))
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
        asset_builder.allocate(AssetBuilderStage::Reward, t0, tob_outcome.total_reward.to());
        let rewards_update = tob_outcome.to_rewards_update();
        // Push the pool update
        pool_updates.push(PoolUpdate {
            zero_for_one: false,
            pair_index: 0,
            swap_in_quantity: quantity_in,
            rewards_update
        });
        // Add the ToB order to our tob order list - This is currently converting
        // between two ToB order formats
        if let Some(tob) = solution.searcher.as_ref() {
            // Account for our ToB order
            let (asset_in, asset_out) = if tob.is_bid { (t1, t0) } else { (t0, t1) };
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
            let ray_price = Ray::from(uniswap_price);
            let quantity_in = if order.is_bid {
                ray_price.mul_quantity(quantity_out)
            } else {
                ray_price.inverse_quantity(quantity_out)
            };
            // Account for our user order
            let (asset_in, asset_out) = if order.is_bid { (t1, t0) } else { (t0, t1) };
            asset_builder.external_swap(
                AssetBuilderStage::UserOrder,
                asset_in,
                asset_out,
                quantity_in.to(),
                quantity_out.to()
            );
            user_orders.push(to_contract_order(order, outcome, pair_idx as u16));
        }
    }
    Ok(AngstromBundle::new(
        asset_builder.get_asset_array(),
        pairs,
        pool_updates,
        top_of_block_orders,
        user_orders
    ))
}

pub fn to_contract_order(
    order: &OrderWithStorageData<GroupedVanillaOrder>,
    outcome: &OrderOutcome,
    pair_index: u16
) -> UserOrder {
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
    UserOrder {
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

#[cfg(test)]
mod tests {
    use alloy_primitives::FixedBytes;
    use angstrom_types::primitive::PoolId;
    use reth_primitives::Address;
    use testing_tools::type_generator::consensus::proposal::ProposalBuilder;
    use validation::order::state::{
        config::{PoolConfig, ValidationConfig},
        pools::AngstromPoolsTracker
    };

    use super::to_contract_format;

    #[test]
    fn basic_test() {
        let token0 = Address::random();
        let token1 = Address::random();
        let pool_id: PoolId = FixedBytes::random();
        let poolconfig = PoolConfig { token0, token1, pool_id };
        let mut config = ValidationConfig::default();
        config.pools = vec![poolconfig.clone()];
        let proposal_pools = vec![poolconfig.pool_id];
        let proposal = ProposalBuilder::new()
            .order_count(10)
            .for_pools(proposal_pools)
            .build();
        let pools = AngstromPoolsTracker::new(config);
        println!("{:?}", to_contract_format(&proposal, &pools));
        panic!("Butts!");
    }
}
