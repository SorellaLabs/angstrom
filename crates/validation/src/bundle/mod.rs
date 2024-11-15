pub mod types;
pub use types::*;

pub mod validator;
use std::fmt::Debug;

pub use validator::*;

pub struct BundleValidator<DB> {
    db: DB
}

impl<DB> BundleValidator<DB>
where
    DB: Unpin + Clone + 'static + reth_provider::BlockNumReader + revm::DatabaseRef + Send + Sync,
    <DB as revm::DatabaseRef>::Error: Send + Sync + Debug
{
    fn new(db: DB) -> Self {
        Self { db }
    }
}
