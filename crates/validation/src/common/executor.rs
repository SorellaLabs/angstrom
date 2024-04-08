use futures_util::{pin_mut, Future};
use tokio::{runtime::Runtime, task::JoinHandle};

use crate::bundle::errors::SimError;

/// executes tasks on the runtime
/// used for a thread pool for the simulator
pub(crate) struct ThreadPool {
    pub runtime: Runtime
}

impl ThreadPool {
    pub fn new() -> Result<Self, SimError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        Ok(Self { runtime })
    }

    /// Spawns a task with a return value
    pub fn spawn_return_task_as<F, T>(&self, fut: F) -> JoinHandle<F::Output>
    where
        F: Future<Output = T> + Send + Sync + 'static,
        T: Send + Sync + 'static
    {
        let task = async move {
            pin_mut!(fut);
            fut.await
        };

        self.runtime.handle().spawn(task)
    }

    /// Spawns a task depending on the given [TaskKind]
    pub fn spawn_task_as<F>(&self, fut: F, task_kind: TaskKind) -> JoinHandle<()>
    where
        F: Future<Output = ()> + Send + Sync + 'static
    {
        let task = async move {
            pin_mut!(fut);
            let _ = fut.await;
        };

        self.spawn_on_rt(task, task_kind)
    }

    /// Spawns a future on the tokio runtime depending on the [TaskKind]
    fn spawn_on_rt<F>(&self, fut: F, task_kind: TaskKind) -> JoinHandle<()>
    where
        F: Future<Output = ()> + Send + 'static
    {
        let handle = self.runtime.handle().clone();
        match task_kind {
            TaskKind::Default => handle.spawn(fut),
            TaskKind::Blocking => self.runtime.spawn_blocking(move || handle.block_on(fut))
        }
    }

    /// Spawns a future blocking tokio runtime
    pub fn block_on_rt<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static
    {
        self.runtime.block_on(fut)
    }
}

#[allow(dead_code)]
/// specifies a blocking or non blocking task
pub(crate) enum TaskKind {
    Default,
    Blocking
}
