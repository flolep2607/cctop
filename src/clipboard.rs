//! The image on the system clipboard, as a file an agent can be pointed at.
//!
//! Terminals do not deliver images. A bracketed paste carries text and nothing
//! else, so a screenshot copied with the system's own shortcut arrives at an
//! agent as nothing at all — the keystroke reaches cctop, cctop forwards it,
//! and the pane shows an empty paste. Every harness cctop watches reads an
//! image the same second way, though: a path in the prompt. So the image is
//! written to a file and the *path* is what gets typed, which is a paste the
//! pty can carry.
//!
//! Reading the clipboard is a platform helper's job, as writing it already is
//! in [`crate::ui::render::copy_to_clipboard`]. The helpers are tried in turn
//! and the first that produces a PNG wins.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where pasted images are kept.
///
/// Under the cache directory, but never deleted by cctop: the path has been
/// handed to an agent by then, and a conversation resumed a week later may read
/// it again. `--clear-cache` does not touch them either — it removes one file,
/// the cost cache.
///
/// ponytail: nothing prunes this directory. Each image is one screenshot the
/// user deliberately pasted, the directory is theirs to empty, and a cctop that
/// deleted an image out from under a transcript that still refers to it would
/// be losing the user's data to save a megabyte.
fn paste_dir() -> PathBuf {
    crate::config::CACHE_DIR.join("pastes")
}

/// The eight bytes every PNG starts with.
///
/// Checked on whatever a helper produced, because most of them cannot say "the
/// clipboard holds no image": `xclip` prints an error to stderr and exits 0
/// with an empty stdout, and a text clipboard converted by a helper that tried
/// too hard is a file the agent would open and reject. A file that is not a PNG
/// is treated as nothing having been pasted.
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Why there is no image to paste.
///
/// Two cases, kept apart because they need different things of the user: an
/// empty clipboard is answered by copying something, and a machine with no
/// helper installed is answered by installing one. A single "could not paste"
/// would leave a Linux user without `wl-clipboard` waiting for a screenshot
/// that can never arrive.
#[derive(Debug, PartialEq, Eq)]
pub enum NoImage {
    /// A helper ran and reported no image on the clipboard.
    Clipboard,
    /// No helper cctop knows is installed here.
    NoTool,
}

impl NoImage {
    pub fn message(&self) -> String {
        match self {
            NoImage::Clipboard => "No image on the clipboard".to_string(),
            // Over ssh the advice to install a clipboard tool is worse than no
            // advice: the clipboard is on the machine the ssh was typed on, and
            // nothing installed on this one will ever see it. What works there
            // is sending the image as text, which is the one thing the
            // connection already carries.
            NoImage::NoTool if over_ssh() => {
                // One line, and short. A continued literal here once had the
                // source's own indentation folded into it, and the status bar
                // showed the message with a gap chewed out of the middle.
                "The clipboard is on the machine you sshed from — F1 says how to get one here"
                    .to_string()
            }
            NoImage::NoTool => format!("No tool here can read an image clipboard — {}", HOW),
        }
    }
}

/// Whether this cctop is being watched from another machine.
///
/// Any of the three: `SSH_TTY` is absent when the session has no terminal,
/// `SSH_CLIENT` is dropped by some sshd builds, and a login shell may pass on
/// only one of them.
fn over_ssh() -> bool {
    ["SSH_CONNECTION", "SSH_TTY", "SSH_CLIENT"]
        .iter()
        .any(|var| std::env::var_os(var).is_some_and(|v| !v.is_empty()))
}

/// What to install, per platform, named in the one message that needs it.
#[cfg(target_os = "macos")]
const HOW: &str = "install pngpaste, or use a build of macOS with osascript";
#[cfg(target_os = "windows")]
const HOW: &str = "powershell.exe was not found on PATH";
#[cfg(all(unix, not(target_os = "macos")))]
const HOW: &str = "install wl-clipboard or xclip (WSL uses powershell.exe)";

/// Write the clipboard's image to a new file and return its path.
pub fn image_to_file() -> Result<PathBuf, NoImage> {
    let dir = paste_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return Err(NoImage::NoTool);
    }
    let dest = dir.join(new_name());

    let mut ran_something = false;
    for helper in HELPERS {
        match helper.run(&dest) {
            Attempt::Wrote => return Ok(dest),
            // The helper is here and answered; the clipboard simply holds no
            // image. Later helpers would be asking the same clipboard.
            Attempt::Empty => ran_something = true,
            Attempt::Missing => {}
        }
    }
    // Nothing was left behind by a helper that started and then produced
    // something unusable.
    let _ = std::fs::remove_file(&dest);
    match ran_something {
        true => Err(NoImage::Clipboard),
        false => Err(NoImage::NoTool),
    }
}

/// The PNG a paste is carrying, if it is carrying one.
///
/// The way an image reaches a cctop that cannot see the clipboard at all: over
/// ssh, where the clipboard is on the machine you typed the `ssh` on and no
/// helper on this side can ever reach it. Base64 is text, text is what a
/// terminal pastes, so an image encoded on the near side arrives intact on the
/// far one.
///
/// Two spellings are read: a `data:image/png;base64,…` URI, which is what a
/// browser and most "copy as base64" tools produce, and the bare base64 of a
/// PNG. The bare form is recognised by its first characters — every base64 PNG
/// begins `iVBORw0KGgo`, being the encoding of the file's magic number — which
/// is a strong enough sniff that ordinary pasted text can never be mistaken
/// for one.
pub fn png_from_paste(text: &str) -> Option<Vec<u8>> {
    let trimmed = text.trim();
    // Cheap rejections first: this is asked of every paste, including the
    // hundred-line ones people put in front of an agent all day.
    let body = match trimmed.strip_prefix("data:image/png;base64,") {
        Some(rest) => rest,
        None if trimmed.starts_with("iVBORw0KGgo") => trimmed,
        None => return None,
    };
    let bytes = crate::util::b64_decode(body)?;
    bytes.starts_with(PNG_MAGIC).then_some(bytes)
}

/// Write `png` where a pasted image goes, and give back the path.
pub fn write_png(png: &[u8]) -> std::io::Result<PathBuf> {
    let dir = paste_dir();
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(new_name());
    std::fs::write(&dest, png)?;
    Ok(dest)
}

/// `paste-20260901-142233.png`.
///
/// The time rather than a counter: this is what the user sees in the agent's
/// prompt and in the status line, and `paste-7.png` says nothing about which
/// screenshot it was.
fn new_name() -> String {
    format!("paste-{}.png", chrono::Local::now().format("%Y%m%d-%H%M%S"))
}

/// What one helper did.
enum Attempt {
    /// A PNG is at the destination.
    Wrote,
    /// The helper ran and there was no image to be had.
    Empty,
    /// The helper is not installed.
    Missing,
}

/// One way of getting the clipboard's image out of the system.
struct Helper {
    command: &'static str,
    /// Arguments, with `{}` standing for the destination path — in the form
    /// that helper wants, which on WSL is the Windows spelling of it.
    args: &'static [&'static str],
    /// Whether the PNG comes back on stdout or is written by the helper itself.
    output: Output,
}

enum Output {
    Stdout,
    File,
}

/// The helpers, in the order they are asked.
///
/// Each platform's own first: a Linux desktop is answered by `wl-paste` or
/// `xclip`, a Mac by `pngpaste` when it is installed and by `osascript` when it
/// is not, and Windows — including a WSL cctop watching agents that run under
/// it, which is where a Windows clipboard reaches a Linux process at all — by
/// PowerShell. Absent commands are skipped, so the list is tried top to bottom
/// on any machine and the order is only about which answer is preferred.
///
/// ponytail: PNG only. Every screenshot tool on every one of these platforms
/// puts a PNG on the clipboard, and asking each helper for a second format
/// would double the list to catch a case nobody has reported.
const HELPERS: &[Helper] = &[
    Helper {
        command: "wl-paste",
        args: &["--no-newline", "--type", "image/png"],
        output: Output::Stdout,
    },
    Helper {
        command: "xclip",
        args: &["-selection", "clipboard", "-t", "image/png", "-o"],
        output: Output::Stdout,
    },
    Helper {
        command: "pngpaste",
        args: &["-"],
        output: Output::Stdout,
    },
    // AppleScript's clipboard, which every Mac has: `«class PNGf»` is the
    // clipboard's PNG flavour, and the script writes it rather than printing
    // it because osascript would mangle binary on stdout.
    Helper {
        command: "osascript",
        args: &[
            "-e",
            "set f to open for access POSIX file \"{}\" with write permission",
            "-e",
            "try",
            "-e",
            "write (the clipboard as «class PNGf») to f",
            "-e",
            "end try",
            "-e",
            "close access f",
        ],
        output: Output::File,
    },
    // Windows, and WSL through it. `-STA` because the clipboard API refuses to
    // answer a multi-threaded apartment, which is what a `-Command` process is
    // otherwise; without it this returns nothing on a clipboard that holds a
    // perfectly good screenshot.
    Helper {
        command: "powershell.exe",
        args: &[
            "-NoProfile",
            "-STA",
            "-Command",
            "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; \
             $i=[System.Windows.Forms.Clipboard]::GetImage(); \
             if ($null -eq $i) { exit 1 }; \
             $i.Save('{}',[System.Drawing.Imaging.ImageFormat]::Png)",
        ],
        output: Output::File,
    },
];

/// Where Windows keeps PowerShell, for the WSL sessions whose `PATH` does not
/// carry the interop entries.
///
/// Which is any session nobody logged into interactively: an ssh into the WSL
/// distribution, a cron job, a service. `powershell.exe` is on `PATH` in a
/// terminal opened by hand and absent from one reached over ssh — the same
/// machine, the same clipboard, and cctop reporting "no tool here can read an
/// image clipboard" in the second case only. Both spellings, because the drive
/// is mounted case-insensitively and either can be what exists.
const WSL_POWERSHELL: &[&str] = &[
    "/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe",
    "/mnt/c/WINDOWS/System32/WindowsPowerShell/v1.0/powershell.exe",
];

impl Helper {
    /// The command, then the places it might be when the name alone misses.
    fn programs(&self) -> Vec<&'static str> {
        let mut out = vec![self.command];
        if self.command == "powershell.exe" {
            out.extend(
                WSL_POWERSHELL
                    .iter()
                    .filter(|p| Path::new(p).exists())
                    .copied(),
            );
        }
        out
    }

    fn run(&self, dest: &Path) -> Attempt {
        let Some(path) = self.dest_for(dest) else {
            return Attempt::Missing;
        };
        let args: Vec<String> = self
            .args
            .iter()
            .map(|arg| arg.replace("{}", &path))
            .collect();
        // The first spelling of the command that is actually here. A helper
        // that ran and found no image stops the search: the next spelling would
        // be asking the same clipboard.
        let mut out = None;
        for program in self.programs() {
            let attempt = Command::new(program)
                .args(&args)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output();
            if let Ok(done) = attempt {
                out = Some(done);
                break;
            }
        }
        let Some(out) = out else {
            return Attempt::Missing;
        };
        match self.output {
            Output::Stdout => match out.stdout.starts_with(PNG_MAGIC) {
                true => match std::fs::write(dest, &out.stdout) {
                    Ok(()) => Attempt::Wrote,
                    Err(_) => Attempt::Empty,
                },
                false => Attempt::Empty,
            },
            Output::File => match is_png(dest) {
                true => Attempt::Wrote,
                false => Attempt::Empty,
            },
        }
    }

    /// The destination in the spelling this helper understands.
    ///
    /// A Windows program cannot open `/home/…`: under WSL the file lives on the
    /// Linux side and PowerShell reaches it through `\\wsl.localhost\…`, which
    /// is what `wslpath -w` prints. Without the translation the save fails on
    /// the one platform this helper exists for.
    fn dest_for(&self, dest: &Path) -> Option<String> {
        if self.command != "powershell.exe" || cfg!(windows) {
            return Some(dest.display().to_string());
        }
        let out = Command::new("wslpath")
            .arg("-w")
            .arg(dest)
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!path.is_empty()).then_some(path)
    }
}

fn is_png(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    bytes.starts_with(PNG_MAGIC)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Anything that is not a PNG is nothing having been pasted.
    ///
    /// The check exists because the helpers cannot say so themselves: `xclip`
    /// exits 0 with an empty stdout when the clipboard holds text, and a file
    /// of HTML named `.png` is a path the agent would open and reject with an
    /// error nobody could trace back to here.
    #[test]
    fn only_a_png_counts_as_an_image() {
        let dir = tempfile::tempdir().expect("tempdir");
        let good = dir.path().join("good.png");
        std::fs::write(&good, PNG_MAGIC).expect("write");
        assert!(is_png(&good));

        let text = dir.path().join("text.png");
        std::fs::write(&text, "<html>not an image</html>").expect("write");
        assert!(!is_png(&text));

        assert!(!is_png(&dir.path().join("missing.png")));
    }

    /// The real thing, against whatever is on this machine's clipboard.
    ///
    /// Ignored by default: it needs a desktop session and a helper installed,
    /// which CI has neither of, and it reads a clipboard that belongs to
    /// whoever is sitting there. Run it deliberately —
    /// `cargo test -- --ignored clipboard` — after copying an image, which is
    /// the only way to find out that a helper's arguments are wrong on a
    /// platform: every one of them fails by producing nothing, which is
    /// indistinguishable from an empty clipboard.
    #[test]
    #[ignore = "reads the machine's real clipboard"]
    fn the_clipboard_image_becomes_a_png_on_disk() {
        match image_to_file() {
            Ok(path) => {
                assert!(is_png(&path), "{} is not a PNG", path.display());
                eprintln!(
                    "pasted {} bytes to {}",
                    std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                    path.display()
                );
            }
            Err(why) => panic!("nothing was pasted: {}", why.message()),
        }
    }

    /// The destination reaches each helper in the spelling it can open, and
    /// every `{}` in the argument list is filled — a helper that was handed a
    /// literal `{}` would write a file by that name and report success.
    #[test]
    fn the_destination_is_substituted_into_every_argument() {
        let script = HELPERS
            .iter()
            .find(|h| h.command == "osascript")
            .expect("the AppleScript helper");
        let dest = Path::new("/tmp/paste-1.png");
        let path = script.dest_for(dest).expect("a path for a local helper");
        assert_eq!(path, "/tmp/paste-1.png");

        let filled: Vec<String> = script
            .args
            .iter()
            .map(|arg| arg.replace("{}", &path))
            .collect();
        assert!(
            filled.iter().any(|a| a.contains("/tmp/paste-1.png")),
            "the path never reached the script: {filled:?}"
        );
        assert!(
            !filled.iter().any(|a| a.contains("{}")),
            "an argument kept its placeholder: {filled:?}"
        );
    }
}
