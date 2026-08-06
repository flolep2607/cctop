//! Shell aliases that route agent commands through cctop's pty shim.
//!
//! An agent started straight from the shell owns a terminal cctop can't type
//! into; started as `cctop claude` it runs on a pty cctop holds, which is what
//! makes `s` work. That difference is invisible in use, so the alias is worth
//! installing for the user rather than documenting and hoping.
//!
//! The block is written once, marked, and self-guarding: every alias is
//! conditional on both cctop and the agent being on `PATH`, so an uninstalled
//! cctop leaves `claude` meaning the real `claude` instead of a broken command.

use std::path::PathBuf;

const BEGIN: &str = "# >>> cctop >>>";
const END: &str = "# <<< cctop <<<";

/// Agent commands worth routing through the shim. Absent ones are skipped by the
/// block's own guard, so listing an agent the user doesn't have costs nothing.
const AGENTS: &str = "claude codex opencode pi";

/// The managed block, in the bash/zsh syntax both shells share.
fn block() -> String {
    format!(
        "{BEGIN}\n\
         # Added by cctop so it can type into these sessions (press `s` in the UI).\n\
         # Remove with `cctop --remove-alias`, or delete this block by hand. The\n\
         # guards make each alias a no-op unless both commands exist, so removing\n\
         # cctop leaves the agents working as before.\n\
         if command -v cctop >/dev/null 2>&1; then\n\
         \x20 for _cctop_agent in {AGENTS}; do\n\
         \x20   command -v \"$_cctop_agent\" >/dev/null 2>&1 && alias \"$_cctop_agent=cctop $_cctop_agent\"\n\
         \x20 done\n\
         \x20 unset _cctop_agent\n\
         fi\n\
         {END}\n"
    )
}

/// Shell startup files to manage, limited to those that already exist: creating
/// a `.bashrc` for someone who runs zsh would be litter, not help.
fn rc_files() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    [".zshrc", ".bashrc"]
        .iter()
        .map(|f| home.join(f))
        .filter(|p| p.is_file())
        .collect()
}

/// Fish's own file, which is a whole file rather than a block.
///
/// Fish sources everything in `conf.d` automatically, so the aliases get a file
/// of their own: nothing to parse back out of `config.fish`, and removal is a
/// deletion. `None` unless fish is configured on this machine.
///
/// Fish keeps its config under `~/.config/fish` on every platform, including
/// macOS — which is why this doesn't go through `dirs::config_dir`.
fn fish_file() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => dirs::home_dir()?.join(".config"),
    };
    let fish = base.join("fish");
    fish.is_dir()
        .then(|| fish.join("conf.d").join("cctop.fish"))
}

/// Fish equivalent of [`block`]. Same guards, fish syntax.
fn fish_config() -> String {
    format!(
        "# Added by cctop so it can type into these sessions (press `s` in the UI).\n\
         # Remove with `cctop --remove-alias`, or delete this file. The guards make\n\
         # each alias a no-op unless both commands exist, so removing cctop leaves\n\
         # the agents working as before.\n\
         if type -q cctop\n\
         \x20   for _cctop_agent in {AGENTS}\n\
         \x20       if type -q $_cctop_agent\n\
         \x20           alias $_cctop_agent \"cctop $_cctop_agent\"\n\
         \x20       end\n\
         \x20   end\n\
         end\n"
    )
}

/// `text` with any previously written block removed.
fn without_block(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut skipping = false;
    for line in text.lines() {
        if line.trim_end() == BEGIN {
            skipping = true;
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
        if skipping && line.trim_end() == END {
            skipping = false;
        }
    }
    out
}

/// Write the aliases into every shell's startup file, replacing any earlier
/// copy. Returns the files changed.
pub fn install() -> Vec<PathBuf> {
    let mut changed = edit(|text| {
        let mut out = without_block(text);
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&block());
        out
    });
    if let Some(path) = fish_file()
        && std::fs::read_to_string(&path).ok() != Some(fish_config())
        && path
            .parent()
            .is_some_and(|d| std::fs::create_dir_all(d).is_ok())
        && std::fs::write(&path, fish_config()).is_ok()
    {
        changed.push(path);
    }
    changed
}

/// Remove the aliases from every shell's startup file. Returns the files changed.
pub fn remove() -> Vec<PathBuf> {
    let mut changed = edit(without_block);
    if let Some(path) = fish_file()
        && path.is_file()
        && std::fs::remove_file(&path).is_ok()
    {
        changed.push(path);
    }
    changed
}

fn edit(f: impl Fn(&str) -> String) -> Vec<PathBuf> {
    rc_files()
        .into_iter()
        .filter(|path| {
            let Ok(text) = std::fs::read_to_string(path) else {
                return false;
            };
            let new = f(&text);
            // Rewriting an unchanged file would bump its mtime for nothing, and
            // would report a change that didn't happen.
            new != text && std::fs::write(path, new).is_ok()
        })
        .collect()
}

/// Install the aliases the first time cctop runs interactively, then never again
/// — so removing the block, by flag or by hand, stays removed.
pub fn install_once(prefs: &mut crate::cache::UiPrefs) {
    if prefs.shell_alias_installed {
        return;
    }
    prefs.shell_alias_installed = true;
    prefs.save();
    let changed = install();
    if changed.is_empty() {
        return;
    }
    let files: Vec<_> = changed.iter().map(|p| p.display().to_string()).collect();
    // Printed before the UI takes the alternate screen, so it is still there
    // after quitting rather than flashing past.
    eprintln!(
        "cctop: aliased {AGENTS} to `cctop <agent>` in {} so it can type into \
         those sessions. Undo with `cctop --remove-alias`.",
        files.join(" and ")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Installing has to be repeatable — the block is rewritten on every version
    /// that changes it — and removing has to leave the file exactly as found.
    #[test]
    fn the_block_is_replaced_not_stacked_and_removes_cleanly() {
        let original = "export PATH=$PATH:/opt/bin\nalias ll='ls -l'\n";
        let installed = {
            let mut t = without_block(original);
            t.push_str(&block());
            t
        };
        assert_eq!(installed.matches(BEGIN).count(), 1);

        // A second install over the first leaves one block, not two.
        let again = {
            let mut t = without_block(&installed);
            t.push_str(&block());
            t
        };
        assert_eq!(again, installed);
        assert_eq!(without_block(&again), original);
    }

    /// The user's own lines around the block must survive its removal.
    #[test]
    fn removal_keeps_surrounding_lines() {
        let text = format!("before\n{}after\n", block());
        assert_eq!(without_block(&text), "before\nafter\n");
    }
}
