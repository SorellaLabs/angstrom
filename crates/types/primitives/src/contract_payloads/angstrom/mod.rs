use std::{collections::HashMap, hash::Hash, ops::Deref, sync::Arc};

use alloy_eips::BlockId;
use alloy_network::Network;
use alloy_primitives::{Address, B256, I256, U256, keccak256};
use alloy_provider::Provider;
use alloy_sol_types::SolValue;
use base64::{Engine, prelude::BASE64_STANDARD};
use dashmap::DashMap;
use itertools::Itertools;
use pade_macro::{PadeDecode, PadeEncode};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{Asset, CONFIG_STORE_SLOT, POOL_CONFIG_STORE_ENTRY_SIZE, Pair, rewards::PoolUpdate};
use crate::{
    contract_bindings::angstrom::Angstrom::PoolKey,
    primitive::{PoolId, Ray, UniswapPoolRegistry}
};

mod order;
mod tob;
pub use order::{OrderQuantities, StandingValidation, UserOrder};
pub use tob::*;

pub const LP_DONATION_SPLIT: f64 = 0.75;
// We set high for ticks
const BASE_GAS_FOR_POOL: usize = 350_000;
const BASE_EST_FOR_USER: usize = 90_000;

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

    pub fn crude_gas_estimation(&self) -> u64 {
        let pool_swap_cnt = self
            .pool_updates
            .iter()
            .filter(|update| update.swap_in_quantity != 0)
            .unique_by(|f| f.pair_index)
            .count();

        ((self.top_of_block_orders.len() + self.user_orders.len()) * BASE_EST_FOR_USER) as u64
            + (pool_swap_cnt * BASE_GAS_FOR_POOL) as u64
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
}

#[allow(unused)]
#[derive(Debug, Clone, Default)]
pub struct BundleGasDetails {
    /// total gas to execute the bundle on angstrom
    total_gas_cost_wei: u64
}

impl BundleGasDetails {
    pub fn new(total_gas_cost_wei: u64) -> Self {
        Self { total_gas_cost_wei }
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

#[derive(Debug, Hash, Eq, PartialEq, Copy, Clone, Serialize, Deserialize)]
pub struct AngstromPoolPartialKey([u8; 27]);

impl AngstromPoolPartialKey {
    pub fn new(key: [u8; 27]) -> Self {
        Self(key)
    }
}

impl Deref for AngstromPoolPartialKey {
    type Target = [u8; 27];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Display for AngstromPoolPartialKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl std::str::FromStr for AngstromPoolPartialKey {
    type Err = base64::DecodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = BASE64_STANDARD.decode(s)?;
        if bytes.len() != 27 {
            return Err(base64::DecodeError::InvalidLength(bytes.len()));
        }
        let mut arr = [0u8; 27];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct AngPoolConfigEntry {
    pub pool_partial_key: AngstromPoolPartialKey,
    pub tick_spacing:     u16,
    pub fee_in_e6:        u32,
    pub store_index:      usize
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AngstromPoolConfigStore {
    #[serde(
        serialize_with = "serialize_dashmap_with_display_keys",
        deserialize_with = "deserialize_dashmap_with_display_keys"
    )]
    entries: DashMap<AngstromPoolPartialKey, AngPoolConfigEntry>
}

impl From<DashMap<AngstromPoolPartialKey, AngPoolConfigEntry>> for AngstromPoolConfigStore {
    fn from(value: DashMap<AngstromPoolPartialKey, AngPoolConfigEntry>) -> Self {
        Self { entries: value }
    }
}

fn serialize_dashmap_with_display_keys<S>(
    dashmap: &DashMap<AngstromPoolPartialKey, AngPoolConfigEntry>,
    serializer: S
) -> Result<S::Ok, S::Error>
where
    S: Serializer
{
    let hashmap: HashMap<String, AngPoolConfigEntry> = dashmap
        .iter()
        .map(|entry| (entry.key().to_string(), *entry.value()))
        .collect();
    hashmap.serialize(serializer)
}

fn deserialize_dashmap_with_display_keys<'de, D>(
    deserializer: D
) -> Result<DashMap<AngstromPoolPartialKey, AngPoolConfigEntry>, D::Error>
where
    D: Deserializer<'de>
{
    let hashmap: HashMap<String, AngPoolConfigEntry> = HashMap::deserialize(deserializer)?;
    let dashmap = DashMap::new();
    for (key_str, value) in hashmap {
        let key = key_str
            .parse::<AngstromPoolPartialKey>()
            .map_err(serde::de::Error::custom)?;
        dashmap.insert(key, value);
    }
    Ok(dashmap)
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
            .map_err(|e| format!("Error getting storage: {e}"))?;

        let value_bytes: [u8; 32] = value.to_be_bytes();
        tracing::debug!("storage slot of poolkey storage {:?}", value_bytes);
        let config_store_address =
            Address::from(<[u8; 20]>::try_from(&value_bytes[4..24]).unwrap());
        tracing::info!(?config_store_address);

        let code = provider
            .get_code_at(config_store_address)
            .block_id(block_id)
            .await
            .map_err(|e| format!("Error getting code: {e}"))?;

        tracing::info!(len=?code.len(), "bytecode: {:x}", code);

        AngstromPoolConfigStore::try_from(code.as_ref())
            .map_err(|e| format!("Failed to deserialize code into AngstromPoolConfigStore: {e}"))
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
        if !adjusted_entries
            .len()
            .is_multiple_of(POOL_CONFIG_STORE_ENTRY_SIZE)
        {
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
