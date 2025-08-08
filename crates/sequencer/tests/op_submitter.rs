#![cfg(feature = "op-stack")]

use angstrom_sequencer::OpStackSequencerSubmitter;
use angstrom_types::primitive::AngstromSigner;
use angstrom_types::submission::ChainSubmitter;
use alloy_primitives::Address;
use alloy_signer_local::PrivateKeySigner;

#[tokio::test]
async fn submitter_noop_without_http() {
    let submitter = OpStackSequencerSubmitter::new(Address::ZERO);
    let signer = AngstromSigner::new(PrivateKeySigner::random());
    let res = submitter.submit(&signer, None, &dummy_features()).await.unwrap();
    assert!(res.is_none());
}

fn dummy_features() -> angstrom_types::submission::TxFeatureInfo {
    // Values are unused in the current path because no HTTP is set; provide zeros.
    let fees = alloy::eips::eip1559::Eip1559Estimation {
        max_fee_per_gas: 0u128.into(),
        max_priority_fee_per_gas: 0u128.into(),
        base_fee: None,
        estimated_fee: None,
    };
    angstrom_types::submission::TxFeatureInfo { nonce: 0, fees, chain_id: 0, target_block: 0 }
}
