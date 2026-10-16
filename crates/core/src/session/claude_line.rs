//! One line of a Claude Code transcript, read into types rather than a
//! [`serde_json::Value`].
//!
//! Every reader of a Claude transcript — the session list, the extractor, the
//! conversation view — reads this one model, so they agree on what a line
//! says. Three things it does that a `Value` did not:
//!
//! - **Strings are borrowed** from the line wherever the JSON has no escapes
//!   in them, rather than copied out one allocation per field.
//! - **The heavy fields are not built at all.** A tool's input, a tool
//!   result's content and an attachment are kept as the slice of the line they
//!   occupy ([`Raw`]) and read by whichever consumer needs them, in the shape
//!   it needs; a field nobody reads — the file an edit's `toolUseResult`
//!   carries whole, an image's base64 — is skipped without being allocated.
//! - **Keys are not stored.** A `Value` built with `preserve_order` keeps
//!   every object as an ordered map of owned keys.
//!
//! What it keeps from the `Value` walk is its tolerance, because Claude Code
//! changes this format without notice: unknown fields are ignored, a missing
//! one is absent, and one of the wrong type reads as absent too rather than
//! losing the line. See [`super::lenient`].

use super::lenient::{Lenient, Object, Raw, Shape, Str};
use serde::Deserialize;
use serde::de::IgnoredAny;
use std::path::Path;

/// One line of a Claude Code transcript.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Line<'a> {
    #[serde(rename = "type", borrow)]
    pub kind: Str<'a>,
    #[serde(borrow)]
    pub timestamp: Str<'a>,
    #[serde(borrow)]
    pub cwd: Str<'a>,
    /// The id on the command line that started the session, which is not this
    /// transcript's own once the session has been resumed.
    #[serde(borrow)]
    pub session_id: Str<'a>,
    #[serde(rename = "requestId", borrow)]
    pub request_id: Str<'a>,
    #[serde(rename = "isSidechain")]
    pub is_sidechain: Lenient<bool>,
    #[serde(rename = "isMeta")]
    pub is_meta: Lenient<bool>,
    #[serde(rename = "isCompactSummary")]
    pub is_compact_summary: Lenient<bool>,
    #[serde(borrow)]
    pub subtype: Str<'a>,
    #[serde(rename = "durationMs")]
    pub duration_ms: Lenient<u64>,
    #[serde(rename = "customTitle", borrow)]
    pub custom_title: Str<'a>,
    #[serde(rename = "aiTitle", borrow)]
    pub ai_title: Str<'a>,
    #[serde(borrow)]
    pub origin: Object<Origin<'a>>,
    #[serde(borrow)]
    pub message: Object<Message<'a>>,
    #[serde(rename = "toolUseResult", borrow)]
    pub tool_use_result: Object<ToolUseResult<'a>>,
    /// Read by [`Line::attachment`], and sized whole by the context meter.
    #[serde(borrow)]
    pub attachment: Raw<'a>,
}

/// Who wrote a prompt-shaped record. See [`crate::chat`]'s `author`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Origin<'a> {
    #[serde(borrow)]
    pub kind: Str<'a>,
    #[serde(borrow)]
    pub name: Str<'a>,
    #[serde(borrow)]
    pub from: Str<'a>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Message<'a> {
    #[serde(borrow)]
    pub id: Str<'a>,
    #[serde(borrow)]
    pub model: Str<'a>,
    /// A string for a typed prompt, a list of blocks for everything else.
    #[serde(borrow)]
    pub content: Shape<'a, Block<'a>>,
    pub usage: Object<Usage>,
}

/// One content block, of whichever kind: the fields of all of them, each
/// absent on the kinds that do not carry it.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Block<'a> {
    #[serde(rename = "type", borrow)]
    pub kind: Str<'a>,
    #[serde(borrow)]
    pub text: Str<'a>,
    #[serde(borrow)]
    pub thinking: Str<'a>,
    /// A `tool_use` block's tool, and its id.
    #[serde(borrow)]
    pub name: Str<'a>,
    #[serde(borrow)]
    pub id: Str<'a>,
    #[serde(borrow)]
    pub input: Raw<'a>,
    /// A `tool_result` block's call, and what came back from it.
    #[serde(borrow)]
    pub tool_use_id: Str<'a>,
    pub is_error: Lenient<bool>,
    #[serde(borrow)]
    pub content: Raw<'a>,
}

/// Token counts as the API reported them. A count that is not a whole
/// number reads as zero, as `as_u64` did.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Usage {
    pub input_tokens: Lenient<u64>,
    pub cache_read_input_tokens: Lenient<u64>,
    pub output_tokens: Lenient<u64>,
    pub cache_creation_input_tokens: Lenient<u64>,
    pub cache_creation: Object<CacheCreation>,
}

impl Usage {
    /// Everything the request held in its window, which is how full it was.
    pub(crate) fn window(&self) -> u64 {
        let n = |v: &Lenient<u64>| v.value().unwrap_or(0);
        n(&self.input_tokens)
            .saturating_add(n(&self.cache_read_input_tokens))
            .saturating_add(n(&self.cache_creation_input_tokens))
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct CacheCreation {
    pub ephemeral_5m_input_tokens: Lenient<u64>,
    pub ephemeral_1h_input_tokens: Lenient<u64>,
}

/// What an edit or a write reported back, beside its `tool_result`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ToolUseResult<'a> {
    #[serde(rename = "type", borrow)]
    pub kind: Str<'a>,
    /// The whole file a `create` wrote. Kept unread until it is known to be
    /// one: on most results it is a file's contents nobody here wants.
    #[serde(borrow)]
    pub content: Raw<'a>,
    #[serde(rename = "structuredPatch", borrow)]
    pub structured_patch: Lenient<Vec<Object<Hunk<'a>>>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Hunk<'a> {
    #[serde(borrow)]
    pub lines: Lenient<Vec<Str<'a>>>,
}

impl<'a> ToolUseResult<'a> {
    /// Every line of the reported patch, `+` and `-` prefixes included.
    ///
    /// `None` when there is no patch, which is not the same as an empty one:
    /// a reader falls back to the `create` case only for the first.
    pub(crate) fn patch_lines(&self) -> Option<impl Iterator<Item = &str>> {
        let hunks = self.structured_patch.get()?;
        Some(
            hunks
                .iter()
                .filter_map(Object::get)
                .filter_map(|hunk| hunk.lines.get())
                .flatten()
                .filter_map(Str::get),
        )
    }

    /// The file a `create` wrote, when this is one.
    pub(crate) fn created(&self) -> Option<Str<'a>> {
        if !self.kind.is("create") {
            return None;
        }
        self.content.read::<Str<'a>>()
    }
}

/// An `attachment` entry's own record: what the harness spliced in.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Attachment<'a> {
    #[serde(rename = "type", borrow)]
    pub kind: Str<'a>,
    /// A `queued_command`'s message: a string, or blocks when an image came
    /// with it.
    #[serde(borrow)]
    pub prompt: Shape<'a, Block<'a>>,
    #[serde(rename = "commandMode", borrow)]
    pub command_mode: Str<'a>,
    #[serde(borrow)]
    pub origin: Object<Origin<'a>>,
    #[serde(borrow)]
    pub content: Raw<'a>,
}

/// Whether a transcript line is one of the records Claude Code 2.1.300 began
/// writing beside each API request — `api-request`, and its `-blob` and
/// `-shape` — which hold the system prompt, the tools and the parameters, so a
/// shared transcript can replay the request.
///
/// Nothing cctop shows is in them, and they are most of the file: three
/// quarters of a long session's bytes, each line tens of kilobytes that a full
/// parse walks only to throw away. Claude Code writes `type` first, so the
/// prefix is the whole test; a line that put it elsewhere would still parse,
/// and be ignored by kind as before, only more slowly.
fn is_request_record(bytes: &[u8]) -> bool {
    bytes.starts_with(br#"{"type":"api-request"#)
}

impl<'a> Line<'a> {
    /// Read one line, or `None` when it is not JSON.
    ///
    /// JSON that is not an object is a line with nothing in it, as it was to
    /// the `Value` walk, and is checked for here: serde's derive would take a
    /// struct positionally from an array.
    pub(crate) fn parse(bytes: &'a [u8]) -> Option<Line<'a>> {
        if is_request_record(bytes) {
            return Some(Line::default());
        }
        if bytes.trim_ascii_start().first() != Some(&b'{') {
            serde_json::from_slice::<IgnoredAny>(bytes).ok()?;
            return Some(Line::default());
        }
        serde_json::from_slice(bytes).ok()
    }

    /// The billing fields of a line too large to read in full, or `None` when
    /// it has no usage to salvage.
    ///
    /// Everything else — the base64 image, the file body — is skipped as
    /// [`IgnoredAny`], which walks the JSON without allocating it. Dropping
    /// the line whole silently loses its tokens and their cost.
    pub(crate) fn slim(bytes: &'a [u8]) -> Option<Line<'a>> {
        if is_request_record(bytes) {
            return None;
        }
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct Slim<'b> {
            #[serde(rename = "type", borrow)]
            kind: Str<'b>,
            #[serde(borrow)]
            timestamp: Str<'b>,
            #[serde(rename = "requestId", borrow)]
            request_id: Str<'b>,
            #[serde(rename = "isSidechain")]
            is_sidechain: Lenient<bool>,
            #[serde(borrow)]
            message: Object<SlimMessage<'b>>,
        }
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct SlimMessage<'b> {
            #[serde(borrow)]
            id: Str<'b>,
            #[serde(borrow)]
            model: Str<'b>,
            usage: Object<Usage>,
        }
        let slim: Object<Slim> = serde_json::from_slice(bytes).ok()?;
        let slim = slim.0?;
        let message = slim.message.0?;
        message.usage.get()?;
        slim.kind.get()?;
        Some(Line {
            kind: slim.kind,
            timestamp: slim.timestamp,
            request_id: slim.request_id,
            is_sidechain: slim.is_sidechain,
            message: Object(Some(Message {
                id: message.id,
                model: message.model,
                usage: message.usage,
                ..Message::default()
            })),
            ..Line::default()
        })
    }

    /// Whether the entry was flagged as this, `true` and nothing else.
    pub(crate) fn sidechain(&self) -> bool {
        self.is_sidechain.value() == Some(true)
    }

    pub(crate) fn meta(&self) -> bool {
        self.is_meta.value() == Some(true)
    }

    pub(crate) fn compact_summary(&self) -> bool {
        self.is_compact_summary.value() == Some(true)
    }

    /// The message's content, `Other` when there is no message.
    pub(crate) fn content(&self) -> &Shape<'a, Block<'a>> {
        static NONE: Shape<'static, Block<'static>> = Shape::Other;
        self.message.get().map_or(&NONE, |m| &m.content)
    }

    /// The `attachment`, read as one.
    pub(crate) fn attachment(&self) -> Option<Attachment<'a>> {
        self.attachment.read::<Object<Attachment<'a>>>()?.0
    }
}

/// Stream a Claude transcript, one [`Line`] at a time.
///
/// Lines over the size cap are read as [`Line::slim`], and lines that are not
/// JSON — a truncated tail from a crashed writer — are skipped. See
/// [`super::extract::for_each_line`].
pub(crate) fn for_each(path: &Path, mut f: impl FnMut(&Line)) -> std::io::Result<()> {
    super::extract::for_each_line(path, |bytes, oversized| {
        let line = match oversized {
            true => Line::slim(bytes),
            false => Line::parse(bytes),
        };
        if let Some(line) = line {
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

    const ASSISTANT: &str = r#"{"type":"assistant","timestamp":"t1","requestId":"req_1","isSidechain":false,"message":{"id":"m1","model":"claude-opus-5","content":[{"type":"text","text":"hi"},{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/a"}}],"usage":{"input_tokens":10,"cache_read_input_tokens":20,"cache_creation_input_tokens":30,"output_tokens":4,"cache_creation":{"ephemeral_5m_input_tokens":30}}}}"#;

    #[test]
    fn an_assistant_line_reads_whole() {
        let line = parse(ASSISTANT);
        assert!(line.kind.is("assistant"));
        assert_eq!(line.request_id.get(), Some("req_1"));
        assert!(!line.sidechain());
        let message = line.message.get().unwrap();
        assert_eq!(message.model.get(), Some("claude-opus-5"));
        let usage = message.usage.get().unwrap();
        assert_eq!(usage.window(), 60);
        assert_eq!(
            usage
                .cache_creation
                .get()
                .and_then(|c| c.ephemeral_5m_input_tokens.value()),
            Some(30)
        );
        let blocks: Vec<_> = message
            .content
            .list()
            .unwrap()
            .iter()
            .filter_map(Shape::object)
            .collect();
        assert_eq!(blocks[0].text.get(), Some("hi"));
        assert_eq!(blocks[1].name.get(), Some("Read"));
        assert_eq!(blocks[1].input.value()["file_path"], "/a");
    }

    /// Fields Claude Code may add tomorrow, at every level of the line.
    #[test]
    fn extra_fields_change_nothing() {
        let line = parse(
            r#"{"type":"assistant","brandNew":{"x":[1,2]},"message":{"model":"m","extra":"y","content":[{"type":"text","text":"hi","citations":[]}],"usage":{"input_tokens":5,"server_tool_use":{"web_search_requests":1}}}}"#,
        );
        let message = line.message.get().unwrap();
        assert_eq!(message.model.get(), Some("m"));
        assert_eq!(message.usage.get().unwrap().window(), 5);
        assert_eq!(message.content.list().unwrap().len(), 1);
    }

    /// Each field holds a value of the wrong type. The line is still read and
    /// every field that is right is still there — which is what the `Value`
    /// walk gave, one `as_str` at a time.
    #[test]
    fn a_wrong_typed_field_is_absent_and_the_line_is_kept() {
        let line = parse(
            r#"{"type":"assistant","timestamp":7,"requestId":["r"],"isSidechain":"yes","isMeta":1,"origin":"human","toolUseResult":"Error: no","message":{"id":{"a":1},"model":"m","content":[{"type":"tool_use","name":5,"id":"t"}],"usage":{"input_tokens":"10","output_tokens":3,"cache_creation":[1]}}}"#,
        );
        assert!(line.kind.is("assistant"));
        assert_eq!(line.timestamp.get(), None);
        assert_eq!(line.request_id.get(), None);
        assert_eq!(line.is_sidechain.value(), None);
        assert!(!line.meta());
        assert!(line.origin.get().is_none());
        assert!(line.tool_use_result.get().is_none());
        let message = line.message.get().unwrap();
        assert_eq!(message.id.get(), None);
        assert_eq!(message.model.get(), Some("m"));
        let usage = message.usage.get().unwrap();
        assert_eq!(usage.input_tokens.value(), None);
        assert_eq!(usage.output_tokens.value(), Some(3));
        assert!(usage.cache_creation.get().is_none());
        let block = message.content.list().unwrap()[0].object().unwrap();
        assert_eq!(block.name.get(), None);
        assert_eq!(block.id.get(), Some("t"));
    }

    #[test]
    fn a_message_that_is_not_an_object_is_no_message() {
        for message in [r#""text""#, "[1,2]", "null", "3"] {
            let json = format!(r#"{{"type":"user","message":{message},"timestamp":"t"}}"#);
            let line = parse(&json);
            assert!(line.message.get().is_none(), "{message}");
            assert!(matches!(line.content(), Shape::Other));
            assert_eq!(line.timestamp.get(), Some("t"));
        }
    }

    #[test]
    fn missing_fields_are_absent() {
        let line = parse(r#"{"type":"user"}"#);
        assert!(line.kind.is("user"));
        assert!(line.message.get().is_none());
        assert!(line.attachment.0.is_none());
        assert!(line.attachment().is_none());
        assert_eq!(line.duration_ms.value(), None);
        let line = parse("{}");
        assert_eq!(line.kind.get(), None);
    }

    /// JSON that is not an object was a line with nothing in it to the
    /// `Value` walk, not a line to take positionally.
    #[test]
    fn a_line_that_is_not_an_object_reads_as_empty() {
        let line = parse(r#"["assistant","t"]"#);
        assert_eq!(line.kind.get(), None);
        assert_eq!(line.timestamp.get(), None);
        assert!(Line::parse(b"{\"type\":\"user\"").is_none());
        assert!(Line::parse(b"\0\0\0").is_none());
    }

    #[test]
    fn a_null_field_is_kept_apart_from_a_missing_one() {
        let line = parse(r#"{"type":"attachment","attachment":{"type":"x","content":null}}"#);
        let attachment = line.attachment().unwrap();
        assert!(attachment.content.0.is_some());
        assert_eq!(attachment.content.content_chars(), 0);
        let line = parse(r#"{"type":"attachment","attachment":{"type":"x"}}"#);
        assert!(line.attachment().unwrap().content.0.is_none());
    }

    #[test]
    fn content_chars_measures_text_bare_and_structure_as_json() {
        let line = parse(r#"{"attachment":{"content":"a\nb","list":[1, 2]}}"#);
        assert_eq!(line.attachment().unwrap().content.content_chars(), 3);
        let whole: serde_json::Value =
            serde_json::from_str(r#"{"content":"a\nb","list":[1, 2]}"#).unwrap();
        assert_eq!(
            line.attachment.content_chars(),
            whole.to_string().len() as u64
        );
    }

    #[test]
    fn a_queued_command_reads_its_prompt_and_origin() {
        let line = parse(
            r#"{"type":"attachment","attachment":{"type":"queued_command","prompt":[{"type":"text","text":"hi"},{"type":"image","source":{"data":"AAAA"}}],"commandMode":"prompt","origin":{"kind":"peer","name":"n","from":"f"}}}"#,
        );
        let a = line.attachment().unwrap();
        assert!(a.kind.is("queued_command"));
        assert_eq!(a.command_mode.get(), Some("prompt"));
        let origin = a.origin.get().unwrap();
        assert_eq!(
            (origin.kind.get(), origin.name.get(), origin.from.get()),
            (Some("peer"), Some("n"), Some("f"))
        );
        assert_eq!(a.prompt.list().unwrap().len(), 2);
    }

    #[test]
    fn a_patch_reads_its_lines_and_skips_what_is_not_one() {
        let line = parse(
            r#"{"toolUseResult":{"structuredPatch":[{"lines":["+a","-b",3]},"junk",{"lines":"x"},{"lines":[" c"]}]}}"#,
        );
        let lines: Vec<_> = line
            .tool_use_result
            .get()
            .unwrap()
            .patch_lines()
            .unwrap()
            .collect();
        assert_eq!(lines, ["+a", "-b", " c"]);
        let line = parse(r#"{"toolUseResult":{"type":"create","content":"x\ny"}}"#);
        let result = line.tool_use_result.get().unwrap();
        assert!(result.patch_lines().is_none());
        assert_eq!(result.created().unwrap().get(), Some("x\ny"));
        let line = parse(r#"{"toolUseResult":{"type":"update","content":"x"}}"#);
        assert!(line.tool_use_result.get().unwrap().created().is_none());
    }

    #[test]
    fn an_oversized_line_keeps_only_its_usage() {
        let line = Line::slim(ASSISTANT.as_bytes()).unwrap();
        assert!(line.kind.is("assistant"));
        assert_eq!(line.request_id.get(), Some("req_1"));
        let message = line.message.get().unwrap();
        assert_eq!(message.id.get(), Some("m1"));
        assert_eq!(message.usage.get().unwrap().window(), 60);
        assert!(matches!(message.content, Shape::Other));
        assert!(Line::slim(br#"{"type":"user","message":{"content":"big"}}"#).is_none());
        assert!(Line::slim(br#"{"message":{"usage":{}}}"#).is_none());
    }

    /// The request records are passed over before parsing, and say nothing;
    /// a line that only starts like one is not.
    #[test]
    fn request_records_are_skipped_unread() {
        for kind in ["api-request", "api-request-blob", "api-request-shape"] {
            let line = format!(r#"{{"type":"{kind}","sessionId":"s","body":{{"system":"long"}}}}"#);
            let read = Line::parse(line.as_bytes()).expect("a line");
            assert!(read.kind.get().is_none(), "{kind}");
            assert!(Line::slim(line.as_bytes()).is_none(), "{kind}");
        }
        let assistant =
            br#"{"type":"assistant","message":{"id":"m","model":"x","usage":{"input_tokens":1}}}"#;
        assert!(Line::parse(assistant).unwrap().kind.get().is_some());
    }
}
