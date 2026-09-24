//! The run's two credentials, and `--token-file`, which lets them outlive it.
//!
//! By default every `cctop serve` mints fresh tokens, and a restart revokes
//! every link handed out — the right default for a link that can type at your
//! agents. It is the wrong one for a bookmarked dashboard, or a read-only link
//! pinned to a status board, which break on every restart and have to be
//! handed out again. `--token-file` is the opt-in: the tokens are read from the
//! file when it exists, and minted and written to it when it does not.
//!
//! A Prometheus scrape is not one of the reasons: `/metrics` answers without a
//! token at all — see [`super::metrics`].
//!
//! # Why the file is refused rather than repaired
//!
//! The file holds the full token, which is a prompt box wired to a live agent.
//! So a file that is not plainly the user's own is not used:
//!
//! - **Readable by group or others**: anyone who can read it already holds
//!   the credential, so serving with it would be serving with a token that is
//!   no longer a secret. Tightening the mode after the fact does not un-leak it.
//! - **Owned by another user**: that user can rewrite it, and a token someone
//!   else chose is a token someone else knows.
//! - **A symlink, or not a regular file**: the check has to be made on the
//!   thing that is read, and a link can be pointed somewhere else between the
//!   check and the read.
//!
//! Each refusal says to delete the file or pass `--rotate-token`, both of which
//! mint new tokens — the old ones being exactly what is in doubt.
//!
//! ponytail: the directories above the file are not checked. A token file in a
//! directory another user can write to can be replaced wholesale; putting it
//! somewhere only you can write is left to whoever chose the path.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

/// The credentials one run answers to. All empty under `--no-token`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    /// Every page, every GET, and the actions when they are on.
    pub full: String,
    /// Every page and every GET, never an action.
    pub readonly: String,
}

impl Tokens {
    /// Two tokens minted for this run and forgotten when it ends.
    pub fn fresh() -> Tokens {
        Tokens {
            full: super::new_token(),
            readonly: super::new_token(),
        }
    }

    /// No credentials at all, which is what `--no-token` serves with.
    pub fn none() -> Tokens {
        Tokens {
            full: String::new(),
            readonly: String::new(),
        }
    }
}

/// Whether [`load_or_create`] found the file or had to write it — the
/// announcement says which, since only one of them keeps old links working.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loaded {
    Reused,
    Created,
}

/// The most of a token file that will be read. Three lines of hex and a
/// comment fit many times over; anything bigger is not a token file.
const MAX_FILE_BYTES: u64 = 4096;

/// Read the tokens at `path`, or mint them and write it when it is absent.
///
/// `rotate` deletes the file first, which is the whole of rotation: every link
/// built on the old tokens stops working the moment the new ones are served.
pub fn load_or_create(path: &Path, rotate: bool) -> anyhow::Result<(Tokens, Loaded)> {
    if rotate {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => anyhow::bail!("could not remove {} to rotate it: {e}", path.display()),
        }
    }
    if let Some(tokens) = read(path)? {
        return Ok((tokens, Loaded::Reused));
    }
    match create(path)? {
        Some(tokens) => Ok((tokens, Loaded::Created)),
        // Another serve created it between our read and our create. Its tokens
        // are the ones to use: two serves on one file must agree, or the second
        // would announce links the file does not hold.
        None => read(path)?
            .map(|tokens| (tokens, Loaded::Reused))
            .ok_or_else(|| anyhow::anyhow!("{} vanished while it was read", path.display())),
    }
}

/// The tokens in an existing file, `None` when there is no file, or why the
/// file there is refused.
fn read(path: &Path) -> anyhow::Result<Option<Tokens>> {
    // O_NOFOLLOW so the file checked below is the file read, not wherever a
    // link pointed at the moment of opening.
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => anyhow::bail!(
            "{} is a symlink, and a token file must be the file itself — \
             a link can be repointed between the check and the read.\n{}",
            path.display(),
            REMEDY
        ),
        Err(e) => anyhow::bail!("could not open {}: {e}", path.display()),
    };
    // Checked on the open handle, not the path, for the same reason.
    let meta = file.metadata()?;
    // SAFETY: geteuid reads a field of the calling process and cannot fail.
    let me = unsafe { libc::geteuid() };
    if let Some(why) = objection(meta.is_file(), meta.mode(), meta.uid(), me) {
        anyhow::bail!("refusing {}: {why}\n{}", path.display(), REMEDY);
    }
    let mut text = String::new();
    file.take(MAX_FILE_BYTES)
        .read_to_string(&mut text)
        .map_err(|e| anyhow::anyhow!("could not read {}: {e}", path.display()))?;
    parse(&text)
        .map(Some)
        .map_err(|why| anyhow::anyhow!("refusing {}: {why}\n{}", path.display(), REMEDY))
}

/// What to do about any refused file — the same two ways out for every reason.
const REMEDY: &str = "Delete it, or pass --rotate-token, and cctop writes a new one.";

/// Why a file with this metadata must not be served from, if it must not.
///
/// Split out from [`read`] because it is the whole of the policy, and another
/// owner is not a thing a test can arrange with a real file.
fn objection(is_file: bool, mode: u32, owner: u32, me: u32) -> Option<String> {
    if !is_file {
        return Some("it is not a regular file".to_string());
    }
    if owner != me {
        return Some(format!(
            "it belongs to uid {owner}, not you (uid {me}) — whoever owns it \
             can rewrite it, and a token someone else chose is one they know"
        ));
    }
    if mode & 0o077 != 0 {
        return Some(format!(
            "its mode is {:o}, so other users can read or change it and the tokens \
             in it are no longer a secret — it must be 600",
            mode & 0o777
        ));
    }
    None
}

/// Write fresh tokens to a file that did not exist, or `None` when one
/// appeared first.
///
/// `create_new` is O_EXCL: the file is made by this call or not at all, so it
/// cannot be a file someone else left there with a mode of their choosing, and
/// it does not follow a symlink planted at the path. The mode is set at
/// creation, so there is no moment at which the tokens sit in a readable file.
fn create(path: &Path) -> anyhow::Result<Option<Tokens>> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(|e| anyhow::anyhow!("could not create {}: {e}", parent.display()))?;
    }
    let mut file: File = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(None),
        Err(e) => anyhow::bail!("could not create {}: {e}", path.display()),
    };
    let tokens = Tokens::fresh();
    let written = file
        .write_all(render(&tokens).as_bytes())
        .and_then(|()| file.sync_all());
    if let Err(e) = written {
        // A half-written file would be refused as malformed on the next run
        // with no hint that this run is why.
        let _ = std::fs::remove_file(path);
        anyhow::bail!("could not write {}: {e}", path.display());
    }
    Ok(Some(tokens))
}

/// The file's text: one `scope token` pair per line, under a comment saying
/// what it is, so someone who finds it in their home directory knows not to
/// paste it anywhere.
fn render(tokens: &Tokens) -> String {
    format!(
        "# cctop serve --token-file. Whoever reads the `full` line can type at your agents.\n\
         full {}\nreadonly {}\n",
        tokens.full, tokens.readonly
    )
}

/// The two tokens back out of [`render`]'s text, or why not.
///
/// Strict rather than forgiving, because the forgiving readings are the
/// dangerous ones: a missing `readonly` line quietly served as no read-only
/// link, or a `readonly` equal to `full`, which the gate would answer as full
/// access to someone handed the "read-only" link.
fn parse(text: &str) -> Result<Tokens, String> {
    let mut tokens = Tokens::none();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (scope, token) = line
            .split_once(char::is_whitespace)
            .map(|(s, t)| (s, t.trim()))
            .ok_or_else(|| format!("the line `{line}` is not `scope token`"))?;
        let slot = match scope {
            "full" => &mut tokens.full,
            "readonly" => &mut tokens.readonly,
            // A scrape scope existed on the branch that introduced this file,
            // before `/metrics` left the gate; a file written then still loads.
            "metrics" => continue,
            other => return Err(format!("`{other}` is not a token scope")),
        };
        if token.len() != super::TOKEN_BYTES * 2 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!(
                "the {scope} token is not {} hex characters",
                super::TOKEN_BYTES * 2
            ));
        }
        *slot = token.to_string();
    }
    for (scope, token) in [("full", &tokens.full), ("readonly", &tokens.readonly)] {
        if token.is_empty() {
            return Err(format!("it has no {scope} token"));
        }
    }
    if tokens.full == tokens.readonly {
        return Err("its two scopes share a token, which would grant the wider one".into());
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn a_token_file_is_written_private_and_reused_by_the_next_run() {
        let dir = tempfile::tempdir().unwrap();
        // A directory that does not exist yet, as `~/.config/cctop/` might not.
        let path = dir.path().join("cctop").join("serve-tokens");

        let (first, how) = load_or_create(&path, false).unwrap();
        assert_eq!(how, Loaded::Created);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "written as {mode:o}");

        let (second, how) = load_or_create(&path, false).unwrap();
        assert_eq!(how, Loaded::Reused);
        assert_eq!(first, second, "a restart minted new tokens");
    }

    #[test]
    fn rotating_replaces_every_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("serve-tokens");
        let (old, _) = load_or_create(&path, false).unwrap();
        let (new, how) = load_or_create(&path, true).unwrap();
        assert_eq!(how, Loaded::Created);
        assert_ne!(old.full, new.full);
        assert_ne!(old.readonly, new.readonly);
        assert_eq!(load_or_create(&path, false).unwrap().0, new);
    }

    #[test]
    fn a_file_other_users_can_read_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("serve-tokens");
        load_or_create(&path, false).unwrap();
        for mode in [0o640, 0o604, 0o660] {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            let err = load_or_create(&path, false).unwrap_err().to_string();
            assert!(err.contains("no longer a secret"), "{mode:o}: {err}");
            assert!(err.contains("--rotate-token"), "{mode:o}: {err}");
        }
        // Rotation is the way out, and leaves a private file behind.
        load_or_create(&path, true).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_symlinked_token_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        load_or_create(&real, false).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let err = load_or_create(&link, false).unwrap_err().to_string();
        assert!(err.contains("symlink"), "{err}");
    }

    #[test]
    fn a_file_owned_by_someone_else_is_refused() {
        // Not arrangeable with a real file without root, so the policy is
        // asked directly.
        assert!(
            objection(true, 0o100600, 1001, 1000)
                .unwrap()
                .contains("uid 1001")
        );
        assert!(objection(false, 0o040700, 1000, 1000).is_some());
        assert_eq!(objection(true, 0o100600, 1000, 1000), None);
        assert_eq!(objection(true, 0o100400, 1000, 1000), None);
    }

    #[test]
    fn a_malformed_file_is_refused_rather_than_half_used() {
        let t = |c: char| c.to_string().repeat(super::super::TOKEN_BYTES * 2);
        let good = format!("full {}\nreadonly {}\n", t('a'), t('b'));
        let tokens = parse(&good).unwrap();
        assert_eq!((tokens.full, tokens.readonly), (t('a'), t('b')));

        let missing = format!("full {}\n", t('a'));
        assert!(parse(&missing).unwrap_err().contains("readonly"));
        let shared = format!("full {}\nreadonly {}\n", t('a'), t('a'));
        assert!(parse(&shared).unwrap_err().contains("share"));
        let short = format!("full abc\nreadonly {}\n", t('b'));
        assert!(parse(&short).is_err());
        assert!(parse("admin 00\n").is_err());
    }

    #[test]
    fn the_file_round_trips_both_scopes() {
        let tokens = Tokens::fresh();
        let text = render(&tokens);
        assert_eq!(parse(&text).unwrap(), tokens);
        assert!(!text.contains("metrics"), "{text}");
        // A file from before `/metrics` left the gate still loads.
        let legacy = format!("{text}metrics {}\n", super::super::new_token());
        assert_eq!(parse(&legacy).unwrap(), tokens);
    }
}
