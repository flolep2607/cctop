//! Reading a tunnel token.
//!
//! The token Cloudflare's dashboard shows for a tunnel — and the one
//! `cloudflared tunnel run --token` takes — is base64 of a small JSON object:
//!
//! ```text
//! {"a": "<account tag>", "t": "<tunnel id, a UUID>", "s": "<secret, itself base64>"}
//! ```
//!
//! with sometimes an `"e"` naming an endpoint, which nothing here needs. Those
//! three fields are exactly what the edge asks for on registration.
//!
//! Every error here is in the user's words and never repeats the input: the
//! input is a credential, and an error is something that gets printed.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use serde::Deserialize;
use uuid::Uuid;

use super::Credentials;

/// What every tunnel token starts with: base64 of `{"a":"`.
const TUNNEL_TOKEN_PREFIX: &str = "eyJhIjoi";

/// Why a pasted string is not a tunnel token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenError {
    /// Not base64, or base64 of something that is not the token's JSON.
    NotAToken,
    /// The JSON is there but lacks a field the edge needs.
    Missing(&'static str),
    /// The tunnel id is not a UUID.
    BadTunnelId,
    /// The secret is not base64, or is empty.
    BadSecret,
}

impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenError::NotAToken => write!(
                f,
                "that is not a tunnel token (a tunnel token is the long string \
                 the Cloudflare dashboard shows under the tunnel's install command)"
            ),
            TokenError::Missing(what) => {
                write!(
                    f,
                    "that tunnel token has no {what}; copy it again from the dashboard"
                )
            }
            TokenError::BadTunnelId => {
                write!(
                    f,
                    "that tunnel token's tunnel id is not valid; copy it again from the dashboard"
                )
            }
            TokenError::BadSecret => {
                write!(
                    f,
                    "that tunnel token's secret is not valid; copy it again from the dashboard"
                )
            }
        }
    }
}

impl std::error::Error for TokenError {}

/// Whether `pasted` has a tunnel token's shape — as opposed to an API token,
/// which is 40 characters of `[A-Za-z0-9_-]` and never starts with this.
///
/// Shape only: a string that passes may still fail [`decode`].
pub fn looks_like_tunnel_token(pasted: &str) -> bool {
    pasted.trim().starts_with(TUNNEL_TOKEN_PREFIX)
}

#[derive(Deserialize)]
struct Fields {
    a: Option<String>,
    t: Option<String>,
    s: Option<String>,
}

pub(super) fn decode(token: &str) -> Result<Credentials, TokenError> {
    let json = decode_base64(token.trim()).ok_or(TokenError::NotAToken)?;
    let fields: Fields = serde_json::from_slice(&json).map_err(|_| TokenError::NotAToken)?;
    let account_tag = fields
        .a
        .filter(|a| !a.is_empty())
        .ok_or(TokenError::Missing("account tag"))?;
    let tunnel_id = fields
        .t
        .filter(|t| !t.is_empty())
        .ok_or(TokenError::Missing("tunnel id"))?;
    let tunnel_id = Uuid::parse_str(&tunnel_id).map_err(|_| TokenError::BadTunnelId)?;
    let secret = fields
        .s
        .filter(|s| !s.is_empty())
        .ok_or(TokenError::Missing("secret"))?;
    let secret = decode_base64(&secret)
        .filter(|s| !s.is_empty())
        .ok_or(TokenError::BadSecret)?;
    Ok(Credentials {
        account_tag,
        tunnel_id,
        secret,
    })
}

/// Base64 in whichever alphabet and padding it arrived in. Dashboard tokens
/// are standard and padded, but a token that went through a URL or a shell
/// may have lost its padding or been made URL-safe, and the bytes are the same.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    [STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD]
        .iter()
        .find_map(|engine| engine.decode(text).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up token: base64 of JSON written here, naming nobody's tunnel.
    fn token(json: &str) -> String {
        STANDARD.encode(json)
    }

    const SECRET_B64: &str = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA=";
    const TUNNEL: &str = "6ff42ae2-765d-4adf-8112-31c55c1551ef";

    fn good() -> String {
        token(&format!(
            r#"{{"a":"0123456789abcdef","t":"{TUNNEL}","s":"{SECRET_B64}"}}"#
        ))
    }

    #[test]
    fn a_valid_token_gives_the_three_credentials() {
        let creds = decode(&good()).unwrap();
        assert_eq!(creds.account_tag, "0123456789abcdef");
        assert_eq!(creds.tunnel_id.to_string(), TUNNEL);
        assert_eq!(creds.secret.len(), 32);
        assert_eq!(creds.secret[..4], [1, 2, 3, 4]);
    }

    #[test]
    fn surrounding_whitespace_and_lost_padding_are_forgiven() {
        let padded = good();
        let unpadded = padded.trim_end_matches('=');
        assert!(decode(&format!("  {unpadded}\n")).is_ok());
    }

    #[test]
    fn an_endpoint_field_is_ignored() {
        let t = token(&format!(
            r#"{{"a":"acct","t":"{TUNNEL}","s":"{SECRET_B64}","e":"somewhere"}}"#
        ));
        assert!(decode(&t).is_ok());
    }

    #[test]
    fn every_way_of_being_wrong_has_its_own_answer() {
        assert_eq!(decode("not base64 at all!"), Err(TokenError::NotAToken));
        assert_eq!(
            decode(&token("plainly not json")),
            Err(TokenError::NotAToken)
        );
        assert_eq!(
            decode(&token(&format!(r#"{{"t":"{TUNNEL}","s":"{SECRET_B64}"}}"#))),
            Err(TokenError::Missing("account tag"))
        );
        assert_eq!(
            decode(&token(&format!(r#"{{"a":"x","s":"{SECRET_B64}"}}"#))),
            Err(TokenError::Missing("tunnel id"))
        );
        assert_eq!(
            decode(&token(&format!(r#"{{"a":"x","t":"{TUNNEL}"}}"#))),
            Err(TokenError::Missing("secret"))
        );
        assert_eq!(
            decode(&token(&format!(
                r#"{{"a":"x","t":"not-a-uuid","s":"{SECRET_B64}"}}"#
            ))),
            Err(TokenError::BadTunnelId)
        );
        assert_eq!(
            decode(&token(&format!(r#"{{"a":"x","t":"{TUNNEL}","s":"!!!"}}"#))),
            Err(TokenError::BadSecret)
        );
    }

    #[test]
    fn no_error_repeats_what_was_pasted() {
        let inputs = [
            "sekrit-sekrit-sekrit".to_string(),
            token(r#"{"a":"sekrit-account"}"#),
            token(r#"{"a":"sekrit-account","t":"sekrit-tunnel","s":"c2Vrcml0"}"#),
            token(&format!(
                r#"{{"a":"sekrit-account","t":"{TUNNEL}","s":"sekrit!!"}}"#
            )),
        ];
        for input in inputs {
            let message = decode(&input).unwrap_err().to_string();
            assert!(!message.contains("sekrit"), "{message}");
            assert!(!message.contains(&input), "{message}");
        }
    }

    #[test]
    fn a_tunnel_token_is_told_from_an_api_token_by_its_shape() {
        assert!(looks_like_tunnel_token(&good()));
        assert!(looks_like_tunnel_token(&format!("  {}", good())));
        // An API token's shape: 40 characters of a URL-safe alphabet.
        assert!(!looks_like_tunnel_token(
            "Abcdefghij0123456789_-Abcdefghij01234567"
        ));
        assert!(!looks_like_tunnel_token(""));
    }

    #[test]
    fn credentials_debug_prints_no_secret() {
        let creds = decode(&good()).unwrap();
        let shown = format!("{creds:?}");
        assert!(!shown.contains("0123456789abcdef"), "{shown}");
        assert!(!shown.contains("[1, 2, 3"), "{shown}");
        assert!(shown.contains("[redacted]"), "{shown}");
    }
}
