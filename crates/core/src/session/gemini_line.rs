//! One record of a Gemini CLI chat, read into types rather than a
//! [`serde_json::Value`].
//!
//! The same approach as [`super::claude_line`] and [`super::codex_line`]:
//! strings are borrowed from the line, a call's arguments stay as the text they
//! were written as until a reader asks, and a field of the wrong type reads as
//! absent rather than losing the record. Gemini has already changed its chat
//! format once — a whole-file JSON object, then a JSONL log with `$set`
//! patches — so tolerance is the point, not a nicety.
//!
//! The conversation view still walks [`super::gemini::for_each_record`]'s
//! `Value`s: it renders every field a message has, which is the opposite of
//! what a typed reader buys. This is what the table's figures are read from.

use super::lenient::{Lenient, Object, Raw, Str};
use serde::Deserialize;
use serde::de::IgnoredAny;
use std::path::Path;

/// One record: a header, a message, or — inside `$set` — a patch to the
/// header. Every field of all three at once, each absent where it is not
/// carried.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Record<'a> {
    /// `user`, `gemini`, `info`, `error`, `warning`.
    #[serde(rename = "type", borrow)]
    pub kind: Str<'a>,
    /// The message's id. A message rewritten later under the same id is the
    /// same message, not a second one — see [`super::gemini::extract`].
    #[serde(borrow)]
    pub id: Str<'a>,
    #[serde(borrow)]
    pub timestamp: Str<'a>,
    #[serde(borrow)]
    pub model: Str<'a>,
    /// The topic Gemini's `update_topic` tool writes, on the header or a patch.
    #[serde(borrow)]
    pub summary: Str<'a>,
    pub tokens: Object<TokenCounts>,
    #[serde(rename = "toolCalls", borrow)]
    pub tool_calls: Lenient<Vec<Object<ToolCall<'a>>>>,
    /// A JSONL patch revising header fields.
    #[serde(rename = "$set", borrow)]
    pub set: Object<Box<Record<'a>>>,
    /// The older whole-file shape: every message, inside the header.
    #[serde(borrow)]
    pub messages: Lenient<Vec<Object<Record<'a>>>>,
}

/// A `gemini` message's token counts. See [`super::gemini`]'s `turn_tokens`
/// for how they add up.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct TokenCounts {
    pub input: Lenient<u64>,
    pub output: Lenient<u64>,
    pub cached: Lenient<u64>,
    pub thoughts: Lenient<u64>,
    pub tool: Lenient<u64>,
    pub total: Lenient<u64>,
}

/// One call a `gemini` message made, with its outcome recorded on it.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ToolCall<'a> {
    #[serde(borrow)]
    pub id: Str<'a>,
    #[serde(borrow)]
    pub name: Str<'a>,
    #[serde(borrow)]
    pub args: Raw<'a>,
    /// `success`, `error`, `cancelled`; absent while the call runs.
    #[serde(borrow)]
    pub status: Str<'a>,
    #[serde(borrow)]
    pub timestamp: Str<'a>,
}

impl<'a> Record<'a> {
    /// Read one record, or `None` when it is not JSON. A record that is JSON
    /// but not an object reads as empty, as [`super::claude_line::Line::parse`]
    /// explains.
    pub(crate) fn parse(bytes: &'a [u8]) -> Option<Record<'a>> {
        if bytes.trim_ascii_start().first() != Some(&b'{') {
            serde_json::from_slice::<IgnoredAny>(bytes).ok()?;
            return Some(Record::default());
        }
        serde_json::from_slice(bytes).ok()
    }

    /// The calls this message made, skipping any entry that is not an object.
    pub(crate) fn calls(&self) -> impl Iterator<Item = &ToolCall<'a>> {
        self.tool_calls
            .get()
            .into_iter()
            .flatten()
            .filter_map(Object::get)
    }
}

impl TokenCounts {
    pub(crate) fn count(field: &Lenient<u64>) -> u64 {
        field.value().unwrap_or(0)
    }
}

/// Walk a chat file's records in order, whichever shape it is written in —
/// the typed twin of [`super::gemini::for_each_record`], and unwrapping `$set`
/// patches the same way.
pub(crate) fn for_each(path: &Path, mut f: impl FnMut(&Record)) -> std::io::Result<()> {
    if path.extension().is_some_and(|ext| ext == "jsonl") {
        return super::extract::for_each_line(path, |bytes, oversized| {
            // An oversized line is a message whose tool output alone passes the
            // cap. The output is a field this reader skips without building, so
            // the turn's tokens and calls are kept — the `Value` reader lost
            // them — and only the arguments are dropped, since a whole-file
            // `write_file` is exactly what would be built into a `Value` next.
            if let Some(mut record) = Record::parse(bytes) {
                if oversized && let Some(calls) = record.tool_calls.0.as_mut() {
                    calls
                        .iter_mut()
                        .filter_map(|call| call.0.as_mut())
                        .for_each(|call| call.args = Raw::default());
                }
                match record.set.get() {
                    Some(patch) => f(patch),
                    None => f(&record),
                }
            }
        });
    }
    let text = std::fs::read(path)?;
    let root: Record = serde_json::from_slice(&text)?;
    f(&root);
    for message in root.messages.get().into_iter().flatten() {
        if let Some(message) = message.get() {
            f(message);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Record<'_> {
        Record::parse(json.as_bytes()).expect("a record")
    }

    #[test]
    fn a_reply_reads_whole() {
        let r = parse(
            r#"{"id":"m1","type":"gemini","timestamp":"t","model":"gemini-3-pro","tokens":{"input":10,"output":2,"cached":4,"thoughts":3,"tool":0,"total":15},"toolCalls":[{"id":"c1","name":"read_file","args":{"file_path":"a"},"status":"success","result":[{"big":"x"}]}],"thoughts":[{"subject":"s"}]}"#,
        );
        assert!(r.kind.is("gemini"));
        assert_eq!(r.id.get(), Some("m1"));
        let t = r.tokens.get().unwrap();
        assert_eq!(TokenCounts::count(&t.cached), 4);
        assert_eq!(TokenCounts::count(&t.total), 15);
        let calls: Vec<_> = r.calls().collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name.get(), Some("read_file"));
        assert!(calls[0].status.is("success"));
        assert_eq!(calls[0].args.value()["file_path"], "a");
    }

    #[test]
    fn a_patch_reads_as_a_record() {
        let r = parse(r#"{"$set":{"summary":"Topic","lastUpdated":"t"}}"#);
        assert_eq!(r.set.get().unwrap().summary.get(), Some("Topic"));
    }

    #[test]
    fn a_wrong_typed_field_is_absent_and_the_record_is_kept() {
        let r = parse(
            r#"{"type":"gemini","model":7,"tokens":"many","toolCalls":[3,{"name":"x","status":false}],"timestamp":"t"}"#,
        );
        assert_eq!(r.model.get(), None);
        assert!(r.tokens.get().is_none());
        assert_eq!(r.timestamp.get(), Some("t"));
        let calls: Vec<_> = r.calls().collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name.get(), Some("x"));
        assert_eq!(calls[0].status.get(), None);
    }

    #[test]
    fn not_an_object_is_empty_and_not_json_is_none() {
        assert!(parse("[1,2]").kind.get().is_none());
        assert!(Record::parse(b"{nope").is_none());
    }
}
