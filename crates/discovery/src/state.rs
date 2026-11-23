use crate::Provider;
use serde::{Deserialize, Serialize};

/// Where a session is being driven from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Surface {
    /// A coding agent in a terminal.
    Cli,
    /// An agent hosted by an editor rather than a dedicated CLI process.
    Editor,
    /// Claude for Mac, running Claude Code locally.
    DesktopCode,
    /// Claude for Mac, running in a cloud VM.
    DesktopCowork,
}

/// What a live agent is doing, inferred from the newest transcript event — or,
/// for [`ActivityState::Asking`], reported by the agent's own hooks.
///
/// This deliberately captures only states that have a clear user-facing
/// meaning.  A missing or unrecognised event remains normal work rather than
/// guessing that the agent is stalled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ActivityState {
    #[default]
    Working,
    /// The turn is over and the prompt is the user's. Nobody is blocked: the
    /// agent has said what it had to say and will sit there indefinitely.
    WaitingForInput,
    /// The agent is blocked on a question — a permission prompt, or an MCP
    /// elicitation — and cannot proceed until it is answered.
    ///
    /// Told apart from [`ActivityState::WaitingForInput`] because the two are
    /// worth different amounts of your attention, and a transcript cannot tell
    /// them apart at all: a held permission prompt leaves no record, so the
    /// newest thing written is a tool call in flight, which reads exactly like
    /// an agent still working. Only the agent's own hooks say this outright —
    /// see `Signal::NeedsInput` in cctop-core's `hook`.
    Asking,
    ApiError,
}

impl Surface {
    pub fn is_desktop(&self) -> bool {
        matches!(self, Surface::DesktopCode | Surface::DesktopCowork)
    }

    pub fn label(&self, provider: Provider) -> &'static str {
        match (self, provider) {
            (Surface::Editor, Provider::Cursor) => "Cursor",
            (Surface::DesktopCowork, _) => "Claude Cowork",
            (Surface::DesktopCode, _) => "Claude Code",
            (_, Provider::Claude) => "Claude",
            (_, Provider::Codex) => "Codex",
            (_, Provider::Cursor) => "Cursor",
            (_, Provider::Devin) => "Devin",
            (_, Provider::Gemini) => "Gemini",
            (_, Provider::OpenCode) => "OpenCode",
            (_, Provider::Pi) => "Pi",
            (_, Provider::Vibe) => "Vibe",
            (_, Provider::Windsurf) => "Windsurf",
            (_, Provider::Antigravity) => "Antigravity",
        }
    }
}

///
/// Only a live agent's own hooks can answer this — it is a setting, not an
/// event, and no transcript records it. That is what makes it worth a column:
/// an agent running with its permission prompts turned off looks exactly like
/// every other agent right up until it does something you would have refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Asks before anything it is not already allowed to do.
    Ask,
    /// Files are written without asking; everything else still asks.
    AcceptEdits,
    /// Reading and planning only — it cannot act at all yet.
    Plan,
    /// Asks about nothing. `--dangerously-skip-permissions`.
    Bypass,
}

impl Permission {
    /// Parse the mode a harness reported, or `None` for one cctop has not seen.
    ///
    /// An unknown mode is deliberately not folded into [`Permission::Ask`]: a
    /// future mode is at least as likely to be a *looser* one, and quietly
    /// drawing it as the safe end is the wrong way to be wrong about this.
    /// The spellings cctop's own [`label`](Permission::label) uses are accepted
    /// too, so a mode that has been through `--json` and back — which is how a
    /// remote machine's rows arrive — reads as the mode it left as.
    pub fn parse(word: &str) -> Option<Permission> {
        match word {
            "default" | "ask" => Some(Permission::Ask),
            "acceptEdits" | "auto" | "edits" => Some(Permission::AcceptEdits),
            "plan" => Some(Permission::Plan),
            "bypassPermissions" | "dontAsk" | "BYPASS" => Some(Permission::Bypass),
            _ => None,
        }
    }

    /// A word for the column, short enough for it.
    pub fn label(self) -> &'static str {
        match self {
            Permission::Ask => "ask",
            Permission::AcceptEdits => "edits",
            Permission::Plan => "plan",
            Permission::Bypass => "BYPASS",
        }
    }

    /// Whether this mode lets the agent act without asking at all — the case
    /// the column exists to make visible.
    pub fn is_unrestricted(self) -> bool {
        matches!(self, Permission::Bypass)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A remote machine's rows arrive through `--json`, which carries the label:
    /// each mode has to read back as itself, not as "unknown".
    #[test]
    fn every_permission_parses_back_from_its_label() {
        for p in [
            Permission::Ask,
            Permission::AcceptEdits,
            Permission::Plan,
            Permission::Bypass,
        ] {
            assert_eq!(Permission::parse(p.label()), Some(p), "{p:?}");
        }
    }
}
