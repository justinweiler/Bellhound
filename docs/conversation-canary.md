# Bellhound conversation canary

This canary tests turn boundaries, acknowledgements, tool use, stale-response protection, and cleanup without requiring Justin to improvise a script.

## Before the call

- Use an existing session that already has the relevant context, tools, and Bellhound access.
- Do not create a generic call-only session.
- The agent completes the foreground-owner survival checks in the operator reference.
- Justin disables Focus or other call screening and waits for the Bellhound opening to finish before speaking.

## Justin's script

Speak naturally. The pauses below are deliberate.

1. **Basic turn:** “First test. Please repeat the phrase magic sauce.”
2. **Continuation turn:** “Second test. Please remember purple” — pause for about half a second — “crocodile” — pause again — “and repeat the complete phrase.”
3. **Tool turn:** “Third test. Check the current time in Los Angeles and tell me the hour and minute.”
4. **Conversation advance:** Immediately after Bellhound says “Got it. Give me a moment,” say: “Change of plan. Do not tell me the time. What was the phrase from the first test?”
5. **Closing:** “Thank you, goodbye.”

## Expected behavior

- Each numbered item produces one final `utterance` event.
- The continuation turn reports `fragment_count` greater than one when Telnyx splits it, while its text remains one coherent phrase.
- The holding acknowledgement occurs at most once per substantive turn.
- The closing receives no holding acknowledgement.
- The tool result is obtained and spoken while the same context-rich session remains on the call.
- A reply targeting an older sequence is rejected with `stale_response`; the agent drains newer events and answers the newest applicable sequence.
- The call ends with `ended`, the foreground owner exits zero, all runtime processes stop, and Telnyx reports the telephone leg inactive.

## Evidence to retain

Record event IDs, sequence numbers, `fragment_count`, acknowledgement timing, response latency, stale-response errors, owner exit status, final process state, and final carrier state. Do not retain phone numbers, private identifiers, temporary URLs, credentials, or real-call transcript text in the report.
