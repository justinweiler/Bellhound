# Contributing to Bellhound

Bellhound welcomes focused issues and pull requests.

## Development

Install stable Rust, clone the repository, and run:

```sh
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Keep provider-specific behavior behind a transport boundary. The JSON Lines and blocking CLI contracts must remain usable without a particular language-model SDK.

Never commit telephone numbers, API keys, relay tokens, Call Control IDs, tunnel URLs tied to a private deployment, or transcripts from real calls. Use the documented placeholders and deterministic simulator fixtures.

Changes to the command or event schema need corresponding protocol documentation and process-level integration coverage.
