use alloy::primitives::Uint;
use angstrom_types::{
    matching::Ray,
    primitive::{AngstromSigner, PoolId},
    sol_bindings::grouped_orders::{GroupedVanillaOrder, OrderWithStorageData}
};
use eyre::eyre;
use rand_distr::{num_traits::ToPrimitive, Distribution, SkewNormal};

use super::{DistributionParameters, UserOrderBuilder};

#[derive(Default)]
pub struct OrderDistributionBuilder {
    is_bid:       bool,
    order_count:  Option<usize>,
    priceparams:  Option<DistributionParameters>,
    volumeparams: Option<DistributionParameters>,
    pool_id:      Option<PoolId>,
    valid_block:  Option<u64>,
    signing_key:  Option<AngstromSigner>
}

impl OrderDistributionBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bid(self) -> Self {
        Self { is_bid: true, ..self }
    }

    pub fn ask(self) -> Self {
        Self { is_bid: false, ..self }
    }

    pub fn is_bid(self, is_bid: bool) -> Self {
        Self { is_bid, ..self }
    }

    pub fn order_count(self, order_count: usize) -> Self {
        Self { order_count: Some(order_count), ..self }
    }

    pub fn price_params(self, price_params: DistributionParameters) -> Self {
        Self { priceparams: Some(price_params), ..self }
    }

    pub fn volume_params(self, volume_params: DistributionParameters) -> Self {
        Self { volumeparams: Some(volume_params), ..self }
    }

    pub fn pool_id(self, pool_id: PoolId) -> Self {
        Self { pool_id: Some(pool_id), ..self }
    }

    pub fn valid_block(self, valid_block: u64) -> Self {
        Self { valid_block: Some(valid_block), ..self }
    }

    pub fn signing_key(self, signing_key: Option<AngstromSigner>) -> Self {
        Self { signing_key, ..self }
    }

    pub fn build(self) -> eyre::Result<Vec<OrderWithStorageData<GroupedVanillaOrder>>> {
        let order_count = self.order_count.unwrap_or_default();
        let pool_id = self.pool_id.unwrap_or_default();
        let valid_block = self.valid_block.unwrap_or_default();
        let DistributionParameters {
            location: price_location,
            scale: price_scale,
            shape: price_shape
        } = self.priceparams.unwrap_or_default();
        let DistributionParameters { location: v_location, scale: v_scale, shape: v_shape } =
            self.volumeparams.unwrap_or_default();

        // We need two RNG handles because we hand them out as a mutable
        let mut rng = rand::thread_rng();
        let mut rng2 = rand::thread_rng();

        let price_gen = SkewNormal::new(price_location, price_scale, price_shape)
            .map_err(|e| eyre!("Error creating price distribution: {}", e))?;
        let volume_gen = SkewNormal::new(v_location, v_scale, v_shape)
            .map_err(|e| eyre!("Error creating price distribution: {}", e))?;
        Ok(price_gen
            .sample_iter(&mut rng)
            .zip(volume_gen.sample_iter(&mut rng2))
            .map(|(p, v)| {
                UserOrderBuilder::new()
                    .is_standing(false)
                    .block(valid_block)
                    .amount(v.to_u128().unwrap_or_default())
                    .min_price(Ray::from(Uint::from(p.to_u128().unwrap_or_default())))
                    .signing_key(self.signing_key.clone())
                    .with_storage()
                    .pool_id(pool_id)
                    .is_bid(self.is_bid)
                    .build()
            })
            .take(order_count)
            .collect())
    }
}
