// use angstrom_types::sol_bindings::sol::{
//     SolMockContractMessage, SolPoolRewardsUpdate, SolRewardsUpdate
// };

mod primitives;
// Re-export bitvec so our macro crate can rely on it
pub use bitvec;

pub struct Sequence<const B: usize, T>(std::marker::PhantomData<T>);
impl<const B: usize, T> Sequence<B, T> {}

pub trait PadeEncode {
    const PADE_HEADER_BITS: usize = 0;
    const PADE_VARIANT_MAP_BITS: usize = 0;
    fn pade_encode(&self) -> Vec<u8>;

    fn pade_encode_with_width(&self, width: usize) -> Vec<u8> {
        let bytes = self.pade_encode();
        let encoded_len = bytes.len();
        match encoded_len.cmp(&width) {
            std::cmp::Ordering::Less => {
                let pad = width - encoded_len;
                std::iter::repeat(0_u8).take(pad).chain(bytes).collect()
            }
            std::cmp::Ordering::Equal => bytes,
            std::cmp::Ordering::Greater => {
                let skip = encoded_len - width;
                bytes.into_iter().skip(skip).collect()
            }
        }
    }
    /// The number of bytes in the PADE-encoded output that represent header
    /// information.  0 for most encoding schemes but Enum and List both
    /// have header metadata that are added
    #[inline]
    fn pade_header_bits(&self) -> usize {
        Self::PADE_HEADER_BITS
    }

    #[inline]
    fn pade_variant_map_bits(&self) -> usize {
        Self::PADE_VARIANT_MAP_BITS
    }
}

//Implementation for arrays
impl<T: PadeEncode, const N: usize> PadeEncode for [T; N] {
    fn pade_encode(&self) -> Vec<u8> {
        self.iter().flat_map(|item| item.pade_encode()).collect()
    }

    fn pade_encode_with_width(&self, width: usize) -> Vec<u8> {
        self.iter()
            .flat_map(|item| item.pade_encode_with_width(width))
            .collect()
    }
}

// Option<T: PadeEncode> encodes as an enum
impl<T: PadeEncode> PadeEncode for Option<T> {
    fn pade_encode(&self) -> Vec<u8> {
        match self {
            Some(v) => {
                std::iter::once(1_u8).chain(v.pade_encode().into_iter()).collect()
            },
            None => vec![0_u8]
        }
    }

    fn pade_encode_with_width(&self, width: usize) -> Vec<u8> {
        match self {
            Some(v) => {
                std::iter::once(1_u8).chain(v.pade_encode_with_width(width).into_iter()).collect()
            },
            None => vec![0_u8]
        }
    }
    
    #[inline]
    fn pade_variant_map_bits(&self) -> usize { 1 }
}

// Booleans encode as enums
impl PadeEncode for bool {
    fn pade_encode(&self) -> Vec<u8> {
        match self {
            true => vec![1_u8],
            false => vec![0_u8]
        }
    }

    #[inline]
    fn pade_variant_map_bits(&self) -> usize {
        1
    }
}
// Decided on a generic List<3> implementation - no variant bits because we
// don't want to hoist them in a struct
impl<T: PadeEncode> PadeEncode for Vec<T> {
    const PADE_HEADER_BITS: usize = 24;

    fn pade_encode(&self) -> Vec<u8> {
        let items: Vec<u8> = self.iter().flat_map(|i| i.pade_encode()).collect();
        let len_bytes = items.len().to_be_bytes();
        let len = vec![len_bytes[5], len_bytes[6], len_bytes[7]];
        [len, items].concat()
    }

    fn pade_encode_with_width(&self, width: usize) -> Vec<u8> {
        let items: Vec<u8> = self
            .iter()
            .flat_map(|i| i.pade_encode_with_width(width))
            .collect();
        let len_bytes = items.len().to_be_bytes();
        let len = vec![len_bytes[5], len_bytes[6], len_bytes[7]];
        [len, items].concat()
    }
}

#[cfg(test)]
mod tests {
    use crate::PadeEncode;

    #[test]
    fn can_encode_array() {
        let array = [100_u128, 300_u128, 256_u128];
        array.pade_encode();
        assert!(array.pade_header_bits() == 0);
        assert!(array.pade_variant_map_bits() == 0);
    }

    #[test]
    fn can_encode_vec() {
        let vec = vec![100_u128, 300_u128, 256_u128];
        vec.pade_encode();
        assert!(vec.pade_header_bits() == 24);
        assert!(vec.pade_variant_map_bits() == 0);
    }
}
