use std::{collections::HashMap, task::Poll};

use angstrom_types::{primitive::PoolId, uni_structure::BaselinePoolState};
use outputs::{TelemetryOutput, log::LogOutput};
use snapshot::Snapshot;
use tokio::sync::mpsc::UnboundedReceiver;

pub mod client;
pub mod outputs;
pub mod snapshot;

pub enum TelemetryMessage {
    /// Message indicating that a new block has begun.  Sent by the pool manager
    /// with the updated pool snapshot for that block
    NewBlock { blocknum: u64, pool_snapshots: HashMap<PoolId, BaselinePoolState> },
    /// Message indicating an incoming order to be validated
    Order { blocknum: u64 },
    /// Message indicating an incoming Consensus message
    Consensus { blocknum: u64 },
    /// Message indicating an error has happened, marking a block for output
    Error { blocknum: u64, message: String },
    /// For legacy reasons - allowing us to migrate to these new messages
    Snapshot { snapshot: Snapshot }
}

pub struct Telemetry {
    rx:          UnboundedReceiver<TelemetryMessage>,
    block_cache: HashMap<u64, Snapshot>,
    block_list:  Vec<u64>,
    outputs:     Vec<Box<dyn TelemetryOutput>>
}

impl Telemetry {
    pub fn new(rx: UnboundedReceiver<TelemetryMessage>) -> Self {
        // By default let's just turn on all our outputs, we only have one
        let outputs: Vec<Box<dyn TelemetryOutput>> = vec![Box::new(LogOutput {})];
        let block_cache = HashMap::new();
        let block_list = Vec::new();
        Self { rx, block_cache, block_list, outputs }
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
                Poll::Ready(Some(req)) => match req {
                    TelemetryMessage::Snapshot { snapshot } => self.on_new_snapshot(snapshot),
                    _ => println!("Unhandled!")
                },
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

pub fn init_telemetry(telemetry_rx: UnboundedReceiver<TelemetryMessage>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(4)
            .build()
            .unwrap();

        rt.block_on(Telemetry::new(telemetry_rx))
    });
}
