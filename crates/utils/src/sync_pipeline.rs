use std::{
    collections::{HashMap, VecDeque},
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll, Waker}
};

use futures::{stream::FuturesUnordered, Future, FutureExt, StreamExt};

type OperationMap<OP, CX> = HashMap<u8, fn(OP, &mut CX) -> PipelineFut<OP>>;

pub enum PipelineAction<T: PipelineOperation> {
    Next(T),
    Return(T::End),
    Err
}

pub trait ThreadPool: Unpin {
    fn spawn<F>(
        &self,
        item: F
    ) -> Pin<Box<dyn Future<Output = F::Output> + Send + Unpin + 'static>>
    where
        F: Future + Send + 'static + Unpin,
        F::Output: Send + 'static + Unpin;
}

impl ThreadPool for tokio::runtime::Handle {
    fn spawn<F>(&self, item: F) -> Pin<Box<dyn Future<Output = F::Output> + Send + Unpin + 'static>>
    where
        F: Future + Send + 'static + Unpin,
        F::Output: Send + 'static + Unpin
    {
        Box::pin(self.spawn(item).map(|res| res.unwrap()))
    }
}

pub trait PipelineOperation: Unpin + Send + 'static {
    type End: Send + Unpin + 'static;
    fn get_next_operation(&self) -> u8;
}

pub type PipelineFut<OP> = Pin<Box<dyn Future<Output = PipelineAction<OP>> + Send + Unpin>>;

pub struct PipelineBuilder<OP, CX>
where
    OP: PipelineOperation,
    CX: Unpin
{
    operations: OperationMap<OP, CX>,
    _p:         PhantomData<CX>
}

impl<OP, CX> Default for PipelineBuilder<OP, CX>
where
    OP: PipelineOperation,
    CX: Unpin
{
    fn default() -> Self {
        Self::new()
    }
}

impl<OP, CX> PipelineBuilder<OP, CX>
where
    OP: PipelineOperation,
    CX: Unpin
{
    pub fn new() -> Self {
        Self { operations: HashMap::new(), _p: PhantomData }
    }

    pub fn add_step(mut self, id: u8, item: fn(OP, &mut CX) -> PipelineFut<OP>) -> Self {
        self.operations.insert(id, item);
        self
    }

    pub fn build<T: ThreadPool>(self, threadpool: T) -> PipelineWithIntermediary<T, OP, CX> {
        PipelineWithIntermediary {
            threadpool,
            needing_queue: VecDeque::new(),
            operations: self.operations,
            tasks: FuturesUnordered::new(),
            waker: None
        }
    }
}

/// What a pipeline is a way to chain operations, in which each async operation
/// runs on its own thread
pub struct PipelineWithIntermediary<T, OP, CX>
where
    OP: PipelineOperation,
    CX: Unpin
{
    threadpool: T,
    operations: OperationMap<OP, CX>,
    waker:      Option<Waker>,

    needing_queue: VecDeque<OP>,
    tasks:         FuturesUnordered<PipelineFut<OP>>
}

impl<T, OP, CX> PipelineWithIntermediary<T, OP, CX>
where
    T: ThreadPool,
    OP: PipelineOperation,
    CX: Unpin
{
    pub fn add(&mut self, item: OP) {
        self.needing_queue.push_back(item);

        if let Some(waker) = self.waker.as_ref() {
            waker.wake_by_ref();
        }
    }

    fn spawn_task(&mut self, op: OP, pipeline_cx: &mut CX) {
        let id = op.get_next_operation();
        let c_fn = self.operations.get(&id).unwrap();
        self.tasks
            .push(self.threadpool.spawn(c_fn(op, pipeline_cx)))
    }

    pub fn poll(&mut self, cx: &mut Context<'_>, pipeline_cx: &mut CX) -> Poll<Option<OP::End>> {
        // ensure we have a waker
        if self.waker.is_none() {
            self.waker = Some(cx.waker().clone());
        }

        while let Some(item) = self.needing_queue.pop_front() {
            self.spawn_task(item, pipeline_cx)
        }

        while let Poll::Ready(Some(pipeline_finished_tasks)) = self.tasks.poll_next_unpin(cx) {
            match pipeline_finished_tasks {
                PipelineAction::Next(item) => {
                    self.spawn_task(item, pipeline_cx);
                }
                PipelineAction::Return(r) => return Poll::Ready(Some(r)),
                PipelineAction::Err => return Poll::Ready(None)
            }
        }

        Poll::Pending
    }
}
