//! What gets embedded, and where the vectors are kept.
//!
//! # What a transcript is mostly made of
//!
//! Almost none of it is conversation. Measured over 56 MB of real transcripts:
//! tool results were 10.7 MB, the model's own thinking 5.6 MB, tool calls
//! 5.2 MB — and what a person actually wrote, plus what the agent wrote back,
//! came to about 2.3 MB between them. Four percent.
//!
//! So only that four percent is indexed. The rest is not merely wasted work: a
//! chunk of `ls` output or a file someone read has no topic, and it would sit in
//! the index competing to answer questions it has no business answering.
//! Thinking is excluded on the same grounds even though it reads well — it is
//! five times the volume of the conversation it accompanies, and would decide
//! every query by weight alone.
//!
//! That leaves a corpus small enough that the index is under a megabyte and
//! builds in about a second.
//!
//! # Why the chunks are large
//!
//! A static model has no context window — [`super::Model`] would take a whole
//! transcript in one call — so chunk size is not a limit being worked around.
//! It is only the granularity of an answer, and the answer wanted here is "this
//! conversation", not "this sentence". Larger chunks also average away the
//! function words that a mean over tokens is otherwise dominated by.

use super::{Model, cosine};
use crate::session::search::Target;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

/// How much prose goes into one chunk.
///
/// Big enough that a chunk is a subject rather than a sentence, small enough
/// that a long session is still several.
const CHUNK_CHARS: usize = 4000;

/// A chunk shorter than this is folded into the previous one rather than
/// embedded alone: a two-word tail is a vector of almost nothing.
const MIN_CHUNK_CHARS: usize = 200;

/// How much of a chunk is kept to show as the reason for a match.
const SNIPPET_CHARS: usize = 160;

/// Machine-generated user turns, which are addressed to the agent rather than
/// written by the person, and carry no topic of their own.
/// Found by indexing a real corpus and reading the results: each of these
/// turned up as a search hit, and none of them is something a person wrote.
const NOISE_PREFIXES: &[&str] = &[
    "<task-notification>",
    "<system-reminder>",
    "<command-name>",
    "<command-message>",
    "<local-command-stdout>",
    "<local-command-caveat>",
    "<cross-session-message",
    "Another Claude session sent a message",
    "Caveat: The messages below",
];

/// The on-disk format. Bumped when the layout or the model changes, because a
/// vector is only comparable with vectors from the same model.
const MAGIC: &[u8; 8] = b"cctopemb";
const VERSION: u32 = 1;

/// One embedded piece of one session.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// [`Session::key`], so a hit maps back to a row.
    pub key: String,
    pub vector: Vec<f32>,
    /// The opening of the chunk, for showing why it matched.
    pub snippet: String,
}

/// Every embedded chunk on the machine, and what it was built from.
#[derive(Debug, Default)]
pub struct Index {
    pub chunks: Vec<Chunk>,
    /// Per session, the fingerprint of the transcript the chunks came from, so
    /// a session whose transcript has grown can be re-embedded on its own
    /// rather than forcing the whole index to be rebuilt.
    pub built_from: HashMap<String, Fingerprint>,
    pub dim: usize,
}

/// Enough of a file to notice it changed, without reading it.
///
/// Size and modification time, which is what a growing transcript changes.
/// ponytail: an edit that preserves both is not noticed. Transcripts are
/// append-only logs, so that does not arise for the files this indexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint {
    pub len: u64,
    pub modified: u64,
}

impl Fingerprint {
    pub fn of(path: &Path) -> Option<Fingerprint> {
        let meta = std::fs::metadata(path).ok()?;
        let modified = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        Some(Fingerprint {
            len: meta.len(),
            modified,
        })
    }
}

/// Pull the conversation out of one transcript, as chunks of prose.
///
/// ponytail: the shapes understood here are the ones the file-backed harnesses
/// write — a string `content`, or a list of blocks with `text` in them. A
/// provider that stores its conversation somewhere else contributes nothing and
/// is simply absent from this index; the literal search still covers it.
pub fn chunks_of(path: &Path) -> Vec<String> {
    let mut pieces: Vec<String> = Vec::new();
    let _ = crate::session::extract::for_each_jsonl(path, |item| {
        let Some(content) = item.get("message").and_then(|m| m.get("content")) else {
            return;
        };
        match content {
            serde_json::Value::String(s) => {
                if let Some(text) = keep(s) {
                    pieces.push(text);
                }
            }
            serde_json::Value::Array(blocks) => {
                for b in blocks {
                    // `text` only: `thinking`, `tool_use` and `tool_result` are
                    // deliberately not conversation. See the module docs.
                    if b.get("type").and_then(|t| t.as_str()) != Some("text") {
                        continue;
                    }
                    if let Some(text) = b.get("text").and_then(|t| t.as_str()).and_then(keep) {
                        pieces.push(text);
                    }
                }
            }
            _ => {}
        }
    });
    gather(pieces)
}

/// Whether a turn is prose a person or an agent wrote, and its trimmed text.
fn keep(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if NOISE_PREFIXES.iter().any(|p| trimmed.starts_with(p)) {
        return None;
    }
    Some(trimmed.to_string())
}

/// Pack turns into chunks, without splitting a turn across two.
///
/// A turn is the unit someone wrote; cutting one in half puts the start of a
/// thought in one vector and the end of it in another, and neither is then
/// about what was said.
fn gather(pieces: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for piece in pieces {
        if !current.is_empty() && current.chars().count() + piece.chars().count() > CHUNK_CHARS {
            out.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(&piece);
    }
    if !current.is_empty() {
        // A short tail belongs with what came before it rather than alone.
        match out.last_mut() {
            Some(last) if current.chars().count() < MIN_CHUNK_CHARS => {
                last.push('\n');
                last.push_str(&current);
            }
            _ => out.push(current),
        }
    }
    out
}

/// The first line or so of a chunk, flattened for display.
fn snippet_of(text: &str) -> String {
    let mut out = String::with_capacity(SNIPPET_CHARS);
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() || c.is_control() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(c);
        if out.chars().count() >= SNIPPET_CHARS {
            break;
        }
    }
    out
}

impl Index {
    /// Bring the index up to date with the sessions on disk.
    ///
    /// Sessions whose transcript is unchanged keep the vectors they already
    /// have; only what grew is embedded again. Returns how many chunks were
    /// computed, which is zero on the common path where nothing moved.
    pub fn refresh(&mut self, model: &Model, sessions: &[Target]) -> usize {
        self.dim = model.dim();
        let mut live: HashMap<String, Fingerprint> = HashMap::new();
        let mut embedded = 0;

        for session in sessions {
            let Some(path) = session.data_file.as_deref() else {
                continue;
            };
            let Some(print) = Fingerprint::of(path) else {
                continue;
            };
            let key = session.key.clone();
            live.insert(key.clone(), print);
            if self.built_from.get(&key) == Some(&print) {
                continue;
            }
            self.chunks.retain(|c| c.key != key);
            for text in chunks_of(path) {
                self.chunks.push(Chunk {
                    key: key.clone(),
                    vector: model.embed(&text),
                    snippet: snippet_of(&text),
                });
                embedded += 1;
            }
            self.built_from.insert(key, print);
        }

        // A session that is gone from disk must not keep answering queries.
        self.chunks.retain(|c| live.contains_key(&c.key));
        self.built_from.retain(|k, _| live.contains_key(k));
        embedded
    }

    /// The best-scoring chunk per session, above `floor`, best first.
    ///
    /// Per session rather than per chunk: the question is which conversation,
    /// and a long session with five decent chunks should not crowd out four
    /// other sessions.
    pub fn search(&self, query: &[f32], floor: f32, limit: usize) -> Vec<(String, String, f32)> {
        let mut best: HashMap<&str, (f32, &str)> = HashMap::new();
        for chunk in &self.chunks {
            let score = cosine(query, &chunk.vector);
            if score < floor {
                continue;
            }
            let slot = best.entry(&chunk.key).or_insert((score, &chunk.snippet));
            if score > slot.0 {
                *slot = (score, &chunk.snippet);
            }
        }
        let mut hits: Vec<(String, String, f32)> = best
            .into_iter()
            .map(|(k, (s, snip))| (k.to_string(), snip.to_string(), s))
            .collect();
        hits.sort_by(|a, b| b.2.total_cmp(&a.2));
        hits.truncate(limit);
        hits
    }

    /// Write the index out.
    ///
    /// A plain binary layout rather than JSON: these are floats, and a text
    /// encoding of them is both larger than the vectors and lossy about their
    /// last digit.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(self.dim as u32).to_le_bytes());
        out.extend_from_slice(&(self.chunks.len() as u32).to_le_bytes());
        for chunk in &self.chunks {
            write_str(&mut out, &chunk.key);
            write_str(&mut out, &chunk.snippet);
            for v in &chunk.vector {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out.extend_from_slice(&(self.built_from.len() as u32).to_le_bytes());
        for (key, print) in &self.built_from {
            write_str(&mut out, key);
            out.extend_from_slice(&print.len.to_le_bytes());
            out.extend_from_slice(&print.modified.to_le_bytes());
        }

        // Written beside and renamed, so an interrupted save leaves the old
        // index rather than half of a new one.
        let tmp = path.with_extension("tmp");
        std::fs::File::create(&tmp)
            .and_then(|mut f| f.write_all(&out))
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("renaming onto {}", path.display()))?;
        Ok(())
    }

    /// Read an index back, or `None` if there is not a usable one there.
    ///
    /// Anything unexpected — a short file, a version from another release, a
    /// width from another model — is treated as absent rather than as an error.
    /// The index is a cache, and the answer to a bad one is to build it again.
    pub fn load(path: &Path) -> Option<Index> {
        let mut raw = Vec::new();
        std::fs::File::open(path).ok()?.read_to_end(&mut raw).ok()?;
        let mut r = Reader { raw: &raw, at: 0 };
        if r.take(8)? != MAGIC {
            return None;
        }
        if r.u32()? != VERSION {
            return None;
        }
        let dim = r.u32()? as usize;
        if dim == 0 {
            return None;
        }
        let count = r.u32()? as usize;
        let mut chunks = Vec::with_capacity(count);
        for _ in 0..count {
            let key = r.string()?;
            let snippet = r.string()?;
            let bytes = r.take(dim.checked_mul(4)?)?;
            let vector = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect();
            chunks.push(Chunk {
                key,
                vector,
                snippet,
            });
        }
        let sessions = r.u32()? as usize;
        let mut built_from = HashMap::with_capacity(sessions);
        for _ in 0..sessions {
            let key = r.string()?;
            let len = u64::from_le_bytes(r.take(8)?.try_into().ok()?);
            let modified = u64::from_le_bytes(r.take(8)?.try_into().ok()?);
            built_from.insert(key, Fingerprint { len, modified });
        }
        Some(Index {
            chunks,
            built_from,
            dim,
        })
    }
}

fn write_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// A bounds-checked walk over the saved bytes.
struct Reader<'a> {
    raw: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let slice = self.raw.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn string(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::tests::tiny;
    use crate::pricing::Provider;
    use crate::session::Session;
    use std::path::PathBuf;

    fn temp(name: &str, body: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("cctop-index-{name}.jsonl"));
        let mut f = std::fs::File::create(&p).expect("create");
        f.write_all(body.as_bytes()).expect("write");
        p
    }

    fn line(role: &str, blocks: serde_json::Value) -> String {
        serde_json::json!({"type": role, "message": {"content": blocks}}).to_string()
    }

    /// Only what was said is indexed. Tool traffic and thinking are the bulk of
    /// a transcript and none of its subject.
    #[test]
    fn tool_traffic_and_thinking_are_not_indexed() {
        let body = [
            line("user", serde_json::json!("we should price out some GPUs")),
            line(
                "assistant",
                serde_json::json!([
                    {"type": "thinking", "thinking": "the user wants pricing, I should search"},
                    {"type": "tool_use", "input": {"command": "curl https://example.com"}},
                    {"type": "text", "text": "vast.ai looks cheapest for an A100"},
                ]),
            ),
            line(
                "user",
                serde_json::json!([{"type": "tool_result", "content": "HTTP 200 ..."}]),
            ),
        ]
        .join("\n");
        let path = temp("kinds", &body);

        let chunks = chunks_of(&path);
        let all = chunks.join(" ");
        assert!(all.contains("price out some GPUs"), "{all}");
        assert!(all.contains("vast.ai looks cheapest"), "{all}");
        assert!(!all.contains("I should search"), "thinking leaked: {all}");
        assert!(!all.contains("curl"), "tool call leaked: {all}");
        assert!(!all.contains("HTTP 200"), "tool result leaked: {all}");
        let _ = std::fs::remove_file(path);
    }

    /// A turn the harness generated is not something anyone said.
    #[test]
    fn machine_written_turns_are_dropped() {
        let body = [
            line(
                "user",
                serde_json::json!("<task-notification>\n<task-id>abc</task-id>"),
            ),
            // Both of these were real search hits before they were filtered.
            line(
                "user",
                serde_json::json!("<local-command-caveat>Caveat: generated while running"),
            ),
            line(
                "user",
                serde_json::json!("Another Claude session sent a message: hello"),
            ),
            line("user", serde_json::json!("but this one is mine")),
        ]
        .join("\n");
        let path = temp("noise", &body);
        let all = chunks_of(&path).join(" ");
        assert!(all.contains("this one is mine"), "{all}");
        assert!(!all.contains("task-notification"), "{all}");
        assert!(!all.contains("local-command-caveat"), "{all}");
        assert!(!all.contains("Another Claude session"), "{all}");
        let _ = std::fs::remove_file(path);
    }

    /// Turns are packed up to the chunk size and never split across two.
    #[test]
    fn chunks_fill_up_without_splitting_a_turn() {
        let turn = "x".repeat(1500);
        let pieces = vec![turn.clone(), turn.clone(), turn.clone(), turn.clone()];
        let chunks = gather(pieces);
        // Four 1500-character turns at a 4000 cap: two and two.
        assert_eq!(
            chunks.len(),
            2,
            "{:?}",
            chunks.iter().map(|c| c.len()).collect::<Vec<_>>()
        );
        for c in &chunks {
            // No chunk holds a partial turn: every one is a multiple of the
            // turn length plus the newlines between them.
            assert!(c.contains(&turn));
        }
    }

    /// A short tail joins the chunk before it rather than becoming a vector of
    /// almost nothing.
    #[test]
    fn a_short_tail_is_folded_into_the_previous_chunk() {
        let chunks = gather(vec!["y".repeat(3900), "and one last thought".into()]);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].ends_with("and one last thought"));
    }

    /// Refreshing embeds what changed and leaves alone what did not — the
    /// property that keeps a growing transcript cheap.
    #[test]
    fn refresh_embeds_only_what_changed() {
        let model_dir = std::env::temp_dir().join("cctop-index-refresh-model");
        let _ = std::fs::remove_dir_all(&model_dir);
        tiny(&model_dir, true);
        let model = Model::load(&model_dir).expect("model");

        let path = temp(
            "refresh",
            &[
                line("user", serde_json::json!("alpha")),
                line(
                    "assistant",
                    serde_json::json!([{"type":"text","text":"beta"}]),
                ),
            ]
            .join("\n"),
        );
        let mut session = Session::new(Provider::Claude, "s1".into());
        session.data_file = Some(path.clone());

        let target = Target::of(&session);
        let mut index = Index::default();
        let first = index.refresh(&model, std::slice::from_ref(&target));
        assert!(first > 0, "a new session must be embedded");
        assert_eq!(index.chunks.len(), first);
        assert!(index.chunks.iter().all(|c| c.key == session.key()));
        assert!(!index.chunks[0].snippet.is_empty());

        // Nothing moved, so nothing is recomputed.
        assert_eq!(index.refresh(&model, std::slice::from_ref(&target)), 0);
        assert_eq!(index.chunks.len(), first);

        // The transcript grows: it is embedded again, and the stale vectors for
        // that session are replaced rather than added to.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("append");
        f.write_all(line("user", serde_json::json!("a wholly new subject")).as_bytes())
            .expect("write");
        drop(f);
        assert!(
            index.refresh(&model, std::slice::from_ref(&target)) > 0,
            "growth re-embeds"
        );
        assert!(
            index.chunks.iter().all(|c| c.key == session.key()),
            "no duplicate keys"
        );

        // A session that is gone stops answering.
        index.refresh(&model, &[]);
        assert!(
            index.chunks.is_empty(),
            "removed sessions must leave the index"
        );
        assert!(index.built_from.is_empty());

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir_all(model_dir);
    }

    /// The index is a cache: it must survive a round trip to disk exactly, and
    /// treat anything it cannot read as simply absent.
    #[test]
    fn an_index_round_trips_and_refuses_rubbish() {
        let dir = std::env::temp_dir().join("cctop-index-roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("embeddings.bin");

        let mut index = Index {
            dim: 3,
            ..Index::default()
        };
        index.chunks.push(Chunk {
            key: "a/b".into(),
            vector: vec![0.5, -0.25, 1.0],
            snippet: "about GPUs".into(),
        });
        index.built_from.insert(
            "a/b".into(),
            Fingerprint {
                len: 42,
                modified: 7,
            },
        );
        index.save(&path).expect("save");

        let back = Index::load(&path).expect("load");
        assert_eq!(back.dim, 3);
        assert_eq!(back.chunks.len(), 1);
        assert_eq!(back.chunks[0].key, "a/b");
        assert_eq!(back.chunks[0].vector, vec![0.5, -0.25, 1.0]);
        assert_eq!(back.chunks[0].snippet, "about GPUs");
        assert_eq!(
            back.built_from.get("a/b"),
            Some(&Fingerprint {
                len: 42,
                modified: 7
            })
        );

        // Truncation, a foreign version and plain noise are all "no index".
        std::fs::write(&path, b"cctopemb").expect("write");
        assert!(Index::load(&path).is_none());
        std::fs::write(&path, b"not an index at all").expect("write");
        assert!(Index::load(&path).is_none());
        assert!(Index::load(&dir.join("missing.bin")).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Searching answers with sessions, not chunks: the best chunk stands for
    /// its session so one long conversation cannot fill the results.
    #[test]
    fn search_returns_the_best_chunk_of_each_session() {
        let index = Index {
            dim: 2,
            chunks: vec![
                Chunk {
                    key: "one".into(),
                    vector: vec![1.0, 0.0],
                    snippet: "exact".into(),
                },
                Chunk {
                    key: "one".into(),
                    vector: vec![0.7, 0.7],
                    snippet: "weaker".into(),
                },
                Chunk {
                    key: "two".into(),
                    vector: vec![0.0, 1.0],
                    snippet: "orthogonal".into(),
                },
            ],
            built_from: HashMap::new(),
        };
        // With no floor, both sessions answer, each by its best chunk.
        let hits = index.search(&[1.0, 0.0], -1.0, 10);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(hits[0].0, "one");
        assert_eq!(hits[0].1, "exact", "the stronger chunk should represent it");
        assert_eq!(hits[1].0, "two", "and the weaker session ranks below it");

        // The floor excludes rather than ranks: an orthogonal chunk scores 0.0
        // and does not reach even a low bar.
        let floored = index.search(&[1.0, 0.0], 0.1, 10);
        assert_eq!(floored.len(), 1, "{floored:?}");
        assert_eq!(floored[0].0, "one");

        // And a limit truncates after ranking, not before.
        assert_eq!(index.search(&[1.0, 0.0], -1.0, 1).len(), 1);
    }
}
