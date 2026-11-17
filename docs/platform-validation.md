# Platform validation

This page records what has been exercised on real operating systems and keeps
packaging proof separate from live telephone acceptance.

## Automated release gates

Every release archive is built natively for its target, checksum-verified,
extracted, and inspected for the executable, documentation, examples, scripts,
and skill. The extracted package must then:

1. report its version;
2. validate `bellhound.example.toml`;
3. complete the packaged three-turn simulation through
   `examples/crocodile-call.jsonl`; and
4. initialize the packaged stdio MCP server and discover all five Bellhound
   tools through `examples/mcp-status-only.toml`.

These gates prove offline package integrity. They do not spend Telnyx credit or
prove carrier audio.

## Linux

The x86-64 glibc archive was exercised on Ubuntu 24.04. Checksum verification,
the packaged binary, configuration validation, environment and Secret Service
credential resolution, offline simulation, MCP discovery, and isolated Codex
skill installation passed. Live Telnyx testing completed a multi-turn two-way
conversation and explicit hangup. A separate voicemail call exposed suppression
cases corrected in 0.6.5.

Bellhound does not yet ship a Linux systemd installer. Operators own the relay,
TLS tunnel, and optional MCP host process lifecycle.

## Windows

The x86-64 MSVC archive was exercised on Windows. The binary, configuration
validation, missing-credential diagnostic, environment credential resolution,
and stdio MCP initialization/tool discovery passed. One authorized live call
connected and delivered caller speech to Bellhound; an agent reply was submitted,
but audible playback was not independently confirmed before hangup. Treat that as
a transport partial pass rather than complete two-way audio acceptance.

Windows PowerShell onboarding and packaged offline examples are documented and
enforced by the release workflow as of 0.6.5. Human acceptance still remains for:

- a complete packaged multi-turn simulation on a clean Windows machine;
- Credential Manager onboarding on that clean machine;
- interruption and repeated-turn behavior;
- Codex skill installation and shell compatibility;
- SmartScreen behavior once signed binaries exist; and
- any future Windows service installer.

Bellhound does not yet ship a Windows Service installer, and release executables
are not yet Authenticode-signed.
