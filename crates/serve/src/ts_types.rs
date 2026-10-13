//! The web UI's TypeScript types, generated from the structs the API answers
//! with, and checked against the copies committed in `web/src/lib/gen/`.
//!
//! The page used to declare these shapes by hand, and they drifted: it read a
//! cost's `total` as a number where the server sends a string or `null`, and a
//! tool-error count as always present where a harness that records no outcome
//! leaves it out. Now a change to a struct below that is not carried into the
//! committed files fails this test — and so `verify`, which runs it.
//!
//! To regenerate after changing one of these structs:
//!
//! ```text
//! CCTOP_WRITE_TS=1 cargo test -p cctop-serve ts_types
//! ```
//!
//! and commit `web/src/lib/gen/` with the change. Like the snapshot tests, CI
//! only compares and never writes.
//!
//! Only under test: ts-rs is a dev-dependency of this crate, and core derives
//! its declarations behind the `ts` feature, which only this crate's
//! dev-dependencies turn on. A release build compiles none of it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use ts_rs::{Config, TS};

/// Every type the page reads, each with whatever it refers to.
fn export(dir: &Path) {
    use cctop_core::{chat, json, provider_status, report};
    // `number` for 64-bit integers rather than ts-rs's `bigint`: the page reads
    // them with `JSON.parse`, which never makes a bigint, and no count or size
    // here comes near 2^53.
    let cfg = Config::new().with_out_dir(dir).with_large_int("number");
    let out = |result: Result<(), ts_rs::ExportError>| result.expect("ts-rs export");
    out(json::JsonSession::export_all(&cfg));
    out(chat::Conversation::export_all(&cfg));
    out(report::Report::export_all(&cfg));
    out(cctop_core::access::Access::export_all(&cfg));
    out(provider_status::Document::export_all(&cfg));
    out(crate::analytics::Analytics::export_all(&cfg));
    out(crate::search::Hit::export_all(&cfg));
    out(crate::tabs::Tab::export_all(&cfg));
    out(crate::address::Address::export_all(&cfg));
    out(crate::address::Done::export_all(&cfg));
}

/// The `.ts` files under `dir`, by name.
fn read(dir: &Path) -> BTreeMap<String, String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "ts"))
                .map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    (name, std::fs::read_to_string(e.path()).unwrap_or_default())
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn the_web_ui_types_are_current() {
    let committed = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/src/lib/gen");
    let fresh = tempfile::tempdir().unwrap();
    export(fresh.path());
    let want = read(fresh.path());
    assert!(want.contains_key("JsonSession.ts"), "{:?}", want.keys());

    if std::env::var_os("CCTOP_WRITE_TS").is_some() {
        std::fs::create_dir_all(&committed).unwrap();
        for name in read(&committed).keys() {
            std::fs::remove_file(committed.join(name)).unwrap();
        }
        for (name, body) in &want {
            std::fs::write(committed.join(name), body).unwrap();
        }
        return;
    }

    let have = read(&committed);
    let stale: Vec<&String> = want
        .keys()
        .chain(have.keys())
        .filter(|name| want.get(*name) != have.get(*name))
        .collect();
    assert!(
        stale.is_empty(),
        "web/src/lib/gen/ is out of date with the Rust types ({stale:?}). Regenerate it with \
         `CCTOP_WRITE_TS=1 cargo test -p cctop-serve ts_types` and commit it."
    );
}
