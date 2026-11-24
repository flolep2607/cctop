//! Timestamps as the listers write them: RFC 3339, UTC.

use chrono::{DateTime, Utc};

/// An RFC 3339 timestamp, or `None` for an empty or malformed one.
pub fn parse_ts(value: &str) -> Option<DateTime<Utc>> {
    if value.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// Format epoch milliseconds as RFC-3339; empty string if out of range.
pub fn ms_to_rfc3339(ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339())
        .unwrap_or_default()
}
