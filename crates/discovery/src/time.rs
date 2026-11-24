//! Timestamps as the listers write them: RFC 3339, UTC.

use chrono::{DateTime, Utc};

/// Format epoch milliseconds as RFC-3339; empty string if out of range.
pub fn ms_to_rfc3339(ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339())
        .unwrap_or_default()
}
