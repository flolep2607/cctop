//! How a move reaches the other machine. One transport so far: ssh to a
//! `--host`, running that machine's `cctop move …` with every parameter as
//! JSON on stdin.

use std::io::Write;
use std::path::Path;

use super::{Answer, Ask, Hello, Manifest, PROTOCOL, Placement, Receipt, Ticket};
use crate::fleet::Host;

/// The far side of a move.
pub trait Wire: Send + Sync {
    /// The machine's name, as the person knows it.
    fn machine(&self) -> &str;
    fn hello(&self) -> Result<Hello, String>;
    fn plan(&self, ask: &Ask) -> Result<Placement, String>;
    /// Send the manifest and then each file's bytes, reporting bytes sent
    /// of the total as they go.
    fn receive(
        &self,
        manifest: &Manifest,
        files: &[&Path],
        progress: &(dyn Fn(u64, u64) + Sync),
    ) -> Result<Receipt, String>;
    fn finish(&self, ticket: &str) -> Result<(), String>;
    fn abort(&self, ticket: &str) -> Result<(), String>;
}

/// A `--host`, over ssh.
pub struct Ssh(pub Host);

impl Ssh {
    fn ask<T: serde::de::DeserializeOwned>(
        &self,
        word: &str,
        json: Option<String>,
    ) -> Result<T, String> {
        let out = self
            .0
            .run_input(&["move", word], move |stdin| match json {
                Some(line) => writeln!(stdin, "{line}"),
                None => Ok(()),
            })
            .map_err(|why| self.explain(&why))?;
        answer(&out)
    }

    /// What a failed ssh round trip means for a move.
    fn explain(&self, why: &str) -> String {
        if too_old(why) {
            return format!(
                "{}'s cctop is too old to receive a moved session — update it there \
                 (cctop --update) and move again",
                self.0.target
            );
        }
        if crate::fleet::is_missing(why) {
            return format!("there is no cctop on {} to receive it", self.0.target);
        }
        format!("{}: {why}", self.0.target)
    }
}

/// An older cctop reads `move` as an argument it does not take: clap's
/// complaint, in either of the spellings it has used.
pub fn too_old(why: &str) -> bool {
    why.contains("unexpected argument")
        || why.contains("unrecognized subcommand")
        || why.contains("Found argument")
}

/// The last line of the far side's stdout, as an [`Answer`].
fn answer<T: serde::de::DeserializeOwned>(out: &str) -> Result<T, String> {
    let line = out
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .ok_or("the other machine answered nothing")?;
    serde_json::from_str::<Answer<T>>(line)
        .map_err(|_| format!("the other machine answered something else: {line}"))?
        .into_result()
}

impl Wire for Ssh {
    fn machine(&self) -> &str {
        &self.0.target
    }

    fn hello(&self) -> Result<Hello, String> {
        self.ask("hello", None)
    }

    fn plan(&self, ask: &Ask) -> Result<Placement, String> {
        self.ask(
            "plan",
            Some(serde_json::to_string(ask).map_err(|e| e.to_string())?),
        )
    }

    fn receive(
        &self,
        manifest: &Manifest,
        files: &[&Path],
        progress: &(dyn Fn(u64, u64) + Sync),
    ) -> Result<Receipt, String> {
        let head = serde_json::to_string(manifest).map_err(|e| e.to_string())?;
        let total: u64 = manifest.bundle.len + manifest.transcript.len;
        let paths: Vec<std::path::PathBuf> = files.iter().map(|p| p.to_path_buf()).collect();
        let out = self
            .0
            .run_input(&["move", "receive"], |stdin| {
                writeln!(stdin, "{head}")?;
                let mut sent = 0u64;
                let mut buf = vec![0u8; 1 << 16];
                for path in &paths {
                    let mut file = std::fs::File::open(path)?;
                    loop {
                        let n = std::io::Read::read(&mut file, &mut buf)?;
                        if n == 0 {
                            break;
                        }
                        stdin.write_all(&buf[..n])?;
                        sent += n as u64;
                        progress(sent, total);
                    }
                }
                stdin.flush()
            })
            .map_err(|why| self.explain(&why))?;
        answer(&out)
    }

    fn finish(&self, ticket: &str) -> Result<(), String> {
        let t = Ticket {
            protocol: PROTOCOL,
            ticket: ticket.to_string(),
        };
        self.ask(
            "finish",
            Some(serde_json::to_string(&t).map_err(|e| e.to_string())?),
        )
    }

    fn abort(&self, ticket: &str) -> Result<(), String> {
        let t = Ticket {
            protocol: PROTOCOL,
            ticket: ticket.to_string(),
        };
        self.ask(
            "abort",
            Some(serde_json::to_string(&t).map_err(|e| e.to_string())?),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_cctop_is_told_from_a_failed_one() {
        assert!(too_old("error: unexpected argument 'move' found"));
        assert!(too_old("error: unrecognized subcommand 'move'"));
        assert!(!too_old("Permission denied (publickey)."));
    }

    #[test]
    fn the_answer_is_the_last_line() {
        let got: u32 = answer("progress\n{\"ok\":7}\n\n").unwrap();
        assert_eq!(got, 7);
        let err = answer::<u32>("{\"error\":\"full\"}").unwrap_err();
        assert_eq!(err, "full");
        assert!(answer::<u32>("garbage").is_err());
    }
}
