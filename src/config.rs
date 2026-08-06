//! Filesystem locations and process-wide constants.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub static HOME: LazyLock<PathBuf> =
    LazyLock::new(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")));

/// `$CLAUDE_CONFIG_DIR`, falling back to `~/.claude`.
pub static CLAUDE_CONFIG_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| HOME.join(".claude"))
});

pub static CLAUDE_PROJECTS_ROOT: LazyLock<PathBuf> =
    LazyLock::new(|| CLAUDE_CONFIG_DIR.join("projects"));

/// `$CODEX_HOME`, falling back to `~/.codex`.
pub static CODEX_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| HOME.join(".codex"))
});

pub static CODEX_SESSIONS_ROOT: LazyLock<PathBuf> = LazyLock::new(|| CODEX_HOME.join("sessions"));

/// Cursor's native agent transcripts, grouped by project slug.
pub static CURSOR_PROJECTS_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("CURSOR_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| HOME.join(".cursor"))
        .join("projects")
});

/// `$PI_CODING_AGENT_DIR`, falling back to `~/.pi/agent`.
pub static PI_AGENT_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("PI_CODING_AGENT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| HOME.join(".pi").join("agent"))
});

/// `$PI_CODING_AGENT_SESSION_DIR`, falling back to Pi's standard session root.
pub static PI_SESSIONS_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("PI_CODING_AGENT_SESSION_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PI_AGENT_DIR.join("sessions"))
});

/// `$GEMINI_DIR`, falling back to `~/.gemini`.
pub static GEMINI_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("GEMINI_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| HOME.join(".gemini"))
});

/// Gemini CLI files its chats under a scratch directory, one subtree per
/// project: `tmp/<project>/chats/session-*.json{,l}`. The name reads like
/// something disposable, but it is where the transcripts actually live.
pub static GEMINI_CHATS_ROOT: LazyLock<PathBuf> = LazyLock::new(|| GEMINI_HOME.join("tmp"));

/// Windsurf keeps per-workspace editor state where its VS Code base does.
///
/// `$WINDSURF_USER_DIR` overrides the whole `User` directory, which is what a
/// portable install moves; without it, follow the platform convention.
pub static WINDSURF_USER_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    if let Some(dir) = std::env::var_os("WINDSURF_USER_DIR") {
        return PathBuf::from(dir);
    }
    let base = if cfg!(target_os = "macos") {
        HOME.join("Library").join("Application Support")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| HOME.join("AppData").join("Roaming"))
    } else {
        dirs::config_dir().unwrap_or_else(|| HOME.join(".config"))
    };
    base.join("Windsurf").join("User")
});

pub static WINDSURF_WORKSPACE_STORAGE: LazyLock<PathBuf> =
    LazyLock::new(|| WINDSURF_USER_DIR.join("workspaceStorage"));

/// OpenCode follows the platform data directory (`~/.local/share` on Linux).
pub static OPENCODE_DATA_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("OPENCODE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| HOME.join(".local").join("share"))
                .join("opencode")
        })
});

/// Cowork (VM) sessions. macOS only.
pub static CLAUDE_MAC_COWORK_ROOT: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
    cfg!(target_os = "macos").then(|| {
        HOME.join("Library")
            .join("Application Support")
            .join("Claude")
            .join("local-agent-mode-sessions")
    })
});

/// Claude Code sessions launched from the desktop app. macOS only.
pub static CLAUDE_MAC_CODE_ROOT: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
    cfg!(target_os = "macos").then(|| {
        HOME.join("Library")
            .join("Application Support")
            .join("Claude")
            .join("claude-code-sessions")
    })
});

pub static CACHE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    dirs::cache_dir()
        .unwrap_or_else(|| HOME.join(".cache"))
        .join("cctop")
});

pub static COST_CACHE_FILE: LazyLock<PathBuf> = LazyLock::new(|| CACHE_DIR.join("cost-cache.json"));
pub static PRICING_CACHE_FILE: LazyLock<PathBuf> =
    LazyLock::new(|| CACHE_DIR.join("litellm-pricing.json"));
pub static UI_PREFS_FILE: LazyLock<PathBuf> = LazyLock::new(|| CACHE_DIR.join("ui-prefs.json"));

pub const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

pub const PRICING_CACHE_MAX_AGE_SECS: u64 = 24 * 60 * 60;

/// Claude Code triggers auto-compaction at ~83.5% of the context window.
/// Overridable via `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` (an integer percentage).
pub static COMPACT_THRESHOLD: LazyLock<f64> = LazyLock::new(|| {
    std::env::var("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .map(|p| p / 100.0)
        .unwrap_or(0.835)
});

/// Lines above this size are almost always base64 image payloads carrying no
/// token, cost, or tool data. Skipping them keeps large transcripts cheap.
pub const MAX_JSONL_LINE_BYTES: usize = 512 * 1024;

/// Cap on retained per-tool invocation details, keeping the newest.
pub const MAX_TOOL_DETAILS: usize = 200;

/// Cap on diff lines kept per edit, so a large refactor doesn't bloat the cache.
pub const MAX_DIFF_LINES: usize = 60;

pub const CLAUDE_DEFAULT_CTX: u64 = 200_000;
pub const CLAUDE_1M_CTX: u64 = 1_048_576;
pub const CODEX_DEFAULT_CTX: u64 = 258_400;

/// `true` if `s` is exactly a lowercase hyphenated UUID.
pub fn is_full_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (i, c) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if *c != b'-' {
                    return false;
                }
            }
            _ => {
                if !c.is_ascii_hexdigit() || c.is_ascii_uppercase() {
                    return false;
                }
            }
        }
    }
    true
}

/// Extract a trailing UUID from a Codex rollout filename stem.
pub fn trailing_uuid(stem: &str) -> Option<&str> {
    if stem.len() < 36 {
        return None;
    }
    let tail = &stem[stem.len() - 36..];
    is_full_uuid(tail).then_some(tail)
}

pub fn dir_exists(p: &Path) -> bool {
    p.is_dir()
}

/// Directory entry names, sorted. Returns empty on any IO error.
pub fn list_dir(p: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(p) else {
        return Vec::new();
    };
    let mut out: Vec<String> = rd
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    out.sort();
    out
}

/// Recursively collect files under `dir` whose name ends with `ext`.
pub fn rglob(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in rd.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            let path = entry.path();
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() && path.to_string_lossy().ends_with(ext) {
                out.push(path);
            }
        }
    }
    out
}

/// Modification time in milliseconds since the Unix epoch, or 0 if unavailable.
pub fn file_mtime_ms(p: &Path) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_validation() {
        assert!(is_full_uuid("7026d578-8cba-4880-b464-9700f1b77b71"));
        assert!(!is_full_uuid("7026D578-8CBA-4880-B464-9700F1B77B71")); // uppercase
        assert!(!is_full_uuid("7026d578-8cba-4880-b464-9700f1b77b7")); // short
        assert!(!is_full_uuid("7026d5788cba4880b4649700f1b77b71")); // no hyphens
    }

    #[test]
    fn uuid_extraction() {
        let stem = "rollout-2026-06-29T10-59-07-019f1075-3f22-7ad0-b496-73dcda6a7a25";
        assert_eq!(
            trailing_uuid(stem),
            Some("019f1075-3f22-7ad0-b496-73dcda6a7a25")
        );
    }
}
