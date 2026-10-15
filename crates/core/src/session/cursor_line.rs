//! One line of a Cursor agent transcript, read into types rather than a
//! [`serde_json::Value`].
//!
//! The same approach as [`super::claude_line`]: a call's input kept as the
//! text it was written as until a reader asks, the words of a turn never read
//! at all, and a field of the wrong type read as absent rather than losing the
//! line. The format is Cursor's export, which carries a role and the blocks of
//! the message and nothing else — no model, no usage, no stamp — so that is all
//! there is to read.

use super::lenient::{Object, Raw, Shape, Str};
use serde::Deserialize;
use serde::de::IgnoredAny;
use std::path::Path;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Line<'a> {
    #[serde(borrow)]
    pub role: Str<'a>,
    #[serde(borrow)]
    pub message: Object<Message<'a>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Message<'a> {
    #[serde(borrow)]
    pub content: Shape<'a, Block<'a>>,
}

/// One block of a message. Only a `tool_use` is read; text is skipped by not
/// having a field here.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Block<'a> {
    #[serde(rename = "type", borrow)]
    pub kind: Str<'a>,
    #[serde(borrow)]
    pub name: Str<'a>,
    #[serde(borrow)]
    pub input: Raw<'a>,
}

impl<'a> Line<'a> {
    /// Read one line, or `None` when it is not JSON. See
    /// [`super::claude_line::Line::parse`].
    pub(crate) fn parse(bytes: &'a [u8]) -> Option<Line<'a>> {
        if bytes.trim_ascii_start().first() != Some(&b'{') {
            serde_json::from_slice::<IgnoredAny>(bytes).ok()?;
            return Some(Line::default());
        }
        serde_json::from_slice(bytes).ok()
    }

    /// The tool calls on this line, in the order they were made.
    pub(crate) fn calls(&self) -> impl Iterator<Item = &Block<'a>> {
        self.message
            .get()
            .and_then(|m| m.content.list())
            .unwrap_or_default()
            .iter()
            .filter_map(Shape::object)
            .filter(|b| b.kind.is("tool_use"))
    }
}

/// Stream a transcript one [`Line`] at a time, oversized lines included —
/// nothing a line is read into can be large.
pub(crate) fn for_each(path: &Path, mut f: impl FnMut(&Line)) -> std::io::Result<()> {
    super::extract::for_each_line(path, |bytes, _oversized| {
        if let Some(line) = Line::parse(bytes) {
            f(&line);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_read_and_text_is_skipped() {
        let line = Line::parse(
            br#"{"role":"assistant","message":{"content":[{"type":"text","text":"ok"},{"type":"tool_use","name":"Read","input":{"path":"a"}}]}}"#,
        )
        .unwrap();
        assert!(line.role.is("assistant"));
        let calls: Vec<_> = line.calls().collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name.get(), Some("Read"));
        assert_eq!(calls[0].input.value()["path"], "a");
    }

    #[test]
    fn a_wrong_typed_field_is_absent_and_the_line_is_kept() {
        let line =
            Line::parse(br#"{"role":3,"message":{"content":"plain words"},"extra":[1]}"#).unwrap();
        assert_eq!(line.role.get(), None);
        assert_eq!(line.calls().count(), 0);
        let line = Line::parse(br#"{"message":[1]}"#).unwrap();
        assert!(line.message.get().is_none());
        assert!(Line::parse(b"{broken").is_none());
    }
}
