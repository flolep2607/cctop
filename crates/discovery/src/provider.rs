#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    Claude,
    Codex,
    Cursor,
    Devin,
    Gemini,
    OpenCode,
    Pi,
    Vibe,
    Windsurf,
    Antigravity,
}

impl Provider {
    /// Every provider, in declaration order.
    ///
    /// The one list. A hand-written copy of this enum is a list that goes
    /// stale, and one had: `--provider devin` was rejected while Devin's
    /// sessions were already priced into every figure `optimize` printed,
    /// because the names accepted there were a copy nobody had to keep in step.
    /// Everything that wants all of them iterates this instead.
    pub const ALL: [Provider; 10] = [
        Provider::Claude,
        Provider::Codex,
        Provider::Cursor,
        Provider::Devin,
        Provider::Gemini,
        Provider::OpenCode,
        Provider::Pi,
        Provider::Vibe,
        Provider::Windsurf,
        Provider::Antigravity,
    ];

    /// The harness's own name, as its vendor writes it.
    ///
    /// Not [`Self::as_str`], which is the key cctop files things under, and not
    /// the name of the process either — `Claude Code` and `Claude` are one
    /// harness, and only one of them is what it calls itself. A reader
    /// choosing `--provider claude` should see the same name doctor calls it.
    pub fn display_name(self) -> &'static str {
        match self {
            Provider::Claude => "Claude Code",
            Provider::Codex => "Codex",
            Provider::Cursor => "Cursor",
            Provider::Devin => "Devin",
            Provider::Gemini => "Gemini CLI",
            Provider::OpenCode => "OpenCode",
            Provider::Pi => "Pi",
            Provider::Vibe => "Mistral Vibe",
            Provider::Windsurf => "Windsurf",
            Provider::Antigravity => "Antigravity CLI",
        }
    }

    /// The `trace` label for parsing this provider's transcripts.
    ///
    /// Split per provider because the figure across every harness cannot say
    /// whether a slow walk is one transcript format or all of them. Spelled
    /// here rather than at the loop that opens the span, which is the only way
    /// the extraction and tail labels can be sure to name a harness the same
    /// way — they were two copies of this table and had begun to differ.
    // cctop's own tracing labels, not part of what other programs use.
    #[doc(hidden)]
    pub fn extract_trace_name(self) -> &'static str {
        match self {
            Provider::Claude => "extract.claude",
            Provider::Codex => "extract.codex",
            Provider::Cursor => "extract.cursor",
            Provider::Devin => "extract.devin",
            Provider::Gemini => "extract.gemini",
            Provider::OpenCode => "extract.opencode",
            Provider::Pi => "extract.pi",
            Provider::Vibe => "extract.vibe",
            Provider::Windsurf => "extract.windsurf",
            Provider::Antigravity => "extract.antigravity",
        }
    }

    /// The `trace` label for reading this provider's transcript tails — the
    /// [`Self::extract_trace_name`] of the other loop that dominates a walk.
    // cctop's own tracing labels, not part of what other programs use.
    #[doc(hidden)]
    pub fn tails_trace_name(self) -> &'static str {
        match self {
            Provider::Claude => "tails.claude",
            Provider::Codex => "tails.codex",
            Provider::Cursor => "tails.cursor",
            Provider::Devin => "tails.devin",
            Provider::Gemini => "tails.gemini",
            Provider::OpenCode => "tails.opencode",
            Provider::Pi => "tails.pi",
            Provider::Vibe => "tails.vibe",
            Provider::Windsurf => "tails.windsurf",
            Provider::Antigravity => "tails.antigravity",
        }
    }

    /// Whether this provider's transcript says, per call, that it failed.
    ///
    /// Kept as a fact about the provider rather than a flag on the extraction:
    /// a transcript with no failures in it and a transcript that cannot express
    /// one look identical, and the difference decides whether an error rate is
    /// `0%` or `─`. Pi answers each call with a `toolResult` that says
    /// `isError`; Cursor and Windsurf record no outcome at all.
    pub fn records_tool_outcomes(&self) -> bool {
        match self {
            Provider::Claude
            | Provider::Codex
            | Provider::Devin
            | Provider::Gemini
            | Provider::OpenCode
            | Provider::Pi
            | Provider::Vibe
            | Provider::Antigravity => true,
            Provider::Cursor | Provider::Windsurf => false,
        }
    }

    /// Whether this provider's transcript records the tokens it used.
    ///
    /// The same kind of fact as [`Self::records_tool_outcomes`], for the same
    /// reason: a session with no tokens is either a parser that missed them or
    /// a harness that never wrote them down, and only this tells which. Cursor
    /// and Windsurf keep their accounting server-side; Devin records tokens but
    /// no dollars, which is a different question.
    pub fn records_token_usage(&self) -> bool {
        !matches!(self, Provider::Cursor | Provider::Windsurf)
    }

    /// The provider named by `name`, or `None` for anything else.
    ///
    /// The inverse of [`Provider::as_str`], and deliberately total in the other
    /// direction: a string that names no provider cctop knows is `None` rather
    /// than a guess, because every caller here is about to act on the answer.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "claude" => Provider::Claude,
            "codex" => Provider::Codex,
            "cursor" => Provider::Cursor,
            "devin" => Provider::Devin,
            "gemini" => Provider::Gemini,
            "opencode" => Provider::OpenCode,
            "pi" => Provider::Pi,
            "vibe" => Provider::Vibe,
            "windsurf" => Provider::Windsurf,
            "antigravity" => Provider::Antigravity,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
            Provider::Cursor => "cursor",
            Provider::Devin => "devin",
            Provider::Gemini => "gemini",
            Provider::OpenCode => "opencode",
            Provider::Pi => "pi",
            Provider::Vibe => "vibe",
            Provider::Windsurf => "windsurf",
            Provider::Antigravity => "antigravity",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `parse` is how a name from a session file, a flag or `--json` comes
    /// back, so every provider's own key has to make the trip.
    #[test]
    fn every_provider_parses_back_from_its_key() {
        for p in Provider::ALL {
            assert_eq!(Provider::parse(p.as_str()), Some(p), "{p:?}");
        }
    }
}
