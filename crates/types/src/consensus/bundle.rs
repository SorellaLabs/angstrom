use std::collections::HashSet;

use bytes::Bytes;
use ethers_core::types::H256;
use secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId},
    Message, SECP256K1
};

use super::{GuardSet, Time};
use crate::on_chain::{RawBundle, RecoveryError, Signature};

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    RlpDecodable,
    RlpEncodable,
    PartialEq,
    Eq,
    Hash,
    ethers_contract::EthAbiType,
    ethers_contract::EthAbiCodec,
)]
pub struct BundleVote {
    pub hash:      H256,
    pub height:    u64,
    pub round:     u64,
    pub timestamp: Time,
    pub signature: Signature
}

impl BundleVote {
    pub fn recover_public_key(&self) -> Result<[u8; 32], RecoveryError> {
        let sig = RecoverableSignature::from_compact(
            &self.signature.to_vec()[0..64],
            RecoveryId::from_i32(self.sig.to_vec()[64] as i32)
                .map_err(|e| RecoveryError::UnableToDecodeSignature(e.to_string()))?
        )
        .map_err(|err| RecoveryError::UnableToDecodeSignature(err.to_string()))?;

        trace!(?sig, "Validating Signature -- RECOVERING PUBLIC KEY");
        // secp256k1 public key
        SECP256K1
            .recover_ecdsa(
                &Message::from_slice(&self.hash[..32])
                    .map_err(|e| RecoveryError::UnableToRecoverSigner(e.to_string()))?,
                &sig
            )
            .map(|public_key| H512::from_slice(&public_key.serialize_uncompressed()[1..]))
            .map_err(|err| RecoveryError::UnableToRecoverSigner(err.to_string()))
    }
}

/// propagated when we hit more than 2/3 votes for a bundle
#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    RlpDecodable,
    RlpEncodable,
    PartialEq,
    Eq,
    Hash,
    ethers_contract::EthAbiType,
    ethers_contract::EthAbiCodec,
)]
pub struct Bundle23Votes {
    hash:       H256,
    height:     u64,
    round:      u64,
    timestamp:  Time,
    signatures: HashSet<Signature>
}

impl Bundle23Votes {
    pub fn new(hash: H256, height: u64, round: u64, signatures: Vec<Signature>) -> Self {
        Self { height, round, hash, signatures, timestamp: Time::now() }
    }

    pub fn verify_signatures(&self, guards: &GuardSet) -> bool {
        // TODO: This seems wrong
        if self.signatures.len() % guards.len() < 67 {
            return false
        }

        !self
            .signatures
            .iter()
            .map(|signature| {
                let sig = RecoverableSignature::from_compact(
                    &signature.to_vec()[0..64],
                    RecoveryId::from_i32(signature.to_vec()[64] as i32).ok()?
                )
                .ok()?;

                trace!(?sig, "Validating Signature -- RECOVERING PUBLIC KEY");
                // secp256k1 public key
                let key = SECP256K1
                    .recover_ecdsa(&Message::from_slice(&self.hash[..32])?, &sig)
                    .map(|public_key| H512::from_slice(&public_key.serialize_uncompressed()[1..]))
                    .ok()?;

                Some(guards.contains_key(&key))
            })
            .any(|y| y.is_none())
    }
}
