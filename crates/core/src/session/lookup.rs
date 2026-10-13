//! Which session a typed id names.
//!
//! Every face that takes a session id from a person — the CLI's detail flags,
//! `cctop why`, `cctop wait`, `recall --read`, the MCP tools and the web
//! server's routes — answers it here, so they agree on what a prefix means.
//! They used to each have their own, and they disagreed on the one case that
//! matters: a full id that is also the prefix of a longer one was found by the
//! server and called ambiguous by the CLI.

use super::Session;

/// What an id resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup<T> {
    /// One item, by an exact id or by a prefix only it has.
    Found(T),
    /// Nothing has that id or starts with it — including the empty string,
    /// which every caller means as "not given", never as "match anything".
    None,
    /// A prefix several ids share and none of them equals. All of them, in the
    /// order given, because a prefix is only ambiguous relative to ids the
    /// person cannot see from where they typed it, and listing them is what
    /// makes the refusal actionable.
    Ambiguous(Vec<T>),
}

impl<T> Lookup<T> {
    pub fn found(self) -> Option<T> {
        match self {
            Lookup::Found(t) => Some(t),
            _ => None,
        }
    }
}

/// Resolve `id` among `items`, reading each one's id with `key`.
///
/// An exact match wins before prefixes are considered: ids are not all the
/// same length (`_pid_` placeholders, Codex's, Cursor's), so a complete id can
/// be the prefix of another, and pasting the whole of one must never be
/// refused as ambiguous. Otherwise a prefix exactly one id has finds it, and
/// one several share finds none of them — the wrong session's answer is worse
/// than no answer.
pub fn lookup_by<T>(
    items: impl IntoIterator<Item = T>,
    id: &str,
    key: impl Fn(&T) -> &str,
) -> Lookup<T> {
    if id.is_empty() {
        return Lookup::None;
    }
    let mut matching = Vec::new();
    for item in items {
        let candidate = key(&item);
        if candidate == id {
            return Lookup::Found(item);
        }
        if candidate.starts_with(id) {
            matching.push(item);
        }
    }
    if matching.len() > 1 {
        return Lookup::Ambiguous(matching);
    }
    matching.pop().map_or(Lookup::None, Lookup::Found)
}

/// [`lookup_by`] over sessions, by their session id.
pub fn lookup<'a>(
    sessions: impl IntoIterator<Item = &'a Session>,
    id: &str,
) -> Lookup<&'a Session> {
    lookup_by(sessions, id, |s| s.session_id.as_str())
}

/// Ambiguous candidates as indented lines, `  <id> (<provider>)`, for an error
/// that tells the person which to pick between.
pub fn candidate_lines(many: &[&Session]) -> String {
    many.iter()
        .map(|s| format!("  {} ({})", s.session_id, s.provider.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A session id short enough to read and still unique in practice: the first
/// eight characters, which for a uuid is its whole first group.
pub fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;

    fn session(id: &str) -> Session {
        Session::new(Provider::Claude, id.into())
    }

    fn ids(found: Lookup<&Session>) -> Lookup<&str> {
        match found {
            Lookup::Found(s) => Lookup::Found(s.session_id.as_str()),
            Lookup::None => Lookup::None,
            Lookup::Ambiguous(many) => {
                Lookup::Ambiguous(many.iter().map(|s| s.session_id.as_str()).collect())
            }
        }
    }

    #[test]
    fn an_exact_id_beats_a_longer_id_it_prefixes() {
        // Either order: the exact match must not depend on coming first.
        for rows in [
            vec![session("abc"), session("abcdef")],
            vec![session("abcdef"), session("abc")],
        ] {
            assert_eq!(ids(lookup(&rows, "abc")), Lookup::Found("abc"));
            assert_eq!(ids(lookup(&rows, "abcd")), Lookup::Found("abcdef"));
        }
    }

    #[test]
    fn a_shared_prefix_lists_every_candidate() {
        let rows = vec![session("ab12"), session("zz"), session("ab34")];
        assert_eq!(
            ids(lookup(&rows, "ab")),
            Lookup::Ambiguous(vec!["ab12", "ab34"])
        );
        assert_eq!(ids(lookup(&rows, "ab3")), Lookup::Found("ab34"));
        assert_eq!(
            candidate_lines(&[&rows[0], &rows[2]]),
            "  ab12 (claude)\n  ab34 (claude)"
        );
    }

    #[test]
    fn nothing_and_the_empty_string_find_nothing() {
        let rows = vec![session("ab12")];
        assert_eq!(ids(lookup(&rows, "")), Lookup::None);
        assert_eq!(ids(lookup(&rows, "nope")), Lookup::None);
    }

    #[test]
    fn short_takes_eight_characters_when_there_are_eight() {
        assert_eq!(short("0123456789"), "01234567");
        assert_eq!(short("abc"), "abc");
    }
}
