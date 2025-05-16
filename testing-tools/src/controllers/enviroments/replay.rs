use angstrom::components::initialize_strom_handles;
use order_pool::OrderPoolHandle;
use reth_tasks::TaskExecutor;
use telemetry::{TelemetryMessage, blocklog::BlockLog};

use crate::{
    controllers::strom::initialize_strom_components_at_block, providers::AnvilProvider,
    types::WithWalletProvider
};

pub async fn replay_stuff<Provider: WithWalletProvider>(
    provider: AnvilProvider<Provider>,
    executor: TaskExecutor,
    replay_log: BlockLog
) {
    // The target block is the one in our replay
    let block_id = replay_log.blocknum();

    let telemetry_constants = replay_log.constants().unwrap().clone();
    let pools = replay_log.pool_keys().unwrap().clone();
    let strom_handles = initialize_strom_handles();
    let consensus_sender = strom_handles.consensus_tx_op.clone();

    let handle = initialize_strom_components_at_block(
        strom_handles,
        telemetry_constants,
        provider,
        executor,
        pools,
        block_id
    )
    .await
    .unwrap();

    // Playback our events in order
    for event in replay_log.events() {
        match event {
            TelemetryMessage::NewBlock { .. } => (),
            TelemetryMessage::NewOrder { origin, order, .. } => {
                let _res = handle.new_order(*origin, order.clone()).await;
            }
            TelemetryMessage::CancelOrder { cancel, .. } => {
                let _res = handle.cancel_order(cancel.clone()).await;
            }
            TelemetryMessage::Consensus { event, .. } => {
                let _res = consensus_sender.send(event.clone());
            }
            TelemetryMessage::Error { message, .. } => {
                println!("Error: {message}");
            }
        }
    }
}
