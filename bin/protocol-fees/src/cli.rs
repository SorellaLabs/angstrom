use tracing::Level;
use tracing_subscriber::{filter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Clone, clap::Parser)]
pub struct ProtocolFeesCli {
    #[clap(short, long = "ws-url")]
    pub eth_ws_url: String
}

pub fn init_tracing(verbosity: u8) {
    let level = match verbosity - 1 {
        0 => Level::ERROR,
        1 => Level::WARN,
        2 => Level::INFO,
        3 => Level::DEBUG,
        _ => Level::TRACE
    };

    let envfilter = filter::EnvFilter::builder().try_from_env().ok();
    let format = tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .with_target(true);

    if let Some(f) = envfilter {
        let _ = tracing_subscriber::registry()
            .with(format)
            .with(f)
            .try_init();
    } else {
        let filter = filter::Targets::new()
            .with_target("testnet", level)
            .with_target("replay", level)
            .with_target("devnet", level)
            .with_target("protocol_fees", level)
            .with_target("angstrom_rpc", level)
            .with_target("angstrom", level)
            .with_target("testing_tools", level)
            .with_target("angstrom_eth", level)
            .with_target("matching_engine", level)
            .with_target("uniswap_v4", level)
            .with_target("angstrom_types_primitives", level)
            .with_target("angstrom_types_constants", level)
            .with_target("angstrom_rpc_api", level)
            .with_target("angstrom_rpc_types", level)
            .with_target("consensus", level)
            .with_target("validation", level)
            .with_target("order_pool", level)
            .with_target("telemetry", level);
        let _ = tracing_subscriber::registry()
            .with(format)
            .with(filter)
            .try_init();
    }
}
