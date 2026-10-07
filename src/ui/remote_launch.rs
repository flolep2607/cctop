//! The launcher's Remote entry: a host, a directory on it, then a tab running
//! `cctop sandbox <host>:<dir>`.
//!
//! Two short steps after the launcher rather than one modal with two fields,
//! because the second question depends on the first — a directory means
//! nothing until you know whose — and each step is then the familiar shape:
//! a list narrowed by typing, like the tab switcher, and a one-line field.

use super::*;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

/// Longest host a person types. A `user@host.domain` is a few dozen
/// characters; this is room for that and no paragraph.
const HOST_MAX: usize = 256;

/// The directory a Remote launch starts in when none is typed: the host's home,
/// expanded on the host by the sandbox's probe.
pub(super) const DEFAULT_PATH: &str = "~";

/// What a remote tab is called: the agent, then where it works.
///
/// The arrow is the one the table's Host column puts after a sandboxed row's
/// host, so a tab and its row read as the same thing. `claude` first, because
/// a tab's harness is read off the front of its name and that is what picks
/// which screen phrases mean "working" and "asking".
pub(super) fn remote_label(host: &str) -> String {
    format!("claude ⇄ {host}")
}

/// The command a Remote launch runs, as an argv for the pane.
///
/// Through `env`, which is how every launch here already carries a variable,
/// and which [`tabs::label_of`] knows to look past.
pub(super) fn sandbox_argv(exe: &std::path::Path, host: &str, path: &str) -> Vec<String> {
    let path = match path.trim() {
        "" => DEFAULT_PATH,
        path => path,
    };
    vec![
        "env".to_string(),
        format!("{}=1", crate::sandbox::ENV_HOLD),
        exe.to_string_lossy().into_owned(),
        "sandbox".to_string(),
        format!("{host}:{path}"),
    ]
}

impl App {
    /// Open the host step, on the hosts `~/.ssh/config` names.
    pub(super) fn sandbox_prompt(&mut self) {
        self.sandbox_hosts = crate::ssh_config::hosts();
        self.sandbox_filter.clear();
        self.sandbox_cursor = 0;
        self.mode = Mode::RemoteHost;
        self.needs_redraw = true;
    }

    /// Indexes into `sandbox_hosts` that what has been typed matches, in the
    /// config's order.
    pub fn sandbox_matches(&self) -> Vec<usize> {
        let needle = self.sandbox_filter.trim().to_lowercase();
        self.sandbox_hosts
            .iter()
            .enumerate()
            .filter(|(_, host)| host.matches(&needle))
            .map(|(i, _)| i)
            .collect()
    }

    /// The host Enter would take: the highlighted one, or what was typed when
    /// nothing in the list matches it — `user@10.0.0.5` is a host too, and a
    /// config is not a precondition for ssh.
    pub fn sandbox_pick(&self) -> Option<String> {
        if let Some(&i) = self.sandbox_matches().get(self.sandbox_cursor) {
            return Some(self.sandbox_hosts[i].name.clone());
        }
        let typed = self.sandbox_filter.trim();
        (!typed.is_empty() && !typed.contains(char::is_whitespace)).then(|| typed.to_string())
    }

    pub(super) fn on_key_sandbox_host(&mut self, key: KeyEvent) {
        match key.code {
            // Back to the launcher rather than out of it: the Remote row was
            // one choice among several, and the others are still a key away.
            KeyCode::Esc => self.mode = Mode::Launch,
            KeyCode::Up | KeyCode::Down => {
                let n = self.sandbox_matches().len();
                if n > 0 {
                    let step = if key.code == KeyCode::Up { n - 1 } else { 1 };
                    self.sandbox_cursor = (self.sandbox_cursor + step) % n;
                }
            }
            KeyCode::Enter => {
                if let Some(host) = self.sandbox_pick() {
                    self.sandbox_host = host;
                    self.sandbox_path = DEFAULT_PATH.into();
                    self.mode = Mode::RemotePath;
                }
            }
            // Letters are the filter, as in the tab switcher: what is spelled
            // is what is searched, so `j` and `k` belong to a host name.
            _ => {
                if self.sandbox_filter.key(key, HOST_MAX).changed() {
                    self.sandbox_cursor = 0;
                }
            }
        }
        self.needs_redraw = true;
    }

    pub(super) fn on_key_sandbox_path(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::RemoteHost,
            KeyCode::Enter => {
                self.mode = Mode::List;
                self.launch_sandbox();
            }
            _ => {
                self.sandbox_path.key(key, input::MAX_PATH_INPUT);
            }
        }
        self.needs_redraw = true;
    }

    /// Start `cctop sandbox` on the picked host and directory, where the
    /// launcher was going to put its pick.
    ///
    /// Nothing about the host is checked here. The sandbox checks all of it in
    /// the tab — the connection, the directory, sshfs — and says what is wrong
    /// there, held on screen; checking twice would mean an ssh round trip on
    /// the thread that draws.
    pub(super) fn launch_sandbox(&mut self) {
        let host = self.sandbox_host.clone();
        if host.is_empty() {
            return;
        }
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(error) => {
                self.set_status(format!("Could not find cctop's own binary: {error}"));
                return;
            }
        };
        let argv = sandbox_argv(&exe, &host, &self.sandbox_path);
        let label = remote_label(&host);
        let Some(own) = self.own_preferring_rmux(Deferred::Remote, || {
            crate::rmux::free_name(&format!("claude-{host}"))
        }) else {
            return;
        };
        let mut pane = match tabs::Pane::launch(&argv, None, own) {
            Ok(pane) => pane,
            Err(error) => {
                self.set_status(format!("Could not start the sandbox on {host}: {error}"));
                return;
            }
        };
        pane.label = label.clone();
        let kept = match pane.outlives_cctop() {
            true => " — it will outlive cctop",
            false => "",
        };
        match self.launch_into {
            LaunchInto::Split { stacked } => {
                let Some(tab) = self.active_tab() else { return };
                tab.split(pane, stacked);
            }
            LaunchInto::Tab => {
                self.tabs.push(tabs::Tab::new(pane));
                self.go_to_tab(self.tabs.len());
            }
        }
        let path = argv.last().map(String::as_str).unwrap_or_default();
        self.set_status(format!("Started {label} in {path}{kept}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh_config::Host;

    fn hosts() -> Vec<Host> {
        vec![
            Host {
                name: "nz-b-procurementdb1".into(),
                aliases: vec!["procdb".into()],
            },
            Host {
                name: "devbox".into(),
                aliases: vec![],
            },
        ]
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::from(code)
    }

    fn typed(app: &mut App, text: &str) {
        for c in text.chars() {
            app.on_key_sandbox_host(key(KeyCode::Char(c)));
        }
    }

    /// Opened on the launcher's Remote row, the host step lists the config's
    /// hosts; Esc goes back to the launcher, not out of it.
    #[test]
    fn the_remote_row_opens_the_host_step_and_esc_returns_to_the_launcher() {
        let mut app = crate::ui::tests::test_app();
        app.launch_offer = vec![
            tabs::Choice::Start(vec!["claude".into()]),
            tabs::Choice::Remote { sshfs: true },
        ];
        app.launch_cursor = 1;
        app.mode = Mode::Launch;
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::RemoteHost);
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Launch);
    }

    /// `c` changes a local directory, which a remote launch has none of.
    #[test]
    fn the_local_directory_key_is_not_offered_on_the_remote_row() {
        let mut app = crate::ui::tests::test_app();
        app.launch_offer = vec![tabs::Choice::Remote { sshfs: false }];
        app.mode = Mode::Launch;
        app.on_key(key(KeyCode::Char('c')));
        assert_eq!(app.mode, Mode::Launch);
    }

    /// Any alias finds a host, and Enter takes the host's first name, which is
    /// the line's own first word and what ssh will be given.
    #[test]
    fn typing_narrows_the_hosts_and_enter_takes_the_first_name() {
        let mut app = crate::ui::tests::test_app();
        app.sandbox_hosts = hosts();
        app.mode = Mode::RemoteHost;
        assert_eq!(app.sandbox_matches(), vec![0, 1]);
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.sandbox_pick().as_deref(), Some("devbox"));
        typed(&mut app, "procdb");
        assert_eq!(app.sandbox_matches(), vec![0]);
        assert_eq!(app.sandbox_cursor, 0, "a narrowed list starts at the top");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::RemotePath);
        assert_eq!(app.sandbox_host, "nz-b-procurementdb1");
        assert_eq!(app.sandbox_path, DEFAULT_PATH);
        // Back a step keeps the host list as it was left.
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::RemoteHost);
    }

    /// A host the config does not name is still a host: what was typed is
    /// taken when nothing matches it, and a config with no hosts at all still
    /// gets somewhere.
    #[test]
    fn a_host_that_matches_nothing_is_taken_as_typed() {
        let mut app = crate::ui::tests::test_app();
        app.sandbox_hosts = Vec::new();
        app.mode = Mode::RemoteHost;
        assert_eq!(app.sandbox_pick(), None);
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::RemoteHost, "nothing to take yet");
        typed(&mut app, "me@10.0.0.5");
        assert_eq!(app.sandbox_pick().as_deref(), Some("me@10.0.0.5"));
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.sandbox_host, "me@10.0.0.5");
    }

    /// The tab runs `cctop sandbox host:path` and is named so it reads as
    /// remote, and as Claude.
    #[test]
    fn the_launch_is_cctop_sandbox_on_host_and_path() {
        let exe = std::path::Path::new("/usr/local/bin/cctop");
        assert_eq!(
            sandbox_argv(exe, "devbox", "/srv/api"),
            [
                "env",
                "CCTOP_SANDBOX_HOLD=1",
                "/usr/local/bin/cctop",
                "sandbox",
                "devbox:/srv/api"
            ]
        );
        // Nothing typed is the remote home.
        assert_eq!(
            sandbox_argv(exe, "devbox", "  ").last().map(String::as_str),
            Some("devbox:~")
        );
        let label = remote_label("procdb");
        assert!(label.contains("procdb"), "{label}");
        assert_eq!(tabs::harness_of(&label), "claude");
        // Parsed back the way the sandbox will parse it.
        let spec = crate::sandbox::Spec::parse("devbox:~").expect("spec");
        assert_eq!((spec.host.as_str(), spec.path.as_str()), ("devbox", "~"));
    }
}
