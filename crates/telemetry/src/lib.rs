use std::task::Poll;

use snapshot::Snapshot;
use tokio::sync::mpsc::UnboundedReceiver;

pub mod snapshot;

pub struct Telemetry {
    rx: UnboundedReceiver<Snapshot>
}

impl Telemetry {
    fn on_new_snapshot(&self, snap: Snapshot) {}
}

impl Future for Telemetry {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        loop {
            match self.rx.poll_recv(cx) {
                // As long as we're getting snapshots, process them
                Poll::Ready(Some(req)) => {
                    self.on_new_snapshot(req);
                }
                // End of receiver stream should end this task as well
                Poll::Ready(None) => {
                    return Poll::Ready(());
                }
                // Otherwise we're scheduled to wake on next message, so let's pend
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}
