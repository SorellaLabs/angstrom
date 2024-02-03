use std::{collections::HashMap, sync::Arc};

use alloy_primitives::{Address, U256};
use futures_util::stream::FuturesUnordered;
use reth_provider::StateProviderFactory;
use tokio::task::JoinHandle;

use super::OrderValidationRequest;
use crate::common::lru_db::RevmLRU;

/// sims the pre and post hook assuming
#[derive(Clone)]
pub struct SimValidation<DB> {
    db: Arc<RevmLRU<DB>>
}

impl<DB> SimValidation<DB>
where
    DB: StateProviderFactory + Unpin +Clone + 'static
{
    pub fn new(db: Arc<RevmLRU<DB>>) -> Self {
        Self { db }
    }

    pub fn validate_pre_hook(
        &self,
        order: OrderValidationRequest
    ) -> (OrderValidationRequest, HashMap<Address, HashMap<U256, U256>>) {
        todo!()
    }

    pub fn validate_post_hook(
        &self,
        order: OrderValidationRequest,
        overrides: HashMap<Address, HashMap<U256, U256>>
    ) -> (OrderValidationRequest, HashMap<Address, HashMap<U256, U256>>) {
        todo!()
    }
}
