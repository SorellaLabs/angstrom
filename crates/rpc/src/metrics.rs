use angstrom_metrics::{
    MetricsStreamSource,
    block_metrics_stream::{
        increment_lagged_events, increment_receiver_closed, increment_serialize_failures
    }
};
use angstrom_rpc_api::MetricsApiServer;
use futures::StreamExt;
use jsonrpsee::{PendingSubscriptionSink, SubscriptionMessage};
use reth_tasks::TaskSpawner;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

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
    async fn subscribe_metric_events(
        &self,
        pending: PendingSubscriptionSink
    ) -> jsonrpsee::core::SubscriptionResult {
        let sink = pending.accept().await?;
        let mut subscription = self.stream_source.subscribe_block_events();

        self.task_spawner.spawn_task(Box::pin(async move {
            while let Some(result) = subscription.next().await {
                if sink.is_closed() {
                    break;
                }

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

            increment_receiver_closed();
        }));

        Ok(())
    }
}
