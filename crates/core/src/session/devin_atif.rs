//! A Devin ATIF transcript, read into types rather than a
//! [`serde_json::Value`].
//!
//! The same lenient wrappers as [`super::claude_line`]: an unknown field is
//! skipped, a missing one is absent, and one of the wrong type reads as absent
//! rather than losing the transcript. ATIF is a versioned schema
//! (`schema_version`, "ATIF-v1.7" when this was written), and a later version
//! adding or retyping a field should cost that field, not the session.
//!
//! The document is read whole — it is one JSON object, not a log — but the
//! parts the table never looks at stay unbuilt: the system prompt, every
//! message, the reasoning, and the tool results in `observation`, which are
//! most of the bytes.

use super::lenient::{Lenient, Object, Raw, Str};
use serde::Deserialize;

/// The transcript: who ran it, every step, and the session's own totals.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Transcript<'a> {
    #[serde(borrow)]
    pub session_id: Str<'a>,
    #[serde(borrow)]
    pub agent: Object<Agent<'a>>,
    #[serde(borrow)]
    pub steps: Lenient<Vec<Object<Step<'a>>>>,
    pub final_metrics: Object<FinalMetrics>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Agent<'a> {
    /// The model's display name ("SWE-2 High"), where steps carry its slug.
    #[serde(borrow)]
    pub model_name: Str<'a>,
}

/// One step. Only `agent` steps carry a model, metrics and calls; `system` and
/// `user` steps are the prompt and what the person typed.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Step<'a> {
    #[serde(borrow)]
    pub source: Str<'a>,
    #[serde(borrow)]
    pub timestamp: Str<'a>,
    #[serde(borrow)]
    pub model_name: Str<'a>,
    pub metrics: Object<StepMetrics>,
    #[serde(borrow)]
    pub tool_calls: Lenient<Vec<Object<ToolCall<'a>>>>,
}

/// One inference request's tokens.
///
/// `cached_tokens` is the part of `prompt_tokens` served from cache, not an
/// addition to it: across 793 recorded steps it never exceeded the prompt, and
/// on each step it was the previous step's prompt less one — the conversation
/// so far, read back from cache.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct StepMetrics {
    pub prompt_tokens: Lenient<u64>,
    pub completion_tokens: Lenient<u64>,
    pub cached_tokens: Lenient<u64>,
}

/// The session's totals, in the same three counts as a step's.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct FinalMetrics {
    pub total_prompt_tokens: Lenient<u64>,
    pub total_completion_tokens: Lenient<u64>,
    pub total_cached_tokens: Lenient<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ToolCall<'a> {
    #[serde(borrow)]
    pub tool_call_id: Str<'a>,
    #[serde(borrow)]
    pub function_name: Str<'a>,
    #[serde(borrow)]
    pub arguments: Raw<'a>,
}

impl<'a> Transcript<'a> {
    pub(crate) fn parse(text: &'a str) -> serde_json::Result<Transcript<'a>> {
        serde_json::from_str(text)
    }

    /// Every step that is an object, in order.
    pub(crate) fn steps(&self) -> impl Iterator<Item = &Step<'a>> {
        self.steps
            .get()
            .into_iter()
            .flatten()
            .filter_map(Object::get)
    }
}

impl<'a> Step<'a> {
    pub(crate) fn calls(&self) -> impl Iterator<Item = &ToolCall<'a>> {
        self.tool_calls
            .get()
            .into_iter()
            .flatten()
            .filter_map(Object::get)
    }
}

/// Prompt, completion and cached counts as cctop's [`super::Tokens`], with the
/// cached part split out of the prompt the way Codex's and Gemini's are.
///
/// Saturating, like every other sum over a file any process on the machine
/// can write: see the Claude parser for why.
fn tokens(prompt: u64, completion: u64, cached: u64) -> super::Tokens {
    super::Tokens {
        total: prompt.saturating_add(completion),
        ..super::Tokens::from_prompt(prompt, cached, completion)
    }
}

impl StepMetrics {
    /// The step's tokens, or `None` when it carries neither a prompt nor a
    /// completion count: a `metrics` object with nothing usable in it is no
    /// count, and must not stand in front of `final_metrics` as a zero.
    pub(crate) fn tokens(&self) -> Option<super::Tokens> {
        let prompt = self.prompt_tokens.value();
        let completion = self.completion_tokens.value();
        if prompt.is_none() && completion.is_none() {
            return None;
        }
        Some(tokens(
            prompt.unwrap_or(0),
            completion.unwrap_or(0),
            self.cached_tokens.value().unwrap_or(0),
        ))
    }
}

impl FinalMetrics {
    pub(crate) fn tokens(&self) -> super::Tokens {
        tokens(
            self.total_prompt_tokens.value().unwrap_or(0),
            self.total_completion_tokens.value().unwrap_or(0),
            self.total_cached_tokens.value().unwrap_or(0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_step_reads_whole_and_the_rest_is_skipped() {
        let t = Transcript::parse(
            r#"{"schema_version":"ATIF-v1.7","session_id":"s","agent":{"model_name":"SWE-2 High","extra":{"backend":"x"}},
            "steps":[{"source":"system","message":"long prompt"},
                     {"source":"agent","timestamp":"t","model_name":"swe-2-high","reasoning_content":"r",
                      "metrics":{"prompt_tokens":100,"completion_tokens":5,"cached_tokens":90},
                      "tool_calls":[{"tool_call_id":"c1","function_name":"read","arguments":{"file_path":"/x"}}],
                      "observation":{"results":[{"source_call_id":"c1","content":"file body"}]}}],
            "final_metrics":{"total_prompt_tokens":100,"total_completion_tokens":5,"total_cached_tokens":90}}"#,
        )
        .unwrap();
        assert_eq!(t.session_id.get(), Some("s"));
        assert_eq!(t.agent.get().unwrap().model_name.get(), Some("SWE-2 High"));
        let steps: Vec<_> = t.steps().collect();
        assert_eq!(steps.len(), 2);
        let agent = steps[1];
        assert!(agent.source.is("agent"));
        let tokens = agent.metrics.get().unwrap().tokens().unwrap();
        assert_eq!(
            (tokens.input, tokens.cached_input, tokens.output),
            (10, 90, 5)
        );
        assert_eq!(tokens.all_input(), 100, "cached is inside the prompt");
        let call = agent.calls().next().unwrap();
        assert_eq!(call.function_name.get(), Some("read"));
        assert_eq!(call.arguments.value()["file_path"], "/x");
        assert_eq!(t.final_metrics.get().unwrap().tokens().total, 105);
    }

    #[test]
    fn wrong_typed_fields_cost_only_themselves() {
        let t = Transcript::parse(
            r#"{"session_id":5,"agent":"devin","steps":[7,{"source":"agent","metrics":{"prompt_tokens":"many","completion_tokens":3},"tool_calls":{"x":1}}],"final_metrics":[]}"#,
        )
        .unwrap();
        assert_eq!(t.session_id.get(), None);
        assert!(t.agent.get().is_none());
        assert!(t.final_metrics.get().is_none());
        let steps: Vec<_> = t.steps().collect();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].metrics.get().unwrap().tokens().unwrap().output, 3);
        assert_eq!(steps[0].calls().count(), 0);
    }
}
