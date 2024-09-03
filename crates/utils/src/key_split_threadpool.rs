use std::{
    collections::{HashMap,hash_map::Entry, VecDeque},
    future::Future,
    hash::Hash,
    pin::Pin,
    sync::{atomic::AtomicBool, Arc},
    task::{Context, Poll}
};

use futures::{stream::FuturesUnordered, Stream, StreamExt};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::sync_pipeline::ThreadPool;

pub struct AtomicQueue<F: Future> {
    pub queue:      VecDeque<F>,
    pub processing: Arc<AtomicBool>
}

pub struct KeySplitThreadpool<K: PartialEq + Eq + Hash + Clone, F: Future, TP: ThreadPool> {
    tp:              TP,
    pending_results: FuturesUnordered<Pin<Box<dyn Future<Output = F::Output> + Send + Unpin>>>,
    pending:         HashMap<K, AtomicQueue<F>>,
    tx:              UnboundedSender<K>,
    rx:              UnboundedReceiver<K>
}

impl<K: PartialEq + Eq + Hash + Clone, F: Future, TP: ThreadPool> KeySplitThreadpool<K, F, TP>
where
    K: Send + Unpin +'static,
    F: Send + 'static + Unpin,
    <F as Future>::Output: Send + 'static + Unpin
{
    pub fn new(theadpool: TP) -> Self {
        let (tx, rx) = unbounded_channel();
        Self {
            tp: theadpool,
            tx,
            rx,
            pending: HashMap::default(),
            pending_results: FuturesUnordered::default()
        }
    }

    fn process_finished_keys(&mut self, cx: &mut Context<'_>) {
        while let Poll::Ready(Some(next)) = self.rx.poll_recv(cx) {
            self.process_next_task_key(next);
        }
    }

    fn process_next_task_key(&mut self, key:K ){
        if let Entry::Occupied(mut o) = self.pending.entry(key.clone()) {
            let e = o.get_mut();
            let Some(fut) = e.queue.pop_front() else {
                o.remove();
                return
            };

            let cloned_key = key.clone();
            let cloned_tx = self.tx.clone();
            let cloned_atomic = e.processing.clone();

            self.pending_results.push(self.tp.spawn(Box::pin(async move {
                cloned_atomic.store(true, std::sync::atomic::Ordering::SeqCst);
                let result = fut.await;
                cloned_tx.send(cloned_key);
                cloned_atomic.store(false, std::sync::atomic::Ordering::SeqCst);

                result
            }))); }

        }     

    /// goes though all task queues where no processing is occurring
    fn queue_tasks_not_processing(&mut self) {
        self.pending.retain(|key, queue| {
            // if we are processing some already for the given key, we will wait till its
            // done before the next key
            if queue.processing.load(std::sync::atomic::Ordering::SeqCst) {
                return true
            }

            let Some(fut) = queue.queue.pop_front() else { return false };
            let cloned_key = key.clone();
            let cloned_tx = self.tx.clone();
            let cloned_atomic = queue.processing.clone();

            self.pending_results.push(self.tp.spawn(Box::pin(async move {
                cloned_atomic.store(true, std::sync::atomic::Ordering::SeqCst);
                let result = fut.await;
                cloned_tx.send(cloned_key);
                cloned_atomic.store(false, std::sync::atomic::Ordering::SeqCst);

                result
            })));

            !queue.queue.is_empty()
        });
    }
}

impl<K: PartialEq + Eq + Hash + Clone, F: Future, TP: ThreadPool> Stream
    for KeySplitThreadpool<K, F, TP>
{
    type Item = F::Output;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> Poll<Option<Self::Item>> {

        self.pending_results.poll_next_unpin(cx)
        Poll::Pending
    }
}
