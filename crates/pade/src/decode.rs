use std::fmt::Debug;

pub trait PadeDecode: super::PadeEncode {
    /// the var field should be None while calling this on any struct or enum.
    /// It is only here for dealing with the case where a struct contains enum
    /// fields. However this is delt with the decoding macro and thus should
    /// be ignored.
    fn pade_decode(buf: &mut &[u8], var: Option<u8>) -> Result<Self, PadeDecodeError>
    where
        Self: Sized;

    /// the varient that was used if enum.
    fn pade_decode_with_width(
        buf: &mut &[u8],
        width: usize,
        var: Option<u8>
    ) -> Result<Self, PadeDecodeError>
    where
        Self: Sized;
}

//Implementation for arrays
impl<T: PadeDecode + Debug, const N: usize> PadeDecode for [T; N] {
    fn pade_decode(buf: &mut &[u8], var: Option<u8>) -> Result<Self, PadeDecodeError> {
        let mut this = vec![];
        for _ in 0..N {
            this.push(T::pade_decode(buf, var)?);
        }

        Ok(this.try_into().unwrap())
    }

    fn pade_decode_with_width(
        buf: &mut &[u8],
        width: usize,
        var: Option<u8>
    ) -> Result<Self, PadeDecodeError> {
        let mut this = vec![];
        for _ in 0..N {
            this.push(T::pade_decode_with_width(buf, width, var)?);
        }

        Ok(this.try_into().unwrap())
    }
}

// Option<T: PadeEncode> encodes as an enum
impl<T: PadeDecode> PadeDecode for Option<T> {
    fn pade_decode(buf: &mut &[u8], var: Option<u8>) -> Result<Self, PadeDecodeError> {
        if buf.is_empty() {
            return Err(PadeDecodeError::InvalidSize)
        }
        // check first byte;
        let ctr = buf[0] != 0;
        // progress buffer
        *buf = &buf[1..];

        if ctr {
            Ok(Some(T::pade_decode(buf, var)?))
        } else {
            Ok(None)
        }
    }

    fn pade_decode_with_width(
        buf: &mut &[u8],
        width: usize,
        var: Option<u8>
    ) -> Result<Self, PadeDecodeError> {
        if buf.is_empty() {
            return Err(PadeDecodeError::InvalidSize)
        }
        // check first byte;
        let ctr = buf[0] != 0;
        // progress buffer
        *buf = &buf[1..];

        if ctr {
            Ok(Some(T::pade_decode_with_width(buf, width, var)?))
        } else {
            Ok(None)
        }
    }
}

impl PadeDecode for bool {
    fn pade_decode(buf: &mut &[u8], var: Option<u8>) -> Result<Self, PadeDecodeError> {
        if let Some(var) = var {
            return Ok(var != 0)
        }

        if buf.is_empty() {
            return Err(PadeDecodeError::InvalidSize)
        }
        // check first byte;
        let ctr = buf[0] != 0;
        // progress buffer
        *buf = &buf[1..];
        Ok(ctr)
    }

    fn pade_decode_with_width(
        _: &mut &[u8],
        _: usize,
        _: Option<u8>
    ) -> Result<Self, PadeDecodeError> {
        unreachable!()
    }
}

// Decided on a generic List<3> implementation - no variant bits because we
// don't want to hoist them in a struct
impl<T: PadeDecode> PadeDecode for Vec<T> {
    fn pade_decode(buf: &mut &[u8], var: Option<u8>) -> Result<Self, PadeDecodeError> {
        if buf.len() < 3 {
            return Err(PadeDecodeError::InvalidSize)
        }
        // read vec length.
        let length = &buf[0..3];
        let length = usize::from_be_bytes([0, 0, 0, 0, 0, length[0], length[1], length[2]]);

        // progress buf pass offset
        *buf = &buf[3..];
        // capture length to ensure we don't over decode.
        let mut decode_slice = &buf[0..length];
        let mut res = Vec::new();
        while let Ok(d) = T::pade_decode(&mut decode_slice, var) {
            res.push(d);
        }
        assert!(decode_slice.is_empty());

        // progress
        *buf = &buf[length..];

        Ok(res)
    }

    fn pade_decode_with_width(
        buf: &mut &[u8],
        width: usize,
        var: Option<u8>
    ) -> Result<Self, PadeDecodeError> {
        if buf.len() < 3 {
            return Err(PadeDecodeError::InvalidSize)
        }
        // read vec length.
        let length = &buf[0..3];
        let length = usize::from_be_bytes([0, 0, 0, 0, 0, length[0], length[1], length[2]]);

        // progress buf
        *buf = &buf[3..];

        let mut res = Vec::with_capacity(length);
        for _ in 0..length {
            res.push(T::pade_decode_with_width(buf, width, var)?);
        }

        Ok(res)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PadeDecodeError {
    #[error("not enough bytes remaining in the buffer")]
    InvalidSize,
    #[error("got a invalid enum variant: {0:?}")]
    InvalidEnumVariant(u8)
}

#[cfg(test)]
mod tests {

    use crate::PadeEncode;

    #[test]
    fn can_encode_decode_array() {
        let array = [100_u128, 300_u128, 256_u128];
        let bytes = array.pade_encode();
        assert!(array.pade_header_bits() == 0);
        assert!(array.pade_variant_map_bits() == 0);
        let mut slice = bytes.as_slice();

        let decoded: [u128; 3] = super::PadeDecode::pade_decode(&mut slice, None).unwrap();
        assert_eq!(array, decoded);
    }

    #[test]
    fn can_encode_decode_vec() {
        let vec = vec![100_u128, 300_u128, 256_u128];
        let bytes = vec.pade_encode();
        assert!(vec.pade_header_bits() == 24);
        assert!(vec.pade_variant_map_bits() == 0);
        let mut slice = bytes.as_slice();

        let decoded: Vec<u128> = super::PadeDecode::pade_decode(&mut slice, None).unwrap();
        assert_eq!(vec, decoded);
    }
}
