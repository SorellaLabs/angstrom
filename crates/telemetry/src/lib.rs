use std::task::Poll;

use outputs::{TelemetryOutput, log::LogOutput};
use snapshot::Snapshot;
use tokio::sync::mpsc::UnboundedReceiver;

pub mod outputs;
pub mod snapshot;

pub struct Telemetry {
    rx:      UnboundedReceiver<Snapshot>,
    outputs: Vec<Box<dyn TelemetryOutput>>
}

impl Telemetry {
    pub fn new(rx: UnboundedReceiver<Snapshot>) -> Self {
        // By default let's just turn on all our outputs, we only have one
        let outputs: Vec<Box<dyn TelemetryOutput>> = vec![Box::new(LogOutput {})];
        Self { rx, outputs }
    }

    /// On a snapshot being received, we want each output to save it
    fn on_new_snapshot(&self, snap: Snapshot) {
        self.outputs.iter().for_each(|out| out.snapshot(&snap));
    }
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

pub fn init_telemetry(telemetry_rx: UnboundedReceiver<Snapshot>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(4)
            .build()
            .unwrap();

        rt.block_on(Telemetry::new(telemetry_rx))
    });
}
