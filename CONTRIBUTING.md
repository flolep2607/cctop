# Contributing to cctop

Thanks for helping improve cctop. Please keep changes focused and include tests
when behaviour changes.

## Development

Build and run the test suite with Rust 1.88 or newer:

```bash
cargo test
cargo clippy --all-targets
```

Before opening a pull request, also check formatting:

```bash
cargo fmt --all -- --check
```

## Releasing

Change the package `version` in `Cargo.toml` and push that commit to `main`.
GitHub Actions derives the matching `v<version>` tag, creates the GitHub
release, builds the platform archives, and publishes the crate. Do not create a
separate release tag for a normal version bump.
