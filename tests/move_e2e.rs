//! A session moved between two machines, run the way it runs.
//!
//! Two home directories stand in for the two machines, and an `ssh` on PATH
//! stands in for the network: it drops ssh's options, as the real one consumes
//! them, and runs the far command as the real cctop binary with the second
//! home as its `$HOME`. So the receiving side is a separate process with its
//! own config, cache, runtime directory and rmux daemon — everything but the
//! wire is the real thing. The agent is a script that sleeps, so "the agent
//! is running there" can be checked without a model.
//!
//! The sending side runs in this test's process, through the same calls the
//! terminal's `X` makes.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use cctop_core::migrate::{self, Kind};
use cctop_core::pricing::Provider;
use cctop_core::session::Session;

const ID: &str = "0b4e8a54-8d39-4f4e-8a39-6b8a3c1c2d01";

fn script(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The far machine: its home, and its cctop run there with nothing of this
/// process's environment.
struct Far {
    home: PathBuf,
    bin: PathBuf,
}

impl Far {
    fn cctop(&self, args: &[&str]) -> String {
        let out = Command::new(self.bin.join("cctop"))
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("XDG_RUNTIME_DIR", self.home.join("run"))
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

#[test]
fn a_claude_session_moves_to_another_machine_files_and_all() {
    // Short, for the rmux socket under it: `sun_path` holds 107 bytes.
    let tmp = std::env::temp_dir().join(format!("cmv{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let _gone = scopeguard({
        let tmp = tmp.clone();
        move || {
            let _ = std::fs::remove_dir_all(&tmp);
        }
    });
    let here = tmp.join("a");
    let far = Far {
        home: tmp.join("b"),
        bin: tmp.join("bin"),
    };
    std::fs::create_dir_all(&far.bin).unwrap();
    std::fs::create_dir_all(far.home.join("run")).unwrap();
    std::fs::set_permissions(far.home.join("run"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_cctop"), far.bin.join("cctop")).unwrap();
    script(&far.bin.join("claude"), "#!/bin/sh\nexec sleep 300\n");
    script(
        &far.bin.join("ssh"),
        &format!(
            "#!/bin/sh\n\
             while [ \"$#\" -gt 0 ] && [ \"$1\" != \"--\" ]; do shift; done\n\
             shift\n\
             exec env -i HOME={home} PATH={bin}:/usr/bin:/bin XDG_RUNTIME_DIR={home}/run \"$@\"\n",
            home = far.home.display(),
            bin = far.bin.display(),
        ),
    );
    // SAFETY: the only test in this binary, so no other thread reads the
    // environment while it changes; every child after this finds the fake ssh.
    unsafe {
        std::env::set_var(
            "PATH",
            format!(
                "{}:{}",
                far.bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
    };

    // Here: a checkout with work in progress, and a session's transcript.
    let app = here.join("app");
    std::fs::create_dir_all(&app).unwrap();
    git(&app, &["init", "-q", "-b", "main"]);
    std::fs::write(app.join("a.txt"), "one\n").unwrap();
    git(&app, &["add", "-A"]);
    git(&app, &["commit", "-q", "-m", "first"]);
    git(
        &app,
        &["remote", "add", "origin", "https://example.com/o/app.git"],
    );
    std::fs::write(app.join("a.txt"), "two\n").unwrap();
    std::fs::write(app.join("new.txt"), "fresh\n").unwrap();
    let transcript = here
        .join(".claude/projects")
        .join(cctop_core::convert::project_slug(&app))
        .join(format!("{ID}.jsonl"));
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(
        &transcript,
        format!(
            "{{\"type\":\"user\",\"sessionId\":\"{ID}\",\"cwd\":\"{}\",\"message\":{{\"content\":\"hi\"}}}}\n",
            app.display()
        ),
    )
    .unwrap();
    let mut session = Session::new(Provider::Claude, ID.into());
    session.label_source = app.to_string_lossy().into_owned();
    session.data_file = Some(transcript.clone());

    let wire = migrate::wire::Ssh(cctop_core::fleet::Host::parse("box").unwrap());
    let plan = migrate::send::prepare(&session, &wire).expect("the far side answers");
    assert_eq!(plan.placement.kind, Kind::Clone, "{:?}", plan.placement);
    let landed = far.home.join("app");
    assert_eq!(PathBuf::from(&plan.placement.dir), landed);

    let outcome = migrate::send::run(&plan, &wire, &|_| {}).expect("moved");
    let rmux = outcome.receipt.rmux.clone();
    let cleanup = scopeguard(|| {
        far.cctop(&["mux", "kill-session", "-t", &rmux]);
    });

    // The files, with the work in progress, on the branch.
    assert_eq!(
        std::fs::read_to_string(landed.join("a.txt")).unwrap(),
        "two\n"
    );
    assert_eq!(
        std::fs::read_to_string(landed.join("new.txt")).unwrap(),
        "fresh\n"
    );
    assert_eq!(git(&landed, &["symbolic-ref", "--short", "HEAD"]), "main");
    // The transcript, under the project directory Claude derives from where
    // it landed, saying so.
    let there = far
        .home
        .join(".claude/projects")
        .join(cctop_core::convert::project_slug(&landed))
        .join(format!("{ID}.jsonl"));
    let text = std::fs::read_to_string(&there).expect("the transcript is there");
    assert!(
        text.contains(&format!("\"cwd\":\"{}\"", landed.display())),
        "{text}"
    );
    // The agent, running in a tab there.
    assert!(
        far.cctop(&["mux", "ls"]).contains(&rmux),
        "no tab {rmux} there"
    );
    // And here it is marked as gone, so a resume here is refused.
    let moved = migrate::moved_to(Provider::Claude, ID).expect("marked as moved");
    assert_eq!(moved.machine, "box");
    // Nothing of the move is left over there but the record that it was
    // kept, which answers a late abort truthfully.
    let staging = far.home.join(".cache/cctop/move");
    let left: Vec<_> = std::fs::read_dir(&staging)
        .map(|d| {
            d.flatten()
                .filter(|e| e.file_name() != "lock" && e.file_name() != "kept")
                .collect()
        })
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
    drop(cleanup);
}

/// Run `f` when dropped, so a failed assertion still ends the far agent.
fn scopeguard(f: impl FnOnce()) -> impl Drop {
    struct Guard<F: FnOnce()>(Option<F>);
    impl<F: FnOnce()> Drop for Guard<F> {
        fn drop(&mut self) {
            if let Some(f) = self.0.take() {
                f();
            }
        }
    }
    Guard(Some(f))
}
