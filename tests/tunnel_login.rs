//! `cctop tunnel setup --browser` and `remove`, run as a user runs them over
//! ssh: the login's address printed, opened "elsewhere", the certificate
//! fetched here, the tunnel created and stored — all against the fake
//! authorize page, store and API, which only a build made for tests can be
//! pointed at. Every token here is made up.

use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cctop_core::cloudflare::fake;
use cctop_core::cloudflare::login::fake as login;

fn sandbox() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cctop-tunnel-login-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["run", "home", "cache", "config", "data"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    dir
}

/// The command, pointed at `dir` and the fakes and nowhere else, and seen
/// as over ssh so that no browser on this machine is asked to open anything.
fn command(dir: &Path, api: &str, store: &str) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_cctop"));
    c.env("XDG_RUNTIME_DIR", dir.join("run"))
        .env("HOME", dir.join("home"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("CCTOP_CLOUDFLARE_API", api)
        .env("CCTOP_CLOUDFLARE_LOGIN", store)
        .env("SSH_CONNECTION", "192.0.2.1 50000 192.0.2.2 22")
        .env_remove("CCTOP_TUNNEL_TOKEN")
        .env_remove("CCTOP_TUNNEL_HOSTNAME")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CCTOP_LOG")
        .stdin(Stdio::null());
    c
}

fn config_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(at) = todo.pop() {
        for entry in std::fs::read_dir(&at).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                todo.push(path);
            } else if path.file_name().is_some_and(|n| n == "config.toml") {
                found.push(path);
            }
        }
    }
    found
}

#[test]
fn a_browser_login_over_ssh_prints_the_address_and_connects() {
    let dir = sandbox();
    let (api, seen) = fake::api(fake::accepting(None));
    let store = login::server(login::Store::Working);

    let mut child = command(&dir, &api, &store)
        .args(["tunnel", "setup", "--browser"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut said = String::new();
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    // The address is on a line of its own; open it as a browser elsewhere
    // would.
    loop {
        let mut line = String::new();
        assert!(
            stderr.read_line(&mut line).unwrap() > 0,
            "no address printed: {said}"
        );
        said.push_str(&line);
        if line.starts_with(&store) {
            assert!(
                line.trim()
                    .starts_with(&format!("{store}/argotunnel?aud=&callback=")),
                "{line}"
            );
            login::open(line.trim());
            break;
        }
    }
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "setup did not finish: {said}"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut rest = String::new();
    std::io::Read::read_to_string(&mut stderr, &mut rest).unwrap();
    said.push_str(&rest);
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take().unwrap(), &mut out).unwrap();
    assert!(status.success(), "{said}");
    assert!(said.contains("Open this address in a browser"), "{said}");
    assert!(
        said.contains("Connected: https://cctop.example.test"),
        "{said}"
    );
    assert!(!said.contains(login::TOKEN) && !out.contains(login::TOKEN));
    // Routed as cloudflared routes, never written as a DNS record.
    let calls: Vec<String> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|(m, p, _)| format!("{m} {p}"))
        .collect();
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("PUT /zones/zone1/tunnels/")),
        "{calls:?}"
    );
    assert!(
        !calls.iter().any(|c| c.starts_with("POST /zones/")),
        "{calls:?}"
    );

    let files = config_files(&dir);
    assert_eq!(files.len(), 1, "{files:?}");
    let text = std::fs::read_to_string(&files[0]).unwrap();
    assert!(text.contains("login = \"browser\""), "{text}");
    assert!(text.contains(login::TOKEN), "the token is what is kept");
    let mode = std::fs::metadata(&files[0]).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    // And `remove` forgets it, saying the token itself is the dashboard's to
    // revoke.
    let removed = command(&dir, &api, &store)
        .args(["tunnel", "remove"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&removed.stdout);
    assert!(removed.status.success(), "{out}");
    assert!(out.contains("profile/api-tokens"), "{out}");
    let text = std::fs::read_to_string(&files[0]).unwrap();
    assert!(!text.contains(login::TOKEN), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}
