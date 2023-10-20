use reth_primitives::{H256, H512};
use secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId},
    Message, SECP256K1
};

use crate::on_chain::Signature;

pub mod consensus;
pub mod database;
pub mod on_chain;

pub fn validate_signature(signature: &Signature, message: H256, public_key: H512) -> bool {
    // let signature: [u8; 64] = signature.0.to_vec().try_into().unwrap();
    // let rec_id: [u8; 4] = signature.recovery_id().unwrap().try_into().unwrap();
    //
    // let rec_id = i32::from_be_bytes(rec_id);
    // let sig = RecoverableSignature::from_compact(&signature,
    // RecoveryId::from_i32(rec_id).unwrap())     .unwrap();
    //
    // let msg: Message = Message::from_slice(&message.0).unwrap();
    // let recovered_pub_key = sig.recover(&msg).unwrap();
    // let pub_key: [u8; 64] = recovered_pub_key.serialize_uncompressed()[1..]
    //     .try_into()
    //     .unwrap();
    //
    // H512::from_slice(&pub_key) == public_key
    todo!()
}

pub fn get_public_key(check_signature: &Signature, message: H256) -> H512 {
    // let signature: [u8; 64] =
    // check_signature.0.to_vec().0.clone().try_into().unwrap(); let rec_id:
    // [u8; 4] = check_signature.recovery_id().unwrap().try_into().unwrap();
    //
    // let rec_id = i32::from_be_bytes(rec_id);
    // let sig = RecoverableSignature::from_compact(&signature,
    // RecoveryId::from_i32(rec_id).unwrap())     .unwrap();
    //
    // let msg: Message = Message::from_slice(&message.0).unwrap();
    // let recovered_pub_key = sig.recover(&msg).unwrap();
    // let pub_key: [u8; 64] = recovered_pub_key.serialize_uncompressed()[1..]
    //     .try_into()
    //     .unwrap();
    //
    // H512::from_slice(&pub_key)
    todo!()
}
