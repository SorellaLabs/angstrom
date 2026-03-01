use angstrom_metrics::block_metrics_stream::{
    BlockMetricsStreamSource, increment_lagged_events, increment_receiver_closed,
    increment_serialize_failures
};
use angstrom_rpc_api::MetricsApiServer;
use angstrom_rpc_types::metrics::BlockMetricsEventEnvelope;
use futures::StreamExt;
use jsonrpsee::{PendingSubscriptionSink, SubscriptionMessage};
use reth_tasks::TaskSpawner;
use tokio_stream::wrappers::{BroadcastStream, errors::BroadcastStreamRecvError};

pub trait MetricsStreamSource: Clone + Send + Sync + 'static {
    fn subscribe_block_events(&self) -> BroadcastStream<BlockMetricsEventEnvelope>;
}

impl MetricsStreamSource for BlockMetricsStreamSource {
    fn subscribe_block_events(&self) -> BroadcastStream<BlockMetricsEventEnvelope> {
        BroadcastStream::new(self.subscribe())
    }
}

pub struct MetricsApi<StreamSource, Spawner> {
    stream_source: StreamSource,
    task_spawner:  Spawner
}

impl<StreamSource, Spawner> MetricsApi<StreamSource, Spawner> {
    pub fn new(stream_source: StreamSource, task_spawner: Spawner) -> Self {
        Self { stream_source, task_spawner }
    }
}

#[async_trait::async_trait]
impl<StreamSource, Spawner> MetricsApiServer for MetricsApi<StreamSource, Spawner>
where
    StreamSource: MetricsStreamSource,
    Spawner: TaskSpawner + 'static
{
    async fn subscribe_block_events(
        &self,
        pending: PendingSubscriptionSink
    ) -> jsonrpsee::core::SubscriptionResult {
        let sink = pending.accept().await?;
        let mut subscription = self.stream_source.subscribe_block_events();

        self.task_spawner.spawn_task(Box::pin(async move {
            loop {
                if sink.is_closed() {
                    break;
                }

                let Some(result) = subscription.next().await else {
                    increment_receiver_closed();
                    break;
                };

                let event = match result {
                    Ok(event) => event,
                    Err(BroadcastStreamRecvError::Lagged(dropped)) => {
                        increment_lagged_events(dropped);
                        continue;
                    }
                };

                match SubscriptionMessage::new(sink.method_name(), sink.subscription_id(), &event) {
                    Ok(message) => {
                        if sink.send(message).await.is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        increment_serialize_failures();
                        tracing::error!(?error, "failed to serialize metrics subscription event");
                    }
                }
            }
        }));

        Ok(())
    }
}
