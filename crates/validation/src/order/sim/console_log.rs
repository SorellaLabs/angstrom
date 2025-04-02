use std::{collections::HashMap, sync::LazyLock};

use alloy::{
    primitives::{Address, Selector},
    sol_types::SolInterface
};
use revm::{Inspector, primitives::address};

#[rustfmt::skip]
#[allow(clippy::module_inception)]
pub mod console_log {
    alloy::sol!(
        #[allow(missing_docs)]
        #[sol(rpc)]
        #[derive(Debug, Default, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
        ConsoleLog,
        "abi/hh_console.json"
    );
}

// 0x000000000000000000636F6e736F6c652e6c6f67
const CONSOLE_LOG_ADDR: Address = address!("000000000000000000636F6e736F6c652e6c6f67");

/// Inspector that monitors and prints calldata for specific address calls
pub struct CallDataInspector;

impl<CTX> Inspector<CTX> for CallDataInspector {
    fn call(
        &mut self,
        _context: &mut CTX,
        inputs: &mut revm::interpreter::CallInputs
    ) -> Option<revm::interpreter::CallOutcome> {
        if inputs.target_address == CONSOLE_LOG_ADDR {
            let mut input = inputs.input.to_vec();
            patch_hh_console_selector(&mut input);

            let out = console_log::ConsoleLog::ConsoleLogCalls::abi_decode(&input, false);
            tracing::info!(?out);
        }
        None
    }
}

/// Patches the given Hardhat `console` function selector to its ABI-normalized
/// form.
///
/// See [`HARDHAT_CONSOLE_SELECTOR_PATCHES`] for more details.
pub fn patch_hh_console_selector(input: &mut [u8]) {
    if let Some(selector) = hh_console_selector(input) {
        input[..4].copy_from_slice(selector.as_slice());
    }
}

/// Returns the ABI-normalized selector for the given Hardhat `console` function
/// selector.
///
/// See [`HARDHAT_CONSOLE_SELECTOR_PATCHES`] for more details.
pub fn hh_console_selector(input: &[u8]) -> Option<&'static Selector> {
    if let Some(selector) = input.get(..4) {
        let selector: &[u8; 4] = selector.try_into().unwrap();
        HARDHAT_CONSOLE_SELECTOR_PATCHES
            .get(selector)
            .map(Into::into)
    } else {
        None
    }
}

/// Maps all the `hardhat/console.log` log selectors that use the legacy ABI
/// (`int`, `uint`) to their normalized counterparts (`int256`, `uint256`).
///
/// `hardhat/console.log` logs its events manually, and in functions that accept
/// integers they're encoded as `abi.encodeWithSignature("log(int)", p0)`, which
/// is not the canonical ABI encoding for `int` that Solidity and [`sol!`] use.
pub static HARDHAT_CONSOLE_SELECTOR_PATCHES: LazyLock<HashMap<[u8; 4], [u8; 4]>> =
    LazyLock::new(|| HashMap::from_iter(include!("./patches.rs")));
