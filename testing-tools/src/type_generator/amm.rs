use angstrom_types::matching::{
    uniswap::{LiqRange, PoolSnapshot},
    SqrtPriceX96
};
use eyre::{eyre, Context, Error};
use rand_distr::{Distribution, SkewNormal};
use uniswap_v3_math::tick_math::{get_sqrt_ratio_at_tick, get_tick_at_sqrt_ratio};

#[derive(Debug, Default)]
pub struct AMMSnapshotBuilder {
    price: SqrtPriceX96,
    lower_tick: i32,
    upper_tick: i32,
    default_position_width: Option<i32>,
    default_position_liquidity: Option<u128>,
    liquidity_distribution: Option<LiquidityDistributionParameters>,
    positions: Option<Vec<LiqRange>>
}

impl AMMSnapshotBuilder {
    pub fn new(price: SqrtPriceX96) -> Self {
        let price_tick = get_tick_at_sqrt_ratio(price.into()).unwrap();
        let lower_tick = price_tick;
        let upper_tick = price_tick + 1;
        Self { price, lower_tick, upper_tick, ..Self::default() }
    }

    pub fn with_positions(self, positions: Vec<LiqRange>) -> Self {
        let (lower_tick, upper_tick) =
            positions
                .iter()
                .fold((i32::MAX, i32::MIN), |mut state, pos| {
                    if state.0 > pos.lower_tick() {
                        state.0 = pos.lower_tick();
                    }
                    if state.1 < pos.upper_tick() {
                        state.1 = pos.upper_tick();
                    }
                    state
                });
        Self { lower_tick, upper_tick, positions: Some(positions), ..self }
    }

    pub fn with_positions_from_distribution(
        self,
        liquidity_distribution: LiquidityDistributionParameters
    ) -> Self {
        Self { liquidity_distribution: Some(liquidity_distribution), ..self }
    }

    pub fn with_single_position(self, width: i32, liquidity: u128) -> Self {
        Self {
            default_position_width: Some(width),
            default_position_liquidity: Some(liquidity),
            ..self
        }
    }

    pub fn build(self) -> PoolSnapshot {
        // If you've givien me explicit positions
        let ranges = if let Some(positions) = self.positions {
            positions
        } else if let Some(liquidity_distribution) = self.liquidity_distribution {
            generate_pool_distribution(self.lower_tick, self.upper_tick, liquidity_distribution)
                .unwrap()
        } else {
            let width = self.default_position_width.unwrap_or(1);
            let lower_tick = self.lower_tick.saturating_sub(width);
            let upper_tick = self.upper_tick.saturating_add(width);
            let liquidity = self.default_position_liquidity.unwrap_or(1);
            vec![LiqRange::new(lower_tick, upper_tick, liquidity).unwrap()]
        };
        PoolSnapshot::new(ranges, self.price).unwrap()
    }
}

pub fn generate_single_position_amm_at_tick(mid: i32, width: i32, liquidity: u128) -> PoolSnapshot {
    let amm_price = SqrtPriceX96::from(get_sqrt_ratio_at_tick(mid + 1).unwrap());
    let lower_tick = mid - width;
    let upper_tick = mid + width;
    let ranges = vec![LiqRange::new(lower_tick, upper_tick, liquidity).unwrap()];
    PoolSnapshot::new(ranges, amm_price).unwrap()
}

pub fn generate_amm_market(target_tick: i32) -> PoolSnapshot {
    let range = LiqRange::new(target_tick - 100, target_tick + 100, 100_000_000).unwrap();
    let ranges = vec![range];
    let sqrt_price_x96 = SqrtPriceX96::from(get_sqrt_ratio_at_tick(target_tick).unwrap());
    PoolSnapshot::new(ranges, sqrt_price_x96).unwrap()
}

#[derive(Debug, Default)]
pub struct LiquidityDistributionParameters {
    pub liquidity: u128,
    pub scale:     f64,
    pub shape:     f64
}

fn generate_pool_distribution(
    start_tick: i32,
    end_tick: i32,
    liquidity: LiquidityDistributionParameters
) -> Result<Vec<LiqRange>, Error> {
    if end_tick < start_tick {
        return Err(eyre!("End tick greater than start tick, invalid"))
    }
    let tick_count = end_tick - start_tick;
    let LiquidityDistributionParameters {
        liquidity: liq_location,
        scale: liq_scale,
        shape: liq_shape
    } = liquidity;
    let liquidity_gen = SkewNormal::new(liq_location as f64, liq_scale, liq_shape)
        .wrap_err("Error creating liquidity distribution")?;
    let mut rng = rand::thread_rng();
    let liq_values: Vec<u128> = liquidity_gen
        .sample_iter(&mut rng)
        .take(tick_count as usize)
        .map(|item| item as u128)
        .collect();
    (0..tick_count)
        .zip(liq_values)
        .map(|(count, l)| LiqRange::new(start_tick + count, start_tick + count + 1, l))
        .collect()
}
