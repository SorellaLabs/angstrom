use std::sync::Arc;

use reth_provider::StateProviderFactory;
use validation::common::lru_db::RevmLRU;

pub struct TestOrderValidator<DB: StateProviderFactory> {
    /// allows us to set values to ensure
    revm_lru: Arc<RevmLRU<DB>>
}
