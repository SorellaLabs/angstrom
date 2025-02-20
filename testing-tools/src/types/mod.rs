mod handles;
pub use handles::*;
mod hooks;
pub use hooks::*;

mod state_machine_utils;
pub use state_machine_utils::*;
pub mod initial_state;
mod traits;
pub use traits::*;
pub mod config;

pub const WBTC_ADDRESS: alloy_primitives::Address =
    alloy_primitives::address!("2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599");

/// 1000000000
pub const HACKED_TOKEN_BALANCE: u128 = 1000000000000000000000;
