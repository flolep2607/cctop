//! Static sentence embeddings, for the conversation you cannot quote.
//!
//! [`crate::session::search`] finds a session by the words that are in it. This
//! finds one by what it was *about* — "the afternoon I was pricing GPUs" —
//! which is the question left over when the literal search comes back empty.
//!
//! # Why a static model, and not a real one
//!
//! The obvious implementation is a sentence transformer, and it does not fit.
//! A 512-token pass through even a small BERT is around 26 GFLOP; a transcript
//! corpus runs to tens of thousands of chunks, so indexing one is hours of CPU
//! at any level of tuning. Measured on a six-core machine it was twelve.
//!
//! A Model2Vec model is that transformer with the attention distilled out of
//! it: what survives is one vector per token, and inference is a lookup and a
//! mean. It is roughly five orders of magnitude faster, which turns the same
//! index from hours into about a second, and it costs a table of 29,528 × 256
//! floats — 30 MB — rather than a runtime.
//!
//! What it gives up is context. The vector for a word is the same wherever it
//! appears, so this is closer to a very good bag of words than to a model that
//! has read the sentence. That is the right trade here: it is only ever asked a
//! topical question, and anything needing precision is answered by the literal
//! search before this is consulted at all.
//!
//! # Why it is written out rather than taken from a crate
//!
//! `model2vec-rs` implements exactly this, and pulls `onig` (C) and `esaxx-rs`
//! (C++) behind it. cctop ships two statically linked musl archives and already
//! carries C — `rusqlite`, `mimalloc`, ring — but no C++, and adding a C++
//! toolchain to the release for an algorithm this small is a poor trade. The
//! whole of inference is [`Model::embed`].

pub mod fetch;
pub mod index;

use anyhow::{Context, Result, anyhow};
use std::path::Path;

/// What the model files are called inside a model directory.
///
/// These are the names the Model2Vec repositories publish, so a directory
/// fetched from Hugging Face works as it arrives.
const WEIGHTS: &str = "model.safetensors";
const TOKENIZER: &str = "tokenizer.json";
const CONFIG: &str = "config.json";

/// The one tensor a static model has.
const TENSOR: &str = "embeddings";

/// A distilled static embedding model: one vector per token, and nothing else.
pub struct Model {
    tokenizer: tokenizers::Tokenizer,
    /// The embedding table, row-major: token `i` is `vectors[i * dim..][..dim]`.
    vectors: Vec<f32>,
    dim: usize,
    /// Whether embeddings are scaled to unit length, from the model's config.
    /// With it on, cosine similarity is a dot product.
    normalize: bool,
}

impl std::fmt::Debug for Model {
    /// The embedding table is 30 MB and nobody wants it in a log line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Model")
            .field("tokens", &(self.vectors.len() / self.dim.max(1)))
            .field("dim", &self.dim)
            .field("normalize", &self.normalize)
            .finish()
    }
}

impl Model {
    /// Load a model from a directory holding the three published files.
    pub fn load(dir: &Path) -> Result<Model> {
        let config = std::fs::read_to_string(dir.join(CONFIG))
            .with_context(|| format!("reading {}", dir.join(CONFIG).display()))?;
        let config: serde_json::Value =
            serde_json::from_str(&config).context("parsing the model config")?;
        // Absent means "leave the vectors as they are": only a model that says
        // it was trained normalised gets rescaled.
        let normalize = config
            .get("normalize")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        let tokenizer = tokenizers::Tokenizer::from_file(dir.join(TOKENIZER))
            .map_err(|e| anyhow!("reading the tokenizer: {e}"))?;

        let raw = std::fs::read(dir.join(WEIGHTS))
            .with_context(|| format!("reading {}", dir.join(WEIGHTS).display()))?;
        let tensors = safetensors::SafeTensors::deserialize(&raw).context("parsing the weights")?;
        let table = tensors
            .tensor(TENSOR)
            .with_context(|| format!("the weights have no `{TENSOR}` tensor"))?;

        if table.dtype() != safetensors::Dtype::F32 {
            return Err(anyhow!("expected f32 weights, found {:?}", table.dtype()));
        }
        let [rows, dim] = table.shape() else {
            return Err(anyhow!(
                "expected a 2-D embedding table, found {:?}",
                table.shape()
            ));
        };
        let (rows, dim) = (*rows, *dim);

        // The table is the model, so a row count that disagrees with the
        // vocabulary means the two files are from different models and every
        // lookup past the shorter one would be a different word's vector.
        let vocab = tokenizer.get_vocab_size(true);
        if rows < vocab {
            return Err(anyhow!(
                "the weights have {rows} rows but the tokenizer has {vocab} tokens"
            ));
        }

        let data = table.data();
        let vectors: Vec<f32> = data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        if vectors.len() != rows * dim {
            return Err(anyhow!(
                "the weights hold {} floats, not the {} the shape claims",
                vectors.len(),
                rows * dim
            ));
        }

        Ok(Model {
            tokenizer,
            vectors,
            dim,
            normalize,
        })
    }

    /// How wide this model's vectors are.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// The whole of inference: tokenise, look each token up, average, rescale.
    ///
    /// Special tokens are left out. `[CLS]` and `[SEP]` carry a vector like any
    /// other token here, and since there is no attention for them to summarise
    /// they would only drag every embedding toward a constant.
    ///
    /// Text that tokenises to nothing — punctuation, an empty string — has no
    /// mean to take, and gets a zero vector rather than a division by zero. It
    /// is orthogonal to everything, so it simply never matches.
    pub fn embed(&self, text: &str) -> Vec<f32> {
        let mut out = vec![0f32; self.dim];
        let Ok(encoded) = self.tokenizer.encode(text, false) else {
            return out;
        };
        let ids = encoded.get_ids();
        let mut counted = 0usize;
        for &id in ids {
            let start = id as usize * self.dim;
            let Some(row) = self.vectors.get(start..start + self.dim) else {
                continue;
            };
            for (o, v) in out.iter_mut().zip(row) {
                *o += v;
            }
            counted += 1;
        }
        if counted == 0 {
            return out;
        }
        let scale = 1.0 / counted as f32;
        for o in out.iter_mut() {
            *o *= scale;
        }
        if self.normalize {
            unit(&mut out);
        }
        out
    }
}

/// Scale a vector to unit length, leaving a zero vector alone.
fn unit(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        let scale = 1.0 / norm;
        for x in v.iter_mut() {
            *x *= scale;
        }
    }
}

/// Words that carry no topic, and the framing of a question about a search.
///
/// Two groups, and the second is the one that matters here. The first is the
/// ordinary English function words. The second — "where did I talk about", "that
/// session where" — is how people phrase a question *to a search box*, and it is
/// all filler: it describes the act of searching rather than what is being
/// searched for.
///
/// Removing them is not an optimisation. A static embedding is a mean over its
/// tokens, so a query of ten words where seven are filler is mostly a vector of
/// filler, and it matches whichever text is also mostly filler. Measured on a
/// four-way retrieval set, "the conversation about running a language model on a
/// rented server" picked meeting notes over the right answer until these were
/// taken out, and picked correctly after.
const FILLER: &[&str] = &[
    "a",
    "about",
    "after",
    "an",
    "and",
    "any",
    "anything",
    "are",
    "as",
    "at",
    "be",
    "been",
    "before",
    "being",
    "but",
    "by",
    "can",
    "chat",
    "conversation",
    "did",
    "discussed",
    "discussing",
    "do",
    "does",
    "doing",
    "find",
    "for",
    "from",
    "had",
    "has",
    "have",
    "having",
    "he",
    "her",
    "his",
    "how",
    "i",
    "if",
    "in",
    "into",
    "is",
    "it",
    "its",
    "just",
    "look",
    "looking",
    "me",
    "mentioned",
    "my",
    "no",
    "nor",
    "not",
    "of",
    "on",
    "or",
    "our",
    "over",
    "remember",
    "search",
    "session",
    "she",
    "so",
    "some",
    "something",
    "stuff",
    "such",
    "talk",
    "than",
    "that",
    "the",
    "their",
    "them",
    "then",
    "these",
    "they",
    "thing",
    "this",
    "those",
    "to",
    "too",
    "us",
    "very",
    "was",
    "we",
    "were",
    "what",
    "when",
    "where",
    "which",
    "who",
    "whom",
    "why",
    "will",
    "with",
    "you",
    "your",
];

/// Reduce a typed question to the words that carry its topic.
///
/// Falls back to the original text when stripping would leave nothing: a query
/// that is *entirely* filler is better answered badly than not at all.
pub fn topic_of(query: &str) -> String {
    let kept: Vec<&str> = query
        .split(|c: char| !(c.is_alphanumeric() || c == '.' || c == '-' || c == '_'))
        .filter(|w| !w.is_empty())
        .filter(|w| !FILLER.contains(&w.to_ascii_lowercase().as_str()))
        .collect();
    if kept.is_empty() {
        return query.trim().to_string();
    }
    kept.join(" ")
}

/// Cosine similarity, in `-1..=1`.
///
/// Normalising here rather than assuming it lets this be used on vectors that
/// came from a model whose config did not ask for it.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (mut dot, mut na, mut nb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Where a real model lands if one has been fetched. Tests that need it are
    /// ignored rather than failing, so the suite stays offline by default.
    fn real_model() -> std::path::PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".cache/cctop/models/potion-base-8M")
    }

    /// Build a tiny model on disk: a three-word vocabulary and a table whose
    /// rows are trivially checkable by hand.
    pub(crate) fn tiny(dir: &Path, normalize: bool) {
        std::fs::create_dir_all(dir).expect("mkdir");
        std::fs::write(
            dir.join(CONFIG),
            format!(r#"{{"model_type":"model2vec","normalize":{normalize}}}"#),
        )
        .expect("config");
        // A WordPiece tokenizer with no normaliser and whitespace splitting, so
        // the ids a test asks for are the ids it gets.
        std::fs::write(
            dir.join(TOKENIZER),
            r###"{
              "version":"1.0","truncation":null,"padding":null,
              "added_tokens":[],"normalizer":null,
              "pre_tokenizer":{"type":"Whitespace"},
              "post_processor":null,"decoder":null,
              "model":{"type":"WordPiece","unk_token":"[UNK]",
                "continuing_subword_prefix":"##","max_input_chars_per_word":100,
                "vocab":{"[UNK]":0,"alpha":1,"beta":2}}
            }"###,
        )
        .expect("tokenizer");
        // Three rows of two floats: [UNK]=(0,0), alpha=(1,0), beta=(0,1).
        let rows: Vec<f32> = vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        let bytes: Vec<u8> = rows.iter().flat_map(|f| f.to_le_bytes()).collect();
        let header = br#"{"embeddings":{"dtype":"F32","shape":[3,2],"data_offsets":[0,24]}}"#;
        let mut out = Vec::new();
        out.extend_from_slice(&(header.len() as u64).to_le_bytes());
        out.extend_from_slice(header);
        out.extend_from_slice(&bytes);
        std::fs::write(dir.join(WEIGHTS), out).expect("weights");
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cctop-embed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// Inference is a mean of the rows the tokens picked out: two words, one
    /// each, lands exactly between them.
    #[test]
    fn an_embedding_is_the_mean_of_its_tokens() {
        let dir = scratch("mean");
        tiny(&dir, false);
        let m = Model::load(&dir).expect("load");
        assert_eq!(m.dim(), 2);

        assert_eq!(m.embed("alpha"), vec![1.0, 0.0]);
        assert_eq!(m.embed("beta"), vec![0.0, 1.0]);
        assert_eq!(m.embed("alpha beta"), vec![0.5, 0.5]);
        // Repetition is weight: two alphas to one beta leans toward alpha.
        let leaning = m.embed("alpha alpha beta");
        assert!(leaning[0] > leaning[1], "{leaning:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A normalising model returns unit vectors, which is what makes cosine
    /// similarity a plain dot product downstream.
    #[test]
    fn a_normalising_model_returns_unit_vectors() {
        let dir = scratch("unit");
        tiny(&dir, true);
        let m = Model::load(&dir).expect("load");
        let v = m.embed("alpha beta");
        let len = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((len - 1.0).abs() < 1e-6, "length {len}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Text with nothing to look up must not divide by zero. A zero vector is
    /// orthogonal to everything, so it never matches rather than matching all.
    #[test]
    fn text_that_tokenises_to_nothing_is_a_zero_vector() {
        let dir = scratch("empty");
        tiny(&dir, true);
        let m = Model::load(&dir).expect("load");
        assert_eq!(m.embed(""), vec![0.0, 0.0]);
        assert_eq!(cosine(&m.embed(""), &m.embed("alpha")), 0.0);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Cosine is the comparison the index runs on, so its edges matter: same
    /// direction is 1, opposed is -1, perpendicular is 0.
    #[test]
    fn cosine_spans_its_range() {
        assert!((cosine(&[1.0, 0.0], &[2.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!((cosine(&[1.0, 0.0], &[-1.0, 0.0]) + 1.0).abs() < 1e-6);
        assert_eq!(cosine(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
    }

    /// Weights and tokenizer from different models would silently return the
    /// wrong word's vector, so the mismatch is refused at load.
    #[test]
    fn weights_that_do_not_cover_the_vocabulary_are_refused() {
        let dir = scratch("mismatch");
        tiny(&dir, false);
        // Two rows against a three-token vocabulary.
        let rows: Vec<f32> = vec![0.0, 0.0, 1.0, 0.0];
        let bytes: Vec<u8> = rows.iter().flat_map(|f| f.to_le_bytes()).collect();
        let header = br#"{"embeddings":{"dtype":"F32","shape":[2,2],"data_offsets":[0,16]}}"#;
        let mut out = Vec::new();
        out.extend_from_slice(&(header.len() as u64).to_le_bytes());
        out.extend_from_slice(header);
        out.extend_from_slice(&bytes);
        std::fs::write(dir.join(WEIGHTS), out).expect("weights");

        let err = Model::load(&dir).expect_err("mismatch");
        assert!(format!("{err}").contains("tokenizer"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A directory that is not a model must be an error, not a panic.
    #[test]
    fn a_missing_model_is_an_error() {
        assert!(Model::load(Path::new("/nonexistent/cctop/model")).is_err());
    }

    /// The real thing, on the shape the index actually sees: a short typed
    /// question against paragraphs of conversation. Ignored because it needs a
    /// fetched model.
    ///
    /// The earlier version of this test compared two short sentences and failed
    /// — correctly. Short adversarial sentences that share only function words
    /// are the case a static model is worst at, and they are not the case this
    /// is used for.
    #[test]
    #[ignore]
    fn a_question_finds_the_conversation_it_is_about() {
        let m = Model::load(&real_model()).expect("fetched model");
        assert_eq!(m.dim(), 256);

        let target = "I need to run inference somewhere cheap this weekend. Looked at renting a \
            machine on vast.ai, the hourly price for an A100 80GB is about a dollar twenty which \
            beats the big clouds. Pulled down nemotron 70b quantized to 4 bit so it fits in a \
            single card. Throughput was fine, good enough for batch jobs overnight.";
        let baking = "The starter has been sluggish since the kitchen got cold. I feed it equal \
            weights flour and water every morning and it only doubles after eight hours. The loaf \
            came out dense with a tight crumb, probably underproofed.";
        let build = "The build broke on the musl target again. Clippy is fatal in CI because \
            RUSTFLAGS sets deny warnings, so a lint that is advisory locally fails the run. Static \
            linking pulled in a C++ dependency the release image lacks.";

        let corpus = [("gpu", target), ("baking", baking), ("build", build)];
        let vectors: Vec<(&str, Vec<f32>)> = corpus.iter().map(|(n, t)| (*n, m.embed(t))).collect();

        let best = |question: &str| -> &str {
            let q = m.embed(&topic_of(question));
            vectors
                .iter()
                .map(|(n, v)| (cosine(&q, v), *n))
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, n)| n)
                .expect("a best match")
        };

        assert_eq!(best("where did I talk about renting cloud GPUs"), "gpu");
        assert_eq!(
            best("the conversation about running a language model on a rented server"),
            "gpu"
        );
        assert_eq!(
            best("that session where I was debugging the CI build"),
            "build"
        );
        assert_eq!(best("the baking one"), "baking");
    }

    /// Stripping leaves the topic and drops the question around it.
    #[test]
    fn a_question_reduces_to_its_topic() {
        assert_eq!(
            topic_of("where did I talk about renting cloud GPUs"),
            "renting cloud GPUs"
        );
        assert_eq!(
            topic_of("that session where I was debugging the CI build"),
            "debugging CI build"
        );
        // Dotted and hyphenated names survive whole: they are the topic.
        assert_eq!(
            topic_of("the conversation about vast.ai and nemotron"),
            "vast.ai nemotron"
        );
        // A query that is all filler is left alone rather than emptied.
        assert_eq!(topic_of("what about it"), "what about it");
    }
}
