use bytes::BytesMut;
use reth_primitives::{H256, H512};
use reth_rlp::{Decodable, DecodeError, Encodable, RlpDecodable, RlpEncodable};
use secp256k1::PublicKey;
use serde::{Deserialize, Serialize};

use super::{Time, Valid23Bundle};
use crate::{
    database::{BlockCommit, BlockHeader},
    on_chain::{SimmedBundle, SubmissionBundle}
};

#[derive(Debug, Clone, Serialize, Deserialize, RlpDecodable, RlpEncodable, PartialEq, Eq, Hash)]
pub struct Block {
    pub header:        BlockHeader,
    pub submitted_tx:  H256,
    pub data:          SubmissionBundle,
    // TODO move struct
    pub evidence_data: Vec<u8>,
    pub last_commit:   BlockCommit
}
