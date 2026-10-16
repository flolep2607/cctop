//! The tokens cctop keeps, sealed in `secrets.enc` rather than written into
//! `config.toml`.
//!
//! Three kinds of secret used to sit in the config in plain text: a Claude
//! token account's `[accounts.<name>] token`, the Cloudflare tunnel's
//! `[tunnel] token`, and the Cloudflare API token beside it, `api_token`. They
//! live here now, under the keys `accounts.<name>.token`, `tunnel.token` and
//! `tunnel.api_token`, and everything that read one from the config reads it
//! through [`get`] or [`Store::all`].
//!
//! # What this protects against, and what it does not
//!
//! cctop has to read these unattended — the dashboard polls a token account's
//! limits, a service brings the tunnel up at boot — so the key is a file the
//! user's own processes can read, with no passphrase in front of it. Anything
//! running as the user can therefore read the key and decrypt the file, as it
//! could read the plain-text config before. On Linux nothing short of a
//! prompt does better: a keyring daemon hands its secrets to any process of
//! the same user that asks, and Claude Code keeps its own credentials in
//! `~/.claude/.credentials.json` in plain text.
//!
//! What it does stop is the secret leaving by accident:
//!
//! - **Being seen.** `cat config.toml`, a `grep -r token ~/.config`, a shared
//!   screen, or an agent reading the config to answer a question about it.
//! - **Being carried off with the config.** A dotfiles repository, a sync
//!   service, a backup, or a config pasted into a bug report. The config holds
//!   nothing that works without the key, and the key is somewhere else.
//! - **Secret scanners**, which match a token's shape in plain text.
//! - **A copy that is used elsewhere.** The key that seals the file is derived
//!   from the key file *and* `/etc/machine-id`, so `secrets.enc` and
//!   `secret.key` copied together to another machine still do not open.
//!
//! The key file is under the data directory (`~/.local/share/cctop`) and the
//! sealed file beside the config (`~/.config/cctop`) on purpose: what sweeps up
//! one of those directories — a dotfiles repository is `~/.config` — rarely
//! sweeps up the other.
//!
//! `serve-tokens`, beside the config, is not moved here. It is `cctop serve`'s
//! `--token-file`, a format the user chose a path for and other things may
//! read; it is refused unless owner-only, and `--rotate-token` replaces it.
//!
//! # The file
//!
//! `CCTOPSEC`, a version byte, a 24-byte random nonce, then the
//! XChaCha20-Poly1305 ciphertext of a JSON object of strings. The header is the
//! associated data, so a file whose version was edited does not open as if it
//! were the old one. A random nonce per write is safe at XChaCha's 192 bits.
//!
//! # Migration
//!
//! Every read moves a plain-text secret it finds in the config into the sealed
//! file, then rewrites the config without it — through `toml_edit`, so the
//! user's comments and layout are kept — leaving a comment where it was. In
//! that order, so a crash between the two leaves the secret in both places and
//! the next read does the same thing again. A plain-text value wins over the
//! sealed one: it is there because someone pasted it in since.
//!
//! # When it does not open
//!
//! A key that was deleted, a file from another machine, a machine id that
//! changed: [`Store::load`] says which, and the secrets read as absent, so a
//! token account asks for its token again rather than cctop failing to start.
//! Nothing is written over a file that would not open. The next explicit
//! [`set`] moves it aside to `secrets.enc.unreadable-<time>` — kept, in case
//! the key turns up — and starts a new one.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// Every secret, by key.
pub type Secrets = BTreeMap<String, String>;

/// The tunnel connector's credential: `[tunnel] token` in the old config.
pub const TUNNEL_TOKEN: &str = "tunnel.token";

/// The Cloudflare API token cctop set the tunnel up with: `[tunnel] api_token`.
pub const TUNNEL_API_TOKEN: &str = "tunnel.api_token";

/// The key of a token account's token: `[accounts.<name>] token`.
pub fn account_token_key(name: &str) -> String {
    format!("accounts.{name}.token")
}

/// The account a key from [`account_token_key`] is for.
pub fn account_of_key(key: &str) -> Option<&str> {
    key.strip_prefix("accounts.")?
        .strip_suffix(".token")
        .filter(|name| !name.is_empty())
}

const MAGIC: &[u8; 8] = b"CCTOPSEC";
const VERSION: u8 = 1;
const HEADER_LEN: usize = MAGIC.len() + 1;
const NONCE_LEN: usize = 24;
/// The HKDF salt, which names the purpose, so the same key file could never
/// be made to yield this key for anything else.
const SALT: &[u8] = b"cctop secrets.enc v1";

/// Why the sealed file did not open. Each says what to do about it, since it
/// is shown to someone whose accounts have just asked for their tokens again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreadable {
    /// `secrets.enc` is there and the key file is not.
    NoKey,
    /// The key file is there and is not a key.
    BadKey(String),
    /// The file is not one cctop wrote, or one a newer cctop wrote.
    NotOurs,
    /// The key does not open it: it was sealed on another machine, or with a
    /// key file since replaced, or the machine id changed.
    WrongKey,
    /// It could not be read at all.
    Io(String),
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unreadable::NoKey => f.write_str("its key file is missing"),
            Unreadable::BadKey(why) => write!(f, "its key file is damaged ({why})"),
            Unreadable::NotOurs => f.write_str("it is not a file this version of cctop can read"),
            Unreadable::WrongKey => f.write_str(
                "this machine's key does not open it (copied from another machine, \
                 a replaced key file, or a changed /etc/machine-id)",
            ),
            Unreadable::Io(why) => write!(f, "it could not be read ({why})"),
        }
    }
}

/// Where the secrets are kept, and the config they are migrated out of.
///
/// A value rather than fixed paths so a test can point all four at a
/// directory of its own; [`Store::current`] is the real one.
#[derive(Debug, Clone)]
pub struct Store {
    /// The sealed file, `~/.config/cctop/secrets.enc`.
    pub enc: PathBuf,
    /// The key file, `~/.local/share/cctop/secret.key`.
    pub key: PathBuf,
    /// Where the machine id is read from, first that exists wins.
    pub machine_ids: Vec<PathBuf>,
    /// The config plain-text secrets are moved out of.
    pub config: PathBuf,
}

/// What `cctop doctor` shows: the store looked at without changing anything.
///
/// No `Debug`: it holds the secrets, and a derived one would print them.
pub struct Status {
    /// The key file's permission bits, when it is there.
    pub key_mode: Option<u32>,
    /// The sealed file's, when it is there.
    pub enc_mode: Option<u32>,
    /// Whether a machine id was found to bind the key to.
    pub machine_id: bool,
    /// What the sealed file holds, or why it does not open.
    pub sealed: Result<Secrets, Unreadable>,
    /// Secrets still in plain text in the config.
    pub plaintext: Secrets,
}

impl Store {
    /// The store cctop uses: the real one, or under test a directory of the
    /// test's own, so no test reads or migrates the developer's own tokens.
    pub fn current() -> Store {
        #[cfg(any(test, feature = "test-support"))]
        if crate::under_test() {
            return Store::in_dir(&test_dir());
        }
        Store {
            enc: crate::config::config_base()
                .join("cctop")
                .join("secrets.enc"),
            key: crate::config::data_base().join("cctop").join("secret.key"),
            machine_ids: vec![
                PathBuf::from("/etc/machine-id"),
                // Where dbus kept it before systemd, and still does on some
                // distributions and containers without the first.
                PathBuf::from("/var/lib/dbus/machine-id"),
            ],
            config: crate::config::CONFIG_FILE.clone(),
        }
    }

    /// A store laid out under `dir` the way the real one is under `$HOME`,
    /// with a machine id of its own.
    #[cfg(any(test, feature = "test-support"))]
    pub fn in_dir(dir: &Path) -> Store {
        Store {
            enc: dir.join("config").join("secrets.enc"),
            key: dir.join("data").join("secret.key"),
            machine_ids: vec![dir.join("machine-id")],
            config: dir.join("config").join("config.toml"),
        }
    }

    /// Every secret, with any found in plain text in the config moved in
    /// first. Errs only when the sealed file is there and does not open.
    pub fn load(&self) -> Result<Secrets, Unreadable> {
        let mut secrets = self.decrypt()?;
        let plain = self.plaintext();
        if plain.is_empty() {
            return Ok(secrets);
        }
        // Under the lock, and read again under it, so a migration racing a
        // `set` in another process does not write back the map from before.
        // If the lock is not to be had, the plain text still answers this
        // read and the next one moves it.
        if let Ok(_lock) = self.lock() {
            secrets = self.decrypt()?;
            let plain = self.plaintext();
            secrets.extend(plain.clone());
            if !plain.is_empty() && self.write(&secrets).is_ok() {
                let _ = self.strip_config(&plain);
            }
            return Ok(secrets);
        }
        secrets.extend(plain);
        Ok(secrets)
    }

    /// Every secret that can be read: [`Store::load`], or when the sealed file
    /// does not open, whatever is still in plain text in the config — which a
    /// failed open leaves where it is.
    pub fn all(&self) -> Secrets {
        self.load().unwrap_or_else(|_| self.plaintext())
    }

    /// One secret.
    pub fn get(&self, key: &str) -> Option<String> {
        self.all().remove(key).filter(|v| !v.trim().is_empty())
    }

    /// Keep `value` as `key`.
    pub fn set(&self, key: &str, value: &str) -> anyhow::Result<()> {
        self.update(|secrets| {
            secrets.insert(key.to_string(), value.to_string());
        })
    }

    /// Forget `key`, and any copy of it still in plain text in the config.
    pub fn remove(&self, key: &str) -> anyhow::Result<()> {
        self.update(|secrets| {
            secrets.remove(key);
        })
    }

    /// Change the secrets through `edit`, under the lock.
    ///
    /// Plain text in the config is moved in first, so that a value set here
    /// is not overruled at the next read by an older one still there. A
    /// sealed file that does not open is moved aside rather than written
    /// over: this is the user setting a secret again, and whatever the old
    /// file held is kept in case its key comes back.
    pub fn update(&self, edit: impl FnOnce(&mut Secrets)) -> anyhow::Result<()> {
        let _lock = self.lock()?;
        let mut secrets = match self.decrypt() {
            Ok(secrets) => secrets,
            Err(_) => {
                self.set_aside()?;
                Secrets::new()
            }
        };
        let plain = self.plaintext();
        secrets.extend(plain.clone());
        edit(&mut secrets);
        self.write(&secrets)?;
        if !plain.is_empty() {
            self.strip_config(&plain)?;
        }
        Ok(())
    }

    /// The store as it is, for `cctop doctor`: nothing is migrated, created
    /// or moved by looking.
    pub fn status(&self) -> Status {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| {
            std::fs::metadata(p)
                .ok()
                .map(|m| m.permissions().mode() & 0o777)
        };
        Status {
            key_mode: mode(&self.key),
            enc_mode: mode(&self.enc),
            machine_id: !self.machine_id().is_empty(),
            sealed: self.decrypt(),
            plaintext: self.plaintext(),
        }
    }

    /// The machine id, or nothing — which still works, bound to the key file
    /// alone, and `cctop doctor` says so.
    fn machine_id(&self) -> String {
        self.machine_ids
            .iter()
            .find_map(|p| std::fs::read_to_string(p).ok())
            .map(|id| id.trim().to_string())
            .unwrap_or_default()
    }

    /// The sealing key, from the key file and the machine id.
    fn cipher(&self, key: &[u8; 32]) -> XChaCha20Poly1305 {
        let mut okm = [0u8; 32];
        hkdf::Hkdf::<sha2::Sha256>::new(Some(SALT), key)
            .expand(self.machine_id().as_bytes(), &mut okm)
            .expect("32 bytes is a length HKDF-SHA256 can expand to");
        XChaCha20Poly1305::new(chacha20poly1305::Key::from_slice(&okm))
    }

    /// The key file's 32 bytes, `None` when there is no key file.
    fn read_key(&self) -> Result<Option<[u8; 32]>, Unreadable> {
        let text = match std::fs::read_to_string(&self.key) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Unreadable::BadKey(e.kind().to_string())),
        };
        unhex(text.trim())
            .map(Some)
            .ok_or_else(|| Unreadable::BadKey("not 64 hex digits".into()))
    }

    /// The key file's key, made now if there is none.
    ///
    /// Written whole to a temporary and hard-linked into place, which fails if
    /// the name is taken: two cctops making a key at once agree on one, the
    /// winner's, rather than each sealing with its own and one of them losing
    /// every secret the other wrote.
    fn key_or_create(&self) -> anyhow::Result<[u8; 32]> {
        if let Some(key) = self.read_key().map_err(|e| anyhow::anyhow!("{e}"))? {
            return Ok(key);
        }
        use std::io::Write;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        if let Some(dir) = self.key.parent() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
        let mut key = [0u8; 32];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut key)
            .map_err(|_| anyhow::anyhow!("the system's random source failed"))?;
        let tmp = self
            .key
            .with_extension(format!("{}.tmp", std::process::id()));
        let made = (|| {
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?
                .write_all(format!("{}\n", hex(&key)).as_bytes())?;
            std::fs::hard_link(&tmp, &self.key)
        })();
        let _ = std::fs::remove_file(&tmp);
        match made {
            Ok(()) => Ok(key),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => self
                .read_key()
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .ok_or_else(|| anyhow::anyhow!("the key file vanished as it was made")),
            Err(e) => Err(e.into()),
        }
    }

    /// The sealed file opened. No file is no secrets, which is not an error.
    fn decrypt(&self) -> Result<Secrets, Unreadable> {
        let bytes = match std::fs::read(&self.enc) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Secrets::new()),
            Err(e) => return Err(Unreadable::Io(e.kind().to_string())),
        };
        if bytes.len() < HEADER_LEN + NONCE_LEN
            || &bytes[..MAGIC.len()] != MAGIC
            || bytes[MAGIC.len()] != VERSION
        {
            return Err(Unreadable::NotOurs);
        }
        let key = self.read_key()?.ok_or(Unreadable::NoKey)?;
        let (header, rest) = bytes.split_at(HEADER_LEN);
        let (nonce, sealed) = rest.split_at(NONCE_LEN);
        let plain = self
            .cipher(&key)
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: sealed,
                    aad: header,
                },
            )
            .map_err(|_| Unreadable::WrongKey)?;
        // Opened under its key and still not a map: the file is ours and
        // damaged, which is the same thing to do something about.
        serde_json::from_slice(&plain).map_err(|_| Unreadable::NotOurs)
    }

    /// Seal `secrets` into the file, owner-only from the moment it exists.
    fn write(&self, secrets: &Secrets) -> anyhow::Result<()> {
        let key = self.key_or_create()?;
        let mut nonce = [0u8; NONCE_LEN];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut nonce)
            .map_err(|_| anyhow::anyhow!("the system's random source failed"))?;
        let mut header = MAGIC.to_vec();
        header.push(VERSION);
        let sealed = self
            .cipher(&key)
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &serde_json::to_vec(secrets)?,
                    aad: &header,
                },
            )
            .map_err(|_| anyhow::anyhow!("could not seal the secrets"))?;
        let mut bytes = header;
        bytes.extend_from_slice(&nonce);
        bytes.extend_from_slice(&sealed);
        crate::util::write_atomic(&self.enc, &bytes, true)?;
        Ok(())
    }

    /// Move a sealed file that does not open out of the way, keeping it.
    fn set_aside(&self) -> anyhow::Result<()> {
        if !self.enc.exists() {
            return Ok(());
        }
        let mut name = self.enc.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".unreadable-{}", crate::util::now_secs()));
        std::fs::rename(&self.enc, self.enc.with_file_name(name))?;
        Ok(())
    }

    /// One writer at a time across processes, held for a read-modify-write.
    fn lock(&self) -> std::io::Result<std::fs::File> {
        let dir = self.enc.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        crate::util::lock_within(
            &dir.join(".secrets.lock"),
            std::time::Duration::from_secs(2),
        )
    }

    /// The secrets still in plain text in the config.
    fn plaintext(&self) -> Secrets {
        std::fs::read_to_string(&self.config)
            .ok()
            .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok())
            .map(|doc| plaintext_in(&doc))
            .unwrap_or_default()
    }

    /// Rewrite the config without the secrets in `moved`, a comment in place
    /// of each. A config that no longer parses is left alone, as every config
    /// writer in cctop leaves one.
    fn strip_config(&self, moved: &Secrets) -> anyhow::Result<()> {
        let text = match std::fs::read_to_string(&self.config) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let mut doc = crate::quota::parse_toml(&self.config, &text)?;
        for key in moved.keys() {
            if let Some((path, leaf)) = config_path(key) {
                strip_key(&mut doc, &path, leaf);
            }
        }
        crate::util::write_atomic(&self.config, doc.to_string().as_bytes(), true)?;
        Ok(())
    }
}

/// [`Store::get`] on the store cctop uses.
pub fn get(key: &str) -> Option<String> {
    Store::current().get(key)
}

/// [`Store::set`] on the store cctop uses.
pub fn set(key: &str, value: &str) -> anyhow::Result<()> {
    Store::current().set(key, value)
}

/// [`Store::remove`] on the store cctop uses.
pub fn remove(key: &str) -> anyhow::Result<()> {
    Store::current().remove(key)
}

/// Every secret that can be read, from the store cctop uses.
pub fn all() -> Secrets {
    Store::current().all()
}

/// The sentence to show when the sealed file does not open, or `None` when
/// it does. Said once by whoever calls it — the dashboard at start, the
/// command line beside the error it explains — rather than on every read,
/// which happens many times a minute.
pub fn problem() -> Option<String> {
    let store = Store::current();
    let why = store.decrypt().err()?;
    Some(format!(
        "Saved tokens could not be read: {why}. Add them again; {} is kept, \
         and `cctop doctor` says more.",
        crate::util::tildify(&store.enc.to_string_lossy())
    ))
}

/// The token accounts with a token, by name.
pub fn account_tokens(secrets: &Secrets) -> impl Iterator<Item = (&str, &str)> {
    secrets.iter().filter_map(|(key, value)| {
        Some((account_of_key(key)?, value.as_str())).filter(|(_, v)| !v.trim().is_empty())
    })
}

/// `secret` as it may be shown: enough to tell two apart, never enough to use.
///
/// A known prefix is kept whole, since it says what kind of token this is and
/// is the same for everyone; then the last four characters, which is how a
/// token is told apart in Anthropic's and Cloudflare's own dashboards. A short
/// value shows nothing of itself: four characters of eight is half of it.
pub fn redact(secret: &str) -> String {
    let secret = secret.trim();
    const PREFIXES: [&str; 4] = ["sk-ant-oat01-", "sk-ant-api03-", "sk-ant-", "eyJhIjoi"];
    let prefix = PREFIXES
        .iter()
        .find(|p| secret.starts_with(**p))
        .copied()
        .unwrap_or("");
    let rest = &secret[prefix.len()..];
    let count = rest.chars().count();
    if count < 16 {
        return format!("{prefix}…");
    }
    let tail: String = rest.chars().skip(count - 4).collect();
    format!("{prefix}…{tail}")
}

/// The plain-text secrets in a config: the three places they were written.
fn plaintext_in(doc: &toml_edit::DocumentMut) -> Secrets {
    let mut found = Secrets::new();
    let text = |item: Option<&toml_edit::Item>| {
        item.and_then(|i| i.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
    };
    if let Some(accounts) = doc.get("accounts").and_then(|a| a.as_table_like()) {
        for (name, account) in accounts.iter() {
            if let Some(token) = text(account.as_table_like().and_then(|t| t.get("token"))) {
                found.insert(account_token_key(name), token);
            }
        }
    }
    if let Some(tunnel) = doc.get("tunnel").and_then(|t| t.as_table_like()) {
        for (key, leaf) in [(TUNNEL_TOKEN, "token"), (TUNNEL_API_TOKEN, "api_token")] {
            if let Some(value) = text(tunnel.get(leaf)) {
                found.insert(key.to_string(), value);
            }
        }
    }
    found
}

/// Where a secret's key was in the config: the table's path, and the key.
fn config_path(key: &str) -> Option<(Vec<&str>, &str)> {
    if let Some(name) = account_of_key(key) {
        return Some((vec!["accounts", name], "token"));
    }
    let (table, leaf) = key.split_once('.')?;
    (table == "tunnel").then(|| (vec![table], leaf))
}

/// The comment left where a secret was taken out of the config.
fn moved_note(leaf: &str) -> String {
    format!("# {leaf}: moved to secrets.enc, encrypted (`cctop doctor` says where)\n")
}

/// Take `leaf` out of the table at `path`, leaving [`moved_note`] in its place.
///
/// A comment in TOML belongs to whatever follows it, so the note — and any
/// comment the user had written above the secret — goes on the next key in
/// the table; failing that on the next table's header, which is the line after
/// this table's last; failing that at the end of the file. An inline table
/// has nowhere to hold a comment, and loses only the key.
fn strip_key(doc: &mut toml_edit::DocumentMut, path: &[&str], leaf: &str) {
    let note = moved_note(leaf);
    let mut item = doc.as_item_mut();
    for part in path {
        let Some(next) = item.as_table_like_mut().and_then(|t| t.get_mut(part)) else {
            return;
        };
        item = next;
    }
    if !item.is_table() {
        if let Some(table) = item.as_table_like_mut() {
            table.remove(leaf);
        }
        return;
    }
    let Some(table) = item.as_table_mut() else {
        return;
    };
    let Some(at) = table.iter().position(|(k, _)| k == leaf) else {
        return;
    };
    let above = table
        .key(leaf)
        .and_then(|k| k.leaf_decor().prefix())
        .and_then(|p| p.as_str())
        .unwrap_or("")
        .to_string();
    let carried = format!("{above}{note}");
    let position = table.position();
    table.remove(leaf);
    // The next plain value, not the next entry: a subtable is drawn under its
    // own header, wherever that is.
    let next = table
        .iter()
        .skip(at)
        .find(|(_, item)| item.is_value())
        .map(|(k, _)| k.to_string());
    if let Some(next) = next {
        if let Some(mut key) = table.key_mut(&next) {
            let decor = key.leaf_decor_mut();
            let was = decor
                .prefix()
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
            decor.set_prefix(format!("{carried}{was}"));
        }
        return;
    }
    let mut headers = Vec::new();
    collect_headers(doc.as_table(), &mut Vec::new(), &mut headers);
    let following = headers
        .into_iter()
        .filter(|(p, _)| position.is_some_and(|here| *p > here))
        .min_by_key(|(p, _)| *p);
    if let Some((_, header)) = following {
        let mut item = doc.as_item_mut();
        for part in &header {
            match item.as_table_mut().and_then(|t| t.get_mut(part)) {
                Some(next) => item = next,
                None => return,
            }
        }
        if let Some(table) = item.as_table_mut() {
            let decor = table.decor_mut();
            let was = decor
                .prefix()
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
            decor.set_prefix(format!("{carried}{was}"));
        }
        return;
    }
    let trailing = doc.trailing().as_str().unwrap_or("").to_string();
    doc.set_trailing(format!("{carried}{trailing}"));
}

/// Every table drawn under a header of its own, by where it is in the file.
fn collect_headers(
    table: &toml_edit::Table,
    path: &mut Vec<String>,
    out: &mut Vec<(isize, Vec<String>)>,
) {
    for (key, item) in table.iter() {
        let Some(sub) = item.as_table() else {
            continue;
        };
        path.push(key.to_string());
        if !sub.is_implicit()
            && let Some(position) = sub.position()
        {
            out.push((position, path.clone()));
        }
        collect_headers(sub, path, out);
        path.pop();
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static TEST_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// The store a test that claimed none uses: one per process, under the
/// temporary directory, so forgetting to claim one never reaches the real
/// files.
#[cfg(any(test, feature = "test-support"))]
fn test_dir() -> PathBuf {
    TEST_DIR
        .with(|dir| dir.borrow().clone())
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("cctop-secrets-{}", std::process::id()))
        })
}

/// Point this thread's [`Store::current`] at `dir` while the guard lives.
#[cfg(any(test, feature = "test-support"))]
pub fn claim_test_dir(dir: &Path) -> TestDir {
    let previous = TEST_DIR.with(|d| d.replace(Some(dir.to_path_buf())));
    TestDir { previous }
}

/// Gives the thread back the store it had when dropped.
#[cfg(any(test, feature = "test-support"))]
pub struct TestDir {
    previous: Option<PathBuf>,
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for TestDir {
    fn drop(&mut self) {
        let previous = self.previous.take();
        TEST_DIR.with(|d| *d.borrow_mut() = previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn store(dir: &Path) -> Store {
        let store = Store::in_dir(dir);
        std::fs::write(&store.machine_ids[0], "0123456789abcdef0123456789abcdef\n").unwrap();
        store
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_secret_round_trips_and_neither_file_holds_it_in_plain_text() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        assert_eq!(store.get("tunnel.token"), None);
        store
            .set(TUNNEL_TOKEN, "eyJhIjoi-made-up-tunnel-token")
            .unwrap();
        store
            .set(&account_token_key("work"), "sk-ant-oat01-made-up")
            .unwrap();
        assert_eq!(
            store.get(TUNNEL_TOKEN).as_deref(),
            Some("eyJhIjoi-made-up-tunnel-token")
        );
        assert_eq!(
            account_tokens(&store.all()).collect::<Vec<_>>(),
            [("work", "sk-ant-oat01-made-up")]
        );
        let sealed = std::fs::read(&store.enc).unwrap();
        assert!(sealed.starts_with(b"CCTOPSEC\x01"));
        assert!(!String::from_utf8_lossy(&sealed).contains("made-up"));
        let key = std::fs::read_to_string(&store.key).unwrap();
        assert!(!key.contains("made-up"));
        assert_eq!(mode(&store.enc), 0o600);
        assert_eq!(mode(&store.key), 0o600);
        assert_eq!(mode(store.key.parent().unwrap()), 0o700);

        store.remove(TUNNEL_TOKEN).unwrap();
        assert_eq!(store.get(TUNNEL_TOKEN), None);
        assert!(store.get(&account_token_key("work")).is_some());
    }

    #[test]
    fn another_machine_or_another_key_fails_cleanly_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        store.set(TUNNEL_TOKEN, "eyJhIjoi-made-up").unwrap();
        let sealed = std::fs::read(&store.enc).unwrap();

        // The same two files on a machine with another id.
        std::fs::write(&store.machine_ids[0], "fedcba9876543210fedcba9876543210\n").unwrap();
        assert_eq!(store.load(), Err(Unreadable::WrongKey));
        assert_eq!(store.get(TUNNEL_TOKEN), None);
        assert!(problem_of(&store).contains("another machine"));
        assert_eq!(
            std::fs::read(&store.enc).unwrap(),
            sealed,
            "not written over"
        );

        // The right machine, with the key file replaced.
        std::fs::write(&store.machine_ids[0], "0123456789abcdef0123456789abcdef\n").unwrap();
        assert!(store.get(TUNNEL_TOKEN).is_some());
        std::fs::write(&store.key, format!("{}\n", "ab".repeat(32))).unwrap();
        assert_eq!(store.load(), Err(Unreadable::WrongKey));

        // Damaged, and gone.
        std::fs::write(&store.key, "not a key").unwrap();
        assert!(matches!(store.load(), Err(Unreadable::BadKey(_))));
        std::fs::remove_file(&store.key).unwrap();
        assert_eq!(store.load(), Err(Unreadable::NoKey));
        assert_eq!(
            std::fs::read(&store.enc).unwrap(),
            sealed,
            "not written over"
        );

        // A file that is not ours.
        std::fs::write(&store.enc, b"hello").unwrap();
        assert_eq!(store.load(), Err(Unreadable::NotOurs));

        // Setting one again moves the unreadable file aside and starts anew.
        std::fs::write(&store.enc, &sealed).unwrap();
        store.set(TUNNEL_TOKEN, "eyJhIjoi-again").unwrap();
        assert_eq!(store.get(TUNNEL_TOKEN).as_deref(), Some("eyJhIjoi-again"));
        let kept: Vec<_> = std::fs::read_dir(store.enc.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".unreadable-"))
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(std::fs::read(kept[0].path()).unwrap(), sealed);
    }

    fn problem_of(store: &Store) -> String {
        store.decrypt().unwrap_err().to_string()
    }

    const CONFIG: &str = "\
# cctop's config
[accounts.work]
# the work account
token = \"sk-ant-oat01-made-up-work-token\"

[accounts.side]
token = \"sk-ant-oat01-made-up-side-token\" # pasted by hand
label = \"side\"

[settings]
# keep this
read_screen = true

[tunnel]
token = \"eyJhIjoi-made-up-tunnel\"
hostname = \"cctop.example.test\"
api_token = \"made-up-api-token-0000\"
login = \"browser\"

[tunnel.access]
team = \"t.cloudflareaccess.com\"
";

    #[test]
    fn plain_text_moves_into_the_sealed_file_and_the_comments_stay() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        std::fs::create_dir_all(store.config.parent().unwrap()).unwrap();
        std::fs::write(&store.config, CONFIG).unwrap();

        let secrets = store.load().unwrap();
        assert_eq!(secrets.len(), 4, "{secrets:?}");
        assert_eq!(
            secrets.get("accounts.work.token").map(String::as_str),
            Some("sk-ant-oat01-made-up-work-token")
        );
        assert_eq!(
            secrets.get(TUNNEL_API_TOKEN).map(String::as_str),
            Some("made-up-api-token-0000")
        );

        let text = std::fs::read_to_string(&store.config).unwrap();
        assert!(!text.contains("made-up"), "{text}");
        for kept in [
            "# cctop's config\n[accounts.work]\n# the work account\n# token: moved to secrets.enc",
            "[accounts.side]\n# token: moved to secrets.enc, encrypted (`cctop doctor` says where)\nlabel = \"side\"",
            "[settings]\n# keep this\nread_screen = true",
            "[tunnel]\n# token: moved to secrets.enc",
            "hostname = \"cctop.example.test\"\n# api_token: moved to secrets.enc",
            "login = \"browser\"\n",
            "[tunnel.access]\nteam",
        ] {
            assert!(text.contains(kept), "missing {kept:?} in\n{text}");
        }
        // The account the last line of whose table was its token: the note is
        // before the next header, not lost.
        assert!(
            text.contains("# token: moved to secrets.enc, encrypted (`cctop doctor` says where)\n\n[accounts.side]"),
            "{text}"
        );
        assert_eq!(mode(&store.config), 0o600);
        assert!(text.parse::<toml_edit::DocumentMut>().is_ok());
    }

    #[test]
    fn migration_is_idempotent_and_a_pasted_value_wins_again() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        std::fs::create_dir_all(store.config.parent().unwrap()).unwrap();
        std::fs::write(&store.config, CONFIG).unwrap();
        let first = store.load().unwrap();
        let text = std::fs::read_to_string(&store.config).unwrap();

        // Read again: nothing to move, nothing rewritten.
        assert_eq!(store.load().unwrap(), first);
        assert_eq!(std::fs::read_to_string(&store.config).unwrap(), text);

        // A crash between the two writes: the sealed file has them, the
        // config still too. Reading again ends where one read would have.
        std::fs::write(&store.config, CONFIG).unwrap();
        assert_eq!(store.load().unwrap(), first);
        assert_eq!(std::fs::read_to_string(&store.config).unwrap(), text);

        // Pasted in by hand since: it wins, and moves too.
        let pasted = text.replace(
            "[tunnel]\n",
            "[tunnel]\ntoken = \"eyJhIjoi-pasted-by-hand\"\n",
        );
        std::fs::write(&store.config, &pasted).unwrap();
        assert_eq!(
            store.get(TUNNEL_TOKEN).as_deref(),
            Some("eyJhIjoi-pasted-by-hand")
        );
        assert!(
            !std::fs::read_to_string(&store.config)
                .unwrap()
                .contains("pasted-by-hand")
        );
    }

    #[test]
    fn a_failed_open_leaves_the_plain_text_where_it_is_and_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        store.set(TUNNEL_TOKEN, "eyJhIjoi-sealed").unwrap();
        std::fs::remove_file(&store.key).unwrap();
        std::fs::write(
            &store.config,
            "[accounts.work]\ntoken = \"sk-ant-oat01-plain\"\n",
        )
        .unwrap();
        assert_eq!(
            store.get("accounts.work.token").as_deref(),
            Some("sk-ant-oat01-plain")
        );
        assert_eq!(store.get(TUNNEL_TOKEN), None);
        assert!(
            std::fs::read_to_string(&store.config)
                .unwrap()
                .contains("sk-ant-oat01-plain")
        );
        let status = store.status();
        assert_eq!(status.sealed, Err(Unreadable::NoKey));
        assert_eq!(status.plaintext.len(), 1);
    }

    #[test]
    fn a_set_overrules_an_older_plain_text_value() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        std::fs::create_dir_all(store.config.parent().unwrap()).unwrap();
        std::fs::write(
            &store.config,
            "[accounts.work]\ntoken = \"sk-ant-oat01-old\"\n",
        )
        .unwrap();
        store
            .set(&account_token_key("work"), "sk-ant-oat01-new")
            .unwrap();
        assert_eq!(
            store.get(&account_token_key("work")).as_deref(),
            Some("sk-ant-oat01-new")
        );
        assert!(
            !std::fs::read_to_string(&store.config)
                .unwrap()
                .contains("-old")
        );
    }

    #[test]
    fn a_secret_is_shown_by_its_kind_and_last_four() {
        assert_eq!(
            redact("sk-ant-oat01-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789a1b2"),
            "sk-ant-oat01-…a1b2"
        );
        assert_eq!(
            redact("eyJhIjoiMDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWYiLCJ0Ijoi"),
            "eyJhIjoi…Ijoi"
        );
        assert_eq!(redact("0123456789abcdefghijklmnopqrstuvwxyzQRST"), "…QRST");
        // Too short to give any of it away.
        assert_eq!(redact("sk-ant-oat01-short"), "sk-ant-oat01-…");
        assert_eq!(redact("hunter2"), "…");
        assert_eq!(redact(""), "…");
    }

    #[test]
    fn keys_name_their_account() {
        assert_eq!(
            account_of_key(&account_token_key("my.work")),
            Some("my.work")
        );
        assert_eq!(account_of_key(TUNNEL_TOKEN), None);
        assert_eq!(account_of_key("accounts..token"), None);
    }
}
