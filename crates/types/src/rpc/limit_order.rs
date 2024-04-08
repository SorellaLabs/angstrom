use std::collections::HashMap;

use alloy_primitives::{Address, TxHash, U256};
use alloy_rlp::Decodable;
use alloy_rlp_derive::{RlpDecodable, RlpEncodable};
use alloy_sol_types::SolStruct;
use derive_more::{AsRef, Deref};
use secp256k1::Error as SigError;
use serde::{Deserialize, Serialize};

use crate::primitive::{ComposableOrder, Order, Signature, ANGSTROM_DOMAIN};

/// Submitted order pre-processing
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, RlpEncodable, RlpDecodable)]
pub struct SignedLimitOrder {
    /// Order hash
    pub hash:      TxHash,
    /// The original order from the user.
    pub order:     Order,
    /// The user's EIP-712 signature of the Order.
    pub signature: Signature
}

impl SignedLimitOrder {
    pub fn recover_signer(&self) -> Option<Address> {
        let hash = self.order.eip712_signing_hash(&ANGSTROM_DOMAIN);
        self.signature.0.recover_signer(hash)
    }
}

impl TryInto<EcRecoveredLimitOrder> for SignedLimitOrder {
    type Error = SigError;

    fn try_into(self) -> Result<EcRecoveredLimitOrder, Self::Error> {
        let sig = self.recover_signer().ok_or(SigError::IncorrectSignature)?;

        Ok(EcRecoveredLimitOrder { signer: sig, signed_order: self })
    }
}

/// Signed transaction with recovered signer.
#[derive(Debug, Clone, PartialEq, Hash, Eq, AsRef, Deref, RlpEncodable, RlpDecodable)]
pub struct EcRecoveredLimitOrder {
    /// Signer of the transaction
    pub signer:       Address,
    /// Signed transaction
    #[deref]
    #[as_ref]
    pub signed_order: SignedLimitOrder
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, RlpEncodable, RlpDecodable)]
pub struct SignedComposableLimitOrder {
    /// Order hash
    pub hash:      TxHash,
    /// The original order from the user.
    pub order:     ComposableOrder,
    /// The user's EIP-712 signature of the Order.
    pub signature: Signature
}

impl SignedComposableLimitOrder {
    pub fn recover_signer(&self) -> Option<Address> {
        let hash = self.order.eip712_signing_hash(&ANGSTROM_DOMAIN);
        self.signature.0.recover_signer(hash)
    }
}

impl TryInto<EcRecoveredComposableLimitOrder> for SignedComposableLimitOrder {
    type Error = SigError;

    fn try_into(self) -> Result<EcRecoveredComposableLimitOrder, Self::Error> {
        let sig = self.recover_signer().ok_or(SigError::IncorrectSignature)?;

        Ok(EcRecoveredComposableLimitOrder { signer: sig, signed_order: self })
    }
}

/// Signed transaction with recovered signer.
#[derive(Debug, Clone, PartialEq, Hash, Eq, AsRef, Deref, RlpDecodable, RlpEncodable)]
pub struct EcRecoveredComposableLimitOrder {
    /// Signer of the transaction
    pub signer:       Address,
    /// Signed transaction
    #[deref]
    #[as_ref]
    pub signed_order: SignedComposableLimitOrder
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CallerInfo {
    pub address:   Address,
    pub nonce:     u64,
    pub overrides: HashMap<Address, HashMap<U256, U256>>
}

impl TryFrom<alloy_primitives::Bytes> for SignedLimitOrder {
    type Error = alloy_rlp::Error;

    fn try_from(value: alloy_primitives::Bytes) -> Result<Self, Self::Error> {
        let veced = value.0.to_vec();
        let mut sliced = veced.as_slice();
        SignedLimitOrder::decode(&mut sliced)
    }
}

impl TryFrom<alloy_primitives::Bytes> for SignedComposableLimitOrder {
    type Error = alloy_rlp::Error;

    fn try_from(value: alloy_primitives::Bytes) -> Result<Self, Self::Error> {
        let veced = value.0.to_vec();
        let mut sliced = veced.as_slice();

        SignedComposableLimitOrder::decode(&mut sliced)
    }
}
