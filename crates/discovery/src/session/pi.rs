//! Where Pi keeps its sessions, and the header facts of each. Reading the
//! turns stays in cctop-core's `session::pi`.

use super::{Discovered, pi_line};
use crate::{Provider, paths};

/// Discover every persisted Pi session, including custom session roots.
pub fn list() -> Vec<Discovered> {
    let mut sessions = Vec::new();
    let paths: Vec<_> = paths::pi_sessions_roots()
        .iter()
        .flat_map(|root| paths::rglob(root, ".jsonl"))
        .collect();
    for path in paths {
        let mut session_id = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut started = String::new();
        let mut last = String::new();
        let mut cwd = String::new();
        let mut title = None;
        let mut model = String::new();

        let _ = pi_line::for_each(&path, |entry| match entry.kind.or_empty() {
            "session" => {
                if let Some(id) = entry.id.get() {
                    session_id = id.to_string();
                }
                started = entry.timestamp.or_empty().to_string();
                cwd = entry.cwd.or_empty().to_string();
                last = started.clone();
            }
            "session_info" => title = entry.name.get().map(str::to_string),
            "message" => {
                let Some(message) = entry.message() else {
                    return;
                };
                // Only a message is the session doing something: a rename or a
                // model switch written afterwards is not activity.
                let ts = entry.ts();
                if ts > last {
                    last = ts;
                }
                if message.role.is("assistant")
                    && let Some(m) = message.model.get()
                {
                    model = m.to_string();
                }
            }
            _ => {}
        });

        if session_id.is_empty() || started.is_empty() {
            continue;
        }
        let mut session = Discovered::new(Provider::Pi, session_id);
        session.started_at = started;
        session.last_active = last;
        session.label_source = cwd;
        session.title = title;
        session.model = model;
        session.data_file = Some(path);
        sessions.push(session);
    }
    sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    sessions
}
