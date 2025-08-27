use std::sync::Arc;

use angstrom_types::flashblocks::PendingChain;
use tokio::sync::broadcast;
use url::Url;

pub struct FlashblocksSubscriber {
    ws:      Url,
    /// The current pending chain.
    pending: PendingChain,
    sender:  broadcast::Sender<Arc<PendingChain>>
}
