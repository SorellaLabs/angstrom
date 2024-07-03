use alloy::signers::local::PrivateKeySigner;
use alloy_primitives::{Address, U256};
use alloy_provider::{ext::AnvilApi, Provider, RootProvider};
use alloy_pubsub::PubSubFrontend;
use alloy_sol_types::SolCall;
// use secp256k1::{SECP256K1,SecretKey};
use contract_bytecodes::POOL_MANAGER;
use enr::k256::SecretKey;
// use enr::secp256k1::SecretKey;
// use enr::k256::SecretKey;
use sol_bindings::testnet::{PoolManagerDeployer, TestnetHub};

pub mod contract_bytecodes;

pub struct AngstromTestnetAddresses {
    pub contract: Address,
    pub token0:   Address,
    pub token1:   Address
}
/// deploys the angstrom testhub contract along with two tokens, under the
/// secret key
pub async fn deploy_contract_and_create_pool(
    pk: SecretKey,
    provider: RootProvider<PubSubFrontend>
) -> eyre::Result<AngstromTestnetAddresses> {
    // let pk:PrivateKeySigner= pk.into();
    //
    // let mut pool_bytecode = POOL_MANAGER.clone().to_vec();
    // let out= PoolManagerDeployer::deploy(provider, U256::MAX).await?;
    // let signer = alloy_
    //
    // provider.send_transaction(tx)

    todo!()
}
