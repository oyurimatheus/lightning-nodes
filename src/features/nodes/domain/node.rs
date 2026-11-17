use chrono::{DateTime, Utc};

use crate::shared::error::AppError;

/// Satoshis per bitcoin, kept as an integer constant: all monetary math is
/// integer-only, never floating point.
pub const SATS_PER_BTC: i64 = 100_000_000;

/// A validated Lightning node belonging to one snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightningNode {
    pub public_key: String,
    pub alias: String,
    /// Always non-negative; validated against the source's unsigned range.
    pub capacity_sats: i64,
    pub first_seen: DateTime<Utc>,
}

impl LightningNode {
    pub fn new(
        public_key: impl Into<String>,
        alias: impl Into<String>,
        capacity_sats: u64,
        first_seen_unix_seconds: i64,
    ) -> Result<Self, AppError> {
        let public_key = public_key.into();
        if public_key.is_empty() {
            return Err(AppError::InvalidProviderPayload(
                "node public key must not be empty".to_owned(),
            ));
        }
        let capacity_sats = i64::try_from(capacity_sats).map_err(|_| {
            AppError::InvalidCapacity(format!(
                "capacity {capacity_sats} exceeds the signed BIGINT range"
            ))
        })?;
        Ok(Self {
            public_key,
            alias: alias.into(),
            capacity_sats,
            first_seen: from_unix_seconds(first_seen_unix_seconds)?,
        })
    }
}

/// Convert satoshis to BTC using integer arithmetic, rendered with exactly
/// eight fractional digits (e.g. 36_010_516_297 -> "360.10516297").
pub fn capacity_to_btc_string(sats: i64) -> String {
    debug_assert!(sats >= 0, "capacity is validated non-negative");
    let whole = sats / SATS_PER_BTC;
    let fraction = sats % SATS_PER_BTC;
    format!("{whole}.{fraction:08}")
}

/// Convert Unix seconds to an RFC 3339 UTC string with second precision and
/// `Z` suffix (e.g. 1522941222 -> "2018-04-05T15:13:42Z").
pub fn unix_seconds_to_rfc3339(seconds: i64) -> Result<String, AppError> {
    Ok(from_unix_seconds(seconds)
        .map_err(|e| AppError::InvalidTimestamp(e.to_string()))?
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string())
}

/// Convert Unix seconds to UTC. Zero and pre-epoch (negative) values are
/// valid; only values outside the representable calendar range fail.
pub fn from_unix_seconds(seconds: i64) -> Result<DateTime<Utc>, AppError> {
    DateTime::from_timestamp(seconds, 0).ok_or_else(|| {
        AppError::InvalidTimestamp(format!("unix timestamp {seconds} is out of range"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_zero() {
        assert_eq!(capacity_to_btc_string(0), "0.00000000");
    }

    #[test]
    fn capacity_one_satoshi() {
        assert_eq!(capacity_to_btc_string(1), "0.00000001");
    }

    #[test]
    fn capacity_small_with_leading_zeros() {
        assert_eq!(capacity_to_btc_string(550_000), "0.00550000");
        assert_eq!(capacity_to_btc_string(5_550), "0.00005550");
        assert_eq!(capacity_to_btc_string(555), "0.00000555");
    }

    #[test]
    fn capacity_one_btc() {
        assert_eq!(capacity_to_btc_string(100_000_000), "1.00000000");
    }

    #[test]
    fn capacity_spec_example() {
        assert_eq!(capacity_to_btc_string(36_010_516_297), "360.10516297");
    }

    #[test]
    fn capacity_very_large_valid_value() {
        assert_eq!(capacity_to_btc_string(i64::MAX), "92233720368.54775807");
    }

    #[test]
    fn known_timestamp_conversion() {
        assert_eq!(
            unix_seconds_to_rfc3339(1522941222).unwrap(),
            "2018-04-05T15:13:42Z"
        );
    }

    #[test]
    fn zero_timestamp_is_valid() {
        assert_eq!(unix_seconds_to_rfc3339(0).unwrap(), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn pre_epoch_timestamp_is_valid() {
        assert_eq!(
            unix_seconds_to_rfc3339(-86400).unwrap(),
            "1969-12-31T00:00:00Z"
        );
    }

    #[test]
    fn out_of_range_timestamp_is_invalid() {
        assert!(unix_seconds_to_rfc3339(i64::MAX).is_err());
    }

    #[test]
    fn timestamp_second_precision() {
        // Fractional data is dropped; output always has second precision.
        assert_eq!(
            unix_seconds_to_rfc3339(1522941222).unwrap(),
            unix_seconds_to_rfc3339(1522941222).unwrap()
        );
    }

    #[test]
    fn valid_node_accepted() {
        let node = LightningNode::new("03864ef025", "ACINQ", 36_010_516_297, 1522941222).unwrap();
        assert_eq!(node.capacity_sats, 36_010_516_297);
        assert_eq!(node.first_seen.to_string(), "2018-04-05 15:13:42 UTC");
    }

    #[test]
    fn node_with_u64_capacity_overflow_rejected() {
        let err = LightningNode::new("pk", "alias", u64::MAX, 0).unwrap_err();
        assert!(matches!(err, AppError::InvalidCapacity(_)));
    }

    #[test]
    fn node_with_negative_timestamp_rejected_when_out_of_range() {
        assert!(LightningNode::new("pk", "alias", 1, i64::MIN).is_err());
    }

    #[test]
    fn node_with_empty_public_key_rejected() {
        let err = LightningNode::new("", "alias", 1, 0).unwrap_err();
        assert!(matches!(err, AppError::InvalidProviderPayload(_)));
    }
}
