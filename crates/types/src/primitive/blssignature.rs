use std::ops::{BitAnd, BitOr};

use alloy_rlp::{length_of_length, Decodable, Encodable};
use anyhow::Error;
use bincode::{Decode, Encode};
use bitmaps::Bitmap;
use blsful::{
    inner_types::Field, Bls12381G1Impl, MultiPublicKey, MultiSignature, PublicKey, SecretKey,
    Signature, SignatureSchemes
};
use serde::{de::Visitor, Deserialize, Serialize};

/// BLS Validator ID is just going to be a u8 for now.  However, our bitmap is
/// only 127 wide so be careful
pub type BLSValidatorID = u8;

// We might want to parameterize this to allow for different bitmap sizes but
// for now we can probably just hardcode it
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct BLSSignature {
    /// Bitmap denoting which validators have signed this aggregate signature
    validators_included: Bitmap<128>,
    /// BLS aggregated signature data
    aggregate_signature: MultiSignature<Bls12381G1Impl>
}

impl BLSSignature {
    /// Create a new BLSSignature by signing the provided data with the provided
    /// secret key
    pub fn sign(validator_id: BLSValidatorID, sk: &SecretKey<Bls12381G1Impl>, data: &[u8]) -> Self {
        // Sign our data and store as a Multisig
        let signature = sk.sign(SignatureSchemes::ProofOfPossession, data).unwrap();
        let aggregate_signature: MultiSignature<Bls12381G1Impl> =
            MultiSignature::ProofOfPossession(*signature.as_raw_value());
        // Setup our bitmap for this validator
        let mut validators_included: Bitmap<128> = Bitmap::new();
        validators_included.set(validator_id as usize, true);
        Self { validators_included, aggregate_signature }
    }

    /// Sign the given data with the provided secret key and aggregate the
    /// resultant signature into this BLSSignature
    pub fn sign_into(
        &mut self,
        validator_id: BLSValidatorID,
        sk: &SecretKey<Bls12381G1Impl>,
        data: &[u8]
    ) -> bool {
        // Cannot sign anything with a zero key
        if sk.0.is_zero().into() {
            return false
        }
        let new_signature = sk.sign(SignatureSchemes::ProofOfPossession, data).unwrap();
        self.add_signature(validator_id, new_signature)
    }

    /// Validates the wrapped signature by using our validator bitmap to pull
    /// the appropriate public keys out of a supplied public key library.
    pub fn validate(&self, public_key_library: &[PublicKey<Bls12381G1Impl>], data: &[u8]) -> bool {
        // Use our bitmap to create a vector of appropriate public keys

        let included_public_keys: Vec<PublicKey<Bls12381G1Impl>> = self
            .validators_included
            .clone()
            .into_iter()
            .filter_map(|i| public_key_library.get(i))
            .cloned()
            .collect(); // Might be inefficient but let's not prematurely optimize
        let multikey = MultiPublicKey::from_public_keys(included_public_keys);
        // We're swallowing the error here and might want to record it, but a fail is a
        // fail
        self.aggregate_signature.verify(multikey, data).is_ok()
    }

    /// Returns the number of validators that have signed this message
    pub fn num_signed(&self) -> usize {
        self.validators_included.len()
    }

    /// Returns true if this signature claims to include the public key of the
    /// specified validator.  This does not inherently validate the
    /// signature so make sure to do that as well!
    pub fn signed_by(&self, validator_id: BLSValidatorID) -> bool {
        self.validators_included.get(validator_id as usize)
    }

    /// Get a reference to the validator bitmap for this signature
    pub fn validator_map(&self) -> &Bitmap<128> {
        &self.validators_included
    }

    /// Aggregate another `BLSSignature` into this one, modifying this signature
    /// in place.  This aggegration function will succeed and return `true`
    /// if both signatures are unique, i.e. no validator is included in both
    /// signatures.  If a validator is included in both signatures, the
    /// validator will return `false` and this signature will remain
    /// unaltered.
    pub fn simple_aggregate(&mut self, other: &Self) -> bool {
        // Make sure that we aren't doubling up on public keys
        let check = self.validators_included.bitand(other.validators_included);
        if check.first_index().is_some() {
            return false
        }

        // Merge the two signatures
        let Ok(new_sig) =
            MultiSignature::from_signatures([self.as_signature(), other.as_signature()])
        else {
            return false;
        };

        self.aggregate_signature = new_sig;
        self.validators_included = self.validators_included.bitor(other.validators_included);
        true
    }

    pub fn as_signature(&self) -> Signature<Bls12381G1Impl> {
        Signature::ProofOfPossession(*self.aggregate_signature.as_raw_value())
    }

    pub fn add_signature(
        &mut self,
        validator_id: BLSValidatorID,
        new_signature: Signature<Bls12381G1Impl>
    ) -> bool {
        // If our validator is already included, just return false
        if self.validators_included.get(validator_id.into()) {
            return false
        }
        // Otherwise note that we're included and aggregate the new signature
        let Ok(new_sig) = MultiSignature::from_signatures([self.as_signature(), new_signature])
        else {
            return false;
        };
        self.validators_included.set(validator_id.into(), true);
        self.aggregate_signature = new_sig;
        true
    }

    pub fn to_bytes(&self) -> [u8; 64] {
        let mut bytes: [u8; 64] = [0; 64];
        // Because of the size of our bitmap, validators_included is u128 (16 bytes).
        // Note that if we change the size of the bitmap we might wind up changing the
        // number of bytes written and expected here, which would cause a panic!
        bytes[0..16].copy_from_slice(&self.validators_included.as_value().to_be_bytes());
        // Compressed signature from a G1 type BLS sig is always 48 bytes, fills the
        // rest of our byte array
        bytes[16..].copy_from_slice(&self.aggregate_signature.as_raw_value().to_compressed());
        bytes
    }

    pub fn from_bytes(bytes: &[u8; 64]) -> Result<Self, Error> {
        // These should never fail since we're getting precisely 64 bytes in
        let (bitmap_slice, sig_slice) = bytes.split_at(16);
        let bitmap_bytes: [u8; 16] = bitmap_slice.try_into()?;
        let sig_bytes: [u8; 48] = sig_slice.try_into()?;

        // Parse out the u128 for our bitmap
        let bitmap_value = u128::from_be_bytes(bitmap_bytes);
        let validators_included = Bitmap::from_value(bitmap_value);
        // Parse the remaining bytes into the signature data
        let parsed_g1 = blsful::inner_types::G1Projective::from_compressed(&sig_bytes);
        if parsed_g1.is_none().into() {
            return Err(Error::msg("Unable to parse signature curve data"))
        }
        let aggregate_signature: MultiSignature<Bls12381G1Impl> =
            MultiSignature::ProofOfPossession(parsed_g1.unwrap());
        Ok(BLSSignature { validators_included, aggregate_signature })
    }
}

impl Serialize for BLSSignature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer
    {
        serializer.serialize_bytes(&self.to_bytes())
    }
}

/// Visitor for Serde deserialization of this value.  Right now we only have a
/// handler for `visit_seq` because `serde_json` encodes our bytes block into an
/// array of u8s.  It's possible that for different serialization and
/// deserialization mechanisms we'll have to expand this.
struct BLSSignatureVisitor {}
impl<'de> Visitor<'de> for BLSSignatureVisitor {
    type Value = BLSSignature;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("precisely 64 bytes")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>
    {
        let mut bytes: Vec<u8> = Vec::with_capacity(64);
        loop {
            match seq.next_element() {
                Ok(Some(x)) => bytes.push(x),
                Ok(None) => break,
                Err(_e) => return Err(serde::de::Error::custom("Invalid content in sequence"))
            }
        }
        if bytes.len() != 64 {
            return Err(serde::de::Error::invalid_length(bytes.len(), &"precisely 64 bytes"))
        }
        let b: [u8; 64] = bytes.try_into().unwrap(); // We just made sure this always works
        BLSSignature::from_bytes(&b)
            .map_err(|_| serde::de::Error::custom("Unable to parse signature"))
    }
}

impl<'de> Deserialize<'de> for BLSSignature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>
    {
        deserializer.deserialize_bytes(BLSSignatureVisitor {})
    }
}

// impl Encodable for BLSSignature {
//     fn encode(&self, out: &mut dyn bytes::BufMut) {
//         self.to_bytes().encode(out);
//     }
//
//     fn length(&self) -> usize {
//         // 64 bytes plus the length of the header
//         64 + length_of_length(64)
//     }
// }
//
// impl Decodable for BLSSignature {
//     fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
//         //let v = alloy_rlp::Header::decode_bytes(buf, false)?.to_vec();
//         // Pull both of our elements back out of the buf
//         let bytes = <[u8; 64]>::decode(buf)?;
//         let decoded = BLSSignature::from_bytes(&bytes)
//             .map_err(|_| alloy_rlp::Error::Custom("BLSSignature from_bytes
// error"))?;         Ok(decoded)
//     }
// }

#[cfg(test)]
mod tests {
    use blsful::{SecretKey, SignatureSchemes};
    use bytes::BytesMut;

    use super::*;

    /// Create an aggregate BLSSignature over arbitrary data that uses specific
    /// keys out of a larger list of keys
    fn sign_data_with(
        keys: &[SecretKey<Bls12381G1Impl>],
        idxs: &[usize],
        data: &[u8]
    ) -> BLSSignature {
        let mut validators_included = Bitmap::new();
        let sigs: Vec<Signature<Bls12381G1Impl>> = idxs
            .iter()
            .map(|i| {
                validators_included.set(*i, true);
                keys[*i]
                    .sign(SignatureSchemes::ProofOfPossession, data)
                    .unwrap()
            })
            .collect();
        let aggregate_signature = MultiSignature::from_signatures(&sigs).unwrap();
        BLSSignature { validators_included, aggregate_signature }
    }

    static FIXED_DATA: &str = "This is an arbitrary chunk of test data";

    #[test]
    fn will_sign_and_verify() {
        let mut sig = BLSSignature::default();
        let my_key = SecretKey::new();
        let my_signature = my_key
            .sign(SignatureSchemes::ProofOfPossession, FIXED_DATA.as_bytes())
            .unwrap();
        sig.add_signature(0, my_signature);
        assert!(
            sig.validate(&[my_key.public_key()], FIXED_DATA.as_bytes()),
            "Unable to validate signature"
        );
    }

    #[test]
    fn will_serialize_and_deserialize() {
        let mut sig = BLSSignature::default();
        let my_key = SecretKey::new();
        let my_signature = my_key
            .sign(SignatureSchemes::ProofOfPossession, FIXED_DATA.as_bytes())
            .unwrap();
        sig.add_signature(0, my_signature);
        // Serialize the structure
        let serialized = serde_json::to_string(&sig).unwrap();
        println!("{}", serialized);
        // Deserialize the structure
        let deserialized: BLSSignature = serde_json::from_str(&serialized).unwrap();
        // Compare that both the before and after are the same
        assert_eq!(
            sig.validators_included, deserialized.validators_included,
            "Aggregate signatures don't match"
        );
        assert_eq!(
            sig.aggregate_signature, deserialized.aggregate_signature,
            "Aggregate signatures don't match"
        );
        assert!(
            deserialized.validate(&[my_key.public_key()], FIXED_DATA.as_bytes()),
            "Rehydrated signature does not validate"
        )
    }

    // #[test]
    // fn will_do_rlp_encoding() {
    //     // Build and validate our sig
    //     let mut sig = BLSSignature::default();
    //     let my_key = SecretKey::new();
    //     let my_signature = my_key
    //         .sign(SignatureSchemes::ProofOfPossession, FIXED_DATA.as_bytes())
    //         .unwrap();
    //     sig.add_signature(0, my_signature);
    //     assert!(sig.validate(&[my_key.public_key()], FIXED_DATA.as_bytes()));
    //     // Encode and decode into a new sig
    //     let mut buf = BytesMut::with_capacity(64);
    //     sig.encode(&mut buf);
    //     let final_bytes = buf.freeze();
    //     let bytevec: Vec<u8> = final_bytes.iter().cloned().collect();
    //     let newsig = BLSSignature::decode(&mut bytevec.as_slice()).unwrap();
    //     // Validate the reconstituted sig
    //     assert!(newsig.validate(&[my_key.public_key()], FIXED_DATA.as_bytes()));
    // }

    #[test]
    fn will_perform_simple_aggregation() {
        let keys: Vec<SecretKey<Bls12381G1Impl>> = (0..10).map(|_| SecretKey::new()).collect();
        let pubkeys: Vec<PublicKey<Bls12381G1Impl>> = keys.iter().map(|x| x.public_key()).collect();
        let mut sig = sign_data_with(&keys, &[1, 3, 5], FIXED_DATA.as_bytes());
        let overlapping_sig = sign_data_with(&keys, &[2, 4, 5], FIXED_DATA.as_bytes());
        let nonoverlapping_sig = sign_data_with(&keys, &[2, 4, 6], FIXED_DATA.as_bytes());
        // Perform an overlapping aggregation, which should fail
        assert!(!sig.simple_aggregate(&overlapping_sig), "Overlapping aggregation permitted");
        // Assert that sig remains unchanged
        assert!(sig.validate(&pubkeys, FIXED_DATA.as_bytes()), "Unable to validate signature");
        // Only bytes 1, 3, 5 should be set and no others
        assert!(sig.validators_included.next_index(0).unwrap() == 1);
        assert!(sig.validators_included.next_index(1).unwrap() == 3);
        assert!(sig.validators_included.next_index(3).unwrap() == 5);
        assert!(sig.validators_included.next_index(5).is_none());
        // Perform a valid nonoverlapping aggregation
        assert!(sig.simple_aggregate(&nonoverlapping_sig), "Nonoverlapping aggregation denied");
        // Validate that sig has changed
        assert!(sig.validate(&pubkeys, FIXED_DATA.as_bytes()), "Unable to validate signature");
        // All 6 bytes should be set but no others
        assert!(sig.validators_included.next_index(0).unwrap() == 1);
        assert!(sig.validators_included.next_index(1).unwrap() == 2);
        assert!(sig.validators_included.next_index(2).unwrap() == 3);
        assert!(sig.validators_included.next_index(3).unwrap() == 4);
        assert!(sig.validators_included.next_index(4).unwrap() == 5);
        assert!(sig.validators_included.next_index(5).unwrap() == 6);
        assert!(sig.validators_included.next_index(6).is_none());
    }
}
