use alloy_primitives::{Address, B256, Bytes};
use alloy_signer::Signature;
use pade::{PadeDecode, PadeEncode};
use serde::{Deserialize, Serialize};

use crate::primitive::{AngstromMetaSigner, AngstromSigner};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CancelOrderRequest {
    /// the signature encoded v,r,s in bytes.
    pub signature:    Bytes,
    // if there's no salt to make this a unique signing hash. One can just
    // copy the signature of the order and id and it will verify
    pub user_address: Address,
    pub order_id:     B256
}

impl CancelOrderRequest {
    pub fn new<S: AngstromMetaSigner>(
        user_address: Address,
        order_id: B256,
        signer: &AngstromSigner<S>
    ) -> Self {
        let payload = format!("canceling order: {order_id:?} for user: {user_address:?}");
        let signature = signer.sign_message_sync(payload.as_bytes()).unwrap();
        let encoded: Bytes = signature.pade_encode().into();

        Self { signature: encoded, user_address, order_id }
    }

    fn signing_payload(&self) -> String {
        format!("canceling order: {:?} for user: {:?}", self.order_id, self.user_address)
    }

    pub fn is_valid(&self) -> bool {
        let msg = self.signing_payload();
        let signature = self.signature.to_vec();
        let slice = &mut signature.as_slice();
        let Ok(signature) = Signature::pade_decode(slice, None) else {
            return false;
        };

        let Ok(sender) = signature.recover_address_from_msg(msg) else { return false };

        sender == self.user_address
    }
}

#[cfg(test)]
mod test {
    use alloy_primitives::hex;

    use super::*;
    use crate::primitive::AngstromSigner;

    #[test]
    fn ensure_cancel_order_works() {
        let wallet = AngstromSigner::random();
        let user = wallet.address();
        let order_id = B256::random();

        let cancel_order = CancelOrderRequest::new(user, order_id, &wallet);

        assert!(cancel_order.is_valid());
    }

    #[test]
    fn test_for_frontend() {
        let order_id = alloy::primitives::b256!(
            "0xbfef52d152545f5576f577dfe6f42984658c60ee39bdbbfa7d075d96d40a26c7"
        );
        let address = alloy::primitives::address!("0xcc0bff7564a892045667a68673220116ece65d6f");
        let bytes:Bytes = hex!("0x1c92d4a408d7ba4e41d0d454826f9e981eafe07308af502f62981683ea8bf052a24ca3a7846222225dc2526616440129f1023e4b1ae27f8089c8cab1bd60297ea7").into();

        let cancel = CancelOrderRequest { signature: bytes, user_address: address, order_id };

        assert!(cancel.is_valid());
    }

    #[test]
    fn test_new_funcion() {
        let wallet = AngstromSigner::random();
        let order_id = alloy::primitives::b256!(
            "0xbfef52d152545f5576f577dfe6f42984658c60ee39bdbbfa7d075d96d40a26c7"
        );
        let request = CancelOrderRequest::new(wallet.address(), order_id, &wallet);

        assert!(request.is_valid())
    }
}
