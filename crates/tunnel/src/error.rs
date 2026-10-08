//! Why a tunnel did not come up, one variant per thing a caller would say
//! differently.
//!
//! None of these carries a credential. A token or a tunnel secret never goes
//! into an error string, because error strings are what reach a terminal, a
//! status line and the event log.

use thiserror::Error as ThisError;

/// Business-level error returned inside the trycloudflare API's body when
/// `success = false`. Mirrors cloudflared's `QuickTunnelError`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct QuickTunnelApiError {
    pub code: i32,
    pub message: String,
}

#[derive(ThisError, Debug)]
pub enum Error {
    #[error("quick-tunnel API request failed: {0}")]
    Api(#[from] reqwest::Error),

    #[error("quick-tunnel API returned business errors: {0:?}")]
    ApiBusiness(Vec<QuickTunnelApiError>),

    #[error("quick-tunnel API responded non-JSON ({status}): {body_snippet}")]
    ApiNonJson { status: u16, body_snippet: String },

    #[error("edge discovery failed: {0}")]
    Discovery(String),

    #[error("QUIC dial failed after {attempts} attempt(s); last: {last}")]
    QuicDial { attempts: usize, last: String },

    #[error("capnp-RPC RegisterConnection failed: {0}")]
    Register(String),

    /// The edge answered the registration and said no, for good: the tunnel
    /// was deleted, or its secret no longer matches. Retrying cannot help,
    /// which is what sets it apart from [`Error::Register`].
    #[error("the edge refused this tunnel: {0}")]
    Refused(String),

    /// A named tunnel came up but nothing said which hostname it serves: none
    /// was configured and the edge pushed no ingress rules in time.
    #[error("the tunnel registered, but no hostname was configured for it")]
    NoHostname,

    #[error("connection lost; supervisor giving up after {0} attempts")]
    PermanentFailure(u32),

    #[error("shutdown requested")]
    Shutdown,

    #[error("internal invariant violated: {0}")]
    Internal(String),
}
