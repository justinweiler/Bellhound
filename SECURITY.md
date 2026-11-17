# Security

## Reporting

Please report suspected vulnerabilities through GitHub's
[private vulnerability reporting form](https://github.com/justinweiler/Bellhound/security/advisories/new).
Do not open a public issue for a security vulnerability or include secret material in a report.
Redact API keys, relay tokens, telephone numbers, account identifiers, private URLs, call-control
identifiers, and real-call transcripts before submitting supporting evidence.

## Current boundaries

- API keys belong in an environment variable or operating-system credential store. Bellhound never prints them.
- Relay tokens belong in the configured environment variable and must contain at least 24 characters.
- `relay.bind` and `relay.control_bind` must stay on loopback. Expose only the Conversation Relay and webhook paths through a TLS tunnel or reverse proxy.
- The private control listener requires the relay token, but local malware running as the same user remains outside Bellhound's threat model.
- The prototype authenticates public relay paths with an unguessable URL token. Public deployments should additionally verify Telnyx webhook signatures and use a durable TLS endpoint.
- A verified destination number does not prove who answered. Applications requiring caller identity should add an independent challenge.
- Voice input is remote user input. It must not bypass the originating agent runtime's approval rules.
- The MCP server is stdio-only. Keep `relay.control_bind` on loopback and let an approved private tunnel reach the MCP process rather than the control port.
- Private MCP event results omit call-control and session identifiers, but conversation text crosses the MCP boundary because the originating model needs it. Apply the target workspace's data-handling policy.
- A Secure MCP Tunnel runtime key should have only Tunnels Read + Use. Store it outside repository files and process arguments, independently from the Telnyx API key. A tunnel association and local process boundary are not an end-user OAuth design and are insufficient for a public plugin.
- MCP request IDs provide retry suppression, not caller identity or action approval. The originating runtime keeps its normal confirmation requirements.
- The macOS LaunchAgent contains paths and restart policy only. Its supervisor resolves the tunnel key and relay token from Keychain at startup and does not write them to the plist, process arguments, service state, or logs.
- `relay.max_call_seconds` is validated between 30 and 3600 seconds and is enforced independently by Telnyx, Bellhound's relay timer, and the optional service watchdog. Keep it aligned with the intended cost ceiling.

Do not attach logs or configuration files to a report until telephone numbers, account identifiers, URLs, and tokens have been removed.
