use std::{io::Write, os::unix::process::ExitStatusExt, process::Command};

use convert_case::{Case, Casing};
use itertools::Itertools;

const CONTRACT_LOCATION: &str = "contracts/";
const OUT_DIRECTORY: &str = "contracts/out/";
const BINDINGS_PATH: &str = "/src/contract_bindings/mod.rs";

const WANTED_CONTRACTS: [&str; 5] = [
    "Angstrom.sol",
    "PoolManager.sol",
    "PoolGate.sol",
    "MockRewardsManager.sol",
    "MintableMockERC20.sol"
];

// builds the contracts crate. then goes and generates bindings on this
fn main() {
    let base_dir = workspace_dir();

    let binding = base_dir.clone();
    let this_dir = binding.to_str().unwrap();

    let mut contract_dir = base_dir.clone();
    contract_dir.push(CONTRACT_LOCATION);

    let mut out_dir = base_dir.clone();
    out_dir.push(OUT_DIRECTORY);

    let res = Command::new("forge")
        .arg("build")
        .arg("--optimize")
        .arg("--optimizer-runs")
        .arg("9999999999")
        .current_dir(contract_dir)
        .spawn()
        .expect("foundry is not installed on this machine.\n https://book.getfoundry.sh/getting-started/installation go to here to install")
        .wait()
        .unwrap();

    if res.into_raw() != 0 {
        panic!("foundry failed to build files");
    }

    let sol_macro_invocation = std::fs::read_dir(out_dir)
        .unwrap()
        .filter_map(|folder| {
            let folder = folder.ok()?;
            let mut path = folder.path();
            let file_name = path.file_name()?.to_str()?;
            if !WANTED_CONTRACTS.contains(&file_name) {
                return None
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

    // panic!("{this_dir}/crates/types{BINDINGS_PATH}");

    let mut f = std::fs::File::options()
        .write(true)
        .truncate(true)
        .open(format!("{this_dir}/crates/types{BINDINGS_PATH}"))
        .unwrap();

    for contract_build in sol_macro_invocation {
        write!(&mut f, "{}", contract_build).expect("failed to write sol macro to contract");
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
