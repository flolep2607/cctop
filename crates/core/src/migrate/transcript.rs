//! A transcript, re-homed: copied into the receiving machine's store under the
//! directory the checkout landed in.
//!
//! Each harness files a session by its working directory and writes that
//! directory into its records. A copy left saying `/home/flo/app` on a machine
//! where the checkout is `/home/florian/app` resumes in a directory that is not
//! there — or, for Claude, is not found at all, because it looks for the
//! session under the project directory named after the new path.
//!
//! Only the fields that name the session's directory are rewritten. Paths
//! inside messages — a tool call's `file_path`, a command the agent ran — are
//! what happened, and are left as written.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};

use super::Harness;

/// What the receiving side needs to know about the file beside its bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Meta {
    /// The directory the session ran in here, as its records spell it.
    pub old_cwd: String,
    /// Codex files a rollout under the day it began, `YYYY/MM/DD`.
    pub codex_dated: Option<String>,
    /// And names it `rollout-<time>-<id>.jsonl`; this is the part before the id.
    pub codex_prefix: Option<String>,
}

/// The transcript to send, and what to say about it.
pub fn export(harness: Harness, file: &Path, cwd: &str, id: &str) -> Result<Meta, String> {
    let mut meta = Meta {
        old_cwd: cwd.to_string(),
        codex_dated: None,
        codex_prefix: None,
    };
    if harness == Harness::Codex {
        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("its rollout has no file name")?;
        let prefix = stem
            .strip_suffix(id)
            .ok_or("its rollout's name does not end in its id")?;
        meta.codex_prefix = Some(prefix.to_string());
        let parts: Vec<String> = file
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        meta.codex_dated = parts
            .iter()
            .rposition(|p| p == "sessions")
            .map(|at| parts[at + 1..parts.len() - 1].join("/"));
    }
    Ok(meta)
}

/// `value` with `old` swapped for `new` when it is that directory or under it.
fn rebase(value: &str, old: &str, new: &str) -> Option<String> {
    if value == old {
        return Some(new.to_string());
    }
    value
        .strip_prefix(old)
        .filter(|rest| rest.starts_with('/'))
        .map(|rest| format!("{new}{rest}"))
}

fn rebase_in(value: &mut serde_json::Value, old: &str, new: &str) {
    if let Some(moved) = value.as_str().and_then(|v| rebase(v, old, new)) {
        *value = serde_json::Value::String(moved);
    }
}

/// Install the transcript at `staged` into the store under `store` (the
/// receiving `CLAUDE_CONFIG_DIR` or `CODEX_HOME`), for the session to resume
/// in `new_cwd`. Answers the id it is known by there and the file written.
///
/// The id is kept when nothing there has it, and a new one is minted when
/// something does — a session moved back to the machine it came from finds
/// its old self there — so nothing is ever written over.
pub fn install(
    harness: Harness,
    meta: &Meta,
    old_id: &str,
    staged: &Path,
    store: &Path,
    new_cwd: &str,
) -> Result<(String, PathBuf), String> {
    let (dir, id, name) = match harness {
        Harness::Claude => {
            let taken = crate::convert::claude_id_taken(store, old_id);
            let id = match crate::config::is_full_uuid(old_id) && !taken {
                true => old_id.to_string(),
                false => crate::util::new_uuid(),
            };
            let dir = store
                .join("projects")
                .join(crate::convert::project_slug(Path::new(new_cwd)));
            let name = format!("{id}.jsonl");
            (dir, id, name)
        }
        Harness::Codex => {
            let dated = meta.codex_dated.as_deref().unwrap_or_default();
            let prefix = meta.codex_prefix.as_deref().unwrap_or("rollout-");
            // Both come from the sender and become a path here, so they are
            // held to the shapes Codex itself writes.
            let dated_ok = dated.split('/').count() <= 3
                && dated
                    .split('/')
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
            let prefix_ok = prefix.starts_with("rollout-")
                && prefix.len() <= 80
                && prefix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
            if !(dated.is_empty() || dated_ok) || !prefix_ok {
                return Err("the rollout's name is not one Codex writes".into());
            }
            let taken = crate::convert::codex_id_taken(store, old_id);
            let id = match crate::config::is_full_uuid(old_id) && !taken {
                true => old_id.to_string(),
                false => crate::util::new_uuid(),
            };
            let mut dir = store.join("sessions");
            if !dated.is_empty() {
                dir = dir.join(dated);
            }
            let name = format!("{prefix}{id}.jsonl");
            (dir, id, name)
        }
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(name);
    let source = std::fs::File::open(staged).map_err(|e| format!("the transcript: {e}"))?;
    // `create_new`: an id checked free a moment ago that is not free now is a
    // conversation this would overwrite.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = BufWriter::new(file);
    let write = |out: &mut BufWriter<std::fs::File>, line: &str| {
        writeln!(out, "{line}").map_err(|e| format!("{}: {e}", path.display()))
    };
    for line in std::io::BufReader::new(source).lines() {
        let line = line.map_err(|e| format!("the transcript: {e}"))?;
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&line) else {
            // A line this does not understand is still one the harness wrote
            // and will read.
            write(&mut out, &line)?;
            continue;
        };
        match harness {
            Harness::Claude => rewrite_claude(&mut value, meta, old_id, &id, new_cwd),
            Harness::Codex => rewrite_codex(&mut value, meta, old_id, &id, new_cwd),
        }
        write(&mut out, &value.to_string())?;
    }
    out.flush()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((id, path))
}

fn rewrite_claude(v: &mut serde_json::Value, meta: &Meta, old_id: &str, id: &str, cwd: &str) {
    let Some(record) = v.as_object_mut() else {
        return;
    };
    if let Some(dir) = record.get_mut("cwd") {
        rebase_in(dir, &meta.old_cwd, cwd);
    }
    if record.get("sessionId").and_then(|s| s.as_str()) == Some(old_id) {
        record.insert("sessionId".into(), id.into());
    }
}

fn rewrite_codex(v: &mut serde_json::Value, meta: &Meta, old_id: &str, id: &str, cwd: &str) {
    let kind = v.get("type").and_then(|t| t.as_str()).map(str::to_string);
    let Some(payload) = v.get_mut("payload").and_then(|p| p.as_object_mut()) else {
        return;
    };
    match kind.as_deref() {
        Some("session_meta") => {
            for key in ["id", "session_id"] {
                if payload.get(key).and_then(|s| s.as_str()) == Some(old_id) {
                    payload.insert(key.into(), id.into());
                }
            }
            if let Some(dir) = payload.get_mut("cwd") {
                rebase_in(dir, &meta.old_cwd, cwd);
            }
        }
        Some("turn_context") => {
            if let Some(dir) = payload.get_mut("cwd") {
                rebase_in(dir, &meta.old_cwd, cwd);
            }
            if let Some(roots) = payload
                .get_mut("sandbox_policy")
                .and_then(|p| p.get_mut("writable_roots"))
                .and_then(|r| r.as_array_mut())
            {
                for root in roots {
                    rebase_in(root, &meta.old_cwd, cwd);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0b4e8a54-8d39-4f4e-8a39-6b8a3c1c2d01";

    fn meta(old: &str) -> Meta {
        Meta {
            old_cwd: old.into(),
            codex_dated: None,
            codex_prefix: None,
        }
    }

    /// The records name the new directory and land in the project directory
    /// Claude derives from it; a path inside a message is left as it was.
    #[test]
    fn a_claude_transcript_is_rehomed_under_the_new_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = tmp.path().join("in.jsonl");
        std::fs::write(
            &staged,
            format!(
                "{{\"type\":\"user\",\"sessionId\":\"{ID}\",\"cwd\":\"/home/flo/app/sub\",\"message\":{{\"content\":\"edit /home/flo/app/x\"}}}}\n\
                 not json\n\
                 {{\"type\":\"assistant\",\"sessionId\":\"{ID}\",\"cwd\":\"/home/flo/apple\"}}\n"
            ),
        )
        .unwrap();
        let store = tmp.path().join("claude");
        let (id, path) = install(
            Harness::Claude,
            &meta("/home/flo/app"),
            ID,
            &staged,
            &store,
            "/srv/florian/app",
        )
        .unwrap();
        assert_eq!(id, ID);
        assert_eq!(
            path,
            store
                .join("projects/-srv-florian-app")
                .join(format!("{ID}.jsonl"))
        );
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["cwd"], "/srv/florian/app/sub");
        assert_eq!(first["message"]["content"], "edit /home/flo/app/x");
        assert_eq!(lines[1], "not json");
        // A sibling that merely shares the prefix is not under it.
        let third: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(third["cwd"], "/home/flo/apple");
    }

    /// Moved back to where it began, the old copy is still there: a new id,
    /// and nothing overwritten.
    #[test]
    fn an_id_already_there_gets_a_new_one() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("claude");
        let old = store.join("projects/-x").join(format!("{ID}.jsonl"));
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, "keep\n").unwrap();
        let staged = tmp.path().join("in.jsonl");
        std::fs::write(
            &staged,
            format!("{{\"sessionId\":\"{ID}\",\"cwd\":\"/a\"}}\n"),
        )
        .unwrap();
        let (id, path) = install(Harness::Claude, &meta("/a"), ID, &staged, &store, "/b").unwrap();
        assert_ne!(id, ID);
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "keep\n");
        let line: serde_json::Value =
            serde_json::from_str(std::fs::read_to_string(path).unwrap().trim()).unwrap();
        assert_eq!(line["sessionId"], id.as_str());
    }

    #[test]
    fn a_codex_rollout_keeps_its_day_and_name_and_moves_its_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join(format!(
            "codex/sessions/2026/11/05/rollout-2026-11-05T10-00-00-{ID}.jsonl"
        ));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{ID}\",\"cwd\":\"/home/flo/app\"}}}}\n\
                 {{\"type\":\"turn_context\",\"payload\":{{\"cwd\":\"/home/flo/app\",\"sandbox_policy\":{{\"writable_roots\":[\"/home/flo/app/out\",\"/tmp\"]}}}}}}\n"
            ),
        )
        .unwrap();
        let meta = export(Harness::Codex, &file, "/home/flo/app", ID).unwrap();
        assert_eq!(meta.codex_dated.as_deref(), Some("2026/11/05"));
        assert_eq!(
            meta.codex_prefix.as_deref(),
            Some("rollout-2026-11-05T10-00-00-")
        );
        let store = tmp.path().join("there");
        let (id, path) = install(Harness::Codex, &meta, ID, &file, &store, "/srv/app").unwrap();
        assert_eq!(id, ID);
        assert_eq!(
            path,
            store.join(format!(
                "sessions/2026/11/05/rollout-2026-11-05T10-00-00-{ID}.jsonl"
            ))
        );
        let text = std::fs::read_to_string(path).unwrap();
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines[0]["payload"]["cwd"], "/srv/app");
        assert_eq!(lines[1]["payload"]["cwd"], "/srv/app");
        assert_eq!(
            lines[1]["payload"]["sandbox_policy"]["writable_roots"],
            serde_json::json!(["/srv/app/out", "/tmp"])
        );
    }

    /// The day and the name come from the sender and become a path here.
    #[test]
    fn a_rollout_name_that_walks_out_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = tmp.path().join("in.jsonl");
        std::fs::write(&staged, "{}\n").unwrap();
        for (dated, prefix) in [
            ("../../etc", "rollout-"),
            ("2026", "../rollout-"),
            ("2026", "x-"),
        ] {
            let meta = Meta {
                old_cwd: "/a".into(),
                codex_dated: Some(dated.into()),
                codex_prefix: Some(prefix.into()),
            };
            assert!(install(Harness::Codex, &meta, ID, &staged, tmp.path(), "/b").is_err());
        }
    }
}
