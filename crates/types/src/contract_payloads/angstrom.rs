use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    ops::Deref,
    sync::Arc
};

use alloy::{
    eips::BlockId,
    network::Network,
    primitives::{keccak256, Address, Bytes, FixedBytes, B256, U256},
    providers::Provider,
    sol_types::SolValue,
    transports::Transport
};
use alloy_primitives::{aliases::U40, I256};
use dashmap::DashMap;
use pade::PadeDecode;
use pade_macro::{PadeDecode, PadeEncode};
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::{
    asset::builder::{AssetBuilder, AssetBuilderStage},
    rewards::PoolUpdate,
    tob::ToBOutcome,
    Asset, Pair, Signature, CONFIG_STORE_SLOT, POOL_CONFIG_STORE_ENTRY_SIZE
};
use crate::{
    consensus::{PreProposal, Proposal},
    contract_bindings::angstrom::Angstrom::PoolKey,
    matching::{uniswap::PoolSnapshot, Ray},
    orders::{OrderFillState, OrderOutcome, PoolSolution},
    primitive::{PoolId, UniswapPoolRegistry},
    sol_bindings::{
        grouped_orders::{
            FlashVariants, GroupedVanillaOrder, OrderWithStorageData, StandingVariants
        },
        rpc_orders::{
            ExactFlashOrder, ExactStandingOrder, PartialFlashOrder, PartialStandingOrder,
            TopOfBlockOrder as RpcTopOfBlockOrder
        },
        RawPoolOrder
    },
    testnet::TestnetStateOverrides
};

// This currently exists in types::sol_bindings as well, but that one is
// outdated so I'm building a new one here for now and then migrating
#[derive(
    PadeEncode, PadeDecode, Clone, Default, Debug, Hash, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct TopOfBlockOrder {
    pub use_internal:     bool,
    pub quantity_in:      u128,
    pub quantity_out:     u128,
    pub max_gas_asset_0:  u128,
    pub gas_used_asset_0: u128,
    pub pairs_index:      u16,
    pub zero_for_1:       bool,
    pub recipient:        Option<Address>,
    pub signature:        Signature
}

impl TopOfBlockOrder {
    // eip-712 hash_struct. is a pain since we need to reconstruct values.
    pub fn order_hash(&self, pair: &[Pair], asset: &[Asset], block: u64) -> B256 {
        let pair = &pair[self.pairs_index as usize];
        RpcTopOfBlockOrder {
            quantity_in:     self.quantity_in,
            recipient:       self.recipient.unwrap_or_default(),
            quantity_out:    self.quantity_out,
            asset_in:        if self.zero_for_1 {
                asset[pair.index0 as usize].addr
            } else {
                asset[pair.index1 as usize].addr
            },
            asset_out:       if !self.zero_for_1 {
                asset[pair.index0 as usize].addr
            } else {
                asset[pair.index1 as usize].addr
            },
            use_internal:    self.use_internal,
            max_gas_asset0:  self.max_gas_asset_0,
            valid_for_block: block,
            meta:            Default::default()
        }
        .order_hash()
    }

    pub fn of_max_gas(
        internal: &OrderWithStorageData<RpcTopOfBlockOrder>,
        pairs_index: u16
    ) -> Self {
        let quantity_in = internal.quantity_in;
        let quantity_out = internal.quantity_out;
        let recipient = Some(internal.recipient);
        // Zero_for_1 is an Ask, an Ask is NOT a bid
        let zero_for_1 = !internal.is_bid;
        let sig_bytes = internal.meta.signature.to_vec();
        let decoded_signature =
            alloy::primitives::PrimitiveSignature::pade_decode(&mut sig_bytes.as_slice(), None)
                .unwrap();
        let signature = Signature::from(decoded_signature);
        Self {
            use_internal: false,
            quantity_in,
            quantity_out,
            max_gas_asset_0: internal.max_gas_asset0,
            // set as max so we can use the sim to verify values.
            gas_used_asset_0: internal.max_gas_asset0,
            pairs_index,
            zero_for_1,
            recipient,
            signature
        }
    }

    pub fn of(
        internal: &OrderWithStorageData<RpcTopOfBlockOrder>,
        shared_gas: U256,
        pairs_index: u16
    ) -> eyre::Result<Self> {
        let quantity_in = internal.quantity_in;
        let quantity_out = internal.quantity_out;
        let recipient = Some(internal.recipient);
        // Zero_for_1 is an Ask, an Ask is NOT a bid
        let zero_for_1 = !internal.is_bid;
        let sig_bytes = internal.meta.signature.to_vec();
        let decoded_signature =
            alloy::primitives::PrimitiveSignature::pade_decode(&mut sig_bytes.as_slice(), None)
                .unwrap();
        let signature = Signature::from(decoded_signature);
        let used_gas: u128 = (internal.priority_data.gas + shared_gas).to();

        if used_gas > internal.max_gas_asset0 {
            return Err(eyre::eyre!("order went over gas limit"))
        }

        Ok(Self {
            use_internal: false,
            quantity_in,
            quantity_out,
            max_gas_asset_0: internal.max_gas_asset0,
            // set as max so we can use the sim to verify values.
            gas_used_asset_0: used_gas,
            pairs_index,
            zero_for_1,
            recipient,
            signature
        })
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

impl OrderQuantities {
    pub fn fetch_max_amount(&self) -> u128 {
        match self {
            Self::Exact { quantity } => *quantity,
            Self::Partial { max_quantity_in, .. } => *max_quantity_in
        }
    }
}

#[derive(Debug, PadeEncode, PadeDecode)]
pub struct UserOrder {
    pub ref_id:               u32,
    pub use_internal:         bool,
    pub pair_index:           u16,
    pub min_price:            alloy::primitives::U256,
    pub recipient:            Option<Address>,
    pub hook_data:            Option<Bytes>,
    pub zero_for_one:         bool,
    pub standing_validation:  Option<StandingValidation>,
    pub order_quantities:     OrderQuantities,
    pub max_extra_fee_asset0: u128,
    pub extra_fee_asset0:     u128,
    pub exact_in:             bool,
    pub signature:            Signature
}

impl UserOrder {
    pub fn order_hash(&self, pair: &[Pair], asset: &[Asset], block: u64) -> B256 {
        let pair = &pair[self.pair_index as usize];
        match self.order_quantities {
            OrderQuantities::Exact { quantity } => {
                if let Some(validation) = &self.standing_validation {
                    // exact standing
                    ExactStandingOrder {
                        ref_id: self.ref_id,
                        exact_in: true,
                        use_internal: self.use_internal,
                        asset_in: if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out: if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient: self.recipient.unwrap_or_default(),
                        nonce: validation.nonce,
                        deadline: U40::from_limbs([validation.deadline]),
                        amount: quantity,
                        min_price: self.min_price,
                        hook_data: self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        ..Default::default()
                    }
                    .order_hash()
                } else {
                    // exact flash
                    ExactFlashOrder {
                        ref_id: self.ref_id,
                        exact_in: true,
                        use_internal: self.use_internal,
                        asset_in: if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out: if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient: self.recipient.unwrap_or_default(),
                        valid_for_block: block,
                        amount: quantity,
                        min_price: self.min_price,
                        hook_data: self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        ..Default::default()
                    }
                    .order_hash()
                }
            }
            OrderQuantities::Partial { min_quantity_in, max_quantity_in, .. } => {
                if let Some(validation) = &self.standing_validation {
                    PartialStandingOrder {
                        ref_id: self.ref_id,
                        use_internal: self.use_internal,
                        asset_in: if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out: if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient: self.recipient.unwrap_or_default(),
                        deadline: U40::from_limbs([validation.deadline]),
                        nonce: validation.nonce,
                        min_amount_in: min_quantity_in,
                        max_amount_in: max_quantity_in,
                        min_price: self.min_price,
                        hook_data: self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        ..Default::default()
                    }
                    .order_hash()
                } else {
                    PartialFlashOrder {
                        ref_id: self.ref_id,
                        use_internal: self.use_internal,
                        asset_in: if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out: if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient: self.recipient.unwrap_or_default(),
                        valid_for_block: block,
                        max_amount_in: max_quantity_in,
                        min_amount_in: min_quantity_in,
                        min_price: self.min_price,
                        hook_data: self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        ..Default::default()
                    }
                    .order_hash()
                }
            }
        }
    }

    pub fn from_internal_order(
        order: &OrderWithStorageData<GroupedVanillaOrder>,
        outcome: &OrderOutcome,
        shared_gas: U256,
        pair_index: u16
    ) -> eyre::Result<Self> {
        let order_quantities = match order.order {
            GroupedVanillaOrder::KillOrFill(_) => {
                OrderQuantities::Exact { quantity: order.quantity() }
            }
            GroupedVanillaOrder::Standing(_) => {
                let max_quantity_in: u128 = order.quantity();
                let filled_quantity = match outcome.outcome {
                    OrderFillState::CompleteFill => max_quantity_in,
                    OrderFillState::PartialFill(fill) => fill,
                    _ => 0
                };
                OrderQuantities::Partial { min_quantity_in: 0, max_quantity_in, filled_quantity }
            }
        };
        let hook_data = match order.order {
            GroupedVanillaOrder::KillOrFill(ref o) => o.hook_data().clone(),
            GroupedVanillaOrder::Standing(ref o) => o.hook_data().clone()
        };
        let gas_used: u128 = (order.priority_data.gas + shared_gas).to();
        if gas_used > order.max_gas_token_0() {
            return Err(eyre::eyre!("order used more gas than allocated"))
        }

        let sig_bytes = order.signature().clone().0.to_vec();
        let decoded_signature =
            alloy::primitives::PrimitiveSignature::pade_decode(&mut sig_bytes.as_slice(), None)
                .unwrap();
        let signature = Signature::from(decoded_signature);

        Ok(Self {
            ref_id: 0,
            use_internal: false,
            pair_index,
            min_price: *order.price(),
            recipient: None,
            hook_data: Some(hook_data),
            zero_for_one: !order.is_bid,
            standing_validation: None,
            order_quantities,
            max_extra_fee_asset0: order.max_gas_token_0(),
            extra_fee_asset0: gas_used,
            exact_in: false,
            signature
        })
    }

    pub fn from_internal_order_max_gas(
        order: &OrderWithStorageData<GroupedVanillaOrder>,
        outcome: &OrderOutcome,
        pair_index: u16
    ) -> Self {
        let order_quantities = match &order.order {
            GroupedVanillaOrder::KillOrFill(o) => match o {
                FlashVariants::Exact(_) => OrderQuantities::Exact { quantity: order.quantity() },
                FlashVariants::Partial(p_o) => OrderQuantities::Partial {
                    min_quantity_in: p_o.min_amount_in,
                    max_quantity_in: p_o.max_amount_in,
                    filled_quantity: outcome.fill_amount(p_o.max_amount_in)
                }
            },
            GroupedVanillaOrder::Standing(o) => match o {
                StandingVariants::Exact(_) => OrderQuantities::Exact { quantity: order.quantity() },
                StandingVariants::Partial(p_o) => {
                    let max_quantity_in = p_o.max_amount_in;
                    let filled_quantity = outcome.fill_amount(p_o.max_amount_in);
                    OrderQuantities::Partial {
                        min_quantity_in: p_o.min_amount_in,
                        max_quantity_in,
                        filled_quantity
                    }
                }
            }
        };
        let hook_bytes = match order.order {
            GroupedVanillaOrder::KillOrFill(ref o) => o.hook_data().clone(),
            GroupedVanillaOrder::Standing(ref o) => o.hook_data().clone()
        };
        let hook_data = if hook_bytes.is_empty() { None } else { Some(hook_bytes) };
        let sig_bytes = order.signature().to_vec();
        let decoded_signature =
            alloy::primitives::PrimitiveSignature::pade_decode(&mut sig_bytes.as_slice(), None)
                .unwrap();
        Self {
            ref_id: 0,
            use_internal: false,
            pair_index,
            min_price: *order.price(),
            recipient: None,
            hook_data,
            zero_for_one: !order.is_bid,
            standing_validation: None,
            order_quantities,
            max_extra_fee_asset0: order.max_gas_token_0(),
            extra_fee_asset0: order.max_gas_token_0(),
            exact_in: false,
            signature: Signature::from(decoded_signature)
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

    #[cfg(feature = "testnet")]
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

        asset_builder.allocate(
            AssetBuilderStage::UserOrder,
            user_order.token_in(),
            user_order.amount_in()
        );
        asset_builder.allocate(AssetBuilderStage::UserOrder, user_order.token_out(), {
            let price = Ray::from(user_order.limit_price());
            price.mul_quantity(U256::from(user_order.amount_in())).to()
        });

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
                    price_1over0: U256::from(user_order.limit_price())
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
        assert!(gas_details.total_gas_cost_wei > total_gas);
        if total_swaps == 0 {
            return Err(eyre::eyre!("have a total swaps count of 0"));
        }
        let shared_gas_in_wei = (gas_details.total_gas_cost_wei - total_gas) / total_swaps;

        // fetch gas used
        // Walk through our solutions to add them to the structure
        for solution in proposal.solutions.iter() {
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
                let t0_moving = U256::from(outcome.fill_amount(order.quantity()));
                let t1_moving = Ray::from(ucp).mul_quantity(t0_moving);
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
    fn can_be_cretaed_from_proposal() {
        // AngstromBundle::from_proposal(proposal, pools);
    }
}
