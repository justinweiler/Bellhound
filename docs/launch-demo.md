# One-minute public demonstration

This recipe demonstrates Bellhound's defining property: the agent already doing
the work remains the reasoning agent during the phone call. Bellhound supplies
telephone transport, speech-to-text events, and text-to-speech commands; it does
not insert a proxy model.

## Safety boundary

- Use a dedicated demonstration configuration and a verified destination owned by the caller.
- Keep Telnyx, tunnel, Keychain, environment, and configuration screens out of the recording.
- Do not show telephone numbers, API keys, relay tokens, account identifiers, private URLs,
  call-control identifiers, service receipts, or an unredacted real-call transcript.
- Keep the configured call ceiling enabled. End the call explicitly and confirm Bellhound returns idle.
- Record only participants who have agreed to the demonstration.

## Scenario

Give the active agent a small task and this standing instruction:

> Work on this in the current session. If you need a human decision, call me through Bellhound.

Use a harmless decision the agent cannot infer, such as choosing between two names
for a sample file. During the call, ask the agent to perform one safe local lookup
before answering. This proves that the same context-rich session can hear, use an
ordinary tool, and speak the result back.

## Shot list

1. **0–8 seconds:** Show Bellhound's logo, repository URL, and the sentence
   “The agent already doing the work can call you.”
2. **8–18 seconds:** Show the active session receiving the task and the permission to call.
   Crop the frame to the task and agent response.
3. **18–28 seconds:** Show the agent invoking Bellhound, then cut to the incoming contact
   named Bellhound on the telephone. Do not show the telephone number.
4. **28–47 seconds:** Answer and have the short two-way exchange. Include one request that
   requires the originating agent to consult its current task or use a harmless local tool.
5. **47–55 seconds:** Let the complete spoken response finish, then show normal hangup.
6. **55–60 seconds:** Return to the repository URL and the words
   “Rust · model-neutral · JSONL · CLI · MCP · MIT.”

## Acceptance checklist

- Exactly one outbound call was placed.
- The first spoken opening was audible.
- Caller speech appeared once as a final Bellhound turn.
- The originating session answered from its existing context.
- Any holding cue stopped before the substantive answer.
- Final playback completed before hangup.
- The call ended normally and final status returned healthy and idle.
- The published recording contains none of the sensitive fields listed above.

Run the deterministic offline simulation first if the live path has changed since
the last canary. The live recording is demonstration evidence, not a replacement
for the platform acceptance record in [platform-validation.md](platform-validation.md).
