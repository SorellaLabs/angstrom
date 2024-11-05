use alloy::primitives::{FixedBytes, U256};
use angstrom_types::{
    matching::Ray,
    orders::{OrderId, OrderPriorityData},
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::ExactFlashOrder
    }
};
use rand_distr::{Distribution, SkewNormal};

#[allow(clippy::too_many_arguments)]
pub fn order_distribution(
    is_bid: bool,
    number: usize,
    price_location: f64,
    price_scale: f64,
    price_shape: f64,
    quantity_location: f64,
    quantity_scale: f64,
    quantity_shape: f64
) -> Result<Vec<OrderWithStorageData<GroupedVanillaOrder>>, String> {
    let mut rng = rand::thread_rng();
    let mut rng2 = rand::thread_rng();
    let price_gen = SkewNormal::new(price_location, price_scale, price_shape)
        .map_err(|e| format!("Error creating price distribution: {}", e))?;
    let quantity_gen = SkewNormal::new(quantity_location, quantity_scale, quantity_shape)
        .map_err(|e| format!("Error creating price distribution: {}", e))?;
    Ok(price_gen
        .sample_iter(&mut rng)
        .zip(quantity_gen.sample_iter(&mut rng2))
        .map(|(p, q)| {
            let order = GroupedVanillaOrder::KillOrFill(
                angstrom_types::sol_bindings::grouped_orders::FlashVariants::Exact(
                    ExactFlashOrder {
                        amount: q.floor() as u128,
                        min_price: Ray::from(p).into(),
                        ..Default::default()
                    }
                )
            );
            OrderWithStorageData {
                invalidates: vec![],
                order,
                priority_data: OrderPriorityData {
                    price:     U256::from(p as u128),
                    volume:    q as u128,
                    gas:       U256::ZERO,
                    gas_units: 0
                },
                is_bid,
                is_valid: true,
                is_currently_valid: true,
                order_id: OrderId {
                    flash_block:     None,
                    reuse_avoidance: angstrom_types::sol_bindings::RespendAvoidanceMethod::Block(0),
                    hash:            Default::default(),
                    address:         Default::default(),
                    deadline:        None,
                    pool_id:         FixedBytes::default(),
                    location:        angstrom_types::orders::OrderLocation::Limit
                },
                pool_id: FixedBytes::default(),
                valid_block: 0,
                tob_reward: U256::ZERO
            }
        })
        .take(number)
        .collect())
}
