use std::{io::Write, os::unix::process::ExitStatusExt, process::Command};

use convert_case::{Case, Casing};
use itertools::Itertools;

const CONTRACT_LOCATION: &str = "contracts/";
const OUT_DIRECTORY: &str = "abi-v4/";
const SRC_DIRECTORY: &str = "crates/uniswap-v4/src/uniswap/loaders/";
const BINDINGS_PATH: &str = "/src/uniswap/loaders/mod.rs";

const WANTED_CONTRACTS: [&str; 2] = ["GetUniswapV4PoolData.sol", "GetUniswapV4TickData.sol"];

// builds the contracts crate. then goes and generates bindings on this
fn main() {
    let base_dir = workspace_dir();

    let binding = base_dir.clone();
    let this_dir = binding.to_str().unwrap();

    let mut contract_dir = base_dir.clone();
    contract_dir.push(CONTRACT_LOCATION);

    // Only rerun if our contracts have actually changed
    let mut src_dir = base_dir.clone();
    src_dir.push(SRC_DIRECTORY);
    if let Some(src_dir_str) = src_dir.to_str() {
        for contract in WANTED_CONTRACTS {
            println!("cargo::rerun-if-changed={src_dir_str}{contract}");
        }
    }

    let mut out_dir = base_dir.clone();
    out_dir.push(OUT_DIRECTORY);

    let Ok(mut res) = Command::new("forge")
        .env("FOUNDRY_PROFILE", "loaders")
        .arg("bind")
        .arg("--out")
        .arg(format!("../{OUT_DIRECTORY}"))
        .arg("--optimizer-runs")
        .arg("9999999999")
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
        #[sol(rpc)]
        #[derive(Debug, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
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
        .open(format!("{this_dir}/crates/uniswap-v4/{BINDINGS_PATH}"))
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
