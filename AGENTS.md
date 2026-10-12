# Bellhound repository instructions

Bellhound is the personal project of Justin Weiler and Iris. The repository's origin and division of contribution are recorded in `AUTHORS.md`; preserve that account when changing project metadata, packaging, or public documentation.

## Authority and stewardship

- Iris has standing authority from Justin to maintain this repository and its online home: inspect, edit, test, branch, commit, push, tag, package, and prepare releases as the work requires.
- Keep consequential external actions grounded in reviewable source and verification. Discuss meaningful cost, destructive data changes, credential rotation, legal changes, or an irreversible public release when its scope is unclear.
- Work in the primary checkout by default. Preserve unrelated changes and avoid linked worktrees unless a concrete conflict requires one and Justin approves it.

## Git identity

- Use `Justin Weiler <justin@justinweiler.com>` as both author and committer for Bellhound commits.
- Use ordinary professional commit subjects. Do not add model-name or model-version prefixes.

## Engineering contract

- Keep the core model-provider neutral. Bellhound must not require a particular LLM SDK.
- Preserve both integration surfaces: JSON Lines over a long-lived process and cursor-based blocking CLI commands.
- Keep the public relay listener and private control listener separate and loopback-bound.
- Never commit API keys, relay tokens, telephone numbers, private Call Control identifiers, temporary tunnel URLs, or real-call transcripts.
- Update protocol, architecture, live-setup, changelog, and security documentation when their contracts change.

## Verification

Before committing behavior changes, run:

```sh
cargo fmt --all -- --check
cargo test --all-targets --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo build --release --locked
```

For packaging or workflow changes, also run the applicable ShellCheck, PowerShell parser, `actionlint`, archive checksum, Cargo package, and secret-scan checks documented by the existing release process.
