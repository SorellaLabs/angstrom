use std::{collections::HashMap, sync::Arc};

use alloy_primitives::{Address, U256};
use futures_util::stream::FuturesUnordered;
use tokio::task::JoinHandle;

use super::OrderValidationRequest;
use crate::common::lru_db::{BlockStateProviderFactory, RevmLRU};

/// sims the pre and post hook assuming
#[derive(Clone)]
pub struct SimValidation<DB> {
    db: Arc<RevmLRU<DB>>
}

impl<DB> SimValidation<DB>
where
    DB: BlockStateProviderFactory + Unpin + Clone + 'static
{
    pub fn new(db: Arc<RevmLRU<DB>>) -> Self {
        Self { db }
    }

    pub fn validate_hook(
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
