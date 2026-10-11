# Security

## Reporting

Until a public repository and private vulnerability-reporting channel exist, do not publish suspected vulnerabilities or secret material. Contact the maintainers privately. This section will be updated with the repository's private advisory link before the first public release.

## Current boundaries

- API keys belong in an environment variable or operating-system credential store. Bellhound never prints them.
- Relay tokens belong in the configured environment variable and must contain at least 24 characters.
- `relay.bind` and `relay.control_bind` must stay on loopback. Expose only the Conversation Relay and webhook paths through a TLS tunnel or reverse proxy.
- The private control listener requires the relay token, but local malware running as the same user remains outside Bellhound's threat model.
- The prototype authenticates public relay paths with an unguessable URL token. Public deployments should additionally verify Telnyx webhook signatures and use a durable TLS endpoint.
- A verified destination number does not prove who answered. Applications requiring caller identity should add an independent challenge.
- Voice input is remote user input. It must not bypass the originating agent runtime's approval rules.

Do not attach logs or configuration files to a report until telephone numbers, account identifiers, URLs, and tokens have been removed.
