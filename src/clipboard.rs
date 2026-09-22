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
//! and the first that produces a PNG wins. Where no helper can see the
//! clipboard at all — over ssh, where it lives on the machine the ssh was
//! typed on — the terminal itself is asked through the escapes that reach it
//! down the wire ([`image_from_terminal`]), and a paste that is an image in
//! base64 is filed by [`image_from_paste`].

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

/// What to install, named in the one message that needs it.
const HOW: &str = "install wl-clipboard or xclip (WSL uses powershell.exe)";

/// Write the clipboard's image to a new file and return its path.
///
/// `ask_terminal` is whether the terminal itself may be asked when no helper
/// answers — a request that travels down the wire and can take a moment, so
/// it is for keys that mean "paste an image" and not for `Ctrl+V`, which is
/// pressed to paste *whatever* is there and cannot stall on a clipboard that
/// is usually text.
pub fn image_to_file(ask_terminal: bool) -> Result<PathBuf, NoImage> {
    let dir = paste_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return Err(NoImage::NoTool);
    }
    let Ok(dest) = reserve(&dir, "png") else {
        return Err(NoImage::NoTool);
    };

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

    // The terminal is asked when it might see a clipboard the helpers cannot:
    // always over ssh, where that clipboard is the only one that exists, and
    // locally only when no helper ran at all — a helper that reported empty
    // was already asking the same clipboard the terminal would.
    if ask_terminal
        && (over_ssh() || !ran_something)
        && let Some(image) = image_from_terminal()
    {
        return write_image(&image).map_err(|_| NoImage::NoTool);
    }
    match ran_something {
        true => Err(NoImage::Clipboard),
        false => Err(NoImage::NoTool),
    }
}

/// An image recovered from a paste or a terminal's answer, in whichever
/// format it arrived.
pub struct PastedImage {
    pub bytes: Vec<u8>,
    /// The file extension for the format `bytes` is, which is what
    /// [`write_image`] names the file and what a reader keys off.
    pub ext: &'static str,
}

/// The image a paste is carrying, if it is carrying one.
///
/// The way an image reaches a cctop that cannot see the clipboard at all: over
/// ssh, where the clipboard is on the machine you typed the `ssh` on and no
/// helper on this side can ever reach it. Base64 is text, text is what a
/// terminal pastes, so an image encoded on the near side arrives intact on the
/// far one.
///
/// Two spellings are read: a `data:image/…;base64,…` URI, which is what a
/// browser and most "copy as base64" tools produce, and the bare base64 of an
/// image. The bare form is recognised by its first characters — every base64
/// PNG begins `iVBORw0KGgo`, being the encoding of the file's magic number —
/// and whatever either spelling decodes to is sniffed again, so the claim in
/// the URI is never taken on trust.
pub fn image_from_paste(text: &str) -> Option<PastedImage> {
    let trimmed = text.trim();
    // Cheap rejections first: this is asked of every paste, including the
    // hundred-line ones people put in front of an agent all day.
    let body = match trimmed.strip_prefix("data:image/") {
        // The payload is whatever follows ";base64,"; a mediatype parameter
        // between the two is ignored, because the bytes say what they are.
        Some(rest) => rest.split_once(";base64,")?.1,
        None if BARE_PREFIXES.iter().any(|p| trimmed.starts_with(p)) => trimmed,
        None => return None,
    };
    let bytes = crate::util::b64_decode(body)?;
    let ext = format_of(&bytes)?;
    Some(PastedImage { bytes, ext })
}

/// The first characters of each format's base64 — the encodings of their
/// magic numbers — so a paste that cannot be an image is rejected before it
/// is ever decoded.
const BARE_PREFIXES: &[&str] = &[
    "iVBORw0KGgo", // PNG
    "/9j/",        // JPEG
    "R0lGOD",      // GIF
    "UklGR",       // RIFF — WebP is confirmed on the decoded bytes
];

/// Smaller than this is not a real image. The floor is what keeps the sniff
/// honest: a sentence that happens to open with a format's base64 prefix is
/// prose that decoded to a few bytes, not a picture.
const MIN_IMAGE_BYTES: usize = 32;

/// The image format `bytes` actually are, by magic number — never by the name
/// a `data:` URI or a mime field claimed.
fn format_of(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < MIN_IMAGE_BYTES {
        return None;
    }
    if bytes.starts_with(PNG_MAGIC) {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(&b"WEBP"[..]) {
        Some("webp")
    } else if bytes.starts_with(b"BM") {
        Some("bmp")
    } else {
        None
    }
}

/// Write `image` where a pasted image goes, and give back the path.
pub fn write_image(image: &PastedImage) -> std::io::Result<PathBuf> {
    let dir = paste_dir();
    std::fs::create_dir_all(&dir)?;
    let dest = reserve(&dir, image.ext)?;
    std::fs::write(&dest, &image.bytes)?;
    Ok(dest)
}

/// Claim a path in the pastes directory, creating the file so nobody else can
/// claim it.
///
/// `paste-20260901-142233.png`, named for the time rather than a counter: this
/// is what the reader sees in the agent's prompt and in the status line, and
/// `paste-7.png` says nothing about which screenshot it was.
///
/// The name only resolves to a second, though, and several things can paste
/// inside one: two cctops watching the same machine, a page and a terminal, or
/// one person pressing F9 twice. Whoever wrote second used to overwrite the
/// first — leaving the earlier agent holding a path to somebody else's picture,
/// which is worse than a failure because it looks like it worked. So the name
/// is claimed with `create_new`, which is atomic across processes, and a taken
/// one becomes `-2`, `-3`, and so on.
fn reserve(dir: &Path, ext: &str) -> std::io::Result<PathBuf> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    for n in 1..=99u32 {
        let name = match n {
            1 => format!("paste-{stamp}.{ext}"),
            n => format!("paste-{stamp}-{n}.{ext}"),
        };
        let dest = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
        {
            Ok(_) => return Ok(dest),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    // A hundred pastes in one second is not a person, and the alternative to
    // giving up is a loop that never ends.
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "too many images pasted in the same second",
    ))
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
/// The desktop's own first: a Linux desktop is answered by `wl-paste` or
/// `xclip`, and a WSL cctop — watching agents that run under it, which is where
/// a Windows clipboard reaches a Linux process at all — by PowerShell. Absent
/// commands are skipped, so the list is tried top to bottom on any machine and
/// the order is only about which answer is preferred.
///
/// ponytail: PNG only. Every screenshot tool puts a PNG on the clipboard, and
/// asking each helper for a second format would double the list to catch a case
/// nobody has reported.
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
    // Windows, reached through WSL. `-STA` because the clipboard API refuses to
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
    /// is what `wslpath -w` prints. Without the translation the save fails for
    /// the one helper that needs it.
    fn dest_for(&self, dest: &Path) -> Option<String> {
        if self.command != "powershell.exe" {
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

/// The first byte of a terminal's answer to a clipboard read, and the whole
/// answer's, deadlines.
///
/// The first wait is short on purpose: a terminal that will answer does so at
/// once — it is right there on the wire — and anything longer means both asks
/// went unanswered, which is what most terminals do with a read. Stretching
/// that silence to seconds would make F9 feel broken exactly where it already
/// failed. Once bytes are flowing the deadline stretches for the answer
/// itself: a screenshot is a hundred chunks down an ssh link.
const FIRST_BYTE: Duration = Duration::from_millis(300);
const WHOLE_REPLY: Duration = Duration::from_millis(1500);

/// Ask the terminal itself for the clipboard's image.
///
/// The last resort, and the only one that reaches a clipboard on the other
/// side of an ssh link: the escapes ride the terminal connection, so the
/// machine the clipboard lives on is the one answering. Two asks are sent.
/// Kitty's OSC 5522 names the mime types wanted, which is the only protocol
/// that can hand back `image/png` rather than text; the classic OSC 52 read
/// is for whatever else will give the clipboard up, which is text where it is
/// answered at all — and text that is itself a base64 image files the same
/// way, through [`image_from_paste`]. Terminals that refuse reads answer
/// neither, which costs the first-byte wait and nothing else.
fn image_from_terminal() -> Option<PastedImage> {
    use std::io::Write;
    let mimes =
        crate::util::b64_encode(b"image/png image/jpeg image/webp image/gif image/bmp text/plain");
    let kitty = format!("\x1b]5522;type=read;{mimes}\x1b\\");
    let osc52 = "\x1b]52;c;?\x07";
    let mut query = format!("{kitty}{osc52}");
    // Inside tmux the asks need the passthrough wrapper to reach the real
    // terminal — or tmux answers them itself, from its own paste buffers.
    // The wrapper is a plain DCS to a tmux without `allow-passthrough`, which
    // ignores it, so it is sent unconditionally.
    if std::env::var_os("TMUX").is_some() {
        for seq in [kitty, osc52.to_string()] {
            query.push_str("\x1bPtmux;");
            for c in seq.chars() {
                if c == '\x1b' {
                    query.push_str("\x1b\x1b");
                } else {
                    query.push(c);
                }
            }
            query.push_str("\x1b\\");
        }
    }
    let mut out = std::io::stdout();
    let _ = out.write_all(query.as_bytes());
    let _ = out.flush();
    read_clipboard_replies()
}

/// Drain the reply to a clipboard read off stdin and pick the image out of it.
///
/// Read straight off the fd rather than through crossterm: the reply is an
/// OSC string, which crossterm's parser does not surface as an event, and the
/// loop only calls `event::read` again once this returns — nothing else is
/// touching stdin meanwhile. Whatever else arrives in the window — keystrokes,
/// mostly — is consumed here and lost; the window sits right behind the F9
/// that asked, and is the price of a reply that shares a stream with input.
fn read_clipboard_replies() -> Option<PastedImage> {
    let start = Instant::now();
    let mut buf = Vec::new();
    loop {
        let budget = if buf.is_empty() {
            FIRST_BYTE
        } else {
            WHOLE_REPLY
        };
        let Some(left) = budget.checked_sub(start.elapsed()) else {
            break;
        };
        let mut pfd = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut pfd, 1, left.as_millis() as i32) } <= 0 {
            break;
        }
        let mut chunk = [0u8; 16384];
        let n = unsafe { libc::read(0, chunk.as_mut_ptr().cast(), chunk.len()) };
        if n <= 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n as usize]);
        if clipboard_reply_complete(&buf) {
            break;
        }
    }
    image_from_replies(&buf)
}

/// The OSC packet at the head of `bytes` — which must open with `ESC ]` — as
/// its content and its length including the terminator. `None` on a tail that
/// is still arriving: the caller is reading a stream, and an unfinished packet
/// says wait, not reject.
fn osc_packet(bytes: &[u8]) -> Option<(&[u8], usize)> {
    for (i, &b) in bytes[2..].iter().enumerate() {
        match b {
            0x07 => return Some((&bytes[2..2 + i], i + 3)),
            0x1b if bytes.get(2 + i + 1) == Some(&b'\\') => {
                return Some((&bytes[2..2 + i], i + 4));
            }
            _ => {}
        }
    }
    None
}

/// The image in a terminal's clipboard replies, if it sent one.
///
/// Kitty's `status=DATA` packets arrive chunked per mime type, so the image
/// bytes are accumulated across them; a `text/plain` chunk and an OSC 52
/// payload alike land in `text`, where a base64 image hiding in the clipboard's
/// text is still found by [`image_from_paste`]. Anything between packets —
/// the keystroke that rode in with the reply — is skipped over.
fn image_from_replies(buf: &[u8]) -> Option<PastedImage> {
    let mut image = Vec::new();
    let mut text = Vec::new();
    let mut i = 0;
    while i + 1 < buf.len() {
        if buf[i] == 0x1b && buf[i + 1] == b']' {
            let Some((content, len)) = osc_packet(&buf[i..]) else {
                break;
            };
            let (num, rest) = match content.iter().position(|&b| b == b';') {
                Some(p) => (&content[..p], &content[p + 1..]),
                None => (content, &[][..]),
            };
            match num {
                // `52;<which>;<base64>` — the terminal's clipboard as text,
                // which is decoded once here and sniffed below: a clipboard
                // holding base64 of an image is still an image that got here.
                b"52" => {
                    if let Some(p) = rest.iter().position(|&b| b == b';')
                        && let Ok(s) = std::str::from_utf8(&rest[p + 1..])
                        && let Some(bytes) = crate::util::b64_decode(s)
                    {
                        text.extend_from_slice(&bytes);
                    }
                }
                // `5522;type=read:status=DATA:mime=<base64>;<base64>` — one
                // packet per chunk, the chunks of a type in a row.
                b"5522" => {
                    let (meta, payload) = match rest.iter().position(|&b| b == b';') {
                        Some(p) => (&rest[..p], &rest[p + 1..]),
                        None => (rest, &[][..]),
                    };
                    let mut data = false;
                    let mut mime = String::new();
                    for (k, v) in std::str::from_utf8(meta)
                        .unwrap_or("")
                        .split(':')
                        .filter_map(|f| f.split_once('='))
                    {
                        match k {
                            "status" => data = v == "DATA",
                            "mime" => {
                                mime = crate::util::b64_decode(v)
                                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                                    .unwrap_or_default();
                            }
                            _ => {}
                        }
                    }
                    if data
                        && let Ok(s) = std::str::from_utf8(payload)
                        && let Some(bytes) = crate::util::b64_decode(s)
                    {
                        if mime.starts_with("image/") {
                            image.extend_from_slice(&bytes);
                        } else {
                            text.extend_from_slice(&bytes);
                        }
                    }
                }
                _ => {}
            }
            i += len;
            continue;
        }
        i += 1;
    }
    if let Some(ext) = format_of(&image) {
        return Some(PastedImage { bytes: image, ext });
    }
    // A terminal that handed back raw image bytes is not one the spec
    // describes, but the check costs nothing next to the decode it skips.
    if let Some(ext) = format_of(&text) {
        return Some(PastedImage { bytes: text, ext });
    }
    image_from_paste(std::str::from_utf8(&text).unwrap_or(""))
}

/// Whether every clipboard answer that has started has also ended.
///
/// An OSC 52 reply is one packet and is done when it is; a kitty stream ends
/// only at `status=DONE` or an error code, so a `status=OK` still open keeps
/// the reader waiting for the chunks behind it.
fn clipboard_reply_complete(buf: &[u8]) -> bool {
    let mut saw_52 = false;
    let mut kitty_open = false;
    let mut kitty_done = false;
    let mut i = 0;
    while i + 1 < buf.len() {
        if buf[i] == 0x1b && buf[i + 1] == b']' {
            let Some((content, len)) = osc_packet(&buf[i..]) else {
                break;
            };
            match content.split(|&b| b == b';').next() {
                Some(b"52") => saw_52 = true,
                Some(b"5522") => {
                    // The metadata between `5522;` and the payload's `;`.
                    // Base64 carries neither delimiter, but bounding the read
                    // anyway keeps a malformed packet's payload out of it.
                    let after = content.get(5..).unwrap_or(&[]);
                    let end = after.iter().position(|&b| b == b';').unwrap_or(after.len());
                    let meta = std::str::from_utf8(&after[..end]).unwrap_or("");
                    let status = meta.split(':').find_map(|f| f.strip_prefix("status="));
                    match status {
                        Some("OK") | Some("DATA") => kitty_open = true,
                        Some(_) => kitty_done = true,
                        None => {}
                    }
                }
                _ => {}
            }
            i += len;
            continue;
        }
        i += 1;
    }
    (saw_52 || kitty_done) && !(kitty_open && !kitty_done)
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
        match image_to_file(false) {
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

    /// Two images pasted in the same second are two files.
    ///
    /// The name resolves to a second, and more than one thing can paste inside
    /// one — two cctops on the same machine, the page and a terminal, or a
    /// finger on F9 twice. The second write used to land on the first, handing
    /// an agent a path to somebody else's picture.
    #[test]
    fn a_second_paste_in_the_same_second_gets_its_own_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = reserve(dir.path(), "png").expect("first");
        let second = reserve(dir.path(), "png").expect("second");
        let third = reserve(dir.path(), "png").expect("third");
        assert_ne!(first, second);
        assert_ne!(second, third);
        // Claimed, not merely named: the file is there, which is what stops
        // another process choosing it a moment later.
        for path in [&first, &second, &third] {
            assert!(path.exists(), "{} was not claimed", path.display());
        }
        assert!(
            second
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("-2."),
            "the second name does not say which it is: {}",
            second.display()
        );
    }

    /// Every `{}` in a helper's argument list is filled — a helper that was
    /// handed a literal `{}` would write a file by that name and report
    /// success.
    #[test]
    fn the_destination_is_substituted_into_every_argument() {
        let script = HELPERS
            .iter()
            .find(|h| h.command == "powershell.exe")
            .expect("the PowerShell helper");
        let path = "/tmp/paste-1.png".to_string();

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

    /// Bytes that pass [`format_of`]'s floor — the magic, padded out to a
    /// plausible size. The sniff never looks further than the header, so the
    /// rest of a fake image can be anything.
    fn fake(magic: &[u8]) -> Vec<u8> {
        let mut bytes = magic.to_vec();
        bytes.resize(64, 0);
        bytes
    }

    /// Every spelling of "this paste is an image" files one, with the
    /// extension the bytes — not the mediatype claim — say it is.
    #[test]
    fn an_image_in_base64_is_recognised_in_both_spellings() {
        let png = crate::util::b64_encode(&fake(PNG_MAGIC));
        let jpeg = crate::util::b64_encode(&fake(&[0xFF, 0xD8, 0xFF]));
        let gif = crate::util::b64_encode(&fake(b"GIF89a"));
        let webp = crate::util::b64_encode(&fake(b"RIFF\x00\x00\x00\x00WEBP"));

        for (text, ext) in [
            (format!("data:image/png;base64,{png}"), "png"),
            (format!("data:image/png;charset=utf-8;base64,{png}"), "png"),
            (format!("data:image/jpeg;base64,{jpeg}"), "jpg"),
            (png.clone(), "png"),
            (jpeg, "jpg"),
            (gif, "gif"),
            (webp, "webp"),
        ] {
            let image = image_from_paste(&text).unwrap_or_else(|| panic!("{text:?} was refused"));
            assert_eq!(image.ext, ext, "{text:?}");
        }
    }

    /// A paste is prose far more often than it is a picture, so the sniff's
    /// rejections are the part worth pinning.
    #[test]
    fn prose_is_never_an_image() {
        assert!(image_from_paste("please fix the flywheel").is_none());
        assert!(image_from_paste("iVBORw0KGgo is not an image").is_none());
        // A claimed PNG whose bytes say otherwise is refused — the URI's
        // mediatype is a claim, not a fact.
        let text = format!(
            "data:image/png;base64,{}",
            crate::util::b64_encode(
                b"<html>definitely not a picture, whatever the name says</html>"
            )
        );
        assert!(image_from_paste(&text).is_none());
        // Under the floor is not a picture either.
        let tiny = format!(
            "data:image/png;base64,{}",
            crate::util::b64_encode(PNG_MAGIC)
        );
        assert!(image_from_paste(&tiny).is_none());
    }

    /// Wrapped base64 — `base64` without `-w0` — is still one image.
    #[test]
    fn a_wrapped_blob_decodes() {
        let b64 = crate::util::b64_encode(&fake(PNG_MAGIC));
        let wrapped = b64
            .chars()
            .collect::<Vec<_>>()
            .chunks(20)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        let image = image_from_paste(&wrapped).expect("wrapped base64 was refused");
        assert_eq!(image.ext, "png");
    }

    /// A kitty clipboard answer — OK, the image chunked, DONE — reassembles
    /// into the bytes it chunked.
    #[test]
    fn a_kitty_reply_hands_over_the_image() {
        let png = fake(PNG_MAGIC);
        let (a, b) = png.split_at(40);
        let mime = crate::util::b64_encode(b"image/png");
        let reply = format!(
            "\x1b]5522;type=read:status=OK\x1b\\\
             \x1b]5522;type=read:status=DATA:mime={mime};{}\x1b\\\
             \x1b]5522;type=read:status=DATA:mime={mime};{}\x1b\\\
             \x1b]5522;type=read:status=DONE\x1b\\",
            crate::util::b64_encode(a),
            crate::util::b64_encode(b),
        );
        let image = image_from_replies(reply.as_bytes()).expect("no image in a DATA stream");
        assert_eq!(image.ext, "png");
        assert_eq!(image.bytes, png);
    }

    /// The OSC 52 answer is the clipboard as text — which, when the text is
    /// itself a base64 image, is still the image.
    #[test]
    fn an_osc52_reply_can_carry_an_image_as_text() {
        let inner = crate::util::b64_encode(&fake(PNG_MAGIC));
        let reply = format!(
            "\x1b]52;c;{}\x07",
            crate::util::b64_encode(inner.as_bytes())
        );
        let image = image_from_replies(reply.as_bytes()).expect("no image in the OSC 52 text");
        assert_eq!(image.ext, "png");
    }

    /// Refusals, unrelated packets, and the keystroke that rode in with the
    /// reply are none of them an image.
    #[test]
    fn a_reply_without_an_image_is_no_image() {
        assert!(image_from_replies(b"").is_none());
        assert!(image_from_replies(b"\x1b]5522;type=read:status=EPERM\x1b\\").is_none());
        let mime = crate::util::b64_encode(b"text/plain");
        let reply = format!(
            "q\x1b]52;c;{}\x07\x1b]5522;type=read:status=DATA:mime={mime};{}\x1b\\\
             \x1b]5522;type=read:status=DONE\x1b\\",
            crate::util::b64_encode(b"just words"),
            crate::util::b64_encode(b"more words"),
        );
        assert!(image_from_replies(reply.as_bytes()).is_none());
    }

    /// The reader stops when what answered has finished — an OSC 52 packet is
    /// its own whole answer, and a kitty stream is open until DONE.
    #[test]
    fn the_reply_is_complete_when_its_senders_are() {
        assert!(!clipboard_reply_complete(b""));
        assert!(!clipboard_reply_complete(
            b"\x1b]5522;type=read:status=OK\x1b\\"
        ));
        assert!(clipboard_reply_complete(
            b"\x1b]5522;type=read:status=OK\x1b\\\x1b]5522;type=read:status=DONE\x1b\\"
        ));
        assert!(clipboard_reply_complete(b"\x1b]52;c;aGk=\x07"));
        // A 52 answer does not excuse a kitty stream that is still open.
        assert!(!clipboard_reply_complete(
            b"\x1b]52;c;aGk=\x07\x1b]5522;type=read:status=OK\x1b\\"
        ));
    }
}
