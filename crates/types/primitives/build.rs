use std::{io::Write, os::unix::process::ExitStatusExt, process::Command};

use convert_case::{Case, Casing};
use itertools::Itertools;

const CONTRACT_LOCATION: &str = "contracts/";
const OUT_DIRECTORY: &str = "abis-types/";
const SRC_DIRECTORY: &str = "contracts/src";
const BINDINGS_PATH: &str = "/src/contract_bindings/mod.rs";

const WANTED_CONTRACTS: [&str; 10] = [
    "Angstrom.sol",
    "PoolManager.sol",
    "PoolGate.sol",
    "MockRewardsManager.sol",
    "MintableMockERC20.sol",
    "ControllerV1.sol",
    "PositionFetcher.sol",
    "PositionManager.sol",
    "IPositionDescriptor.sol",
    "AngstromProtocolFeeConfig.sol"
];

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
        println!("cargo::rerun-if-changed={src_dir_str}");
    }
    println!("cargo::rerun-if-changed={OUT_DIRECTORY}");

    let mut out_dir = base_dir.clone();
    out_dir.push(OUT_DIRECTORY);

    // forge compiles into its own gitignored out dir so nothing tracked is
    // replaced until every regenerated artifact has been checked
    let staging_dir = contract_dir.join("out");
    let Ok(mut res) = Command::new("forge")
        .arg("bind")
        .arg("--overwrite")
        .current_dir(&contract_dir)
        .spawn()
    else {
        println!("didn't update binding because foundry isn't installed");

        return;
    };
    if res.wait().unwrap().into_raw() != 0 {
        return;
    }

    let contracts = WANTED_CONTRACTS
        .iter()
        .map(|file_name| {
            let name = file_name.split('.').next().unwrap();
            let artifact = format!("{file_name}/{name}.json");
            let stripped = strip_volatile(&staging_dir.join(&artifact));
            (name, artifact, stripped)
        })
        .sorted_unstable_by_key(|(name, ..)| *name)
        .collect::<Vec<_>>();

    let sol_macro_invocation = contracts
        .into_iter()
        .map(|(name, artifact, stripped)| {
            let path = out_dir.join(&artifact);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, stripped).unwrap();

            let mod_name = name.to_case(Case::Snake);
            format!(
                r#"#[rustfmt::skip]
pub mod {mod_name} {{
    alloy_sol_types::sol!(
        #[allow(missing_docs)]
        #[sol(rpc, abi)]
        #[derive(Debug, Default, PartialEq, Eq,Hash, serde::Serialize, serde::Deserialize)]
        {name},
        "../../../{OUT_DIRECTORY}{artifact}"
    );
}}
"#
            )
        })
        .collect::<Vec<_>>();

    let out_path = format!("{this_dir}/crates/types/primitives{BINDINGS_PATH}");
    let mut f = std::fs::File::options()
        .write(true)
        .truncate(true)
        .open(&out_path)
        .unwrap_or_else(|_| panic!("path not found: '{out_path}'"));

    for contract_build in sol_macro_invocation {
        write!(&mut f, "{contract_build}").expect("failed to write sol macro to contract");
    }
}

/// solc numbers source units by their index in the compilation job, so these
/// fields shift whenever the set of compiled files changes even though the abi
/// and bytecode are identical. `sol!` doesn't read them, so drop them to keep
/// the checked in artifacts stable.
fn strip_volatile(path: &std::path::Path) -> String {
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    // interfaces carry an empty "0x" object; no object at all means forge dropped
    // the bytecode, which would silently strip `BYTECODE` and `deploy` from the
    // bindings
    assert!(
        value["bytecode"]["object"].is_string(),
        "{} has no bytecode.object: this forge's `forge bind` omits bytecode (forge 1.8.3 does); \
         install forge v1.7.0",
        path.display()
    );
    let artifact = value.as_object_mut().unwrap();

    artifact.remove("ast");
    artifact.remove("id");
    for key in ["bytecode", "deployedBytecode"] {
        if let Some(bytecode) = artifact.get_mut(key).and_then(|b| b.as_object_mut()) {
            bytecode.remove("sourceMap");
            bytecode.remove("immutableReferences");
        }
    }

    serde_json::to_string(&value).unwrap()
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
