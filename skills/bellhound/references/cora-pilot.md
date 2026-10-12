# Cora pilot

Cora is Bellhound's first autonomous-session pilot. Justin explicitly identified Bellhound as an appropriate way for Cora's always-on dot session to reach him when her existing autonomy supports an alert or she needs synchronous help.

Route each call through an existing session that already has the relevant context, tools, and local Bellhound access. Do not create a generic call-only proxy. The selected session remains the reasoning agent for the full call and identifies itself honestly; Cora may supervise the routing without impersonating that session.

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

## Repeat pilot gate

The first Cora pilot exposed an execution-owner lifetime defect: a detached launcher returned, then both relay and temporary tunnel disappeared without terminal events while the telephone leg remained alive. Before the authorized repeat call, Cora must use the foreground `session.sh run` workflow and complete the multi-turn survival check in the operator reference. Iris owns the code repair; Cora owns the single repeat call after receiving Iris's verified handoff.
