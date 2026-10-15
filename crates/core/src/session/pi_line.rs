//! One entry of a Pi session file, read into types rather than a
//! [`serde_json::Value`].
//!
//! The same approach as [`super::claude_line`] and [`super::codex_line`]:
//! strings borrowed from the line, a tool call's arguments kept as the text
//! they were written as until a reader asks, and a field of the wrong type
//! read as absent rather than losing the entry.
//!
//! What it adds over the `Value` walk it replaces is that nothing it reads is
//! large. The words of a turn and a tool's output are never fields here, so an
//! entry over the line cap — a tool result that read a whole file, an image —
//! is still read for the usage, the outcome and the stamp it carries, where the
//! `Value` reader had to drop it.

use super::lenient::{Lenient, Object, Raw, Shape, Str};
use serde::Deserialize;
use serde::de::IgnoredAny;
use std::path::Path;

/// One line of a Pi session: a `type`, a stamp, and the fields that type
/// carries.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Entry<'a> {
    #[serde(rename = "type", borrow)]
    pub kind: Str<'a>,
    /// `session`: the session's own id. Every other entry's `id` is the
    /// entry's, which no reader here wants.
    #[serde(borrow)]
    pub id: Str<'a>,
    /// ISO-8601, stamped as the entry was opened.
    #[serde(borrow)]
    pub timestamp: Str<'a>,
    /// `session`: the directory it ran in.
    #[serde(borrow)]
    pub cwd: Str<'a>,
    /// `session_info`: the name the person gave the session.
    #[serde(borrow)]
    pub name: Str<'a>,
    /// `thinking_level_change`: how hard the model is now asked to think.
    #[serde(rename = "thinkingLevel", borrow)]
    pub thinking_level: Str<'a>,
    #[serde(borrow)]
    pub message: Object<Message<'a>>,
}

/// A `message` entry's message: a person's turn, a model's, or a tool's
/// result.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Message<'a> {
    #[serde(borrow)]
    pub role: Str<'a>,
    #[serde(borrow)]
    pub model: Str<'a>,
    /// Milliseconds, stamped as the message was sealed. See
    /// [`super::pi::message_ts`].
    pub timestamp: Lenient<i64>,
    pub usage: Object<Usage>,
    /// The blocks of a model's turn. A person's is a bare string, which reads
    /// as [`Shape::Text`] and is never looked at.
    #[serde(borrow)]
    pub content: Shape<'a, Block<'a>>,
    /// `toolResult`: the call this answers, and whether it failed.
    #[serde(rename = "toolCallId", borrow)]
    pub tool_call_id: Str<'a>,
    #[serde(rename = "isError")]
    pub is_error: Lenient<bool>,
}

/// One block of a model's turn. Only a call is read; text and thinking are
/// skipped by not having fields here.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Block<'a> {
    #[serde(rename = "type", borrow)]
    pub kind: Str<'a>,
    #[serde(borrow)]
    pub id: Str<'a>,
    #[serde(borrow)]
    pub name: Str<'a>,
    #[serde(borrow)]
    pub arguments: Raw<'a>,
}

/// One request's tokens, and what Pi priced them at.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Usage {
    pub input: Lenient<u64>,
    pub output: Lenient<u64>,
    #[serde(rename = "cacheRead")]
    pub cache_read: Lenient<u64>,
    #[serde(rename = "cacheWrite")]
    pub cache_write: Lenient<u64>,
    #[serde(rename = "cacheWrite1h")]
    pub cache_write_1h: Lenient<u64>,
    pub reasoning: Lenient<u64>,
    #[serde(rename = "totalTokens")]
    pub total_tokens: Lenient<u64>,
    pub cost: Object<Cost>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Cost {
    pub input: Lenient<f64>,
    pub output: Lenient<f64>,
    #[serde(rename = "cacheRead")]
    pub cache_read: Lenient<f64>,
    #[serde(rename = "cacheWrite")]
    pub cache_write: Lenient<f64>,
    pub total: Lenient<f64>,
}

impl Usage {
    /// What the request was given, cached or not: the window it filled.
    pub(crate) fn window(&self) -> u64 {
        [&self.input, &self.cache_read, &self.cache_write]
            .iter()
            .filter_map(|n| n.value())
            .sum()
    }
}

impl<'a> Message<'a> {
    /// The tool calls in a model's turn, in the order it made them.
    pub(crate) fn calls(&self) -> impl Iterator<Item = &Block<'a>> {
        self.content
            .list()
            .unwrap_or_default()
            .iter()
            .filter_map(Shape::object)
            .filter(|b| b.kind.is("toolCall"))
    }
}

impl<'a> Entry<'a> {
    /// Read one line, or `None` when it is not JSON. A line that is JSON but
    /// not an object reads as an empty entry, as in
    /// [`super::claude_line::Line::parse`].
    pub(crate) fn parse(bytes: &'a [u8]) -> Option<Entry<'a>> {
        if bytes.trim_ascii_start().first() != Some(&b'{') {
            serde_json::from_slice::<IgnoredAny>(bytes).ok()?;
            return Some(Entry::default());
        }
        serde_json::from_slice(bytes).ok()
    }

    /// The message, when this is a `message` entry that has one.
    pub(crate) fn message(&self) -> Option<&Message<'a>> {
        self.kind
            .is("message")
            .then(|| self.message.get())
            .flatten()
    }

    /// When the entry was written: the message's own millisecond stamp when it
    /// has one, the entry's ISO string otherwise. The same rule as
    /// [`super::pi::message_ts`], which reads it off a `Value`.
    pub(crate) fn ts(&self) -> String {
        self.message
            .get()
            .and_then(|m| m.timestamp.value())
            .map(crate::util::ms_to_rfc3339)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.timestamp.or_empty().to_string())
    }
}

/// Stream a session file one [`Entry`] at a time, oversized lines included —
/// nothing an entry is read into can be large.
pub(crate) fn for_each(path: &Path, mut f: impl FnMut(&Entry)) -> std::io::Result<()> {
    super::extract::for_each_line(path, |bytes, _oversized| {
        if let Some(entry) = Entry::parse(bytes) {
            f(&entry);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Entry<'_> {
        Entry::parse(json.as_bytes()).expect("an entry")
    }

    #[test]
    fn an_assistant_turn_reads_its_usage_and_calls() {
        let e = parse(
            r#"{"type":"message","id":"a1","timestamp":"2026-08-05T00:01:00Z","message":{"role":"assistant","model":"m","timestamp":1785888060000,"usage":{"input":10,"cacheRead":5,"cacheWrite":2,"output":3,"cost":{"total":0.5}},"content":[{"type":"thinking","thinking":"hm"},{"type":"text","text":"ok"},{"type":"toolCall","id":"c1","name":"read","arguments":{"path":"a"}}],"stopReason":"toolUse"}}"#,
        );
        let m = e.message().unwrap();
        assert!(m.role.is("assistant"));
        let usage = m.usage.get().unwrap();
        assert_eq!(usage.window(), 17);
        assert_eq!(usage.cost.get().unwrap().total.value(), Some(0.5));
        let calls: Vec<_> = m.calls().map(|c| c.name.or_empty()).collect();
        assert_eq!(calls, ["read"]);
        assert!(e.ts().starts_with("2026-08-05"));
    }

    #[test]
    fn a_tool_result_reads_its_outcome() {
        let e = parse(
            r#"{"type":"message","message":{"role":"toolResult","toolCallId":"c1","toolName":"bash","content":[{"type":"text","text":"boom"}],"isError":true}}"#,
        );
        let m = e.message().unwrap();
        assert_eq!(m.tool_call_id.get(), Some("c1"));
        assert_eq!(m.is_error.value(), Some(true));
    }

    #[test]
    fn a_wrong_typed_field_is_absent_and_the_entry_is_kept() {
        let e = parse(
            r#"{"type":"message","timestamp":7,"message":{"role":"assistant","model":["x"],"usage":"lots","timestamp":"soon","content":"just text","isError":"yes"}}"#,
        );
        assert_eq!(e.timestamp.get(), None);
        let m = e.message().unwrap();
        assert_eq!(m.model.get(), None);
        assert!(m.usage.get().is_none());
        assert_eq!(m.timestamp.value(), None);
        assert_eq!(m.is_error.value(), None);
        assert_eq!(m.calls().count(), 0);
        assert_eq!(e.ts(), "");
    }

    #[test]
    fn entries_it_does_not_know_still_parse() {
        let e = parse(r#"{"type":"branch_summary","fromId":"x","summary":"y","details":{}}"#);
        assert!(e.kind.is("branch_summary"));
        assert!(e.message().is_none());
        assert!(Entry::parse(b"[1,2]").is_some());
        assert!(Entry::parse(b"{not json").is_none());
    }
}
