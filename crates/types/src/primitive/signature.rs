use std::{
    collections::HashMap,
    ops::{Deref, DerefMut}
};

use alloy_primitives::{Address, FixedBytes, U256};
use alloy_rlp::{Decodable, Encodable, Error};
use reth_network_peers::{pk2id, PeerId};
use reth_primitives::Signature as ESignature;
use secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId},
    Message, SECP256K1
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(transparent)]
#[derive(Default)]
pub struct Signature(pub ESignature);

impl Signature {
    pub fn new_from_bytes(bytes: &[u8]) -> eyre::Result<Self> {
        if bytes.len() != 65 {
            eyre::bail!("invalid sig size");
        }
        let r = U256::from_be_slice(&bytes[0..32]);
        let s = U256::from_be_slice(&bytes[32..64]);
        let odd_y_parity = bytes[65] != 0;
        Ok(Self(ESignature { r, s, odd_y_parity }))
    }

    pub fn recover_signer_full_public_key(
        &self,
        message: FixedBytes<32>
    ) -> Result<PeerId, secp256k1::Error> {
        let mut bytes_sig = [0u8; 65];
        bytes_sig[..32].copy_from_slice(&self.r.to_be_bytes::<32>());
        bytes_sig[32..64].copy_from_slice(&self.s.to_be_bytes::<32>());
        bytes_sig[64] = self.odd_y_parity as u8;

        let sig = RecoverableSignature::from_compact(
            &bytes_sig[0..64],
            RecoveryId::from_i32(bytes_sig[64] as i32)?
        )?;

        let public = SECP256K1.recover_ecdsa(&Message::from_digest_slice(&message[..32])?, &sig)?;
        Ok(pk2id(&public))
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CallerInfo {
    pub address:   Address,
    pub nonce:     u64,
    pub overrides: HashMap<Address, HashMap<U256, U256>>
}

impl Encodable for Signature {
    fn encode(&self, out: &mut dyn open_fastrlp::BufMut) {
        self.0.encode(out);
    }

    fn length(&self) -> usize {
        self.0.payload_len()
    }
}
impl Decodable for Signature {
    fn decode(buf: &mut &[u8]) -> Result<Self, Error> {
        let sig = ESignature::decode(buf)?;
        Ok(Signature(sig))
    }
}

impl Deref for Signature {
    type Target = ESignature;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for Signature {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("failed to decode signature: {0:#?}")]
    UnableToDecodeSignature(String),
    #[error("failed to decode signer: {0:#?}")]
    UnableToRecoverSigner(String),
    #[error("message doesn't match")]
    MessageDoesntMatch
}

#[cfg(test)]
mod tests {
    use rand::thread_rng;
    use reth_primitives::keccak256;
    use secp256k1::SecretKey;

    use super::*;

    #[test]
    fn test_signature_verification() {
        let message = keccak256([12, 56, 32, 56, 0, 0, 0, 0, 2, 63, 122]);
        let mut rng = thread_rng();
        let secp = secp256k1::Secp256k1::new();
        let secret_key = SecretKey::new(&mut rng);
        let pub_key = pk2id(&secret_key.public_key(&secp));

        let sig = Signature(
            reth_primitives::sign_message(FixedBytes(secret_key.secret_bytes()), message).unwrap()
        );
        let recovered = sig.recover_signer_full_public_key(message).unwrap();

        assert_eq!(recovered, pub_key);
    }
}
