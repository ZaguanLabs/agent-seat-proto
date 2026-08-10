# MCP provider-session recovery verification

Date: 2026-08-10. Status: completed process-boundary evidence for
`agent-seat-mcp` 0.1.11 in both supported MCP eras.

## Observed problem

After the X11 provider was restarted, an established companion attempted
`capture_obscured` with a target from its old provider session. Version 0.1.10
returned `unavailable/reconnect` with the raw operating-system diagnostic
`Broken pipe (os error 32)`. The call was not automatically replayed, but the
result did not state that the old context and client ID were invalid or that a
mutation's outcome could be unknown.

Automatic replay is deliberately prohibited. Agent Seat client IDs are opaque
and scoped to one authenticated provider session. After reconnection, the same
number can identify a different client, and a transport failure does not prove
that the old provider failed to execute a mutation before disconnecting.

## Process evidence

Two fixtures start the real stdio companion and a strict local Unix provider.
The provider accepts `Hello`, returns `Welcome`, receives and answers one
`seat.status` request, and then closes its authenticated wire session before
the next tool call.

In modern MCP `2026-07-28`, the next `capture_obscured` returns
`stale_context/reconnect`. Its bounded message states that the context and
client IDs are invalid, requires `seat_status` and fresh observation, and says
the previous call's outcome may be unknown. A second call with the same
context returns the ordinary unknown-or-expired `stale_context`, proving the
dead context was removed.

In legacy MCP `2025-11-25`, the failed capture retains the compatible
`unavailable/reconnect` code and result shape. Its message requires
`seat_status` and fresh observation and states that the previous outcome may
be unknown. A second capture using the old client ID is refused locally until
`seat_status` succeeds. The fixture observes no second provider request,
proving that neither the companion nor an immediate old-target call crosses
into a new provider session.

Typed provider outcomes with `retry: reconnect` are also classified as
session-ending for ordinary calls and pointer-slot replay. Unit coverage
distinguishes `reconnect` from `never` and `reobserve`; the original typed
outcome is returned while its session or modern context is discarded.

Initialization and discovery tests also require both eras' agent instructions
to say that reconnect, stale-context, stale-target, and timeout outcomes need
new status and observation and that an old client ID must never be replayed.

## Limits

This establishes local companion behavior around an ended provider stream. It
does not make an ambiguous mutation idempotent, preserve a context across
provider instances, map old targets to new ones, or prove that an external MCP
host follows the recovery instructions. The agent must inspect fresh state and
decide whether a new action is safe.
