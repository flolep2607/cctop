//! Choosing an address: an agent's own share hostname, or the dashboard's.
//!
//! Both are a DNS name on the connected Cloudflare account's domain, and both
//! are picked where the address is shown: `e` or a right-click on the
//! `Address` line of the share panel `W` opens, and on the internet origin in
//! the serve panel. The field is one label; Enter writes DNS off the UI thread
//! with the spinner (the calls are [`cloudflare::name_share`] and
//! [`cloudflare::name_dashboard`], against the fake API in tests), and Esc
//! goes back with nothing changed.
//!
//! A share renamed is re-minted on its new name, copied, and its code redrawn:
//! the old name's record is deleted, so links on it stop answering, and the
//! status line says so. A dashboard renamed moves the page while it is up; its
//! token is unchanged, so every link here is the same link on the new origin.
//!
//! Nothing here draws a link. The field and the panels show hostnames, which
//! open nothing without the token behind them.

use super::line_edit::LineEdit;
use super::*;
use cctop_core::cloudflare::{self, Renamed};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// Longest thing the field takes: a label and the zone after it.
const ADDRESS_MAX: usize = 253;

/// What is being given an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// An agent's `W` shares.
    Share {
        session_id: String,
        /// The multiplexer session, to mint its share again on the new name.
        name: String,
        label: String,
    },
    /// The page `cctop serve` puts on the account's tunnel.
    Dashboard,
}

/// What came back from the thread: the rename, and for a share the share
/// minted again on its new address.
type Done = Result<(Renamed, Option<Result<cctop_core::rmux::Share, String>>), String>;

/// The address field while it is up.
pub struct AddressEdit {
    pub target: Target,
    /// The hostname it is on now.
    pub was: String,
    /// The domain a label goes under, drawn after the field.
    pub zone: String,
    /// For a share: the hostname an empty field sends it back to.
    pub default: Option<String>,
    pub field: LineEdit,
    /// Why the last Enter was refused, in the user's words.
    pub problem: Option<String>,
    working: Option<(Receiver<Done>, Instant)>,
    /// When a right-click opened it, so the paste some terminals send with
    /// the button is not typed in — as for a tab's name.
    opened_by_click: Option<Instant>,
}

impl AddressEdit {
    /// A field on `was`, for a test to draw or drive.
    #[cfg(test)]
    pub(crate) fn for_test(target: Target, was: &str, zone: &str, field: LineEdit) -> AddressEdit {
        AddressEdit {
            target,
            was: was.to_string(),
            zone: zone.to_string(),
            default: None,
            field,
            problem: None,
            working: None,
            opened_by_click: None,
        }
    }

    /// Whether DNS is being written now.
    pub fn busy(&self) -> bool {
        self.working.is_some()
    }

    /// The spinner's frame while it is.
    pub fn frame(&self) -> Option<char> {
        self.working.as_ref().map(|_| share::spinner_frame())
    }

    /// The hostname the field names, for the warning drawn above it.
    pub fn typed_host(&self) -> Option<String> {
        let typed = self.field.trim();
        cloudflare::label_for(typed, &self.zone)
            .ok()
            .map(|label| format!("{label}.{}", self.zone))
    }
}

impl App {
    /// `e` or a right-click on the share panel's address line.
    pub(super) fn rename_share_prompt(&mut self) {
        let Some(share) = &self.share_qr else {
            return;
        };
        let naming = match &share.rename {
            Ok(naming) => naming.clone(),
            Err(why) => {
                let why = why.clone();
                return self.set_status(why);
            }
        };
        let Some(host) = share.host.clone() else {
            return;
        };
        let zone = naming.zone;
        let label = host
            .strip_suffix(&format!(".{zone}"))
            .unwrap_or(&host)
            .to_string();
        let mut field = LineEdit::default();
        field.set(label);
        self.address = Some(AddressEdit {
            target: Target::Share {
                session_id: share.session_id.clone(),
                name: share.name.clone(),
                label: share.label.clone(),
            },
            was: host,
            zone,
            default: naming.default,
            field,
            problem: None,
            working: None,
            opened_by_click: None,
        });
        self.mode = Mode::RenameAddress;
        self.needs_redraw = true;
    }

    /// Whether the dashboard's address can be chosen now, or why not.
    pub(super) fn dashboard_rename(&self) -> Result<(String, String), String> {
        if !self.serving_on_account() {
            return Err(
                "Only the page on your own domain has an address to choose — t serves it there"
                    .to_string(),
            );
        }
        let connected = self
            .connected
            .as_ref()
            .ok_or_else(|| "No Cloudflare account is connected — a connects one".to_string())?;
        connected.rename?;
        match (connected.hostname.clone(), connected.zone.clone()) {
            (Some(host), Some(zone)) => Ok((host, zone)),
            _ => Err(cctop_core::tunnel::TOKEN_ONLY.to_string()),
        }
    }

    /// `e` or a right-click on the serve panel's internet origin.
    pub(super) fn rename_dashboard_prompt(&mut self) {
        let (host, zone) = match self.dashboard_rename() {
            Ok(both) => both,
            Err(why) => return self.set_status(why),
        };
        let label = host
            .strip_suffix(&format!(".{zone}"))
            .unwrap_or(&host)
            .to_string();
        let mut field = LineEdit::default();
        field.set(label);
        self.address = Some(AddressEdit {
            target: Target::Dashboard,
            was: host,
            zone,
            default: None,
            field,
            problem: None,
            working: None,
            opened_by_click: None,
        });
        self.mode = Mode::RenameAddress;
        self.needs_redraw = true;
    }

    /// The same prompt, from a right-click: the paste that may arrive with
    /// the button is not typed in.
    pub(super) fn rename_address_by_click(&mut self) {
        match self.mode {
            Mode::ShareQr => self.rename_share_prompt(),
            Mode::Serve => self.rename_dashboard_prompt(),
            _ => return,
        }
        if let Some(edit) = self.address.as_mut() {
            edit.opened_by_click = Some(Instant::now());
        }
    }

    /// Where Esc and a finished rename go back to.
    fn address_back(&self) -> Mode {
        match self.address.as_ref().map(|a| &a.target) {
            Some(Target::Share { .. }) => Mode::ShareQr,
            _ => Mode::Serve,
        }
    }

    pub(super) fn on_key_address(&mut self, key: KeyEvent) {
        let Some(edit) = self.address.as_mut() else {
            self.mode = Mode::List;
            return;
        };
        // One write at a time; Esc while it runs would leave a rename landing
        // on a panel nobody is looking at.
        if edit.busy() {
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.mode = self.address_back();
                self.address = None;
            }
            KeyCode::Enter => self.submit_address(),
            _ => {
                if edit.field.key(key, ADDRESS_MAX).changed() {
                    edit.problem = None;
                }
            }
        }
    }

    pub(super) fn paste_address(&mut self, text: &str) {
        let Some(edit) = self.address.as_mut() else {
            return;
        };
        if let Some(at) = edit.opened_by_click.take()
            && at.elapsed() < input::RIGHT_CLICK_PASTE
        {
            return;
        }
        if !edit.busy() {
            edit.field.insert_str(text.trim(), ADDRESS_MAX);
        }
    }

    /// Enter: the DNS writes, off the UI thread.
    fn submit_address(&mut self) {
        // The other agents' labels, for "already used by …": the thread has
        // no table to look them up in.
        let labels: HashMap<String, String> = self
            .sessions
            .iter()
            .map(|s| (s.session_id.clone(), s.display_label().to_string()))
            .collect();
        let Some(edit) = self.address.as_mut() else {
            return;
        };
        let typed = edit.field.trim().to_string();
        let target = edit.target.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let done: Done = match target {
                Target::Share {
                    session_id, name, ..
                } => {
                    let agents = |id: &str| match labels.get(id) {
                        Some(label) => format!("{label}'s shares"),
                        None => "another agent's shares".to_string(),
                    };
                    cloudflare::name_share(&session_id, &typed, &agents).map(|renamed| {
                        let share = match renamed.changed {
                            // Minted afresh on the new name, which is its own
                            // held share — see `rmux::share_link_at`.
                            true => Some(
                                cctop_core::rmux::share_link_at(
                                    &name,
                                    renamed.new.as_deref(),
                                    true,
                                )
                                .map(|(share, _)| share),
                            ),
                            false => None,
                        };
                        (renamed, share)
                    })
                }
                Target::Dashboard => cloudflare::name_dashboard(&typed).map(|r| (r, None)),
            };
            let _ = tx.send(done);
        });
        edit.problem = None;
        edit.working = Some((rx, Instant::now()));
        self.needs_redraw = true;
    }

    /// Take a finished rename, and keep the dashboard's links on the page's
    /// current hostname — which the web page can move too. Whether the screen
    /// changed: every tick while the spinner turns.
    pub(super) fn tick_address(&mut self) -> bool {
        let moved = self.follow_page_host();
        let Some(edit) = self.address.as_mut() else {
            return moved;
        };
        let Some((rx, _)) = &edit.working else {
            return moved;
        };
        let done = match rx.try_recv() {
            Err(TryRecvError::Empty) => return true,
            Ok(done) => done,
            Err(TryRecvError::Disconnected) => Err("the rename gave no answer".to_string()),
        };
        edit.working = None;
        match done {
            Err(why) => {
                edit.problem = Some(why.clone());
                self.set_status(why);
            }
            Ok((renamed, share)) => {
                let target = edit.target.clone();
                self.address = None;
                self.refresh_connected();
                match target {
                    Target::Share { label, .. } => self.share_renamed(&label, &renamed, share),
                    Target::Dashboard => {
                        self.follow_page_host();
                        self.dashboard_renamed(&renamed);
                    }
                }
            }
        }
        true
    }

    fn share_renamed(
        &mut self,
        label: &str,
        renamed: &Renamed,
        share: Option<Result<cctop_core::rmux::Share, String>>,
    ) {
        self.back_from_address(Mode::ShareQr);
        let new = renamed.new.clone().unwrap_or_default();
        if !renamed.changed {
            return self.set_status(format!("{label} stays on {new}"));
        }
        let gone = match &renamed.old {
            Some(old) => format!("; {old} no longer answers"),
            None => String::new(),
        };
        let note = renamed
            .note
            .as_ref()
            .map(|n| format!(" · {n}"))
            .unwrap_or_default();
        match share {
            Some(Ok(share)) => {
                let Some(operator) = share.operator.clone() else {
                    return;
                };
                render::copy_to_clipboard(&operator);
                if let Some(qr) = self.share_qr.as_mut() {
                    qr.link = operator;
                    qr.pin = share.pin.clone();
                    qr.host = share.host.clone().or(Some(new.clone()));
                }
                let pin = match &share.pin {
                    Some(pin) => format!(" · pin {pin}"),
                    None => String::new(),
                };
                self.set_status(format!(
                    "{label} is on {new} — new link copied{pin}{gone}{note}"
                ));
            }
            Some(Err(why)) => {
                self.set_status(format!(
                    "{label} is on {new}, but its new link failed: {why}{gone}"
                ));
            }
            None => {}
        }
    }

    fn dashboard_renamed(&mut self, renamed: &Renamed) {
        self.back_from_address(Mode::Serve);
        let new = renamed.new.clone().unwrap_or_default();
        match (&renamed.old, renamed.changed) {
            (Some(old), true) => {
                let note = renamed
                    .note
                    .as_ref()
                    .map(|n| format!(" · {n}"))
                    .unwrap_or_default();
                self.set_status(format!(
                    "The page moved to https://{new} — links to {old} stop working{note}"
                ));
            }
            _ => self.set_status(format!("The page stays on {new}")),
        }
    }

    /// Back to the panel the field was opened from — unless a click put the
    /// field away while Cloudflare was being asked, in which case the answer
    /// goes to the status line only and no panel springs back up.
    fn back_from_address(&mut self, panel: Mode) {
        if self.mode == Mode::RenameAddress {
            self.mode = panel;
        }
    }

    /// Put the served links on the hostname the account's tunnel answers the
    /// page on now, when a rename — here or on the web page — moved it.
    /// Whether anything changed.
    fn follow_page_host(&mut self) -> bool {
        let Some(page) = cctop_core::tunnel::page_host() else {
            return false;
        };
        let Some(serving) = self.serving.as_mut() else {
            return false;
        };
        let Some(public) = serving.public.clone() else {
            return false;
        };
        let Some(old) = public
            .strip_prefix("https://")
            .and_then(|rest| rest.split(['/', '?']).next())
            .map(str::to_string)
        else {
            return false;
        };
        if old.eq_ignore_ascii_case(&page) {
            return false;
        }
        let (from, to) = (format!("https://{old}"), format!("https://{page}"));
        serving.public = Some(public.replacen(&from, &to, 1));
        if serving.readonly.starts_with(&from) {
            serving.readonly = serving.readonly.replacen(&from, &to, 1);
        }
        self.refresh_connected();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::test_app;
    use ratatui::crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    pub(crate) fn editing(target: Target) -> AddressEdit {
        let mut field = LineEdit::default();
        field.set("myagent");
        AddressEdit {
            target,
            was: "cctop-share.example.test".into(),
            zone: "example.test".into(),
            default: Some("cctop-share.example.test".into()),
            field,
            problem: None,
            working: None,
            opened_by_click: None,
        }
    }

    #[test]
    fn esc_goes_back_with_nothing_changed() {
        let mut app = test_app();
        app.address = Some(editing(Target::Share {
            session_id: "s1".into(),
            name: "cctop-claude-s1".into(),
            label: "fix the flaky test".into(),
        }));
        app.mode = Mode::RenameAddress;
        app.on_key(key(KeyCode::Char('x')));
        assert_eq!(app.address.as_ref().unwrap().field.to_string(), "myagentx");
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::ShareQr);
        assert!(app.address.is_none());

        app.address = Some(editing(Target::Dashboard));
        app.mode = Mode::RenameAddress;
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Serve);
    }

    #[test]
    fn the_typed_name_is_read_as_a_host_under_the_zone() {
        let mut edit = editing(Target::Dashboard);
        assert_eq!(edit.typed_host().as_deref(), Some("myagent.example.test"));
        edit.field.set("Home.example.test");
        assert_eq!(edit.typed_host().as_deref(), Some("home.example.test"));
        edit.field.set("not_one");
        assert_eq!(edit.typed_host(), None);
    }
}
