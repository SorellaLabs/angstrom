use std::ops::{Deref, DerefMut};

use alloy::{
    consensus::{SignableTransaction, TypedTransaction},
    network::{Ethereum, NetworkWallet},
    primitives::Signature,
    signers::{Signer, SignerSync, local::PrivateKeySigner}
};
use alloy_primitives::Address;
use hsm_signer::Pkcs11Signer;
use k256::{
    ecdsa::{SigningKey, VerifyingKey},
    elliptic_curve::sec1::ToEncodedPoint
};
use reth_network_peers::PeerId;
use secp256k1::rand::rngs::OsRng;

/// Wrapper around key and signing to allow for a uniform type across codebase
#[derive(Debug, Clone)]
pub struct AngstromSigner<S: AngstromMetaSigner> {
    id:     PeerId,
    signer: S
}

impl<S: AngstromMetaSigner> AngstromSigner<S> {
    pub fn new(signer: S) -> Self {
        let pub_key = signer.pubkey();
        let peer_id = public_key_to_peer_id(&pub_key);

        Self { signer, id: peer_id }
    }

    pub fn address(&self) -> Address {
        self.signer.address()
    }

    pub fn id(&self) -> PeerId {
        self.id
    }

    fn sign_transaction_inner(
        &self,
        tx: &mut dyn SignableTransaction<Signature>
    ) -> alloy::signers::Result<Signature> {
        let hash = tx.signature_hash();

        self.signer.sign_hash_sync(&hash)
    }
}

/// Taken from alloy impl
pub fn public_key_to_peer_id(pub_key: &VerifyingKey) -> PeerId {
    let affine = pub_key.as_ref();
    let encoded = affine.to_encoded_point(false);

    PeerId::from_slice(&encoded.as_bytes()[1..])
}

impl AngstromSigner<PrivateKeySigner> {
    pub fn random() -> Self {
        Self::new(PrivateKeySigner::random())
    }

    /// Make a dummy signer that targets a specified address, this is used only
    /// for snapshot replay at the moment
    pub fn for_address(address: Address) -> Self {
        let credential = SigningKey::random(&mut OsRng);
        let key = PrivateKeySigner::new_with_credential(credential, address, None);
        Self::new(key)
    }

    pub fn into_signer(self) -> PrivateKeySigner {
        self.signer
    }
}

impl<S: AngstromMetaSigner> Deref for AngstromSigner<S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        &self.signer
    }
}

impl<S: AngstromMetaSigner> DerefMut for AngstromSigner<S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.signer
    }
}

impl<S: AngstromMetaSigner> NetworkWallet<Ethereum> for AngstromSigner<S> {
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

pub trait AngstromMetaSigner:
    SignerSync + Signer + Clone + Unpin + std::fmt::Debug + Send + Sync + 'static
{
    fn pubkey(&self) -> VerifyingKey;
}

impl AngstromMetaSigner for PrivateKeySigner {
    fn pubkey(&self) -> VerifyingKey {
        *self.credential().verifying_key()
    }
}

impl AngstromMetaSigner for Pkcs11Signer {
    fn pubkey(&self) -> VerifyingKey {
        self.verifying_key()
    }
}
