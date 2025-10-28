use angstrom_types::primitive::PoolId;

#[derive(Debug, Clone, clap::Parser)]
pub struct ModifyPoolFeesCommand {
    /// pool id to modify
    #[clap(short, long)]
    pub pool_id: PoolId
}
