use std::{io::Write, os::unix::process::ExitStatusExt, process::Command};

use convert_case::{Case, Casing};
use itertools::Itertools;

const CONTRACT_LOCATION: &str = "contracts/";
const OUT_DIRECTORY: &str = "abis-types/";
const SRC_DIRECTORY: &str = "contracts/src";
const BINDINGS_PATH: &str = "/src/contract_bindings/mod.rs";

const WANTED_CONTRACTS: [&str; 9] = [
    "Angstrom.sol",
    "PoolManager.sol",
    "PoolGate.sol",
    "MockRewardsManager.sol",
    "MintableMockERC20.sol",
    "ControllerV1.sol",
    "PositionFetcher.sol",
    "PositionManager.sol",
    "IPositionDescriptor.sol"
];

// builds the contracts crate. then goes and generates bindings on this
fn main() {
    let base_dir = workspace_dir();

    let binding = base_dir.clone();
    let this_dir = binding.to_str().unwrap();

    let mut contract_dir = base_dir.clone();
    contract_dir.push(CONTRACT_LOCATION);

    let mut src_dir = base_dir.clone();
    src_dir.push(SRC_DIRECTORY);
    if let Some(src_dir_str) = src_dir.to_str() {
        println!("cargo:rerun-if-changed={src_dir_str}"); // ✅ Single colon
    }

    let mut out_dir = base_dir.clone();
    out_dir.push(OUT_DIRECTORY);
    if let Some(out_dir_str) = out_dir.to_str() {
        println!("cargo:rerun-if-changed={out_dir_str}"); // ✅ Single colon
    }

    // ✅ Fix 2: Check if we actually need to rebuild
    let bindings_file_path = format!("{this_dir}/crates/types{BINDINGS_PATH}");

    // Check if output is newer than inputs
    if is_output_up_to_date(&src_dir, &bindings_file_path) {
        return; // Skip rebuild if nothing changed
    }

    let Ok(mut res) = Command::new("forge")
        .arg("bind")
        .arg("--out")
        .arg(format!("../{OUT_DIRECTORY}"))
        .current_dir(contract_dir)
        .spawn()
    else {
        println!("didn't update binding because foundry isn't installed");

        return;
    };
    let res = res.wait().unwrap();

    std::fs::read_dir(&out_dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let file_name = entry.file_name();
            let file_name_str = file_name.to_str().unwrap();
            !WANTED_CONTRACTS.contains(&file_name_str)
        })
        .for_each(|entry| {
            let _ = std::fs::remove_dir_all(entry.path());
        });

    if res.into_raw() != 0 {
        return;
    }

    let sol_macro_invocation = std::fs::read_dir(out_dir)
        .unwrap()
        .filter_map(|folder| {
            let folder = folder.ok()?;
            let mut path = folder.path();
            let file_name = path.file_name()?.to_str()?;
            if !WANTED_CONTRACTS.contains(&file_name) {
                return None;
            }
            let raw = file_name.split('.').collect::<Vec<_>>()[0].to_owned();
            path.push(format!("{raw}.json"));

            Some((raw, path.to_str()?.to_owned()))
        })
        .sorted_unstable_by_key(|key| key.0.clone())
        .map(|(name, path_of_contracts)| {
            let path_of_contracts = path_of_contracts.replace(this_dir, "../..");

            let mod_name = name.clone().to_case(Case::Snake);
            format!(
                r#"#[rustfmt::skip]
pub mod {mod_name} {{
    alloy::sol!(
        #[allow(missing_docs)]
        #[sol(rpc, abi)]
        #[derive(Debug, Default, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
        {name},
        "{path_of_contracts}"
    );
}}
"#
            )
        })
        .collect::<Vec<_>>();

    let mut f = std::fs::File::options()
        .write(true)
        .truncate(true)
        .open(format!("{this_dir}/crates/types{BINDINGS_PATH}"))
        .unwrap();

    for contract_build in sol_macro_invocation {
        write!(&mut f, "{contract_build}").expect("failed to write sol macro to contract");
    }
}

pub fn workspace_dir() -> std::path::PathBuf {
    let output = std::process::Command::new(env!("CARGO"))
        .arg("locate-project")
        .arg("--workspace")
        .arg("--message-format=plain")
        .output()
        .unwrap()
        .stdout;
    let cargo_path = std::path::Path::new(std::str::from_utf8(&output).unwrap().trim());
    cargo_path.parent().unwrap().to_path_buf()
}

fn is_output_up_to_date(src_dir: &std::path::Path, output_file: &str) -> bool {
    let Ok(output_metadata) = std::fs::metadata(output_file) else {
        return false; // Output doesn't exist, need to build
    };

    let output_modified = output_metadata.modified().unwrap();

    // Check if any source file is newer than output
    if let Ok(entries) = std::fs::read_dir(src_dir) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if let Ok(modified) = metadata.modified() {
                    if modified > output_modified {
                        return false; // Source is newer, need rebuild
                    }
                }
            }
        }
    }

    true // Output is up to date
}
