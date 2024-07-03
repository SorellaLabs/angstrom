use alloy::signers::local::PrivateKeySigner;
use alloy_primitives::{Address, U256};
use alloy_provider::{ext::AnvilApi, Provider, RootProvider};
use alloy_pubsub::PubSubFrontend;
use alloy_sol_types::SolCall;
// use secp256k1::{SECP256K1,SecretKey};
use contract_bytecodes::POOL_MANAGER;
use enr::k256::SecretKey;
use futures::future::{join, try_join};
// use enr::secp256k1::SecretKey;
// use enr::k256::SecretKey;
use sol_bindings::testnet::{MockERC20, PoolManagerDeployer, TestnetHub};

use crate::anvil_utils::AnvilWalletRpc;

pub mod contract_bytecodes;

pub struct AngstromTestnetAddresses {
    pub contract: Address,
    pub token0:   Address,
    pub token1:   Address
}
/// deploys the angstrom testhub contract along with two tokens, under the
/// secret key
pub async fn deploy_contract_and_create_pool(
    provider: AnvilWalletRpc
) -> eyre::Result<AngstromTestnetAddresses> {
    let out = PoolManagerDeployer::deploy(provider.clone(), U256::MAX).await?;
    let v4_address = out.address().clone();
    let testhub = TestnetHub::deploy(provider.clone(), Address::ZERO, v4_address).await?;
    let angstrom_address = *testhub.address();

    let token0 = MockERC20::deploy(provider.clone()).await?;
    let token1 = MockERC20::deploy(provider.clone()).await?;
    let token0 = *token0.address();
    let token1 = *token1.address();

    tracing::info!(
        ?angstrom_address,
        ?v4_address,
        ?token0,
        ?token1,
        "deployed v4 and angstrom test contract on anvil"
    );

    Ok(AngstromTestnetAddresses { contract: angstrom_address, token0, token1 })
}
