use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use ethers_core::types::{Block, H256};
use ethers_providers::Middleware;
use futures::Stream;
use futures_util::{Future, FutureExt};

type CatchupFuture<M> =
    Pin<Box<dyn Future<Output = Result<Option<Block<H256>>, <M as Middleware>::Error>> + Send>>;

// TODO: lets catchup by more than one at a time
pub struct RoundRobinSync<M: Middleware + 'static> {
    middleware:     &'static M,
    live_height:    u64,
    current_height: u64,
    catchup:        CatchupFuture<M>
}

impl<M: Middleware> RoundRobinSync<M> {
    pub async fn new(middleware: &'static M, current_height: u64) -> Self {
        let live_height = middleware.get_block_number().await.unwrap().as_u64();
        let catchup = middleware.get_block(current_height);

        Self { middleware, live_height, current_height, catchup }
    }

    pub fn on_new_block(&mut self, block: Arc<Block<H256>>) {
        self.live_height = block.number.unwrap().as_u64();
    }
}

impl<M: Middleware> Stream for RoundRobinSync<M> {
    type Item = Block<H256>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // TODO: clean this shit up
        if let Poll::Ready(val) = self.catchup.poll_unpin(cx) {
            let res = val.unwrap();

            if res.is_none() {
                return Poll::Ready(None)
            }

            let res = res.unwrap();

            self.current_height = res.number.unwrap().as_u64();
            self.catchup = self.middleware.get_block(self.current_height + 1);

            return Poll::Ready(Some(res))
        }

        Poll::Pending
    }
}
