use alloy::signers::local::PrivateKeySigner;
use angstrom_types::primitive::{AngstromSigner, CHAIN_ID, KeyConfig, init_with_chain_id};
use clap::Parser;
use exe_runners::TaskExecutor;
use hsm_signer::{Pkcs11Signer, Pkcs11SignerConfig};
use reth::chainspec::NamedChain;
use tracing::Level;
use tracing_subscriber::{filter, layer::SubscriberExt, util::SubscriberInitExt};
use url::Url;

use crate::commands::modify_fees::ModifyPoolFeesCommand;

#[derive(Debug, Clone, clap::Parser)]
pub struct NodeUpdaterCli {
    /// angstrom endpoint
    #[clap(short, long, default_value = "ws://localhost:8546")]
    pub node_endpoint: Url,
    /// mainnet or sepolia ONLY
    #[clap(long, default_value = "NamedChain::Mainnet")]
    pub chain:         NamedChain,
    #[clap(flatten)]
    pub key_config:    KeyConfig,
    #[clap(subcommand)]
    pub command:       NodeUpdateCommand
}

impl NodeUpdaterCli {
    pub async fn run(self, task_executor: TaskExecutor) -> eyre::Result<()> {
        let this = Self::parse();

        init_tracing();

        assert!(this.chain == NamedChain::Mainnet || this.chain == NamedChain::Sepolia);
        init_with_chain_id(this.chain as u64);

        match self.command {
            NodeUpdateCommand::ModifyFees(modify_pool_fees_command) => todo!()
        }

        Ok(())
    }

    pub fn get_local_signer(&self) -> eyre::Result<Option<AngstromSigner<PrivateKeySigner>>> {
        self.key_config
            .local_secret_key_location
            .as_ref()
            .map(|sk_path| {
                if sk_path.try_exists()? {
                    let contents = std::fs::read_to_string(sk_path)?;
                    Ok(AngstromSigner::new(contents.trim().parse::<PrivateKeySigner>()?))
                } else {
                    Err(eyre::eyre!("no secret_key was found at {:?}", sk_path))
                }
            })
            .transpose()
    }

    pub fn get_hsm_signer(&self) -> eyre::Result<Option<AngstromSigner<Pkcs11Signer>>> {
        Ok((self.key_config.hsm_enabled)
            .then(|| {
                Pkcs11Signer::new(
                    Pkcs11SignerConfig::from_env_with_defaults(
                        self.key_config.hsm_public_key_label.as_ref().unwrap(),
                        self.key_config.hsm_private_key_label.as_ref().unwrap(),
                        self.key_config.pkcs11_lib_path.clone().into(),
                        None
                    ),
                    Some(*CHAIN_ID.get().unwrap())
                )
                .map(AngstromSigner::new)
            })
            .transpose()?)
    }
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum NodeUpdateCommand {
    #[command(name = "modify-fees")]
    ModifyFees(ModifyPoolFeesCommand)
}

fn init_tracing() {
    let level = Level::INFO;

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
            .with_target("node_update_cli", level)
            .with_target("testnet", level)
            .with_target("devnet", level)
            .with_target("angstrom_rpc", level)
            .with_target("angstrom", level)
            .with_target("testing_tools", level)
            .with_target("angstrom_eth", level)
            .with_target("matching_engine", level)
            .with_target("uniswap_v4", level)
            .with_target("consensus", level)
            .with_target("validation", level)
            .with_target("order_pool", level);
        let _ = tracing_subscriber::registry()
            .with(format)
            .with(filter)
            .try_init();
    }
}
