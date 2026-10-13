# Cora pilot

Cora is Bellhound's first autonomous-session pilot. Justin explicitly identified Bellhound as an appropriate way for Cora's always-on dot session to reach him when her existing autonomy supports an alert or she needs synchronous help.

Route each call through the session that owns the reason for the call and already has the best available context, tools, and local Bellhound access. The selected session remains the reasoning agent for the full call and identifies itself honestly; Cora may supervise the routing without impersonating that session.

## Session selection

Use this order:

1. If a specific existing work session discovered the need or is blocked on Justin, that session calls.
2. If Cora's dot session discovered an independent alert and no other session owns it, Cora calls directly from the dot session. A Slack message such as “Brendon has a question for you” belongs to Cora when her monitoring found it; her opening should identify the sender, channel, urgency, and concrete question when known.
3. If the best session cannot use Bellhound, Cora may call from the dot session as a notifier. She must state what she knows, what she cannot inspect or do from that session, and where the fuller context lives.
4. Create a new session only when the underlying work should become a durable, separately owned task and that session can be seeded with the necessary source context and tools. Never create a session merely to proxy a phone call.

When Cora owns the call, she keeps the Bellhound wait, reasoning, tool use, and speech loop in the dot session. If Justin asks her to inspect the Slack thread or perform an ordinary follow-up and the dot session has the appropriate tool and authority, she does it there and reports the actual result on the call.

## Good reasons to call

- A time-sensitive condition merits interruption and Cora has standing authority to alert Justin.
- Cora is performing a long-running task and reaches a concrete decision that materially benefits from synchronous human judgment.
- Justin has said he will be away from the text interface but reachable by phone for the active work.

Use ordinary text notification when the matter is informational, can wait, or can be stated clearly without dialogue.

## Call shape

- Identify yourself immediately: `Cora here.`
- State the reason in one sentence.
- Ask one concrete question or report one actionable condition.
- Keep tool-result responses factual and brief.
- End the call cleanly when the purpose is satisfied.

Do not use a phone answer as identity proof for privileged actions. Do not infer consent from silence, voicemail, background speech, or a disconnected call. One unanswered attempt is enough unless Justin explicitly requested retries.

## Operational baseline

The first Cora pilot exposed an execution-owner lifetime defect in the retired detached launcher. The current foreground `session.sh run` workflow owns the relay and tunnel for the full call and has passed repeated live same-session canaries. Cora must still complete the multi-turn survival check in the operator reference, retain the foreground execution session, and verify complete cleanup after every call.
