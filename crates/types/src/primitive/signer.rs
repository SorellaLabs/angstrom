use std::ops::{Deref, DerefMut};

use alloy::{
    consensus::{SignableTransaction, TypedTransaction},
    network::{Ethereum, NetworkWallet},
    primitives::Signature,
    signers::SignerSync
};
use alloy_primitives::Address;
#[cfg(feature = "aws-signer")]
pub use aws::AngstromAwsSigner;
use k256::{ecdsa::VerifyingKey, elliptic_curve::sec1::ToEncodedPoint};
use reth_network_peers::PeerId;

#[cfg(not(feature = "aws-signer"))]
type AngstromSigningKey = alloy::signers::local::PrivateKeySigner;
#[cfg(feature = "aws-signer")]
type AngstromSigningKey = aws::AngstromAwsSigner;

/// Wrapper around key and signing to allow for a uniform type across codebase
#[derive(Debug, Clone)]
pub struct AngstromSigner<S = AngstromSigningKey> {
    id:     PeerId,
    signer: S
}

impl AngstromSigner {
    pub fn new(signer: AngstromSigningKey) -> Self {
        let peer_id = Self::public_key_to_peer_id(&signer.verifying_key());
        Self { signer, id: peer_id }
    }

    pub fn random() -> AngstromSigner<alloy::signers::local::PrivateKeySigner> {
        let signer = alloy::signers::local::PrivateKeySigner::random();
        let peer_id = AngstromSigner::public_key_to_peer_id(&signer.verifying_key());
        AngstromSigner { signer, id: peer_id }
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
        tx: &mut dyn SignableTransaction<Signature>
    ) -> alloy::signers::Result<Signature> {
        let hash = tx.signature_hash();
        self.signer.sign_hash_sync(&hash)
    }
}

impl<S> Deref for AngstromSigner<S> {
    type Target = S;

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

pub trait GetVerifyingKey {
    fn verifying_key(&self) -> VerifyingKey;
}

impl GetVerifyingKey for alloy::signers::local::PrivateKeySigner {
    fn verifying_key(&self) -> VerifyingKey {
        self.credential().verifying_key().clone()
    }
}

#[cfg(feature = "aws-signer")]
mod aws {

    use alloy::signers::{Signer, SignerSync, aws::AwsSigner};
    use alloy_primitives::{B256, ChainId};
    use tokio::runtime::Handle;

    use super::*;

    #[derive(Debug, Clone)]
    pub struct AngstromAwsSigner {
        signer: AwsSigner,
        handle: Handle
    }

    impl AngstromAwsSigner {
        pub fn new(signer: AwsSigner, handle: Handle) -> Self {
            Self { signer, handle }
        }

        pub fn address(&self) -> Address {
            self.signer.address()
        }
    }

    impl SignerSync for AngstromAwsSigner {
        fn sign_hash_sync(&self, hash: &B256) -> alloy::signers::Result<Signature> {
            self.signer.sign_hash(&hash).to_sync(self.handle.clone())
        }

        fn chain_id_sync(&self) -> Option<ChainId> {
            self.signer.chain_id()
        }
    }

    impl GetVerifyingKey for AngstromAwsSigner {
        fn verifying_key(&self) -> VerifyingKey {
            self.signer
                .get_pubkey()
                .to_sync(self.handle.clone())
                .expect("could not get verifying key from AWS signer")
        }
    }

    trait ToSync: Future + Sized {
        fn to_sync(self, handle: tokio::runtime::Handle) -> <Self as Future>::Output {
            tokio::task::block_in_place(|| handle.block_on(self))
        }
    }

    impl<T: Future> ToSync for T {}
}
