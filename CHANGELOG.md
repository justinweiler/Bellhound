# Changelog

## 0.2.0 - 2026-10-10

- Add a separate loopback-only control listener.
- Add blocking, cursor-based `wait` for event-driven model turns.
- Add one-shot `speak`, `hangup`, and `ping` CLI commands.
- Retain the most recent 256 events with process-local event IDs.
- Warm the Telnyx greeting path with a short audible primer, then dispatch the queued call-specific opening as a measured speech frame after relay setup.
- Document the model-neutral adapter contract and project authorship.

## 0.1.0 - 2026-10-09

- Add outbound Telnyx dialing with embedded Conversation Relay.
- Bridge final STT prompts and TTS text over JSON Lines.
- Resolve API keys through environment variables or operating-system credential stores.
- Add deterministic simulation, duration limits, structured errors, and Call Control hangup.
