use alloy_primitives::{Address, TxHash};
use alloy_rlp::Decodable;
use alloy_rlp_derive::{RlpDecodable, RlpEncodable};
use alloy_sol_types::SolStruct;
use derive_more::{AsRef, Deref};
use secp256k1::Error as SigError;
use serde::{Deserialize, Serialize};

use crate::primitive::{Angstrom::Order, ComposableOrder, Signature, ANGSTROM_DOMAIN};

/// Submitted order pre-processing
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, RlpEncodable, RlpDecodable)]
pub struct SignedSearcherOrder {
    /// Order hash
    pub hash:      TxHash,
    /// The original order from the user.
    pub order:     Order,
    /// The user's EIP-712 signature of the Order.
    pub signature: Signature
}

//TODO: Also implement recovery for 1271 orders see
impl SignedSearcherOrder {
    pub fn recover_signer(&self) -> Option<Address> {
        let hash = self.order.eip712_signing_hash(&ANGSTROM_DOMAIN);
        self.signature.0.recover_signer(hash)
    }
}

impl TryInto<EcRecoveredSearcherOrder> for SignedSearcherOrder {
    type Error = SigError;

    fn try_into(self) -> Result<EcRecoveredSearcherOrder, Self::Error> {
        let sig = self.recover_signer().ok_or(SigError::IncorrectSignature)?;

        Ok(EcRecoveredSearcherOrder { signer: sig, signed_order: self })
    }
}

/// Signed transaction with recovered signer.
#[derive(Debug, Clone, PartialEq, Hash, Eq, AsRef, Deref, RlpEncodable, RlpDecodable)]
pub struct EcRecoveredSearcherOrder {
    /// Signer of the transaction
    pub signer:       Address,
    /// Signed transaction
    #[deref]
    #[as_ref]
    pub signed_order: SignedSearcherOrder
}

/// Submitted order pre-processing
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, RlpEncodable, RlpDecodable)]
pub struct SignedComposableSearcherOrder {
    /// Order hash
    pub hash:      TxHash,
    /// The original order from the user.
    pub order:     ComposableOrder,
    /// The user's EIP-712 signature of the Order.
    pub signature: Signature
}

impl SignedComposableSearcherOrder {
    pub fn recover_signer(&self) -> Option<Address> {
        let hash = self.order.eip712_signing_hash(&ANGSTROM_DOMAIN);
        self.signature.0.recover_signer(hash)
    }
}

impl TryInto<EcRecoveredComposableSearcherOrder> for SignedComposableSearcherOrder {
    type Error = SigError;

    fn try_into(self) -> Result<EcRecoveredComposableSearcherOrder, Self::Error> {
        let sender = self.recover_signer().ok_or(SigError::IncorrectSignature)?;

        Ok(EcRecoveredComposableSearcherOrder { signer: sender, signed_order: self })
    }
}

/// Signed transaction with recovered signer.
#[derive(Debug, Clone, PartialEq, Hash, Eq, AsRef, Deref, RlpDecodable, RlpEncodable)]
pub struct EcRecoveredComposableSearcherOrder {
    /// Signer of the transaction
    pub signer:       Address,
    /// Signed transaction
    #[deref]
    #[as_ref]
    pub signed_order: SignedComposableSearcherOrder
}

impl TryFrom<alloy_primitives::Bytes> for SignedComposableSearcherOrder {
    type Error = alloy_rlp::Error;

    fn try_from(value: alloy_primitives::Bytes) -> Result<Self, Self::Error> {
        let veced = value.0.to_vec();
        let mut sliced = veced.as_slice();

        SignedComposableSearcherOrder::decode(&mut sliced)
    }
}

impl TryFrom<alloy_primitives::Bytes> for SignedSearcherOrder {
    type Error = alloy_rlp::Error;

    fn try_from(value: alloy_primitives::Bytes) -> Result<Self, Self::Error> {
        let veced = value.0.to_vec();
        let mut sliced = veced.as_slice();

        SignedSearcherOrder::decode(&mut sliced)
    }
}
