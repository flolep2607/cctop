//! Getting the embedding model onto the machine.
//!
//! # Why this is a command and not a side effect
//!
//! cctop is one binary that reads files already on the disk, and the topical
//! search is the first thing in it that wants thirty megabytes from the
//! network. Fetching that quietly the first time someone typed a long query
//! would be a surprise of exactly the kind this program does not spring: a
//! dashboard that reaches for the internet on its own, on a machine that may
//! be offline, behind a proxy, or metered.
//!
//! So nothing downloads unless asked. `cctop --fetch-search-model` gets the
//! model; until it has been run, the topical tier is simply absent and the
//! literal search answers everything, which is what it did before this existed.
//!
//! # Why this model
//!
//! `potion-base-8M` is a Model2Vec distillation: a table of token vectors with
//! the transformer that produced them left behind. See [`super`] for why that
//! architecture rather than a sentence transformer.

use anyhow::{Context, Result, bail};
use std::io::Read as _;
use std::path::PathBuf;

/// The repository the three files come from.
const REPO: &str = "minishlab/potion-base-8M";

/// Files that make a model, and the ceiling each is allowed to occupy.
///
/// The caps are a guard against a redirect to something that is not the file
/// asked for — an interception page, an error body — rather than a tuned limit.
/// The weights are about 30 MB and the tokenizer under one.
const FILES: &[(&str, u64)] = &[
    ("model.safetensors", 128 * 1024 * 1024),
    ("tokenizer.json", 8 * 1024 * 1024),
    ("config.json", 64 * 1024),
];

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .user_agent(concat!("cctop/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Where the model is unpacked.
pub fn model_dir() -> PathBuf {
    crate::config::EMBEDDING_MODEL_DIR.join("potion-base-8M")
}

/// Whether a usable model is already here.
///
/// Presence of all three files, not a successful load: this is asked on the
/// hot path to decide whether the topical tier exists at all, and reading
/// thirty megabytes to answer it would defeat the point.
pub fn present() -> bool {
    let dir = model_dir();
    FILES.iter().all(|(name, _)| dir.join(name).exists())
}

/// Download the model, unless it is already here.
pub fn fetch() -> Result<PathBuf> {
    let dir = model_dir();
    if present() {
        println!("Search model already present at {}.", dir.display());
        return Ok(dir);
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;

    println!("Fetching the search model ({REPO})…");
    for (name, cap) in FILES {
        let url = format!("https://huggingface.co/{REPO}/resolve/main/{name}");
        let mut body = Vec::new();
        agent()
            .get(&url)
            .call()
            .with_context(|| format!("could not download {name}"))?
            .body_mut()
            .as_reader()
            .take(*cap + 1)
            .read_to_end(&mut body)
            .with_context(|| format!("could not read {name}"))?;
        if body.len() as u64 > *cap {
            bail!("{name} is larger than expected; refusing it");
        }
        if body.is_empty() {
            bail!("{name} came back empty");
        }
        // Written beside and renamed, so an interrupted fetch never leaves a
        // half-written file that `present` would then call a model.
        let target = dir.join(name);
        let tmp = target.with_extension("part");
        std::fs::write(&tmp, &body).with_context(|| format!("could not write {name}"))?;
        std::fs::rename(&tmp, &target).with_context(|| format!("could not place {name}"))?;
        println!("  {name}  {:.1} MB", body.len() as f64 / 1_048_576.0);
    }

    // Proof rather than optimism: a set of files that will not load is worse
    // than none, because the search would fail on every query instead of
    // saying the model is missing.
    super::Model::load(&dir).context("the downloaded model did not load")?;
    println!("Search model ready at {}.", dir.display());
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// `present` is about all three files, so a partial directory is not a
    /// model — which is what the `.part` rename protects against.
    #[test]
    fn a_partial_download_is_not_a_model() {
        let dir = std::env::temp_dir().join("cctop-fetch-partial");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("config.json"), "{}").expect("write");

        let complete = |d: &Path| FILES.iter().all(|(n, _)| d.join(n).exists());
        assert!(!complete(&dir), "one file of three is not a model");
        std::fs::write(dir.join("tokenizer.json"), "{}").expect("write");
        assert!(!complete(&dir), "two of three is not a model either");
        std::fs::write(dir.join("model.safetensors"), "").expect("write");
        assert!(complete(&dir));
        let _ = std::fs::remove_dir_all(dir);
    }
}
