//! One line of a Codex rollout, read into types rather than a
//! [`serde_json::Value`].
//!
//! The same approach as [`super::claude_line`], for the same reasons: strings
//! are borrowed from the line, the fields only some readers want — a call's
//! arguments, a tool's output — stay as the text they were written as until
//! one of them asks, and a field of the wrong type reads as absent rather than
//! losing the line. Codex changes this format between releases as freely as
//! Claude Code does.

use super::lenient::{Lenient, Object, Raw, Shape, Str};
use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::Value;
use std::path::Path;

/// One line of a rollout: a `type`, a `timestamp`, and a `payload` whose
/// fields depend on the type.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Line<'a> {
    #[serde(rename = "type", borrow)]
    pub kind: Str<'a>,
    #[serde(borrow)]
    pub timestamp: Str<'a>,
    #[serde(borrow)]
    pub payload: Object<Payload<'a>>,
}

/// Every payload's fields at once, each absent on the kinds that do not carry
/// it.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Payload<'a> {
    #[serde(rename = "type", borrow)]
    pub kind: Str<'a>,
    /// `session_meta`: the session's id, start and directory.
    #[serde(borrow)]
    pub id: Str<'a>,
    #[serde(borrow)]
    pub timestamp: Str<'a>,
    #[serde(borrow)]
    pub cwd: Str<'a>,
    /// `turn_context`: the model, and how hard it was asked to think, under
    /// either of the two names Codex has used.
    #[serde(borrow)]
    pub model: Str<'a>,
    #[serde(borrow)]
    pub effort: Str<'a>,
    #[serde(borrow)]
    pub reasoning_effort: Str<'a>,
    /// `user_message`: what the person typed.
    #[serde(borrow)]
    pub message: Str<'a>,
    /// `message`: who spoke, and the blocks they said.
    #[serde(borrow)]
    pub role: Str<'a>,
    #[serde(borrow)]
    pub content: Shape<'a, TextBlock<'a>>,
    /// `reasoning`: the summary Codex keeps of its thinking.
    #[serde(borrow)]
    pub summary: Shape<'a, TextBlock<'a>>,
    /// `token_count`.
    #[serde(borrow)]
    pub info: Object<Info<'a>>,
    /// A call and its output, paired by `call_id`.
    #[serde(borrow)]
    pub call_id: Str<'a>,
    #[serde(borrow)]
    pub name: Str<'a>,
    /// A JSON-encoded string on a `function_call`, `input` on a
    /// `custom_tool_call`, and a raw patch through either for `apply_patch`.
    #[serde(borrow)]
    pub arguments: Raw<'a>,
    #[serde(borrow)]
    pub input: Raw<'a>,
    #[serde(borrow)]
    pub output: Raw<'a>,
    /// `web_search_call`.
    #[serde(borrow)]
    pub action: Object<Action<'a>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct TextBlock<'a> {
    #[serde(borrow)]
    pub text: Str<'a>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Action<'a> {
    #[serde(borrow)]
    pub query: Str<'a>,
}

/// A `token_count` event's figures.
///
/// The two usages are kept as text: whether one is there at all, and whether
/// it is an object, both decide something, and the running total is compared
/// whole rather than read.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Info<'a> {
    #[serde(borrow)]
    pub last_token_usage: Raw<'a>,
    #[serde(borrow)]
    pub total_token_usage: Raw<'a>,
    pub model_context_window: Lenient<u64>,
}

/// One request's token counts.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct TokenUsage {
    pub input_tokens: Lenient<u64>,
    pub cached_input_tokens: Lenient<u64>,
    pub output_tokens: Lenient<u64>,
    pub reasoning_output_tokens: Lenient<u64>,
    pub total_tokens: Lenient<u64>,
}

impl<'a> Payload<'a> {
    /// The call's arguments field, whichever of its two names it came under.
    pub(crate) fn args_field(&self) -> Raw<'a> {
        match self.arguments.0 {
            Some(_) => self.arguments,
            None => self.input,
        }
    }

    /// A call's arguments: the string they were written as, when they were
    /// one, and their value — parsed from that string when it is JSON, `Null`
    /// when it is not (a raw patch), and the field itself when it was never a
    /// string.
    pub(crate) fn call_args(&self) -> (Option<Str<'a>>, Value) {
        let field = self.args_field();
        match field.read::<Str<'a>>().filter(|s| s.get().is_some()) {
            Some(text) => {
                let args = serde_json::from_str(text.or_empty()).unwrap_or(Value::Null);
                (Some(text), args)
            }
            None => (None, field.value()),
        }
    }

    /// The text of the payload's content blocks, the person's or the model's.
    pub(crate) fn texts(&self) -> impl Iterator<Item = &str> {
        self.content
            .list()
            .unwrap_or_default()
            .iter()
            .filter_map(Shape::object)
            .filter_map(|b| b.text.get())
    }
}

impl<'a> Line<'a> {
    /// Read one line, or `None` when it is not JSON. See
    /// [`super::claude_line::Line::parse`] for why a line that is JSON but not
    /// an object reads as empty rather than through the derive.
    pub(crate) fn parse(bytes: &'a [u8]) -> Option<Line<'a>> {
        if bytes.trim_ascii_start().first() != Some(&b'{') {
            serde_json::from_slice::<IgnoredAny>(bytes).ok()?;
            return Some(Line::default());
        }
        serde_json::from_slice(bytes).ok()
    }

    /// The kind that decides what a payload is: its own `type` when the line
    /// is a `response_item` wrapping it, and the line's otherwise.
    pub(crate) fn effective_kind(&self) -> &str {
        match self.kind.get() {
            Some("response_item") => self.payload.get().map_or("", |p| p.kind.or_empty()),
            other => other.unwrap_or(""),
        }
    }
}

/// Stream a rollout one [`Line`] at a time.
///
/// A line over the size cap is skipped, as the `Value` reader skipped it: a
/// Codex line carries no `message.usage` for the slim form to salvage.
pub(crate) fn for_each(path: &Path, mut f: impl FnMut(&Line)) -> std::io::Result<()> {
    super::extract::for_each_line(path, |bytes, oversized| {
        if oversized {
            return;
        }
        if let Some(line) = Line::parse(bytes) {
            f(&line);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Line<'_> {
        Line::parse(json.as_bytes()).expect("a line")
    }

    #[test]
    fn a_token_count_reads_whole() {
        let line = parse(
            r#"{"timestamp":"t","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":9},"last_token_usage":{"input_tokens":5,"cached_input_tokens":2,"output_tokens":1},"model_context_window":1000}}}"#,
        );
        assert_eq!(line.effective_kind(), "event_msg");
        let p = line.payload.get().unwrap();
        assert!(p.kind.is("token_count"));
        let info = p.info.get().unwrap();
        assert_eq!(info.model_context_window.value(), Some(1000));
        let last: Object<TokenUsage> = info.last_token_usage.read().unwrap();
        assert_eq!(last.get().unwrap().input_tokens.value(), Some(5));
        assert_eq!(info.total_token_usage.value()["input_tokens"], 9);
    }

    #[test]
    fn a_wrapped_call_takes_its_kind_from_the_payload() {
        let line = parse(
            r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","call_id":"c1","arguments":"{\"command\":[\"ls\"]}"}}"#,
        );
        assert_eq!(line.effective_kind(), "function_call");
        let p = line.payload.get().unwrap();
        assert_eq!(p.name.get(), Some("shell"));
        let args = p.args_field().read::<Str>().unwrap();
        assert_eq!(args.get(), Some(r#"{"command":["ls"]}"#));
    }

    #[test]
    fn extra_fields_change_nothing() {
        let line = parse(
            r#"{"type":"turn_context","new_top":[1],"payload":{"model":"gpt-6","sandbox_policy":{"mode":"x"},"effort":"high"}}"#,
        );
        let p = line.payload.get().unwrap();
        assert_eq!(p.model.get(), Some("gpt-6"));
        assert_eq!(p.effort.get(), Some("high"));
    }

    #[test]
    fn a_wrong_typed_field_is_absent_and_the_line_is_kept() {
        let line = parse(
            r#"{"type":"response_item","timestamp":5,"payload":{"type":"message","role":["user"],"content":[{"text":7},{"text":"hi"},"bare"],"info":"none","action":[1],"call_id":{"x":1}}}"#,
        );
        assert_eq!(line.timestamp.get(), None);
        let p = line.payload.get().unwrap();
        assert_eq!(p.role.get(), None);
        assert!(p.info.get().is_none());
        assert!(p.action.get().is_none());
        assert_eq!(p.call_id.get(), None);
        assert_eq!(p.texts().collect::<Vec<_>>(), ["hi"]);
    }

    #[test]
    fn missing_fields_and_payloads_are_absent() {
        let line = parse(r#"{"type":"response_item"}"#);
        assert!(line.payload.get().is_none());
        assert_eq!(line.effective_kind(), "");
        let line = parse(r#"{"type":"event_msg","payload":"x"}"#);
        assert!(line.payload.get().is_none());
        let line = parse(r#"{"payload":{}}"#);
        let p = line.payload.get().unwrap();
        assert!(p.args_field().0.is_none());
        assert!(p.output.0.is_none());
    }
}
