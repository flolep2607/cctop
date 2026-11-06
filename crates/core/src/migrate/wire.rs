//! How a move reaches the other machine: ssh to a `--host`, running that
//! machine's `cctop move …` with every parameter as JSON on stdin; or a
//! sibling on the Cloudflare account, asked over its tunnel at `/api/move/…`
//! with requests signed as this machine.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use super::{Answer, Ask, Hello, Manifest, PROTOCOL, Placement, Receipt, Ticket};
use crate::fleet::Host;
use crate::peer::Peer;
use crate::reach::Reach;

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

/// The far side `reach` names.
pub fn to(reach: &Reach) -> Box<dyn Wire> {
    match reach {
        Reach::Ssh(host) => Box::new(Ssh(host.clone())),
        Reach::Peer(peer) => Box::new(Tunnel(peer.clone())),
    }
}

/// Where a sibling's serve answers a move. Its own prefix, so a cctop older
/// than moving answers "no such page", which [`Tunnel`] reads as "update it".
pub const ROUTE: &str = "/api/move/";

/// How long a committed move may take there — placing a large repository,
/// starting the agent and seeing it stay up — before this side stops asking.
const COMMIT_WAIT: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// How long `status` may go unanswered before the move is given up on there.
const UNHEARD_WAIT: std::time::Duration = std::time::Duration::from_secs(2 * 60);

/// Why a `status` question did not end the wait.
enum Status {
    /// The far cctop answered: the receive failed, for this reason.
    Said(String),
    /// No answer from it reached here.
    Unheard(String),
}

/// The name a body goes by on the path: its sha256, in hex.
pub fn body_sha(body: &[u8]) -> String {
    use sha2::Digest;
    super::hex(&sha2::Sha256::digest(body))
}

/// A chunk's bytes as a request body: base64 in JSON, like every other
/// action's body.
pub fn chunk_body(data: &[u8]) -> Vec<u8> {
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(data);
    serde_json::json!({ "data": data }).to_string().into_bytes()
}

/// [`chunk_body`] read back.
pub fn chunk_data(body: &[u8]) -> Result<Vec<u8>, String> {
    use base64::Engine;
    #[derive(serde::Deserialize)]
    struct Chunk {
        data: String,
    }
    let chunk: Chunk = serde_json::from_slice(body).map_err(|e| format!("the request: {e}"))?;
    base64::engine::general_purpose::STANDARD
        .decode(chunk.data)
        .map_err(|_| "the chunk is not base64".to_string())
}

/// A sibling on the account, over its tunnel.
///
/// The signature covers the method and the path but not the body, so every
/// body's sha256 is the last word of the path: what is signed then pins what
/// is sent, and the far side refuses a body that does not match it.
pub struct Tunnel(pub Arc<Peer>);

impl Tunnel {
    /// `POST /api/move/<words…>/<sha256 of body>`, answered as an [`Answer`].
    fn post<T: serde::de::DeserializeOwned>(&self, words: &str, body: &[u8]) -> Result<T, String> {
        let path = format!("{ROUTE}{words}/{}", body_sha(body));
        self.answer(self.0.send("POST", &path, "", Some(body)))
    }

    fn answer<T: serde::de::DeserializeOwned>(
        &self,
        sent: Result<crate::peer::Answer, String>,
    ) -> Result<T, String> {
        let machine = &self.0.machine;
        let answer = sent.map_err(|why| format!("{machine}: {why}"))?;
        match answer.status {
            200 => serde_json::from_slice::<Answer<T>>(&answer.body)
                .map_err(|_| format!("{machine} answered something else"))?
                .into_result(),
            404 => Err(format!(
                "{machine}'s cctop is too old to receive a moved session — update it there \
                 (cctop --update) and move again"
            )),
            _ => Err(format!("{machine} {}", crate::peer::refusal(&answer))),
        }
    }

    fn json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, String> {
        serde_json::to_vec(value).map_err(|e| e.to_string())
    }

    /// Send every byte of `files`, in [`CHUNK`](super::receive::CHUNK)s
    /// across the file boundaries — the far side only appends.
    fn upload(
        &self,
        id: &str,
        files: &[&Path],
        total: u64,
        progress: &(dyn Fn(u64, u64) + Sync),
    ) -> Result<(), String> {
        use std::io::Read;
        let size = super::receive::CHUNK as usize;
        let mut buf = Vec::with_capacity(size);
        let (mut seq, mut sent) = (0u64, 0u64);
        let flush = |buf: &mut Vec<u8>, seq: &mut u64, sent: &mut u64| -> Result<(), String> {
            let body = chunk_body(buf);
            self.post::<()>(&format!("chunk/{id}/{seq}"), &body)?;
            *sent += buf.len() as u64;
            *seq += 1;
            buf.clear();
            progress(*sent, total);
            Ok(())
        };
        for path in files {
            let mut file =
                std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
            loop {
                let room = size - buf.len();
                let start = buf.len();
                buf.resize(size, 0);
                let n = file
                    .read(&mut buf[start..start + room])
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                buf.truncate(start + n);
                if buf.len() == size {
                    flush(&mut buf, &mut seq, &mut sent)?;
                }
                if n == 0 {
                    break;
                }
            }
        }
        if !buf.is_empty() {
            flush(&mut buf, &mut seq, &mut sent)?;
        }
        Ok(())
    }

    /// One `status` question: the far cctop's own answer, or no answer.
    fn status(&self, id: &str) -> Result<Option<Receipt>, Status> {
        let path = format!("{ROUTE}status/{id}/{}", body_sha(b"{}"));
        let answer = self
            .0
            .send("POST", &path, "", Some(b"{}"))
            .map_err(Status::Unheard)?;
        if answer.status != 200 {
            return Err(Status::Unheard(crate::peer::refusal(&answer)));
        }
        serde_json::from_slice::<Answer<Option<Receipt>>>(&answer.body)
            .map_err(|_| Status::Unheard("an answer that was not one".into()))?
            .into_result()
            .map_err(Status::Said)
    }

    fn ticket(&self, word: &str, ticket: &str) -> Result<(), String> {
        let t = Ticket {
            protocol: PROTOCOL,
            ticket: ticket.to_string(),
        };
        self.post(word, &Self::json(&t)?)
    }
}

impl Wire for Tunnel {
    fn machine(&self) -> &str {
        &self.0.machine
    }

    fn hello(&self) -> Result<Hello, String> {
        self.answer(self.0.send("GET", &format!("{ROUTE}hello"), "", None))
    }

    fn plan(&self, ask: &Ask) -> Result<Placement, String> {
        self.post("plan", &Self::json(ask)?)
    }

    fn receive(
        &self,
        manifest: &Manifest,
        files: &[&Path],
        progress: &(dyn Fn(u64, u64) + Sync),
    ) -> Result<Receipt, String> {
        let total = manifest.bundle.len + manifest.transcript.len;
        let id: String = self.post("begin", &Self::json(manifest)?)?;
        let sent = self
            .upload(&id, files, total, progress)
            .and_then(|()| self.post::<()>(&format!("commit/{id}"), b"{}"));
        if let Err(why) = sent {
            // Dropped there, so the next move is not told one is arriving.
            let _ = self.ticket("abort", &id);
            return Err(why);
        }
        let asked = std::time::Instant::now();
        // A request that never got an answer — a 502 from the edge, the
        // client's own timeout — says nothing about the move, and the far
        // side keeps its answer until `finish` or `abort`: so it is asked
        // again. Only an answer from that cctop ends the wait.
        let mut unheard = None;
        loop {
            match self.status(&id) {
                Ok(Some(receipt)) => return Ok(receipt),
                Ok(None) => unheard = None,
                Err(Status::Said(why)) => return Err(why),
                Err(Status::Unheard(why)) => {
                    let since = *unheard.get_or_insert_with(std::time::Instant::now);
                    if since.elapsed() > UNHEARD_WAIT {
                        return Err(format!("{}: {why}", self.0.machine));
                    }
                }
            }
            if asked.elapsed() > COMMIT_WAIT {
                return Err(format!(
                    "{} took too long to say whether it arrived",
                    self.0.machine
                ));
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    fn finish(&self, ticket: &str) -> Result<(), String> {
        self.ticket("finish", ticket)
    }

    fn abort(&self, ticket: &str) -> Result<(), String> {
        self.ticket("abort", ticket)
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

    /// A chunk survives the trip as base64 in JSON, and the path names a body
    /// by its sha256.
    #[test]
    fn a_chunk_is_named_by_its_sha_and_read_back_whole() {
        let bytes: Vec<u8> = (0..=255).collect();
        let body = chunk_body(&bytes);
        assert_eq!(chunk_data(&body).unwrap(), bytes);
        assert!(chunk_data(br#"{"data":"not base64!"}"#).is_err());
        assert_eq!(
            body_sha(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// A sibling older than moving answers "no such page", which is said as
    /// what to do about it; anything else is its own words.
    #[test]
    fn an_older_sibling_is_told_to_update() {
        let peer = Arc::new(Peer::fixed("box", "http://127.0.0.1:9", "s"));
        let tunnel = Tunnel(peer);
        let reply = |status: u16, body: &str| {
            Ok(crate::peer::Answer {
                status,
                content_type: "text/plain".into(),
                body: body.as_bytes().to_vec(),
                version: None,
            })
        };
        let old = tunnel
            .answer::<Hello>(reply(404, "no such page"))
            .unwrap_err();
        assert!(old.contains("box's cctop is too old"), "{old}");
        let refused = tunnel.answer::<Hello>(reply(403, "no")).unwrap_err();
        assert!(refused.starts_with("box answered no"), "{refused}");
        let said = tunnel
            .answer::<u32>(reply(200, r#"{"error":"full"}"#))
            .unwrap_err();
        assert_eq!(said, "full");
        assert_eq!(tunnel.answer::<u32>(reply(200, r#"{"ok":3}"#)).unwrap(), 3);
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
