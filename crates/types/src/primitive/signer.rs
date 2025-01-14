use std::ops::{Deref, DerefMut};

use alloy::{
    consensus::{SignableTransaction, TypedTransaction},
    network::{Ethereum, NetworkWallet},
    primitives::PrimitiveSignature,
    signers::{local::PrivateKeySigner, SignerSync}
};
use alloy_primitives::Address;
use k256::{ecdsa::VerifyingKey, elliptic_curve::sec1::ToEncodedPoint};
use reth_network_peers::PeerId;

/// Wrapper around key and signing to allow for a uniform type across codebase
#[derive(Debug, Clone)]
pub struct AngstromSigner {
    id:     PeerId,
    signer: PrivateKeySigner
}

impl AngstromSigner {
    pub fn new(signer: PrivateKeySigner) -> Self {
        let pub_key = signer.credential().verifying_key();
        let peer_id = Self::public_key_to_peer_id(pub_key);

        Self { signer, id: peer_id }
    }

    pub fn random() -> Self {
        Self::new(PrivateKeySigner::random())
    }

    pub fn address(&self) -> Address {
        self.signer.address()
    }

    pub fn id(&self) -> PeerId {
        self.id
    }

    /// Taken from alloy impl
    pub fn public_key_to_peer_id(pub_key: &VerifyingKey) -> PeerId {
        let affine = pub_key.as_ref();
        let encoded = affine.to_encoded_point(false);

        PeerId::from_slice(&encoded.as_bytes()[1..])
    }

    fn sign_transaction_inner(
        &self,
        tx: &mut dyn SignableTransaction<PrimitiveSignature>
    ) -> alloy::signers::Result<PrimitiveSignature> {
        let hash = tx.signature_hash();

        self.signer.sign_hash_sync(&hash)
    }
}

impl Deref for AngstromSigner {
    type Target = PrivateKeySigner;

    fn deref(&self) -> &Self::Target {
        &self.signer
    }
}

impl DerefMut for AngstromSigner {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.signer
    }
}

impl NetworkWallet<Ethereum> for AngstromSigner {
    fn default_signer_address(&self) -> Address {
        self.address()
    }

    /// Return true if the signer contains a credential for the given address.
    fn has_signer_for(&self, address: &Address) -> bool {
        address == &self.address()
    }

    /// Return an iterator of all signer addresses.
    fn signer_addresses(&self) -> impl Iterator<Item = Address> {
        vec![self.address()].into_iter()
    }

    async fn sign_transaction_from(
        &self,
        _: Address,
        tx: TypedTransaction
    ) -> alloy::signers::Result<alloy::consensus::TxEnvelope> {
        match tx {
            TypedTransaction::Legacy(mut t) => {
                let sig = self.sign_transaction_inner(&mut t)?;
                Ok(t.into_signed(sig).into())
            }
            TypedTransaction::Eip2930(mut t) => {
                let sig = self.sign_transaction_inner(&mut t)?;
                Ok(t.into_signed(sig).into())
            }
            TypedTransaction::Eip1559(mut t) => {
                let sig = self.sign_transaction_inner(&mut t)?;
                Ok(t.into_signed(sig).into())
            }
            TypedTransaction::Eip4844(mut t) => {
                let sig = self.sign_transaction_inner(&mut t)?;
                Ok(t.into_signed(sig).into())
            }
            TypedTransaction::Eip7702(mut t) => {
                let sig = self.sign_transaction_inner(&mut t)?;
                Ok(t.into_signed(sig).into())
            }
        }
    }
}
