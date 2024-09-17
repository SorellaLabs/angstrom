use alloy_primitives::{Address, FixedBytes, Uint, U256};
use angstrom_types::{
    matching::Ray,
    orders::{OrderId, OrderPriorityData},
    sol_bindings::{
        ext::RawPoolOrder,
        grouped_orders::{
            FlashVariants, GroupedVanillaOrder, OrderWithStorageData, StandingVariants
        },
        rpc_orders::{ExactFlashOrder, ExactStandingOrder, TopOfBlockOrder},
        sol::{FlashOrder, StandingOrder}
    }
};
use rand::{rngs::ThreadRng, Rng};
use rand_distr::{num_traits::ToPrimitive, Distribution, SkewNormal};

// fn build_priority_data(order: &GroupedVanillaOrder) -> OrderPriorityData {
//     OrderPriorityData { price: order.price().into(), volume: order.quantity()
// as u128, gas: 10 } }

fn generate_order_id(pool_id: usize, hash: FixedBytes<32>) -> OrderId {
    let address = Address::random();
    OrderId {
        address,
        pool_id,
        hash,
        flash_block: None,
        location: Default::default(),
        deadline: None,
        reuse_avoidance: angstrom_types::sol_bindings::RespendAvoidanceMethod::Block(0)
    }
}

pub fn generate_limit_order(
    rng: &mut ThreadRng,
    kof: bool,
    is_bid: bool,
    pool_id: Option<usize>,
    valid_block: Option<u64>,
    asset_in: Option<Address>,
    asset_out: Option<Address>,
    nonce: Option<u64>,
    from: Option<Address>
) -> OrderWithStorageData<GroupedVanillaOrder> {
    let pool_id = pool_id.unwrap_or_default();
    let valid_block = valid_block.unwrap_or_default();
    // Could update this to be within a distribution
    let price: u128 = rng.gen();
    let volume: u128 = rng.gen();
    let gas: u128 = rng.gen();
    let order = build_limit_order(
        kof,
        valid_block,
        volume,
        price,
        asset_in.unwrap_or_else(|| rng.gen()),
        asset_out.unwrap_or_else(|| rng.gen()),
        nonce.unwrap_or_else(|| rng.gen()),
        from.unwrap_or_else(|| rng.gen())
    );

    let priority_data = OrderPriorityData { price: U256::from(price), volume, gas };
    let order_id = generate_order_id(pool_id, order.hash());
    // Todo: Sign It, make this overall better
    OrderWithStorageData {
        invalidates: vec![],
        order,
        priority_data,
        is_bid,
        is_currently_valid: true,
        is_valid: true,
        order_id,
        pool_id,
        valid_block
    }
}

pub fn generate_top_of_block_order(
    rng: &mut ThreadRng,
    is_bid: bool,
    pool_id: Option<usize>,
    valid_block: Option<u64>
) -> OrderWithStorageData<TopOfBlockOrder> {
    let pool_id = pool_id.unwrap_or_default();
    let valid_block = valid_block.unwrap_or_default();
    // Could update this to be within a distribution
    let price: u128 = rng.gen();
    let volume: u128 = rng.gen();
    let gas: u128 = rng.gen();
    let order = build_top_of_block_order();

    let priority_data = OrderPriorityData { price: U256::from(price), volume, gas };
    let order_id = generate_order_id(pool_id, order.order_hash());
    // Todo: Sign It, make this overall better
    OrderWithStorageData {
        invalidates: vec![],
        order,
        priority_data,
        is_bid,
        is_currently_valid: true,
        is_valid: true,
        order_id,
        pool_id,
        valid_block
    }
}

pub fn build_limit_order(
    kof: bool,
    valid_block: u64,
    volume: u128,
    price: u128,
    asset_in: Address,
    asset_out: Address,
    nonce: u64,
    from: Address
) -> GroupedVanillaOrder {
    if kof {
        GroupedVanillaOrder::KillOrFill(FlashVariants::Exact(ExactFlashOrder {
            amount: volume,
            minPrice: Ray::from(Uint::from(price)).into(),
            validForBlock: valid_block,
            assetIn: asset_in,
            assetOut: asset_out,
            recipient: from,
            ..Default::default()
        }))
    } else {
        GroupedVanillaOrder::Partial(StandingVariants::Exact(ExactStandingOrder {
            amount: volume,
            minPrice: Ray::from(Uint::from(price)).into(),
            assetOut: asset_out,
            assetIn: asset_in,
            nonce,
            recipient: from,
            ..Default::default()
        }))
    }
}

pub fn build_top_of_block_order() -> TopOfBlockOrder {
    TopOfBlockOrder::default()
}

#[derive(Debug, Default)]
pub struct DistributionParameters {
    pub location: f64,
    pub scale:    f64,
    pub shape:    f64
}

impl DistributionParameters {
    pub fn crossed_at(location: f64) -> (Self, Self) {
        let bids = Self { location, scale: 100000.0, shape: -2.0 };
        let asks = Self { location, scale: 100000.0, shape: 2.0 };

        (bids, asks)
    }

    pub fn fixed_at(location: f64) -> (Self, Self) {
        let bids = Self { location, scale: 1.0, shape: 0.0 };
        let asks = Self { location, scale: 1.0, shape: 0.0 };

        (bids, asks)
    }
}

#[rustfmt::skip]
pub fn generate_order_distribution(
    is_bid: bool,
    order_count: usize,
    priceparams: DistributionParameters,
    volumeparams: DistributionParameters,
    pool_id: usize,
    valid_block: u64
) -> Result<Vec<OrderWithStorageData<GroupedVanillaOrder>>, String> {
    let DistributionParameters { location: price_location, scale: price_scale, shape: price_shape } =
        priceparams;
    let DistributionParameters { location: v_location, scale: v_scale, shape: v_shape } =
        volumeparams;
    let mut rng = rand::thread_rng();
    let mut rng2 = rand::thread_rng();
    let price_gen = SkewNormal::new(price_location, price_scale, price_shape)
        .map_err(|e| format!("Error creating price distribution: {}", e))?;
    let volume_gen = SkewNormal::new(v_location, v_scale, v_shape)
        .map_err(|e| format!("Error creating price distribution: {}", e))?;
    Ok(price_gen
        .sample_iter(&mut rng)
        .zip(volume_gen.sample_iter(&mut rng2))
        .map(|(p, v)| {
            let price = p.to_u128().unwrap_or_default();
            let volume = v.to_u128().unwrap_or_default();
            let order = build_limit_order(true, valid_block, volume, price, Address::ZERO,Address::ZERO, 0,Address::ZERO);
            let order_id = generate_order_id(pool_id, order.hash());

            OrderWithStorageData {
                invalidates: vec![],
                order,
                priority_data: OrderPriorityData { price: U256::from(price), volume, gas: 10 },
                is_bid,
                is_valid: true,
                is_currently_valid: true,
                order_id,
                pool_id,
                valid_block
            }
        })
        .take(order_count)
        .collect())
}
