//! Getting sshfs onto this machine when `cctop sandbox` finds it missing.
//!
//! sshfs is the one thing a sandbox needs here that a fresh Linux install does
//! not have, and the TUI's Remote entry starts a sandbox in a tab — so a
//! missing sshfs would otherwise be a tab that prints an apt command and
//! closes. Asking on the terminal instead turns it into one keypress and a
//! sudo password, in the tab where the sandbox was about to start.
//!
//! The decision of what to run is kept apart from running it ([`plan`] is
//! pure), so the tests can check every distribution's command without anything
//! being installed.

use std::io::{BufRead, IsTerminal, Write};
use std::process::Command;

/// A distribution's package manager, as far as installing one package goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    Apt,
    Dnf,
    Yum,
    Pacman,
    Zypper,
    Apk,
}

/// Probed in this order. `dnf` before `yum` because a Fedora or RHEL 8+ box
/// has both and `yum` there is a compatibility shim for `dnf`.
const MANAGERS: &[(Manager, &str)] = &[
    (Manager::Apt, "apt-get"),
    (Manager::Dnf, "dnf"),
    (Manager::Yum, "yum"),
    (Manager::Pacman, "pacman"),
    (Manager::Zypper, "zypper"),
    (Manager::Apk, "apk"),
];

impl Manager {
    /// The first package manager `has` says is on the PATH.
    pub fn detect(has: impl Fn(&str) -> bool) -> Option<Manager> {
        MANAGERS
            .iter()
            .find(|(_, bin)| has(bin))
            .map(|(manager, _)| *manager)
    }

    /// The install command, without any sudo.
    ///
    /// Non-interactive where the manager has a flag for it, because the
    /// question has just been asked and answered; a second "are you sure" from
    /// the package manager is the same question again. Fedora and RHEL call
    /// the package `fuse-sshfs`; everyone else calls it `sshfs`. Each pulls in
    /// fuse3, which is where `fusermount3` comes from.
    pub fn argv(self) -> Vec<String> {
        let words: &[&str] = match self {
            Manager::Apt => &["apt-get", "install", "-y", "sshfs"],
            Manager::Dnf => &["dnf", "install", "-y", "fuse-sshfs"],
            Manager::Yum => &["yum", "install", "-y", "fuse-sshfs"],
            Manager::Pacman => &["pacman", "-S", "--noconfirm", "sshfs"],
            Manager::Zypper => &["zypper", "--non-interactive", "install", "sshfs"],
            Manager::Apk => &["apk", "add", "sshfs"],
        };
        words.iter().map(|w| w.to_string()).collect()
    }
}

/// What would install sshfs here: the argv, and whether it can be run as is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// A command this process can run.
    Run(Vec<String>),
    /// A command the user must run themselves — there is no sudo to reach root
    /// with, so cctop can only say what it would have run.
    Tell(Vec<String>),
    /// No package manager cctop knows.
    Unknown,
}

/// The install for a machine with `manager`, run as root or not, with or
/// without sudo on the PATH.
///
/// Through sudo only when not already root: in a container, root is the usual
/// user and sudo the usual absentee.
pub fn plan(manager: Option<Manager>, root: bool, sudo: bool) -> Plan {
    let Some(manager) = manager else {
        return Plan::Unknown;
    };
    let argv = manager.argv();
    match (root, sudo) {
        (true, _) => Plan::Run(argv),
        (false, true) => Plan::Run(std::iter::once("sudo".to_string()).chain(argv).collect()),
        (false, false) => Plan::Tell(std::iter::once("sudo".to_string()).chain(argv).collect()),
    }
}

/// [`plan`] for this machine.
pub fn plan_here() -> Plan {
    // SAFETY: geteuid cannot fail and touches no memory.
    let root = unsafe { libc::geteuid() } == 0;
    plan(
        Manager::detect(crate::shim::is_command),
        root,
        crate::shim::is_command("sudo"),
    )
}

/// Whether sshfs is on the PATH.
pub fn installed() -> bool {
    crate::shim::is_command("sshfs")
}

/// What to tell someone who has to install it by hand.
fn by_hand(plan: &Plan) -> String {
    match plan {
        Plan::Run(argv) | Plan::Tell(argv) => format!("Install it with:\n  {}", argv.join(" ")),
        Plan::Unknown => "Install it with your package manager: the package is `sshfs` \
                          (`fuse-sshfs` on Fedora and RHEL)."
            .to_string(),
    }
}

/// Make sure sshfs is here, offering to install it if it is not.
///
/// Asked on the terminal, and only on one: piped or scripted, there is nobody
/// to answer and nobody to type a sudo password, so the instruction is printed
/// and the sandbox fails as it did before this existed. The package manager
/// runs with this terminal as its own — sudo asks for the password there, and
/// its output is where the person who said yes can read it.
pub fn ensure() -> anyhow::Result<()> {
    if installed() {
        return Ok(());
    }
    let plan = plan_here();
    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let argv = match &plan {
        Plan::Run(argv) if interactive => argv.clone(),
        _ => anyhow::bail!(
            "cctop sandbox needs sshfs on this machine. {}",
            by_hand(&plan)
        ),
    };
    let mut err = std::io::stderr();
    let _ = write!(err, "sshfs is required. Install it now? [Y/n] ");
    let _ = err.flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    if !says_yes(&answer) {
        anyhow::bail!("sshfs was not installed. {}", by_hand(&plan));
    }
    eprintln!("cctop sandbox: running {}", argv.join(" "));
    let status = Command::new(&argv[0]).args(&argv[1..]).status()?;
    if !status.success() || !installed() {
        anyhow::bail!(
            "installing sshfs did not work ({status}). {}",
            by_hand(&plan)
        );
    }
    Ok(())
}

/// `[Y/n]`: an empty answer is yes, as the capital says.
fn says_yes(answer: &str) -> bool {
    matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "" | "y" | "yes"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn the_first_package_manager_on_the_path_is_the_one_used() {
        assert_eq!(Manager::detect(|_| false), None);
        assert_eq!(
            Manager::detect(|b| b == "apt-get" || b == "dnf"),
            Some(Manager::Apt)
        );
        // RHEL 8+ has both; yum there is dnf in disguise.
        assert_eq!(
            Manager::detect(|b| b == "yum" || b == "dnf"),
            Some(Manager::Dnf)
        );
        assert_eq!(Manager::detect(|b| b == "apk"), Some(Manager::Apk));
    }

    #[test]
    fn each_distribution_gets_its_own_package_name_and_flags() {
        let cases = [
            (Manager::Apt, &["apt-get", "install", "-y", "sshfs"][..]),
            (Manager::Dnf, &["dnf", "install", "-y", "fuse-sshfs"]),
            (Manager::Yum, &["yum", "install", "-y", "fuse-sshfs"]),
            (Manager::Pacman, &["pacman", "-S", "--noconfirm", "sshfs"]),
            (
                Manager::Zypper,
                &["zypper", "--non-interactive", "install", "sshfs"],
            ),
            (Manager::Apk, &["apk", "add", "sshfs"]),
        ];
        for (manager, want) in cases {
            assert_eq!(manager.argv(), argv(want), "{manager:?}");
        }
    }

    #[test]
    fn sudo_is_used_only_when_not_root_and_only_if_it_is_there() {
        assert_eq!(
            plan(Some(Manager::Apt), false, true),
            Plan::Run(argv(&["sudo", "apt-get", "install", "-y", "sshfs"]))
        );
        assert_eq!(
            plan(Some(Manager::Apk), true, false),
            Plan::Run(argv(&["apk", "add", "sshfs"]))
        );
        assert_eq!(
            plan(Some(Manager::Pacman), false, false),
            Plan::Tell(argv(&["sudo", "pacman", "-S", "--noconfirm", "sshfs"]))
        );
        assert_eq!(plan(None, false, true), Plan::Unknown);
    }

    #[test]
    fn an_empty_answer_is_yes_and_anything_else_but_yes_is_no() {
        for yes in ["", "\n", "y\n", "Y", "yes", " YES \n"] {
            assert!(says_yes(yes), "{yes:?}");
        }
        for no in ["n\n", "no", "nope", "q"] {
            assert!(!says_yes(no), "{no:?}");
        }
    }

    #[test]
    fn the_hand_instruction_names_the_command_or_the_package() {
        let told = by_hand(&plan(Some(Manager::Dnf), false, true));
        assert!(told.contains("sudo dnf install -y fuse-sshfs"), "{told}");
        assert!(by_hand(&Plan::Unknown).contains("fuse-sshfs"));
    }
}
