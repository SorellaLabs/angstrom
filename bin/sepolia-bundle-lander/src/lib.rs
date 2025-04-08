pub mod cli;
pub mod env;
pub mod intent_builder;
use clap::Parser;

const SEPOLIA_SIG_CHECK: alloy::primitives::Address =
    alloy::primitives::address!("0x91082B61b9d3C5DFAeF489dB7FF82ae7cfc62cB3");

alloy::sol!(
    function approve(address _spender, uint256 _value) public returns (bool success);
    function balanceOf(address _owner) public view returns (uint256 balance);
    function decimals() public view virtual returns (uint8);
    function isValidSignatureNow(
            address signer,
            bytes32 digest,
            bytes memory signature
        ) external view returns (bool);
);

#[inline]
pub fn run() -> eyre::Result<()> {
    let args = cli::BundleLander::parse();
    reth::CliRunner::try_default_runtime()
        .unwrap()
        .run_command_until_exit(|ctx| args.run(ctx.task_executor))
}
