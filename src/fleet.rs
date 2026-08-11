//! Sessions from other machines, read over ssh.
//!
//! cctop is otherwise a monitor of one computer, which is the wrong shape for
//! the way people actually run agents: a laptop in front of them and a devbox
//! doing the heavy work, each with its own idea of what today cost. `--host`
//! merges the second into the first.
//!
//! The mechanism is deliberately the dullest one available. A thread per host
//! runs `ssh <host> cctop --json` on a timer and parses what comes back — no
//! daemon, no port, no protocol of cctop's own, and nothing to install on the
//! far side beyond the cctop that is already there. ssh has the authentication
//! and the transport, and `--json` is the wire format whether or not anyone
//! reads it over a wire.
//!
//! Remote rows carry [`Remote`], which is the single test every action guards
//! on. A signal, a deleted transcript, a pty, a git directory: all of those are
//! about *this* filesystem, and would quietly do the wrong thing to whatever
//! happens to live at the same path here.
//!
//! ponytail: read-only, and one direction. Attaching to, typing into or
//! stopping a remote agent would each need cctop to be running on the far side
//! and listening — a different and much larger thing than reading a snapshot,
//! and the reading is the half with no failure mode worse than a stale row.

use crate::pricing::Provider;
use crate::session::{ActivityState, ContextUsage, Remote, Session, Surface};
use crate::util;
use serde_json::Value;
use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

/// How long between polls of one host.
///
/// Much slower than the local refresh, and for a different reason than cost: an
/// ssh round trip spawns a process on both ends, and a dashboard that did that
/// every two seconds would be a noticeable load on a machine whose whole job is
/// to be doing something else. Fifteen seconds is inside the window in which
/// anyone looks up from one machine to check on another.
pub const POLL: Duration = Duration::from_secs(15);

/// How long ssh waits to reach the host, and how long a connection may go quiet
/// before it is torn down.
///
/// Both are needed and they cover different failures: `ConnectTimeout` is a
/// host that will not answer, and the keepalives are a connection that answered
/// and then went away — a laptop closed mid-poll, a VPN dropped. Without the
/// second, that thread waits on a socket the kernel will not give up on for
/// hours, and the host's rows never update again.
///
/// ponytail: neither bounds a remote cctop that *is* answering but is slow —
/// a first run over a corpus of thousands of transcripts can take a while, and
/// killing it would mean that machine never produced a first snapshot at all.
const CONNECT_TIMEOUT_SECS: u64 = 10;
const ALIVE_INTERVAL_SECS: u64 = 5;
const ALIVE_RETRIES: u64 = 2;

/// A machine to read, and the command that reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    /// The ssh target as the user spelled it: `box`, `flo@box`, `box.local`.
    pub target: String,
    /// How cctop is invoked there.
    ///
    /// Configurable because the default is wrong more often than it looks. An
    /// `ssh host cctop` runs a *non-interactive* shell, which on most setups
    /// skips the rc file that put `~/.local/bin` or a version manager's shim on
    /// `PATH` — so a cctop that works when you ssh in by hand is not found by
    /// this. An absolute path is the fix, and naming it beats guessing at a
    /// login shell that may not be bash.
    pub command: String,
}

/// What a poll produced.
#[derive(Debug, Clone)]
pub enum Snapshot {
    Rows(Vec<Session>),
    /// The host could not be read. Kept and shown rather than logged: a machine
    /// silently missing from the table is worse than no machine at all, because
    /// the totals still look complete.
    Failed(String),
}

impl Host {
    /// Parse `[user@]host[:command]`.
    ///
    /// The separator is the last colon, so an IPv6 target has to be bracketed
    /// the way ssh already requires. A spec with no colon takes the default
    /// command, which is the overwhelmingly common case.
    pub fn parse(spec: &str) -> Option<Host> {
        let spec = spec.trim();
        if spec.is_empty() {
            return None;
        }
        match spec.rsplit_once(':') {
            Some((target, command)) if !target.is_empty() && !command.is_empty() => Some(Host {
                target: target.to_string(),
                command: command.to_string(),
            }),
            _ => Some(Host {
                target: spec.to_string(),
                command: "cctop".to_string(),
            }),
        }
    }

    /// Every host named on the command line and in `$CCTOP_HOSTS`, in that
    /// order, with duplicates dropped so naming one in both is not an error.
    pub fn collect(flags: &[String]) -> Vec<Host> {
        let env = std::env::var("CCTOP_HOSTS").unwrap_or_default();
        let mut out: Vec<Host> = Vec::new();
        for spec in flags.iter().map(String::as_str).chain(env.split(',')) {
            if let Some(host) = Host::parse(spec)
                && !out.iter().any(|h| h.target == host.target)
            {
                out.push(host);
            }
        }
        out
    }

    /// Read this host once.
    pub fn poll(&self) -> Snapshot {
        match self.run() {
            Ok(json) => match parse(&self.target, &json) {
                Ok(rows) => Snapshot::Rows(rows),
                Err(why) => Snapshot::Failed(why),
            },
            Err(why) => Snapshot::Failed(why),
        }
    }

    fn run(&self) -> Result<String, String> {
        // `BatchMode` is the important one: without it a host whose key needs a
        // passphrase, or one that is not in `known_hosts`, blocks on a prompt
        // that has nowhere to appear — the poll thread would hang forever
        // behind a question nobody can see.
        let out = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                &format!("ConnectTimeout={CONNECT_TIMEOUT_SECS}"),
                "-o",
                &format!("ServerAliveInterval={ALIVE_INTERVAL_SECS}"),
                "-o",
                &format!("ServerAliveCountMax={ALIVE_RETRIES}"),
                &self.target,
                "--",
                &self.command,
                "--json",
            ])
            .output()
            .map_err(|e| format!("could not run ssh: {e}"))?;

        if !out.status.success() {
            // ssh's own diagnostics are on stderr and are almost always the
            // actual answer ("Permission denied", "command not found"), so they
            // are passed through rather than replaced with a status code.
            let why = String::from_utf8_lossy(&out.stderr);
            let why = why.lines().next_back().unwrap_or("").trim();
            return Err(match why.is_empty() {
                true => format!("{} exited {}", self.command, out.status),
                false => why.to_string(),
            });
        }
        String::from_utf8(out.stdout).map_err(|_| "output was not UTF-8".to_string())
    }
}

/// Turn one host's `--json` output into rows.
///
/// Deliberately field-by-field off a `Value` rather than a derived
/// `Deserialize`: the far side is a *different build* of cctop, quite possibly
/// an older one, and a missing field has to cost that field rather than the
/// whole machine. Every read below has an answer for absent.
pub fn parse(host: &str, json: &str) -> Result<Vec<Session>, String> {
    let doc: Value = serde_json::from_str(json).map_err(|e| format!("unreadable output: {e}"))?;
    let list = doc
        .as_array()
        .ok_or("output was not the array --json produces")?;

    let mut rows: Vec<Session> = list.iter().filter_map(|v| row(host, v)).collect();
    // The abbreviation has to be computed over this host's paths as a set —
    // that is what makes it an abbreviation — and separately from the local
    // ones, whose directory tree has nothing to do with this machine's.
    let labels: Vec<String> = rows.iter().map(|s| s.label_source.clone()).collect();
    for (s, short) in rows.iter_mut().zip(util::abbreviate_paths(&labels)) {
        s.abbrev_label = short;
    }
    Ok(rows)
}

fn row(host: &str, v: &Value) -> Option<Session> {
    let provider = provider_of(text(v, "provider"))?;
    let id = text(v, "session_id");
    if id.is_empty() {
        return None;
    }

    let mut s = Session::new(provider, id.to_string());
    s.remote = Some(Remote {
        host: host.to_string(),
        branch: v.get("branch").and_then(Value::as_str).map(str::to_string),
    });
    s.surface = match text(v, "surface") {
        "editor" => Surface::Editor,
        "desktop-code" => Surface::DesktopCode,
        "desktop-cowork" => Surface::DesktopCowork,
        _ => Surface::Cli,
    };
    s.started_at = text(v, "started_at").to_string();
    s.last_active = text(v, "last_active").to_string();
    s.label_source = text(v, "project").to_string();
    s.model = text(v, "model").to_string();
    s.harness = text(v, "harness").to_string();
    s.title = v
        .get("title")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    s.permission = crate::hook::Permission::parse(text(v, "permission"));
    s.activity_state = match text(v, "state") {
        "waiting" => ActivityState::WaitingForInput,
        "error" => ActivityState::ApiError,
        _ => ActivityState::Working,
    };

    // There is no local process, and nothing may go looking for one — but the
    // row still has to read as live and still has the far side's figures. The
    // liveness inference is the same door Cursor sessions already come through.
    s.inferred_running = v.get("running").and_then(Value::as_bool).unwrap_or(false);
    if let Some(p) = v.get("process") {
        s.process = Some(crate::proc::ProcInfo {
            pids: num(p, "pids") as usize,
            cpu: num(p, "cpu") as f32,
            memory: num(p, "memory") as u64,
            command: text(p, "command").to_string(),
            process_list: Vec::new(),
        });
    }

    let tokens = v.get("tokens");
    s.input_tokens = tokens.map(|t| num(t, "input") as u64).unwrap_or(0);
    s.output_tokens = tokens.map(|t| num(t, "output") as u64).unwrap_or(0);
    s.tokens_per_min = num(v, "tokens_per_min");

    if let Some(a) = v.get("activity") {
        s.tool_count = num(a, "tool_count") as u64;
        s.tool_errors = num(a, "tool_errors") as u64;
        s.compactions = num(a, "compactions") as u32;
    }

    if let Some(c) = v.get("cost") {
        s.cost_available = c.get("available").and_then(Value::as_bool).unwrap_or(true);
        // `total` is a formatted string and `included` says why it is absent, so
        // the two have to be read together: a missing total under a bundled plan
        // is `None` on purpose, while one that simply failed to parse is zero.
        let included = c.get("included").and_then(Value::as_bool).unwrap_or(false);
        s.total_cost = match included {
            true => None,
            false => Some(
                c.get("total")
                    .and_then(Value::as_str)
                    .and_then(|t| t.trim_start_matches('$').replace(',', "").parse().ok())
                    .unwrap_or(0.0),
            ),
        };
        s.cost_is_free = c.get("free").and_then(Value::as_bool).unwrap_or(false);
        s.cost_hour = num(c, "this_hour");
        s.cost_today = num(c, "today");
        s.cost_per_min = num(c, "per_min");
        s.costs_by_day = buckets(c.get("by_day"));
        s.costs_by_hour = buckets(c.get("by_hour"));
    }

    if let Some(ctx) = v.get("context") {
        let max = num(ctx, "max") as u64;
        if max > 0 {
            s.context = Some(ContextUsage {
                used: num(ctx, "used") as u64,
                max,
                compacted: ctx
                    .get("compacted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            });
        }
    }

    // Each machine detects its own collisions — it is the only one that can,
    // since the comparison is between paths on its disk — and reports the
    // verdict here.
    s.conflict = match text(v.get("conflict").unwrap_or(&Value::Null), "level") {
        "file" => Some(crate::collide::Overlap::File),
        "directory" => Some(crate::collide::Overlap::Directory),
        _ => None,
    };

    s.subagents = v
        .get("subagents")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|sa| serde_json::from_value(sa.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    s.subagents_cost = s.subagents.iter().map(|sa| sa.cost).sum();

    Some(s)
}

/// Re-nest a flattened `key -> USD` bucket map into the `key -> model -> USD`
/// shape the local pipeline uses.
///
/// The model breakdown is lost across the wire and is not worth carrying: every
/// consumer of these maps sums the models straight back up, and the single
/// synthetic key below is what that sum reads.
fn buckets(v: Option<&Value>) -> HashMap<String, HashMap<String, f64>> {
    let Some(map) = v.and_then(Value::as_object) else {
        return HashMap::new();
    };
    map.iter()
        .filter_map(|(k, v)| v.as_f64().map(|amount| (k.clone(), amount)))
        .map(|(k, amount)| (k, HashMap::from([("remote".to_string(), amount)])))
        .collect()
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn num(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn provider_of(name: &str) -> Option<Provider> {
    Some(match name {
        "claude" => Provider::Claude,
        "codex" => Provider::Codex,
        "cursor" => Provider::Cursor,
        "gemini" => Provider::Gemini,
        "opencode" => Provider::OpenCode,
        "pi" => Provider::Pi,
        "windsurf" => Provider::Windsurf,
        // A provider this build has never heard of: the far side is newer.
        // Dropping the row loses one session; guessing would file it under a
        // harness it is not and put its cost in the wrong column.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_may_name_the_binary_that_ssh_cannot_find() {
        assert_eq!(
            Host::parse("box"),
            Some(Host {
                target: "box".into(),
                command: "cctop".into()
            })
        );
        assert_eq!(
            Host::parse("flo@box:/opt/bin/cctop"),
            Some(Host {
                target: "flo@box".into(),
                command: "/opt/bin/cctop".into()
            })
        );
        assert_eq!(Host::parse("   "), None);
    }

    /// A full row survives the wire: the figures the columns show, the state
    /// the dot shows, and the branch, which is the one field that must come
    /// from the far side rather than be looked up here.
    #[test]
    fn a_snapshot_round_trips_into_rows() {
        let json = r#"[{
            "provider": "claude",
            "surface": "cli",
            "state": "waiting",
            "session_id": "abc123",
            "started_at": "2026-08-11T09:00:00Z",
            "last_active": "2026-08-11T10:00:00Z",
            "project": "/srv/work/api",
            "title": null,
            "model": "claude-opus-5",
            "harness": "ClaudeCode",
            "branch": "feat/idle",
            "permission": "edits",
            "running": true,
            "process": {"cpu": 12.5, "memory": 4096, "command": "claude", "pids": 3},
            "cost": {
                "available": true, "total": "$1.25", "included": false, "free": false,
                "this_hour": 0.5, "today": 1.25, "per_min": 0.01,
                "by_day": {"2026-08-11": 1.25},
                "by_hour": {"2026-08-11T10": 0.5}
            },
            "tokens": {"input": 1000, "output": 200, "total": 1200},
            "tokens_per_min": 42.0,
            "activity": {"tool_count": 40, "tool_errors": 4, "compactions": 2},
            "context": {"used": 100000, "max": 200000},
            "conflict": {"level": "file", "peers": ["def"], "files": ["/srv/work/api/x.rs"]}
        }]"#;

        let rows = parse("box", json).expect("parses");
        let s = &rows[0];
        assert_eq!(s.remote.as_ref().map(|r| r.host.as_str()), Some("box"));
        assert_eq!(
            crate::ui::columns::branch_of(s).as_deref(),
            Some("feat/idle")
        );
        assert!(s.is_running(), "a live remote row must read as live");
        assert_eq!(s.activity_state, ActivityState::WaitingForInput);
        assert_eq!(s.total_cost, Some(1.25));
        assert_eq!(s.cost_today, 1.25);
        assert_eq!(s.tool_errors, 4);
        assert_eq!(s.error_rate(), Some(0.1));
        assert_eq!(s.compactions, 2);
        assert_eq!(s.conflict, Some(crate::collide::Overlap::File));
        assert_eq!(s.process.as_ref().map(|p| p.memory), Some(4096));
        // The buckets have to survive, or a remote machine's spend would reach
        // the lifetime total and none of the overview's windows.
        assert_eq!(s.costs_by_day["2026-08-11"].values().sum::<f64>(), 1.25);
    }

    /// The far side is a different build. A row missing everything optional
    /// still has to arrive, because the alternative is a machine that vanishes
    /// from the table the day it is upgraded.
    #[test]
    fn an_older_remote_still_produces_rows() {
        let json = r#"[
            {"provider": "codex", "session_id": "x", "project": "/w"},
            {"provider": "codex"},
            {"provider": "some-future-agent", "session_id": "y"}
        ]"#;
        let rows = parse("box", json).expect("parses");
        assert_eq!(
            rows.len(),
            1,
            "no id and no known provider are both dropped"
        );
        assert_eq!(rows[0].session_id, "x");
        assert!(!rows[0].is_running());
        assert!(crate::ui::columns::branch_of(&rows[0]).is_none());
    }

    #[test]
    fn output_that_is_not_a_snapshot_is_an_error_not_an_empty_machine() {
        assert!(parse("box", "command not found: cctop").is_err());
        assert!(parse("box", "{}").is_err());
        assert!(parse("box", "[]").expect("valid, just empty").is_empty());
    }
}
