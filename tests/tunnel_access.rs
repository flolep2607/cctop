//! `cctop tunnel setup --access`, `invite`, `access off` and `remove`, run as
//! a user runs them, against the fake Cloudflare API that only a build made
//! for tests can be pointed at. Every token here is made up, and no real
//! account, tunnel or Access team is touched.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use cctop_core::cloudflare::fake;

/// A home of its own per test: they run side by side.
fn sandbox(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("cctop-tunnel-access-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["run", "home", "cache", "config", "data"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    dir
}

/// `cctop tunnel <args>` in `dir` against the fake at `api`, with `stdin`
/// piped in.
fn cctop(dir: &Path, api: &str, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cctop"))
        .arg("tunnel")
        .args(args)
        .env("XDG_RUNTIME_DIR", dir.join("run"))
        .env("HOME", dir.join("home"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("CCTOP_CLOUDFLARE_API", api)
        .env_remove("CCTOP_TUNNEL_TOKEN")
        .env_remove("CCTOP_TUNNEL_HOSTNAME")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CCTOP_LOG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn calls(seen: &fake::Seen) -> Vec<String> {
    seen.lock()
        .unwrap()
        .iter()
        .map(|(m, p, _)| format!("{m} {p}"))
        .collect()
}

#[test]
fn access_is_set_up_with_the_tunnel_invites_reach_the_policy_and_remove_deletes_it_all() {
    let dir = sandbox("setup");
    let (api, seen) = fake::api(fake::accepting(None));

    let out = cctop(
        &dir,
        &api,
        &["setup", "--access", "Owner@Example.test"],
        "made-up-api-token\n",
    );
    assert!(out.status.success(), "{}", said(&out));
    assert!(
        said(&out).contains("log in as owner@example.test"),
        "{}",
        said(&out)
    );
    let made = calls(&seen);
    assert!(
        made.contains(&"POST /accounts/acct1/access/apps".to_string()),
        "{made:?}"
    );
    let said_all = said(&out);
    assert!(
        !said_all.contains("made-up-api-token"),
        "the token is never printed"
    );

    let out = cctop(&dir, &api, &["invite", "add", "@company.test"], "");
    assert!(out.status.success(), "{}", said(&out));
    let out = cctop(
        &dir,
        &api,
        &["invite", "add", "--full", "boss@company.test"],
        "",
    );
    assert!(out.status.success(), "{}", said(&out));
    // The edge's policy is rewritten each time, so it lets in whom the
    // server will.
    let last_put = seen
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|(m, _, _)| m == "PUT")
        .map(|(_, p, b)| (p.clone(), b.clone()))
        .unwrap();
    assert_eq!(last_put.0, "/accounts/acct1/access/policies/pol1");
    assert!(
        last_put
            .1
            .contains("\"email_domain\":{\"domain\":\"company.test\"}"),
        "{}",
        last_put.1
    );
    assert!(last_put.1.contains("boss@company.test"), "{}", last_put.1);

    let out = cctop(&dir, &api, &["invite", "list"], "");
    let list = said(&out);
    assert!(list.contains("owner@example.test (owner): full"), "{list}");
    assert!(
        list.contains("everyone at @company.test: read-only"),
        "{list}"
    );
    assert!(list.contains("boss@company.test: full"), "{list}");

    let out = cctop(&dir, &api, &["status"], "");
    assert!(
        said(&out).contains("Cloudflare Access: on"),
        "{}",
        said(&out)
    );

    let out = cctop(&dir, &api, &["invite", "add", "not an email"], "");
    assert!(!out.status.success());

    let out = cctop(&dir, &api, &["invite", "remove", "@company.test"], "");
    assert!(
        said(&out).contains("can no longer log in"),
        "{}",
        said(&out)
    );

    seen.lock().unwrap().clear();
    let out = cctop(&dir, &api, &["remove"], "");
    assert!(out.status.success(), "{}", said(&out));
    let gone = calls(&seen);
    for call in [
        "DELETE /accounts/acct1/access/apps/app1",
        "DELETE /accounts/acct1/access/policies/pol1",
        "DELETE /accounts/acct1/access/identity_providers/idp1",
    ] {
        assert!(gone.contains(&call.to_string()), "{call} in {gone:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn access_off_deletes_what_was_made_and_keeps_the_tunnel() {
    let dir = sandbox("off");
    let (api, seen) = fake::api(fake::accepting(None));
    assert!(
        cctop(&dir, &api, &["setup"], "made-up-api-token\n")
            .status
            .success()
    );
    let out = cctop(
        &dir,
        &api,
        &["access", "on", "--owner", "owner@example.test"],
        "",
    );
    assert!(out.status.success(), "{}", said(&out));

    seen.lock().unwrap().clear();
    let out = cctop(&dir, &api, &["access", "off"], "");
    assert!(out.status.success(), "{}", said(&out));
    let gone = calls(&seen);
    assert!(gone.iter().all(|c| c.contains("/access/")), "{gone:?}");
    assert_eq!(gone.len(), 3, "{gone:?}");
    let out = cctop(&dir, &api, &["status"], "");
    assert!(said(&out).contains("Connected: https://"), "{}", said(&out));
    assert!(
        said(&out).contains("Cloudflare Access: off"),
        "{}",
        said(&out)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
