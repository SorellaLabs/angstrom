use std::{pin::pin, time::Duration};

use alloy_primitives::{Address, U256};
use alloy_provider::ext::AnvilApi;
use angstrom_types::sol_bindings::testnet::{MockERC20, PoolManagerDeployer, TestnetHub};
pub use deploy_pairs::*;
use futures::Future;
use tokio::time::timeout;

use crate::anvil_utils::AnvilWalletRpc;

pub mod contract_bytecodes;
pub mod deploy_pairs;

pub struct AngstromTestnetAddresses {
    pub contract: Address,
    pub pairs:    Vec<AngstromTokens>
}
/// deploys the angstrom testhub contract along with two tokens, under the
/// secret key
pub async fn deploy_contract_and_create_pool(
    provider: AnvilWalletRpc,
    pair_count: usize
) -> eyre::Result<AngstromTestnetAddresses> {
    let out = anvil_mine_delay(
        Box::pin(PoolManagerDeployer::deploy(provider.clone(), U256::MAX)),
        &provider,
        Duration::from_millis(500)
    )
    .await?;

    let v4_address = *out.address();
    let testhub = anvil_mine_delay(
        Box::pin(TestnetHub::deploy(provider.clone(), Address::ZERO, v4_address)),
        &provider,
        Duration::from_millis(500)
    )
    .await?;
    let angstrom_address = *testhub.address();

    tracing::info!(
        ?angstrom_address,
        ?v4_address,
        "deployed v4 and angstrom test contract on anvil"
    );
    let pairs = deploy_tokens_for_pairs(provider, angstrom_address, pair_count).await?;

    Ok(AngstromTestnetAddresses { contract: angstrom_address, pairs })
}
