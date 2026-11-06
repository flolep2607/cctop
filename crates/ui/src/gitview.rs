//! `Alt+g`: the session's changes in the person's own git tool, in a split
//! beside the agent.
//!
//! The diff view ([`super::diffview`]) reads one file at a time and only
//! reads; lazygit or tig is where you stage a hunk, commit, or read a file
//! too large to draw. So the tool that is installed is opened on the same
//! checkout and, where it takes one, the same base the sidebar counts from —
//! the first of [`ORDER`], or the `git_viewer` setting when it is set.
//!
//! The pane is cctop's own rather than an rmux session: a viewer is opened to
//! be read now and closed, its pane goes when the tool exits, and an rmux
//! session would be adopted as a tab by every other cctop on the machine.

use std::path::Path;

use super::{App, tabs};

/// What the order is chosen by, first installed wins; see [`viewer`].
pub(super) const ORDER: [&str; 5] = ["lazygit", "tig", "delta", "difftool", "git"];

/// The argv `Alt+g` runs for changes against `base`: the `git_viewer` setting
/// when it is set, else the first of [`ORDER`] that `have` says is installed.
///
/// `difftool` is only taken when git has a `diff.tool` configured: without
/// one `git difftool` asks, file by file, which tool to launch — a question
/// in a pane opened to read a diff. A pager is named for the two that page,
/// without `less`'s `-F`, which would quit a short diff the moment it drew and
/// close the pane before it could be read.
pub(super) fn viewer(
    chosen: Option<&str>,
    base: &str,
    have: impl Fn(&str) -> bool,
    difftool: bool,
) -> Option<Vec<String>> {
    let argv = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
    if let Some(command) = chosen.map(str::trim).filter(|c| !c.is_empty()) {
        // The person's own command line, with the base where it can find it.
        let base = format!("CCTOP_DIFF_BASE={base}");
        return Some(argv(&["env", &base, "sh", "-c", command]));
    }
    ORDER.iter().find_map(|tool| match *tool {
        "lazygit" if have("lazygit") => Some(argv(&["lazygit"])),
        "tig" if have("tig") => Some(argv(&["tig", "status"])),
        "delta" if have("delta") && have("git") => Some(argv(&[
            "git",
            "-c",
            "core.pager=delta --paging=always",
            "diff",
            base,
        ])),
        "difftool" if difftool && have("git") => Some(argv(&["git", "difftool", "-d", base])),
        "git" if have("git") => Some(argv(&[
            "git",
            "-c",
            "core.pager=less -R",
            "diff",
            "--color=always",
            base,
        ])),
        _ => None,
    })
}

/// Whether git has a difftool configured for the checkout at `root`.
fn difftool_configured(root: &Path) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", "--get", "diff.tool"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .is_ok_and(|out| out.status.success() && !out.stdout.trim_ascii().is_empty())
}

impl App {
    /// `Alt+g`: open the tool in a split beside the agent.
    pub(super) fn open_git_viewer(&mut self) {
        if !self.sidebar.shown() {
            return self.set_status("Alt+g opens an agent's changes — open an agent's tab first");
        }
        // A tool here runs against a checkout here; the one beside an agent on
        // another machine is there.
        if let Some((reach, _)) = &self.sidebar.remote {
            return self.set_status(format!(
                "Alt+g runs a git tool on this machine, and this agent's checkout is on {} — \
                 click a file in the sidebar for its diff",
                reach.machine()
            ));
        }
        let Some((root, base)) = self.sidebar.checkout.clone() else {
            return self.set_status(match self.sidebar.files.is_none() {
                true => "Still reading this session's changes — try again in a moment",
                false => "This session is not in a git checkout — nothing to open",
            });
        };
        let chosen = cctop_core::settings::Settings::load().git_viewer;
        let Some(argv) = viewer(
            chosen.as_deref(),
            &base,
            cctop_core::shim::is_command,
            difftool_configured(&root),
        ) else {
            return self
                .set_status("No git tool here — install lazygit, tig or delta, or git itself");
        };
        let label = match argv.first().map(String::as_str) {
            Some("env") => "git viewer".to_string(),
            Some("git") if argv.iter().any(|a| a.contains("delta")) => "delta".to_string(),
            Some(first) => first.to_string(),
            None => return,
        };
        match tabs::Pane::launch(&argv, Some(&root), tabs::Own::Cctop) {
            Ok(mut pane) => {
                pane.label = label.clone();
                if let Some(tab) = self.active_tab() {
                    tab.split(pane, false);
                }
                self.set_status(format!(
                    "Opened {label} on {} — q in it, or Alt+w, closes it",
                    cctop_core::util::tildify(&root.to_string_lossy())
                ));
            }
            Err(e) => self.set_status(format!("Could not open {label}: {e}")),
        }
        self.needs_redraw = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first installed tool of the order wins, and the setting beats them
    /// all.
    #[test]
    fn the_first_installed_tool_is_opened_and_the_setting_wins() {
        let only = |names: &'static [&'static str]| move |n: &str| names.contains(&n);
        let first = |have: &dyn Fn(&str) -> bool, difftool| {
            viewer(None, "abc", have, difftool).map(|a| a.join(" "))
        };
        assert_eq!(
            first(&only(&["lazygit", "tig", "git"]), false).unwrap(),
            "lazygit"
        );
        assert_eq!(first(&only(&["tig", "git"]), false).unwrap(), "tig status");
        assert_eq!(
            first(&only(&["delta", "git"]), true).unwrap(),
            "git -c core.pager=delta --paging=always diff abc"
        );
        assert_eq!(first(&only(&["git"]), true).unwrap(), "git difftool -d abc");
        assert_eq!(
            first(&only(&["git"]), false).unwrap(),
            "git -c core.pager=less -R diff --color=always abc"
        );
        assert_eq!(first(&only(&[]), true), None);
        // The setting, whatever is installed, with the base handed over.
        assert_eq!(
            viewer(Some("gitui"), "abc", only(&["lazygit"]), false).unwrap(),
            ["env", "CCTOP_DIFF_BASE=abc", "sh", "-c", "gitui"]
        );
        // An empty setting is no setting.
        assert_eq!(
            viewer(Some("  "), "abc", only(&["lazygit"]), false).unwrap(),
            ["lazygit"]
        );
    }

    /// None of these is an agent: the launch path adds nothing to them.
    #[test]
    fn a_git_tool_is_started_as_written() {
        for tool in ["lazygit", "tig", "git", "env"] {
            let argv = vec![tool.to_string(), "x".to_string()];
            assert_eq!(
                cctop_core::rmux::launch_argv_with(&argv, Some("/bin/cctop"), true),
                argv
            );
        }
    }
}
