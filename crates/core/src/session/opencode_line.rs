//! One OpenCode message or part, read into types rather than a
//! [`serde_json::Value`].
//!
//! OpenCode keeps a session in SQLite rather than a transcript: a `message`
//! row per turn, whose `data` column is the envelope — who spoke, the model,
//! the tokens and dollars, the error — and a `part` row per piece of it — the
//! words, a tool call and its outcome, a compaction. Both columns are JSON
//! that OpenCode reshapes between releases, so they are read the way
//! [`super::claude_line`] reads a transcript line: strings borrowed, a tool's
//! input left as text until a reader asks, and a field of the wrong type read
//! as absent rather than losing the row.

use super::lenient::{Lenient, Object, Raw, Str};
use serde::Deserialize;

/// A `message` row's `data`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Message<'a> {
    #[serde(borrow)]
    pub role: Str<'a>,
    #[serde(rename = "modelID", borrow)]
    pub model_id: Str<'a>,
    /// OpenCode 2 nests the model under `model: {id, variant}` where 1.x wrote a
    /// flat `modelID` and `variant`. Read both, flat first, through
    /// [`Message::model_name`] and [`Message::variant_name`].
    #[serde(borrow)]
    pub model: Object<ModelRef<'a>>,
    /// How hard the model was asked to think — `high`, `medium` — when the
    /// model has such a setting and the turn chose one.
    #[serde(borrow)]
    pub variant: Str<'a>,
    /// Set on the turn that wrote a compaction's summary: the summariser's
    /// request, not the conversation's (see [`Message::is_summary`]).
    pub summary: Lenient<bool>,
    /// The agent mode the turn ran in; `compaction` for that same summary.
    #[serde(borrow)]
    pub mode: Str<'a>,
    /// What OpenCode priced the turn at; zero for a provider it has no rates
    /// for, which is not the same as free.
    pub cost: Lenient<f64>,
    pub tokens: Object<Tokens>,
    pub time: Object<Time>,
    #[serde(borrow)]
    pub error: Object<Error<'a>>,
}

/// OpenCode 2's nested `model` object: the id priced against, and the effort.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ModelRef<'a> {
    #[serde(borrow)]
    pub id: Str<'a>,
    #[serde(borrow)]
    pub variant: Str<'a>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Tokens {
    pub input: Lenient<u64>,
    pub output: Lenient<u64>,
    pub reasoning: Lenient<u64>,
    pub cache: Object<Cache>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Cache {
    pub read: Lenient<u64>,
    pub write: Lenient<u64>,
}

/// Milliseconds. A message is `created` and `completed`; a tool call's state
/// is `start` and `end`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Time {
    pub created: Lenient<i64>,
    pub completed: Lenient<i64>,
    pub start: Lenient<i64>,
    pub end: Lenient<i64>,
}

/// A turn that failed: `APIError`, `MessageAbortedError`, `UnknownError`, with
/// the provider's words under `data.message` or, in older rows, `message`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Error<'a> {
    #[serde(borrow)]
    pub name: Str<'a>,
    /// OpenCode 2 names the failure by a `type` — `provider.rate-limit`,
    /// `aborted` — where 1.x gave a class `name`. Matched on like the name.
    #[serde(rename = "type", borrow)]
    pub kind: Str<'a>,
    #[serde(borrow)]
    pub message: Str<'a>,
    #[serde(borrow)]
    pub data: Object<ErrorData<'a>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ErrorData<'a> {
    #[serde(borrow)]
    pub message: Str<'a>,
}

/// A `part` row's `data`. Only the kinds a reader here wants have fields: a
/// `tool` call, and a `compaction`. Text and reasoning are skipped.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Part<'a> {
    #[serde(rename = "type", borrow)]
    pub kind: Str<'a>,
    #[serde(borrow)]
    pub tool: Str<'a>,
    #[serde(rename = "callID", borrow)]
    pub call_id: Str<'a>,
    #[serde(borrow)]
    pub state: Object<ToolState<'a>>,
}

/// Where a tool call stands: `pending`, `running`, `completed` or `error`, the
/// input it was handed, and when it ran.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ToolState<'a> {
    #[serde(borrow)]
    pub status: Str<'a>,
    #[serde(borrow)]
    pub input: Raw<'a>,
    pub time: Object<Time>,
}

impl Message<'_> {
    pub(crate) fn tokens(&self) -> super::Tokens {
        let no_tokens = Tokens::default();
        let t = self.tokens.get().unwrap_or(&no_tokens);
        let n = |v: &Lenient<u64>| v.value().unwrap_or(0);
        let (read, write) = t.cache.get().map_or((0, 0), |c| (n(&c.read), n(&c.write)));
        let (input, output) = (n(&t.input), n(&t.output));
        super::Tokens {
            input,
            output,
            cache_read: read,
            cache_write_5m: write,
            reasoning_output: n(&t.reasoning),
            total: input
                .saturating_add(output)
                .saturating_add(read)
                .saturating_add(write),
            ..Default::default()
        }
    }

    /// Whether this is the turn a compaction ran to write its summary. It is
    /// billed like any turn, but its input is the whole old window handed to
    /// the summariser and it carries no `variant`, so it says nothing about
    /// the window or the effort the conversation goes on with.
    pub(crate) fn is_summary(&self) -> bool {
        self.summary.value() == Some(true) || self.mode.is("compaction")
    }

    /// What the next turn has to resend: everything the model was given,
    /// cached or not. Output is excluded — the next input count includes it.
    pub(crate) fn window(&self) -> u64 {
        let t = self.tokens();
        t.input + t.cache_read + t.cache_write_5m
    }

    pub(crate) fn created_ms(&self) -> Option<i64> {
        self.time.get().and_then(|t| t.created.value())
    }

    /// How long the model took over the turn, when it finished one.
    pub(crate) fn duration_ms(&self) -> Option<u64> {
        let time = self.time.get()?;
        let (start, end) = (time.created.value()?, time.completed.value()?);
        u64::try_from(end - start).ok()
    }

    /// The model id, flat (`modelID`, 1.x) or nested (`model.id`, 2.x).
    pub(crate) fn model_name(&self) -> Option<&str> {
        self.model_id
            .get()
            .or_else(|| self.model.get().and_then(|m| m.id.get()))
    }

    /// The reasoning effort, flat (`variant`, 1.x) or nested (`model.variant`).
    pub(crate) fn variant_name(&self) -> Option<&str> {
        self.variant
            .get()
            .or_else(|| self.model.get().and_then(|m| m.variant.get()))
    }

    /// The error's name and words, lowercased into one string to match on.
    pub(crate) fn error_text(&self) -> Option<String> {
        let error = self.error.get()?;
        let text = [
            error.name.get(),
            error.kind.get(),
            error.data.get().and_then(|d| d.message.get()),
            error.message.get(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
        Some(text.to_ascii_lowercase())
    }
}

impl ToolState<'_> {
    pub(crate) fn duration_ms(&self) -> Option<i64> {
        let time = self.time.get()?;
        Some((time.end.value()? - time.start.value()?).max(0))
    }
}

/// Read a row's `data`, or `None` when it is not a JSON object.
pub(crate) fn parse<'a, T: Deserialize<'a> + Default>(raw: &'a str) -> Option<T> {
    serde_json::from_str::<Object<T>>(raw).ok()?.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_assistant_message_reads_whole() {
        let m: Message = parse(
            r#"{"role":"assistant","modelID":"m","providerID":"p","variant":"high","cost":0.5,"tokens":{"input":10,"output":3,"reasoning":1,"cache":{"read":5,"write":2}},"time":{"created":1000,"completed":4000},"finish":"stop"}"#,
        )
        .unwrap();
        assert!(m.role.is("assistant"));
        assert_eq!(m.variant.get(), Some("high"));
        assert_eq!(m.tokens().total, 20);
        assert_eq!(m.window(), 17);
        assert_eq!(m.duration_ms(), Some(3000));
        assert!(m.error_text().is_none());
    }

    #[test]
    fn an_error_reads_from_either_place() {
        let m: Message = parse(
            r#"{"role":"assistant","error":{"name":"APIError","data":{"message":"Rate limit"}}}"#,
        )
        .unwrap();
        assert_eq!(m.error_text().as_deref(), Some("apierror rate limit"));
    }

    #[test]
    fn a_tool_part_reads_its_state() {
        let p: Part = parse(
            r#"{"type":"tool","tool":"bash","callID":"c1","state":{"status":"error","input":{"command":"x"},"time":{"start":10,"end":25},"error":"boom"}}"#,
        )
        .unwrap();
        assert!(p.kind.is("tool"));
        let state = p.state.get().unwrap();
        assert!(state.status.is("error"));
        assert_eq!(state.duration_ms(), Some(15));
        assert_eq!(state.input.value()["command"], "x");
    }

    #[test]
    fn a_wrong_typed_field_is_absent_and_the_row_is_kept() {
        let m: Message = parse(
            r#"{"role":"assistant","modelID":7,"cost":"lots","tokens":{"input":"many","cache":[1]},"time":"now","error":"bad"}"#,
        )
        .unwrap();
        assert_eq!(m.model_id.get(), None);
        assert_eq!(m.cost.value(), None);
        assert_eq!(m.tokens().total, 0);
        assert_eq!(m.duration_ms(), None);
        assert!(m.error.get().is_none());
        assert!(parse::<Message>("[1]").is_none());
        assert!(parse::<Message>("{nope").is_none());
    }
}
