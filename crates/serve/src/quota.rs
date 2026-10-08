//! The `/api/quota` document.
//!
//! The page's version of the terminal's limits panel: each account's status
//! and rate-limit windows, serialised once per poll rather than per request.
//! The usage endpoints throttle hard — a 30s poll once earned a sustained 429
//! — so the route serves whatever the last poll left behind, including an
//! empty pair of lists before the first one lands.

use crate::quota::{ProfileQuota, ProviderStatus, Quota};

/// The document `/api/quota` serves before the first poll has answered.
///
/// Valid and empty rather than pending: the accounts a machine has are
/// discovered as part of polling, so until one has run there is nothing to
/// say about them at all.
pub const EMPTY: &str = r#"{"claude":[],"codex":[]}"#;

/// Render `quota` into the shape the route answers.
///
/// `ProviderStatus` carries more than the wire wants — retry hints, error
/// strings, pending markers — so the mapping is deliberate rather than
/// derived: `status` is a fixed vocabulary the page can switch on, and
/// `detail` is the sentence that explains the ones that need explaining.
pub fn document(quota: &Quota) -> serde_json::Value {
    serde_json::json!({
        "claude": quota.claude.iter().map(profile).collect::<Vec<_>>(),
        "codex": quota.codex.iter().map(profile).collect::<Vec<_>>(),
    })
}

fn profile(p: &ProfileQuota) -> serde_json::Value {
    let (status, detail) = match &p.status {
        // A profile the poller has not reached yet. Real only in the gap
        // between startup and the first fetch, and on the dashboard feed.
        ProviderStatus::Pending => ("pending", "not checked yet".to_string()),
        ProviderStatus::Ok(_) => ("ok", String::new()),
        ProviderStatus::ApiBilling => (
            "api_billing",
            "an API key — usage is billed, not capped".to_string(),
        ),
        ProviderStatus::NotSignedIn => ("not_signed_in", "not signed in".to_string()),
        ProviderStatus::Expired => (
            "expired",
            "the sign-in has expired — log in again".to_string(),
        ),
        ProviderStatus::RateLimited { retry_at } => (
            "rate_limited",
            match retry_at.and_then(|at| chrono::DateTime::from_timestamp(at, 0)) {
                Some(at) => format!("rate limited until {} UTC", at.format("%H:%M")),
                None => "rate limited".to_string(),
            },
        ),
        ProviderStatus::Unavailable(why) => ("unavailable", why.clone()),
    };
    // The quota's `limit_reached` is per account — Claude's windows do not say
    // which one tripped — so it is copied onto each window rather than
    // inventing a per-window answer the provider never gave.
    let (plan, windows) = match &p.status {
        ProviderStatus::Ok(q) => (
            q.plan.clone(),
            q.windows
                .iter()
                .map(|w| {
                    serde_json::json!({
                        "label": w.label,
                        "pct": w.pct,
                        "resets_at": w.resets_at,
                        "limit_reached": q.limit_reached,
                    })
                })
                .collect::<Vec<_>>(),
        ),
        _ => (None, Vec::new()),
    };
    serde_json::json!({
        "profile": p.profile,
        "status": status,
        "detail": detail,
        "plan": plan,
        "windows": windows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::{ProviderQuota, Window};

    fn profile_quota(status: ProviderStatus) -> ProfileQuota {
        ProfileQuota {
            profile: "default".into(),
            status,
            source: crate::config::AccountSource::Directory,
        }
    }

    #[test]
    fn each_status_maps_to_a_wire_word_the_page_can_switch_on() {
        for (status, want) in [
            (ProviderStatus::Pending, "pending"),
            (ProviderStatus::Ok(ProviderQuota::default()), "ok"),
            (ProviderStatus::ApiBilling, "api_billing"),
            (ProviderStatus::NotSignedIn, "not_signed_in"),
            (ProviderStatus::Expired, "expired"),
            (
                ProviderStatus::RateLimited { retry_at: None },
                "rate_limited",
            ),
            (
                ProviderStatus::Unavailable("host said no".into()),
                "unavailable",
            ),
        ] {
            let doc = document(&Quota {
                fetched: true,
                claude: vec![profile_quota(status)],
                codex: Vec::new(),
            });
            assert_eq!(doc["claude"][0]["status"], want, "status {want}");
        }
    }

    #[test]
    fn an_ok_account_carries_its_windows_and_its_limit_flag() {
        let quota = Quota {
            fetched: true,
            claude: vec![profile_quota(ProviderStatus::Ok(ProviderQuota {
                plan: Some("max".into()),
                windows: vec![Window {
                    label: "5h",
                    pct: 62,
                    duration: None,
                    resets_at: Some(1_786_019_696),
                }],
                limit_reached: true,
            }))],
            codex: Vec::new(),
        };
        let doc = document(&quota);
        assert_eq!(doc["claude"][0]["plan"], "max");
        assert_eq!(doc["claude"][0]["windows"][0]["label"], "5h");
        assert_eq!(doc["claude"][0]["windows"][0]["pct"], 62);
        assert_eq!(doc["claude"][0]["windows"][0]["resets_at"], 1_786_019_696);
        // The account-level flag lands on the window, per the contract.
        assert_eq!(doc["claude"][0]["windows"][0]["limit_reached"], true);
    }

    #[test]
    fn a_failed_account_still_answers_with_an_empty_window_list() {
        let quota = Quota {
            fetched: true,
            claude: vec![profile_quota(ProviderStatus::Unavailable(
                "timed out".into(),
            ))],
            codex: Vec::new(),
        };
        let doc = document(&quota);
        assert_eq!(doc["claude"][0]["detail"], "timed out");
        assert_eq!(doc["claude"][0]["windows"], serde_json::json!([]));
        assert_eq!(doc["claude"][0]["plan"], serde_json::Value::Null);
    }
}
