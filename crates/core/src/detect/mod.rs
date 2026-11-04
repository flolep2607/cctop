//! What an agent is doing, read off its screen by herdr's per-agent rules.
//!
//! Every coding agent draws its state: a spinner and a timer while a turn runs,
//! a menu with `Esc to cancel` while a permission prompt holds it, an empty
//! prompt box when it is done. None of it is a contract, and each agent words
//! it differently — `esc to cancel` is a permission prompt in Claude Code and a
//! turn in flight in Gemini CLI — so the only way to read it is one rule set
//! per agent, kept up with each agent's releases. herdr keeps exactly that, as
//! data: a TOML manifest per agent, versioned, with rules written against real
//! screens. This module is herdr's rule engine and its manifests, so cctop
//! reads the same 22 agents herdr does rather than keeping a thinner list of
//! its own.
//!
//! A manifest is a list of rules. A rule names a *region* of the screen — the
//! last N non-empty lines, the body of the prompt box, the terminal title — a
//! *gate* over that region's text (`contains`, `regex`, `line_regex`, nested
//! `all`/`any`/`not`), a *state* (idle, working, blocked, unknown) and a
//! *priority*. Of the rules that match, the highest priority wins, and among
//! equals the first in the file — so a manifest says "a permission prompt
//! outranks a spinner left above it" by numbers rather than by code order.
//!
//! Two deliberate differences from herdr:
//!
//! - **No match is [`State::Unknown`], not idle.** herdr calls a recognised
//!   agent whose screen no rule matches idle, because it has PTY activity to
//!   say "working" with. cctop's equivalent is the still-screen clock in
//!   `ui::tabs`, and a manifest that says nothing hands the decision back to it.
//! - **Bundled manifests only.** herdr also loads a local override and a
//!   remote update per agent; here the manifests are compiled in, so they
//!   move with cctop's releases, and herdr's complexity limits — which guard
//!   against a hostile download — have nothing to guard.
//!
//! Cost: each manifest is parsed and its regexes compiled once, the first time
//! that agent's screen is read. A reading runs at most one pass over the
//! visible rows per rule, and rules are tried highest priority first, stopping
//! at the first match — no rule that could not win is evaluated.
//!
//! ## Attribution
//!
//! The manifests under `manifests/` are herdr's, unmodified, from
//! <https://github.com/herdrdev/herdr> at commit
//! `ee5bfb5041bb3e4a49d591c72308aae3c81ecdde` (herdr 0.9.5,
//! `src/detect/manifests/`). This module is a modified port of herdr's
//! `src/detect/manifest.rs` from the same commit: the region extraction and
//! gate evaluation are herdr's; the loading (TOML via `toml_edit`, no override
//! or remote layers), the no-match answer and the API are cctop's. herdr is
//! licensed under the Apache License 2.0, whose text is in
//! `manifests/LICENSE`; see also `NOTICE` at the root of the repository.

use std::sync::OnceLock;

use regex::Regex;
use toml_edit::{Item, TableLike};

#[cfg(test)]
mod tests;

/// What a screen says the agent is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// At its prompt, nothing running.
    Idle,
    /// A turn is in flight.
    Working,
    /// Waiting on a person: a permission prompt, a question, a trust dialog.
    Blocked,
    /// No rule matched, or the one that did says the screen is not the live
    /// one (a transcript viewer).
    Unknown,
}

/// The screen and what the agent last wrote to the terminal around it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Input<'a> {
    /// The visible rows, joined by `\n` — see [`screen_text`].
    pub screen: &'a str,
    /// The last OSC 0/2 window title, or `""`.
    pub title: &'a str,
    /// The last OSC 9 payload after `9;` — `4;3;` while a progress bar is
    /// indeterminate — or `""`.
    pub progress: &'a str,
}

/// One reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detection {
    pub state: State,
    /// The id of the rule that decided it, for whoever has to work out why.
    pub rule: Option<&'static str>,
    /// The screen is the agent's own viewer over its history — Claude Code's
    /// `ctrl+o` transcript — and says nothing about the live turn. Whatever
    /// was read before it still stands.
    pub skip: bool,
    /// The rule matched the agent's live chrome — its prompt box, its
    /// permission menu — rather than text that could be scrollback. herdr's
    /// `visible_idle`/`visible_blocker`/`visible_working`, for the rule's own
    /// state.
    pub visible: bool,
}

/// The visible rows as one string, the shape the manifests are written
/// against: trailing blank rows dropped, each row's trailing blanks dropped,
/// newline-terminated. herdr builds its detection text the same way, and
/// rules anchored at the end of the screen (`\z`) depend on it.
pub fn screen_text(rows: &[String]) -> String {
    let end = rows
        .iter()
        .rposition(|row| !row.trim().is_empty())
        .map_or(0, |i| i + 1);
    let mut text = String::new();
    for row in &rows[..end] {
        text.push_str(row.trim_end());
        text.push('\n');
    }
    text
}

/// Whether `harness` has a manifest — the cheap check, which compiles nothing.
pub fn known(harness: &str) -> bool {
    bundled(harness).is_some()
}

/// What `harness`'s screen says, or `None` when there is no manifest for it.
pub fn detect(harness: &str, input: Input<'_>) -> Option<Detection> {
    Some(bundled(harness)?.manifest()?.detect(input))
}

/// The id of every bundled manifest's agent, for the docs and for tests.
pub fn agents() -> impl Iterator<Item = &'static str> {
    BUNDLED.iter().map(|b| b.names[0])
}

/// One manifest compiled in, with the names it answers to and the compiled
/// form, built on first use.
struct Bundled {
    /// The manifest's `id`, then its `aliases`, as the file spells them. A
    /// test checks the two agree; they are repeated here so finding the
    /// manifest for a harness costs a string compare rather than a parse.
    names: &'static [&'static str],
    source: &'static str,
    compiled: OnceLock<Option<Manifest>>,
}

impl Bundled {
    /// `None` only if the file does not parse, which a test rules out for
    /// every bundled one: a broken manifest reads as an agent cctop has no
    /// rules for, never as a panic in the draw loop.
    fn manifest(&'static self) -> Option<&'static Manifest> {
        self.compiled
            .get_or_init(|| parse(self.source).ok())
            .as_ref()
    }
}

macro_rules! bundled {
    ($file:literal, [$($name:literal),+]) => {
        Bundled {
            names: &[$($name),+],
            source: include_str!(concat!("manifests/", $file)),
            compiled: OnceLock::new(),
        }
    };
}

static BUNDLED: [Bundled; 22] = [
    bundled!("amp.toml", ["amp", "amp-local"]),
    bundled!(
        "antigravity.toml",
        ["agy", "antigravity", "antigravity-cli"]
    ),
    bundled!("claude.toml", ["claude", "claude-code"]),
    bundled!("cline.toml", ["cline"]),
    bundled!("codex.toml", ["codex"]),
    bundled!("cursor.toml", ["cursor", "cursor-agent"]),
    bundled!("devin.toml", ["devin", "devin-cli", "devin cli"]),
    bundled!("droid.toml", ["droid"]),
    bundled!("gemini.toml", ["gemini"]),
    bundled!("github-copilot.toml", ["copilot", "github-copilot", "ghcs"]),
    bundled!("grok.toml", ["grok", "grok-build"]),
    bundled!("hermes.toml", ["hermes", "hermes-agent"]),
    bundled!(
        "kilo.toml",
        ["kilo", "kilo-code", "kilo code", "herdr:kilo"]
    ),
    bundled!("kimi.toml", ["kimi", "kimi-code", "kimi code"]),
    bundled!("kiro.toml", ["kiro", "kiro-cli"]),
    bundled!("letta.toml", ["letta", "letta-code", "letta code"]),
    bundled!("maki.toml", ["maki"]),
    bundled!("muse.toml", ["muse", "muse-code", "muse-cli"]),
    bundled!("opencode.toml", ["opencode", "open-code", "herdr:opencode"]),
    bundled!("pi.toml", ["pi", "herdr:pi"]),
    bundled!(
        "qodercli.toml",
        ["qodercli", "qoderclicn", "qoder", "qodercn"]
    ),
    bundled!("qwen.toml", ["qwen", "qwen-code", "qwen code"]),
];

fn bundled(harness: &str) -> Option<&'static Bundled> {
    BUNDLED.iter().find(|b| {
        b.names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(harness))
    })
}

/// A parsed, compiled manifest. Rules are held highest priority first, ties in
/// file order, so the first that matches is the one herdr would pick.
#[derive(Debug)]
struct Manifest {
    rules: Vec<Rule>,
}

#[derive(Debug)]
struct Rule {
    id: String,
    state: State,
    priority: i32,
    region: Region,
    visible: bool,
    skip: bool,
    gate: Gate,
}

/// All of `contains`, `regex` and `line_regex` must hold, every `all` gate,
/// at least one `any` gate if there are any, and no `not` gate.
#[derive(Debug, Default)]
struct Gate {
    all: Vec<Gate>,
    any: Vec<Gate>,
    not: Vec<Gate>,
    /// Lowercased: `contains` ignores case, as herdr's does.
    contains: Vec<String>,
    regex: Vec<Regex>,
    line_regex: Vec<Regex>,
}

/// Where on the screen a rule looks. herdr names these as strings; they are
/// parsed once here rather than matched on every reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    WholeRecent,
    AfterLastPromptMarker,
    BeforeCurrentPromptMarker,
    WholeRecentWithoutCurrentPromptMarker,
    CurrentPromptBlockMarker,
    AfterCurrentPromptBlockMarker,
    PromptBoxBody,
    AbovePromptBox,
    LastNonEmptyAbovePromptBox,
    AfterLastHorizontalRule,
    OscTitle,
    OscProgress,
    BottomLines(usize),
    BottomNonEmptyLines(usize),
    TopNonEmptyLines(usize),
}

impl Manifest {
    fn detect(&'static self, input: Input<'_>) -> Detection {
        for rule in &self.rules {
            if rule.gate.matches(region(input, rule.region)) {
                return Detection {
                    state: rule.state,
                    rule: Some(rule.id.as_str()),
                    skip: rule.skip,
                    visible: rule.visible,
                };
            }
        }
        Detection {
            state: State::Unknown,
            rule: None,
            skip: false,
            visible: false,
        }
    }
}

impl Gate {
    fn matches(&self, text: &str) -> bool {
        // Lowercased once per gate tree, and only if something in it reads it.
        let lower = self.wants_lower().then(|| text.to_lowercase());
        self.matches_with(text, lower.as_deref().unwrap_or(""))
    }

    fn wants_lower(&self) -> bool {
        !self.contains.is_empty()
            || self
                .all
                .iter()
                .chain(&self.any)
                .chain(&self.not)
                .any(Gate::wants_lower)
    }

    fn matches_with(&self, text: &str, lower: &str) -> bool {
        self.contains.iter().all(|needle| lower.contains(needle))
            && self.regex.iter().all(|re| re.is_match(text))
            && self
                .line_regex
                .iter()
                .all(|re| text.lines().any(|line| re.is_match(line)))
            && self.all.iter().all(|g| g.matches_with(text, lower))
            && (self.any.is_empty() || self.any.iter().any(|g| g.matches_with(text, lower)))
            && !self.not.iter().any(|g| g.matches_with(text, lower))
    }

    fn has_positive(&self) -> bool {
        !self.contains.is_empty()
            || !self.regex.is_empty()
            || !self.line_regex.is_empty()
            || !self.all.is_empty()
            || !self.any.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

const MANIFEST_KEYS: &[&str] = &[
    "id",
    "version",
    "min_engine_version",
    "updated_at",
    "aliases",
    "rules",
];

const RULE_KEYS: &[&str] = &[
    "id",
    "state",
    "priority",
    "region",
    "visible_idle",
    "visible_blocker",
    "visible_working",
    "skip_state_update",
    "all",
    "any",
    "not",
    "contains",
    "regex",
    "line_regex",
];

const GATE_KEYS: &[&str] = &["all", "any", "not", "contains", "regex", "line_regex"];

/// Parse and compile one manifest. Unknown keys are refused, as herdr's
/// `deny_unknown_fields` refuses them: a manifest written for a newer engine
/// must not be read as if its new keys were not there.
fn parse(source: &str) -> Result<Manifest, String> {
    let doc: toml_edit::DocumentMut = source.parse().map_err(|e| format!("{e}"))?;
    let top = doc.as_table();
    only(top, MANIFEST_KEYS, "manifest")?;
    let id = string(top, "id")?.ok_or("manifest has no id")?;
    let tables = match top.get("rules") {
        Some(item) => item
            .as_array_of_tables()
            .ok_or("rules is not an array of tables")?,
        None => return Err("manifest has no rules".into()),
    };
    let mut rules = tables
        .iter()
        .map(|table| rule(table).map_err(|e| format!("{id}: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    if rules.is_empty() {
        return Err(format!("{id}: manifest has no rules"));
    }
    // Stable, so equal priorities keep file order — herdr's tie-break.
    rules.sort_by_key(|rule| std::cmp::Reverse(rule.priority));
    Ok(Manifest { rules })
}

fn rule(table: &dyn TableLike) -> Result<Rule, String> {
    only(table, RULE_KEYS, "rule")?;
    let id = string(table, "id")?
        .filter(|id| !id.trim().is_empty())
        .ok_or("rule has no id")?;
    let state = match string(table, "state")?.as_deref() {
        Some("idle") => State::Idle,
        Some("working") => State::Working,
        Some("blocked") => State::Blocked,
        Some("unknown") | None => State::Unknown,
        Some(other) => return Err(format!("rule {id}: unknown state {other:?}")),
    };
    let priority = match table.get("priority") {
        Some(item) => item
            .as_integer()
            .and_then(|p| i32::try_from(p).ok())
            .ok_or_else(|| format!("rule {id}: priority is not an integer"))?,
        None => 0,
    };
    let region_name = string(table, "region")?.unwrap_or_else(|| "whole_recent".into());
    let region =
        parse_region(&region_name).ok_or_else(|| format!("rule {id}: region {region_name:?}"))?;
    let flag = |key: &str| -> Result<bool, String> {
        match table.get(key) {
            Some(item) => item
                .as_bool()
                .ok_or_else(|| format!("rule {id}: {key} is not a boolean")),
            None => Ok(false),
        }
    };
    let visible = match state {
        State::Idle => flag("visible_idle")?,
        State::Blocked => flag("visible_blocker")?,
        State::Working => flag("visible_working")?,
        State::Unknown => false,
    };
    let skip = flag("skip_state_update")?;
    if skip && state != State::Unknown {
        return Err(format!(
            "rule {id}: skip_state_update without state = \"unknown\""
        ));
    }
    let gate = gate(table).map_err(|e| format!("rule {id}: {e}"))?;
    if !gate.has_positive() {
        return Err(format!("rule {id}: no positive matcher"));
    }
    Ok(Rule {
        id,
        state,
        priority,
        region,
        visible,
        skip,
        gate,
    })
}

fn gate(table: &dyn TableLike) -> Result<Gate, String> {
    let nested = |key: &str| -> Result<Vec<Gate>, String> {
        let Some(item) = table.get(key) else {
            return Ok(Vec::new());
        };
        let array = item
            .as_array()
            .ok_or_else(|| format!("{key} is not an array"))?;
        array
            .iter()
            .map(|value| {
                let inline = value
                    .as_inline_table()
                    .ok_or_else(|| format!("{key} holds something other than a table"))?;
                only(inline, GATE_KEYS, key)?;
                let gate = gate(inline)?;
                // A `not` may hold only negations; anything else must be able
                // to match on its own, or it matches every screen.
                if key != "not" && !gate.has_positive() {
                    return Err(format!("an {key} gate has no positive matcher"));
                }
                Ok(gate)
            })
            .collect()
    };
    let compile = |key: &str| -> Result<Vec<Regex>, String> {
        strings(table, key)?
            .iter()
            .map(|pattern| Regex::new(pattern).map_err(|e| format!("{key} {pattern:?}: {e}")))
            .collect()
    };
    Ok(Gate {
        all: nested("all")?,
        any: nested("any")?,
        not: nested("not")?,
        contains: strings(table, "contains")?
            .iter()
            .map(|needle| needle.to_lowercase())
            .collect(),
        regex: compile("regex")?,
        line_regex: compile("line_regex")?,
    })
}

fn only(table: &dyn TableLike, keys: &[&str], what: &str) -> Result<(), String> {
    match table.iter().find(|(key, _)| !keys.contains(key)) {
        Some((key, _)) => Err(format!("{what} has an unknown key {key:?}")),
        None => Ok(()),
    }
}

fn string(table: &dyn TableLike, key: &str) -> Result<Option<String>, String> {
    table
        .get(key)
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{key} is not a string"))
        })
        .transpose()
}

fn strings(table: &dyn TableLike, key: &str) -> Result<Vec<String>, String> {
    let Some(item) = table.get(key) else {
        return Ok(Vec::new());
    };
    let array: &toml_edit::Array = match item {
        Item::Value(value) => value.as_array(),
        _ => None,
    }
    .ok_or_else(|| format!("{key} is not an array"))?;
    array
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{key} holds something other than a string"))
        })
        .collect()
}

fn parse_region(spec: &str) -> Option<Region> {
    let spec = spec.trim();
    Some(match spec {
        "whole_recent" => Region::WholeRecent,
        "after_last_prompt_marker" => Region::AfterLastPromptMarker,
        "before_current_prompt_marker" => Region::BeforeCurrentPromptMarker,
        "whole_recent_without_current_prompt_marker" => {
            Region::WholeRecentWithoutCurrentPromptMarker
        }
        "current_prompt_block_marker" => Region::CurrentPromptBlockMarker,
        "after_current_prompt_block_marker" => Region::AfterCurrentPromptBlockMarker,
        "prompt_box_body" => Region::PromptBoxBody,
        "above_prompt_box" => Region::AbovePromptBox,
        "last_non_empty_above_prompt_box" => Region::LastNonEmptyAbovePromptBox,
        "after_last_horizontal_rule" => Region::AfterLastHorizontalRule,
        "osc_title" => Region::OscTitle,
        "osc_progress" => Region::OscProgress,
        _ => {
            if let Some(n) = count(spec, "bottom_lines") {
                Region::BottomLines(n)
            } else if let Some(n) = count(spec, "bottom_non_empty_lines") {
                Region::BottomNonEmptyLines(n)
            } else {
                // herdr requires a canonical positive count for this one:
                // no `0`, no `01`, nothing past a u16.
                let n = count(spec, "top_non_empty_lines").filter(|n| {
                    *n > 0
                        && *n <= usize::from(u16::MAX)
                        && spec == format!("top_non_empty_lines({n})")
                })?;
                Region::TopNonEmptyLines(n)
            }
        }
    })
}

fn count(spec: &str, name: &str) -> Option<usize> {
    let digits = spec
        .strip_prefix(name)?
        .strip_prefix('(')?
        .strip_suffix(')')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

// ---------------------------------------------------------------------------
// Regions — herdr's, unchanged in behaviour.
// ---------------------------------------------------------------------------

fn region<'a>(input: Input<'a>, region: Region) -> &'a str {
    let content = input.screen;
    match region {
        Region::OscTitle => input.title,
        Region::OscProgress => input.progress,
        Region::WholeRecent => content,
        Region::AfterLastPromptMarker => after_last_prompt_marker(content),
        Region::BeforeCurrentPromptMarker => before_current_prompt_marker(content),
        Region::WholeRecentWithoutCurrentPromptMarker => {
            whole_recent_without_current_prompt_marker(content)
        }
        Region::CurrentPromptBlockMarker => current_prompt_block_marker(content).unwrap_or(""),
        Region::AfterCurrentPromptBlockMarker => {
            after_current_prompt_block_marker(content).unwrap_or("")
        }
        Region::PromptBoxBody => prompt_box_body(content).unwrap_or(""),
        Region::AbovePromptBox => above_prompt_box(content),
        Region::LastNonEmptyAbovePromptBox => last_non_empty_line(above_prompt_box(content)),
        Region::AfterLastHorizontalRule => after_last_horizontal_rule(content),
        Region::BottomLines(n) => bottom_lines(content, n),
        Region::BottomNonEmptyLines(n) => bottom_non_empty_lines(content, n),
        Region::TopNonEmptyLines(n) => top_non_empty_lines(content, n),
    }
}

fn bottom_lines(content: &str, count: usize) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(count);
    slice_from_line_index(content, &lines, start)
}

fn bottom_non_empty_lines(content: &str, count: usize) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    let Some(start) = lines
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, line)| !line.trim().is_empty())
        .take(count)
        .last()
        .map(|(index, _)| index)
    else {
        return "";
    };
    slice_from_line_index(content, &lines, start)
}

fn top_non_empty_lines(content: &str, count: usize) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    let Some(end) = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .take(count)
        .last()
        .map(|(index, _)| index)
    else {
        return "";
    };
    &content[..line_start_offset(content, &lines, end + 1)]
}

fn after_last_prompt_marker(content: &str) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    let Some(index) = lines.iter().rposition(|line| codex_prompt_line(line)) else {
        return content;
    };
    slice_from_line_index(content, &lines, index + 1)
}

fn before_current_prompt_marker(content: &str) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    let Some(index) = current_codex_prompt_index(&lines) else {
        return content;
    };
    &content[..line_start_offset(content, &lines, index)]
}

fn whole_recent_without_current_prompt_marker(content: &str) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    if current_codex_prompt_index(&lines).is_some() {
        ""
    } else {
        content
    }
}

fn current_prompt_block_marker(content: &str) -> Option<&str> {
    let lines: Vec<&str> = content.lines().collect();
    let prompt = current_codex_prompt_index(&lines)?;
    lines[..prompt]
        .iter()
        .rev()
        .find(|line| codex_block_marker_line(line))
        .copied()
}

fn after_current_prompt_block_marker(content: &str) -> Option<&str> {
    let lines: Vec<&str> = content.lines().collect();
    let prompt = current_codex_prompt_index(&lines)?;
    let block = lines[..prompt]
        .iter()
        .rposition(|line| codex_block_marker_line(line))?;
    Some(slice_from_line_index(content, &lines, block))
}

/// The last `›` prompt line, unless a response block was drawn after it —
/// then that prompt has been sent and is history.
fn current_codex_prompt_index(lines: &[&str]) -> Option<usize> {
    let prompt = lines.iter().rposition(|line| codex_prompt_line(line))?;
    if lines[prompt + 1..]
        .iter()
        .any(|line| codex_block_marker_line(line))
    {
        return None;
    }
    Some(prompt)
}

fn codex_prompt_line(line: &str) -> bool {
    line == "›" || line.starts_with("› ")
}

fn codex_block_marker_line(line: &str) -> bool {
    line.starts_with('•') || line.starts_with('■') || line.starts_with('✗') || line.starts_with('✓')
}

fn prompt_box_body(content: &str) -> Option<&str> {
    let lines: Vec<&str> = content.lines().collect();
    let top = prompt_box_top_border_index(&lines)?;
    let start = line_start_offset(content, &lines, top + 1);
    let end_index = lines[top + 1..]
        .iter()
        .position(|line| is_horizontal_rule(line))
        .map_or(lines.len(), |relative| top + 1 + relative);
    let end = line_start_offset(content, &lines, end_index);
    Some(&content[start.min(content.len())..end.min(content.len())])
}

fn above_prompt_box(content: &str) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    let Some(top) = prompt_box_top_border_index(&lines) else {
        return content;
    };
    &content[..line_start_offset(content, &lines, top)]
}

fn after_last_horizontal_rule(content: &str) -> &str {
    let mut last_rule_end = 0usize;
    let mut offset = 0usize;
    for line in content.lines() {
        let next = offset + line.len() + 1;
        if is_horizontal_rule(line) {
            last_rule_end = next.min(content.len());
        }
        offset = next;
    }
    &content[last_rule_end..]
}

fn last_non_empty_line(content: &str) -> &str {
    content
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
}

/// The second horizontal rule from the bottom: the top edge of a prompt box
/// drawn as two rules with the input between them.
fn prompt_box_top_border_index(lines: &[&str]) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, line)| is_horizontal_rule(line))
        .nth(1)
        .map(|(index, _)| index)
}

/// A row of `─`, or one of at least three followed by a label — Claude Code
/// writes a session's name into its prompt box's top edge.
fn is_horizontal_rule(line: &str) -> bool {
    let trimmed = line.trim();
    let rule_chars = trimmed.chars().take_while(|&ch| ch == '─').count();
    if rule_chars == 0 {
        return false;
    }
    let rule_bytes = rule_chars * '─'.len_utf8();
    trimmed[rule_bytes..].trim_start().is_empty() || rule_chars >= 3
}

fn slice_from_line_index<'a>(content: &'a str, lines: &[&str], index: usize) -> &'a str {
    &content[line_start_offset(content, lines, index)..]
}

/// Where line `index` starts. Assumes `\n` line ends, which is what
/// [`screen_text`] writes.
fn line_start_offset(content: &str, lines: &[&str], index: usize) -> usize {
    lines[..index.min(lines.len())]
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>()
        .min(content.len())
}
