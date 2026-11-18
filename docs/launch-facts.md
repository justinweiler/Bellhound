# Public launch fact sheet

This is a factual source for human-written announcements and interviews. It is not
paste-ready promotional copy. In particular, Hacker News requires submission and
comment text to be written by the human posting it rather than generated or AI-edited.

## One sentence

Bellhound is a model-neutral Rust telephone bridge that lets the AI agent already
working in a context-rich session call a human, hear the answer, use other tools,
and speak back without handing the conversation to an intermediary model.

## What is distinctive

- The originating agent remains the reasoning agent across telephone turns.
- Bellhound contains no language model and makes no decisions.
- Integrations can use JSON Lines, blocking cursor-based CLI commands, or five focused MCP tools.
- The same core targets macOS, Linux, and Windows; the packaged persistent service is currently macOS-only.
- Telnyx provides the telephone, speech-to-text, and text-to-speech path.
- Credentials stay in environment variables or operating-system credential stores.
- Carrier, relay, and optional service-watchdog ceilings independently bound call duration.
- Bellhound is MIT licensed and implemented in Rust.

## Demonstrated behavior

- A durable, context-rich agent session placed and completed a multi-turn call.
- During an MCP canary, that same session used an ordinary tool while the call remained active.
- Continuation-aware speech buffering handled a deliberate three-second pause without interrupting the caller.
- Playback-safe hangup allowed the final spoken response to finish before ending the telephone leg.
- Tunnel freshness and bounded recovery prevent a stale hosted path from becoming a blind dial attempt.

## Honest limitations

- A Telnyx account, telephone number or verified destination, and public WSS relay are required for live calls.
- Prebuilt binaries are checksummed but are not yet Apple-notarized or Windows code-signed.
- Linux and Windows do not yet ship persistent-service installers.
- Windows still needs broader clean-machine human acceptance for audible playback, Credential Manager onboarding,
  SmartScreen behavior, agent-skill installation, and persistent service operation.
- Bellhound does not grant an agent authority that its originating runtime does not already have.

## Useful links

- Repository: <https://github.com/justinweiler/Bellhound>
- Release: <https://github.com/justinweiler/Bellhound/releases/tag/v0.6.6>
- crates.io: <https://crates.io/crates/bellhound>
- MCP Registry: <https://registry.modelcontextprotocol.io/?q=io.github.justinweiler%2Fbellhound>
- Installation: <https://github.com/justinweiler/Bellhound/blob/master/INSTALL.md>
- Architecture: <https://github.com/justinweiler/Bellhound/blob/master/docs/architecture.md>
- MCP integration: <https://github.com/justinweiler/Bellhound/blob/master/docs/mcp.md>
- Platform validation: <https://github.com/justinweiler/Bellhound/blob/master/docs/platform-validation.md>
- Security policy: <https://github.com/justinweiler/Bellhound/blob/master/SECURITY.md>
