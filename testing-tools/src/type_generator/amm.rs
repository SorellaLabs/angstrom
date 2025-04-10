use angstrom_types::matching::{
    SqrtPriceX96,
    uniswap::{LiqRange, PoolSnapshot}
};
use eyre::{Context, Error, eyre};
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
            vec![LiqRange::new_init(lower_tick, upper_tick, liquidity, 0).unwrap()]
        };
        PoolSnapshot::new(1, ranges, self.price, 0).unwrap()
    }
}

pub fn generate_amm_with_liquidity(
    tick_low: i32,
    tick_high: i32,
    liquidity: u128,
    price: SqrtPriceX96
) -> PoolSnapshot {
    let ranges = vec![LiqRange::new_init(tick_low, tick_high, liquidity, 0).unwrap()];
    PoolSnapshot::new(1, ranges, price, 0).unwrap()
}

/// Generates an AMM with multiple liquidity positions across a tick range
///
/// # Arguments
/// * `tick_low` - Lower tick boundary
/// * `tick_high` - Upper tick boundary
/// * `tick_spacing` - Space between each tick range
/// * `liquidity_range` - (min, max) range for random liquidity generation
/// * `price` - Current price as SqrtPriceX96
///
/// # Returns
/// * `PoolSnapshot` with multiple liquidity ranges
pub fn generate_amm_with_distributed_liquidity(
    tick_low: i32,
    tick_high: i32,
    tick_spacing: i32,
    liquidity_range: (u128, u128),
    price: SqrtPriceX96
) -> PoolSnapshot {
    use rand::Rng;
    let mut rng = rand::thread_rng();

    let mut ranges = Vec::new();
    let (min_liq, max_liq) = liquidity_range;

    // Ensure tick_low and tick_high are aligned with tick_spacing
    let aligned_low = (tick_low / tick_spacing) * tick_spacing;
    let aligned_high = ((tick_high + tick_spacing - 1) / tick_spacing) * tick_spacing;

    let mut current_tick = aligned_low;
    while current_tick < aligned_high {
        // Generate random liquidity amount within specified range
        let liquidity = rng.gen_range(min_liq..=max_liq);

        // Create range from current_tick to current_tick + tick_spacing
        if let Ok(range) =
            LiqRange::new_init(current_tick, current_tick + tick_spacing, liquidity, 0)
        {
            ranges.push(range);
        }

        current_tick += tick_spacing;
    }

    PoolSnapshot::new(tick_spacing, ranges, price, 0).unwrap()
}

pub fn generate_single_position_amm_at_tick(mid: i32, width: i32, liquidity: u128) -> PoolSnapshot {
    let amm_price = SqrtPriceX96::from(get_sqrt_ratio_at_tick(mid + 1).unwrap());
    let lower_tick = mid - width;
    let upper_tick = mid + width;
    let ranges = vec![LiqRange::new_init(lower_tick, upper_tick, liquidity, 0).unwrap()];
    PoolSnapshot::new(1, ranges, amm_price, 0).unwrap()
}

pub fn generate_amm_market(target_tick: i32) -> PoolSnapshot {
    let range = LiqRange::new_init(target_tick - 100, target_tick + 100, 100_000_000, 0).unwrap();
    let ranges = vec![range];
    let sqrt_price_x96 = SqrtPriceX96::from(get_sqrt_ratio_at_tick(target_tick).unwrap());
    PoolSnapshot::new(1, ranges, sqrt_price_x96, 0).unwrap()
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
        return Err(eyre!("End tick greater than start tick, invalid"));
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
        .map(|(count, l)| LiqRange::new_init(start_tick + count, start_tick + count + 1, l, 0))
        .collect()
}
