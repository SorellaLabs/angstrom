use std::collections::HashMap;

use alloy::primitives::{Address, U256};

#[derive(Debug, Clone)]
pub struct BundleResponse {
    /// a map (sorted tokens) of how much of token0 in gas is needed per unit of
    /// gas
    token_price_per_wei: HashMap<(Address, Address), U256>,
    total_gas_cost_wei:  u64
}
