//! Shared JSONL reading and tool-argument summarising.

use crate::config::MAX_JSONL_LINE_BYTES;
use regex::Regex;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::LazyLock;

/// Stream a `.jsonl` file, invoking `f` for each parseable object.
///
/// Oversized lines (base64 image payloads) and malformed lines (truncated or
/// null-padded tails from a crashed writer) are skipped rather than fatal.
pub fn for_each_jsonl<F: FnMut(&Value)>(path: &Path, mut f: F) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let mut buf = Vec::with_capacity(8 * 1024);

    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        if buf.len() > MAX_JSONL_LINE_BYTES {
            continue;
        }
        let line = buf.trim_ascii();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_slice::<Value>(line) {
            f(&v);
        }
    }
    Ok(())
}

/// Parse up to `max_lines` objects from the head of a file.
pub fn read_first_lines(path: &Path, max_lines: usize) -> Vec<Value> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let reader = BufReader::with_capacity(64 * 1024, file);
    let mut out = Vec::new();
    for line in reader.lines().map_while(Result::ok) {
        if out.len() >= max_lines {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            out.push(v);
        }
    }
    out
}

/// Parse up to `max_lines` objects from the tail of a file, newest first.
///
/// Reads backwards in chunks so this stays cheap on multi-hundred-megabyte
/// transcripts where we only care about the most recent entries.
pub fn read_last_lines(path: &Path, max_lines: usize) -> Vec<Value> {
    const CHUNK: u64 = 64 * 1024;
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let Ok(size) = file.metadata().map(|m| m.len()) else {
        return Vec::new();
    };

    let mut pos = size;
    let mut carry = String::new();
    let mut out = Vec::new();

    while pos > 0 && out.len() < max_lines {
        let read_size = CHUNK.min(pos);
        pos -= read_size;
        if file.seek(SeekFrom::Start(pos)).is_err() {
            break;
        }
        let mut chunk = vec![0u8; read_size as usize];
        if file.read_exact(&mut chunk).is_err() {
            break;
        }
        let text = format!("{}{}", String::from_utf8_lossy(&chunk), carry);
        let mut lines: Vec<&str> = text.split('\n').collect();

        // The first element may be a partial line continuing into the previous
        // chunk; hold it back unless we've reached the start of the file.
        carry = if pos > 0 && !lines.is_empty() {
            lines.remove(0).to_string()
        } else {
            String::new()
        };

        for line in lines.iter().rev() {
            if out.len() >= max_lines {
                break;
            }
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                out.push(v);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tool argument summarising
// ---------------------------------------------------------------------------

/// A `mcp__<uuid>__` prefix embedded inside a `ToolSearch` select: query.
static MCP_UUID_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)mcp__[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}__")
        .expect("static regex")
});

/// Keys whose values are likely to be user-meaningful, for unknown MCP tools.
static MEANINGFUL_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)query|search|keyword|message|text|content|prompt|input|command|path|url|topic|channel|name",
    )
    .expect("static regex")
});

fn first_line(s: &str, limit: usize) -> String {
    let line = s.split(['\r', '\n']).next().unwrap_or("");
    line.chars().take(limit).collect()
}

/// Collapse a multi-line command into one summary line.
///
/// Showing only the first line is actively misleading: a heredoc or a `cd`
/// followed by the real work reads as though that first line is the whole
/// command. Joining with a visible `↵` keeps it honest, and the trailing `…`
/// marks that there is more than fits.
fn flatten(s: &str, limit: usize) -> String {
    let joined = s
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ↵ ");
    if joined.chars().count() <= limit {
        joined
    } else {
        let mut out: String = joined.chars().take(limit).collect();
        out.push('…');
        out
    }
}

fn str_field<'a>(input: &'a Value, key: &str) -> &'a str {
    input.get(key).and_then(Value::as_str).unwrap_or("")
}

fn file_field(input: &Value) -> &str {
    ["file_path", "filePath", "path"]
        .into_iter()
        .find_map(|key| input.get(key).and_then(Value::as_str))
        .unwrap_or("")
}

/// Short display text and full clipboard text for a tool invocation.
///
/// The two differ only where the display form is truncated (Bash commands,
/// Agent prompts); everywhere else `full` is `None`.
pub fn tool_detail(name: &str, input: &Value) -> (String, Option<String>) {
    if !input.is_object() {
        return (String::new(), None);
    }

    // Tools whose full argument is worth keeping for the clipboard.
    match name {
        "Bash" | "bash" => {
            let cmd = str_field(input, "command");
            let short = flatten(cmd, 300);
            let full = (full_differs(&short, cmd)).then(|| cmd.to_string());
            return (short, full);
        }
        "Agent" | "agent" => {
            let p = str_field(input, "prompt");
            let short = flatten(p, 200);
            let full = (full_differs(&short, p)).then(|| p.to_string());
            return (short, full);
        }
        "TaskCreate" | "task" => {
            let d = str_field(input, "description");
            let short = flatten(d, 200);
            let full = (full_differs(&short, d)).then(|| d.to_string());
            return (short, full);
        }
        _ => {}
    }

    let s = match name {
        "Read" | "read" | "Edit" | "edit" | "Write" | "write" => file_field(input).to_string(),
        "Glob" | "glob" => str_field(input, "pattern").to_string(),
        "WebFetch" | "webfetch" => str_field(input, "url").to_string(),
        "WebSearch" | "websearch" => str_field(input, "query").to_string(),
        "Grep" | "grep" => {
            let pattern = str_field(input, "pattern");
            let path = str_field(input, "path");
            if path.is_empty() {
                pattern.to_string()
            } else {
                format!("{pattern} in {path}")
            }
        }
        "ApplyPatch" | "apply_patch" => input
            .get("patch")
            .or_else(|| input.get("input"))
            .and_then(Value::as_str)
            .map(|patch| parse_apply_patch(patch).0)
            .unwrap_or_default(),
        "ToolSearch" => MCP_UUID_PREFIX
            .replace_all(str_field(input, "query"), "")
            .into_owned(),
        "TaskUpdate" => match input.get("task_id") {
            Some(id) => {
                let id = id
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| id.to_string());
                let status = str_field(input, "status");
                format!("#{id} {status}").trim().to_string()
            }
            None => String::new(),
        },
        "TaskGet" | "TaskStop" | "TaskOutput" => match input.get("task_id") {
            Some(id) => format!(
                "#{}",
                id.as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| id.to_string())
            ),
            None => String::new(),
        },
        "TaskList" => "(list)".to_string(),
        "web__run" | "web.run" => web_run_detail(input),
        "write_stdin" => {
            // Codex names this field `chars`; an empty one means the call is
            // just polling the session for more output, not writing anything.
            let raw = input
                .get("chars")
                .or_else(|| input.get("input"))
                .or_else(|| input.get("stdin"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if raw.is_empty() {
                match input.get("session_id") {
                    Some(id) => format!("(poll session {id})"),
                    None => "(poll)".into(),
                }
            } else {
                flatten(raw, 200)
            }
        }
        "update_plan" => {
            let Some(steps) = input.get("plan").and_then(Value::as_array) else {
                return (String::new(), None);
            };
            let done = steps
                .iter()
                .filter(|s| s.get("status").and_then(Value::as_str) == Some("completed"))
                .count();
            // The step actually being worked on is the informative one.
            let current = steps
                .iter()
                .find(|s| s.get("status").and_then(Value::as_str) == Some("in_progress"))
                .or_else(|| steps.first())
                .and_then(|s| s.get("step"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let full = steps
                .iter()
                .filter_map(|s| {
                    let step = s.get("step").and_then(Value::as_str)?;
                    let status = s.get("status").and_then(Value::as_str).unwrap_or("");
                    Some(format!("[{status}] {step}"))
                })
                .collect::<Vec<_>>()
                .join("\n");
            let short = format!(
                "{}/{} → {}",
                done,
                steps.len(),
                truncate_chars(current, 120)
            );
            return (short, (!full.is_empty()).then_some(full));
        }
        _ => generic_detail(input),
    };
    (s, None)
}

/// Summarise the meaningful target of Codex's bundled web tool. Its payload
/// commonly also carries `response_length`, which is display configuration,
/// not what the agent actually asked the web service to do.
fn web_run_detail(input: &Value) -> String {
    // Keep the query-like operations first: a single call can include several
    // operation types, but the search is normally the useful activity detail.
    for (key, field, label) in [
        ("search_query", "q", "search"),
        ("image_query", "q", "images"),
        ("open", "ref_id", "open"),
        ("find", "pattern", "find"),
        ("click", "ref_id", "click"),
        ("finance", "ticker", "finance"),
        ("weather", "location", "weather"),
        ("sports", "league", "sports"),
        ("time", "utc_offset", "time"),
    ] {
        let Some(items) = input.get(key).and_then(Value::as_array) else {
            continue;
        };
        let targets = items
            .iter()
            .filter_map(|item| item.get(field).and_then(Value::as_str))
            .filter(|value| !value.is_empty())
            .take(4)
            .collect::<Vec<_>>();
        if !targets.is_empty() {
            return format!("{label}: {}", truncate_chars(&targets.join(" · "), 120));
        }
    }
    generic_detail(input)
}

fn truncate_chars(s: &str, limit: usize) -> String {
    if s.chars().count() <= limit {
        return s.to_string();
    }
    let mut out: String = s.chars().take(limit).collect();
    out.push('…');
    out
}

/// `flatten` for callers outside this module.
pub fn flatten_public(s: &str, limit: usize) -> String {
    flatten(s, limit)
}

/// Summarise an `apply_patch` payload and count the lines it changes.
///
/// Codex passes the patch as a raw string rather than JSON, so this reads the
/// `*** Update File:` markers and the +/- lines directly.
pub fn parse_apply_patch(patch: &str) -> (String, super::Delta) {
    let mut files: Vec<String> = Vec::new();
    let mut delta = super::Delta::default();
    for line in patch.lines() {
        if let Some(rest) = line
            .strip_prefix("*** Update File: ")
            .or_else(|| line.strip_prefix("*** Add File: "))
            .or_else(|| line.strip_prefix("*** Delete File: "))
        {
            files.push(rest.trim().to_string());
            continue;
        }
        if line.starts_with("***") || line.starts_with("@@") {
            continue;
        }
        if line.starts_with('+') {
            delta.added += 1;
        } else if line.starts_with('-') {
            delta.removed += 1;
        }
        if delta.hunks.len() < crate::config::MAX_DIFF_LINES
            && (line.starts_with('+') || line.starts_with('-') || line.starts_with(' '))
        {
            delta.hunks.push(line.to_string());
        }
    }
    let summary = match files.len() {
        0 => "(patch)".to_string(),
        1 => files.remove(0),
        n => format!("{} (+{} more)", files[0], n - 1),
    };
    (summary, delta)
}

fn full_differs(short: &str, full: &str) -> bool {
    !full.is_empty() && short != full
}

/// Best-effort summary for tools we don't have a specific rule for (mostly MCP).
///
/// Strings win outright, arrays are joined, and numbers/bools are skipped —
/// a bare `true` or `3` tells the reader nothing. Keys that look like they hold
/// user-visible content are preferred over whatever comes first.
fn generic_detail(input: &Value) -> String {
    let Some(map) = input.as_object() else {
        return String::new();
    };
    let mut fallback = String::new();
    for (k, v) in map {
        let candidate = match v {
            Value::String(s) if !s.is_empty() => {
                let t = s.trim();
                // Skip opaque hex identifiers.
                if (7..=40).contains(&t.len()) && t.chars().all(|c| c.is_ascii_hexdigit()) {
                    continue;
                }
                first_line(s, 120)
            }
            Value::Array(items) => {
                let strings: Vec<&str> = items.iter().filter_map(Value::as_str).take(5).collect();
                if strings.is_empty() {
                    continue;
                }
                strings.join(", ").chars().take(120).collect()
            }
            _ => continue,
        };
        if MEANINGFUL_KEY.is_match(k) {
            return candidate;
        }
        if fallback.is_empty() {
            fallback = candidate;
        }
    }
    fallback
}

/// Append an invocation detail, capping retained history at the newest N.
pub fn push_tool_detail(
    details: &mut std::collections::HashMap<String, Vec<super::ToolDetail>>,
    name: &str,
    short: String,
    full: Option<String>,
    ts: String,
    id: Option<String>,
    origin: Option<String>,
) {
    let entry = details.entry(name.to_string()).or_default();
    entry.push(super::ToolDetail {
        d: if short.is_empty() {
            "(no args)".to_string()
        } else {
            short
        },
        ts,
        full,
        id,
        origin,
        ..Default::default()
    });
    if entry.len() > crate::config::MAX_TOOL_DETAILS {
        let excess = entry.len() - crate::config::MAX_TOOL_DETAILS;
        entry.drain(0..excess);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bash_keeps_full_command_for_clipboard() {
        let (short, full) = tool_detail("Bash", &json!({"command": "ls -la\ncd /tmp"}));
        // Both lines appear: showing only "ls -la" would misrepresent the command.
        assert_eq!(short, "ls -la ↵ cd /tmp");
        assert_eq!(full.as_deref(), Some("ls -la\ncd /tmp"));
    }

    /// Regression: a `cd` followed by a heredoc used to render as just the `cd`,
    /// which reads as though that was the whole command.
    #[test]
    fn multiline_command_is_not_truncated_to_its_first_line() {
        let cmd = "cd /home/flo/cctop\npython3 - <<'PY'\nprint(1)\nPY";
        let (short, full) = tool_detail("Bash", &json!({ "command": cmd }));
        assert!(short.starts_with("cd /home/flo/cctop ↵ python3"), "{short}");
        assert_eq!(full.as_deref(), Some(cmd));
    }

    #[test]
    fn overlong_command_is_elided_with_an_ellipsis() {
        let cmd = "x".repeat(500);
        let (short, _) = tool_detail("Bash", &json!({ "command": cmd }));
        assert!(short.ends_with('…'));
        assert_eq!(short.chars().count(), 301);
    }

    #[test]
    fn single_line_bash_has_no_separate_full() {
        let (short, full) = tool_detail("Bash", &json!({"command": "ls"}));
        assert_eq!(short, "ls");
        assert_eq!(full, None);
    }

    #[test]
    fn grep_combines_pattern_and_path() {
        let (s, _) = tool_detail("Grep", &json!({"pattern": "TODO", "path": "src"}));
        assert_eq!(s, "TODO in src");
        let (s, _) = tool_detail("Grep", &json!({"pattern": "TODO"}));
        assert_eq!(s, "TODO");
    }

    #[test]
    fn toolsearch_strips_uuid_server_prefix() {
        let (s, _) = tool_detail(
            "ToolSearch",
            &json!({"query": "select:mcp__0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0__send"}),
        );
        assert_eq!(s, "select:send");
    }

    /// Regression: these three Codex tools all rendered as "(no args)".
    #[test]
    fn codex_update_plan_shows_progress_and_current_step() {
        let input = json!({"plan":[
            {"step":"Inspect metrics","status":"completed"},
            {"step":"Add p1/p25 tracking","status":"in_progress"},
            {"step":"Expose fields","status":"pending"}
        ]});
        let (short, full) = tool_detail("update_plan", &input);
        assert_eq!(short, "1/3 → Add p1/p25 tracking");
        assert!(full.unwrap().contains("[completed] Inspect metrics"));
    }

    #[test]
    fn codex_write_stdin_uses_the_chars_field() {
        let (short, _) = tool_detail("write_stdin", &json!({"chars":"yes\n","session_id":1}));
        assert_eq!(short, "yes");
        // An empty write is a poll for more output, not a no-op.
        let (short, _) = tool_detail("write_stdin", &json!({"chars":"","session_id":87493}));
        assert_eq!(short, "(poll session 87493)");
    }

    #[test]
    fn apply_patch_yields_file_and_line_counts() {
        let patch = "*** Begin Patch\n*** Update File: /home/flo/rusty/src/raw_h2/conn.rs\n@@\n use std::{\n-        Arc,\n+        Arc, Mutex,\n+    extra,\n*** End Patch";
        let (summary, delta) = parse_apply_patch(patch);
        assert_eq!(summary, "/home/flo/rusty/src/raw_h2/conn.rs");
        assert_eq!((delta.added, delta.removed), (2, 1));
        assert!(!delta.hunks.is_empty());
    }

    #[test]
    fn apply_patch_names_extra_files_without_listing_all() {
        let patch = "*** Update File: a.rs\n*** Add File: b.rs\n*** Delete File: c.rs\n";
        let (summary, _) = parse_apply_patch(patch);
        assert_eq!(summary, "a.rs (+2 more)");
    }

    #[test]
    fn generic_prefers_meaningful_keys_and_skips_hex_ids() {
        let (s, _) = tool_detail(
            "mcp__x__y",
            &json!({"commit": "a1b2c3d4e5f6", "query": "hello world", "verbose": true}),
        );
        assert_eq!(s, "hello world");
    }

    #[test]
    fn generic_joins_arrays() {
        let (s, _) = tool_detail("mcp__x__y", &json!({"keywords": ["a", "b", "c"]}));
        assert_eq!(s, "a, b, c");
    }

    #[test]
    fn codex_web_run_shows_queries_not_response_length() {
        let (s, _) = tool_detail(
            "web__run",
            &json!({
                "search_query": [{"q": "GitHub Actions tag from Cargo version"}],
                "response_length": "medium"
            }),
        );
        assert_eq!(s, "search: GitHub Actions tag from Cargo version");
    }

    #[test]
    fn codex_web_run_summarises_multiple_request_targets() {
        let (s, _) = tool_detail(
            "web__run",
            &json!({"open": [{"ref_id": "turn1search0"}, {"ref_id": "turn1search1"}]}),
        );
        assert_eq!(s, "open: turn1search0 · turn1search1");
    }

    #[test]
    fn tool_details_cap_retains_newest() {
        let mut map = std::collections::HashMap::new();
        for i in 0..(crate::config::MAX_TOOL_DETAILS + 10) {
            push_tool_detail(
                &mut map,
                "Bash",
                format!("cmd{i}"),
                None,
                String::new(),
                None,
                None,
            );
        }
        let entry = &map["Bash"];
        assert_eq!(entry.len(), crate::config::MAX_TOOL_DETAILS);
        assert_eq!(
            entry.last().unwrap().d,
            format!("cmd{}", crate::config::MAX_TOOL_DETAILS + 9)
        );
    }
}
