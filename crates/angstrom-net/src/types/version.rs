//! Support for representing the version of the `eth`.
//! [`Capability`](crate::capability::Capability)
//! and [Protocol](crate::protocol::Protocol).

use std::str::FromStr;

/// Error thrown when failed to parse a valid [`StromVersion`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Unknown eth protocol version: {0}")]
pub struct ParseVersionError(String);

/// The `eth` protocol version.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum StromVersion {
    /// The `strom` protocol version 0
    Strom0 = 0
}

impl StromVersion {
    /// The latest known eth version
    pub const LATEST: StromVersion = StromVersion::Strom0;

    /// Returns the total number of messages the protocol version supports.
    pub const fn total_messages(&self) -> u8 {
        7
    }
}

/// Allow for converting from a `&str` to an `StromVersion`.
impl TryFrom<&str> for StromVersion {
    type Error = ParseVersionError;

    #[inline]
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "0" => Ok(StromVersion::Strom0),
            _ => Err(ParseVersionError(s.to_string()))
        }
    }
}

/// Allow for converting from a u8 to an `StromVersion`.
impl TryFrom<u8> for StromVersion {
    type Error = ParseVersionError;

    #[inline]
    fn try_from(u: u8) -> Result<Self, Self::Error> {
        match u {
            0 => Ok(StromVersion::Strom0),
            _ => Err(ParseVersionError(u.to_string()))
        }
    }
}

impl FromStr for StromVersion {
    type Err = ParseVersionError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        StromVersion::try_from(s)
    }
}

impl From<StromVersion> for u8 {
    #[inline]
    fn from(v: StromVersion) -> u8 {
        v as u8
    }
}

impl From<StromVersion> for &'static str {
    #[inline]
    fn from(v: StromVersion) -> &'static str {
        match v {
            StromVersion::Strom0 => "0"
        }
    }
}

#[cfg(test)]
mod test {
    use std::{convert::TryFrom, string::ToString};

    use super::{ParseVersionError, StromVersion};

    #[test]
    fn test_eth_version_try_from_str() {
        assert_eq!(StromVersion::Strom0, StromVersion::try_from("0").unwrap());
        assert_eq!(Err(ParseVersionError("69".to_string())), StromVersion::try_from("69"));
    }

    #[test]
    fn test_eth_version_from_str() {
        assert_eq!(StromVersion::Strom0, "0".parse().unwrap());
        assert_eq!(Err(ParseVersionError("69".to_string())), "69".parse::<StromVersion>());
    }
}
