# Agent Seat serialization-neutral information model

Status: repository pre-RFC draft, 2026-08-09. This document defines the
portable semantic layer proposed for future standardization. It does not alter
the implemented local JSON wire contract; revision 5 remains defined by
[`specification.md`](specification.md).

## Purpose and separation

The information model describes what an Agent Seat implementation means. A
transport binding describes how those meanings are encoded, framed,
authenticated, bounded, and carried. A backend profile describes what evidence
permits a provider to make a particular assurance claim.

These layers are independent:

```text
information model
    semantic values, lifecycle, authority, invariants, outcomes

transport binding
    encoding, framing, strict grammar, limits, local authentication

backend profile
    desktop evidence, ordering, races, negative authority, assurance
```

Matching an operation name in another agent API is not an Agent Seat binding.
Matching a binding is not backend-profile conformance. A profile may support a
strict subset of optional operations, but it cannot weaken the common semantic
invariants below.

## Common value model

An information-model value is one of:

- a Boolean;
- a bounded integer with an explicitly declared range;
- bounded text whose limit is measured in encoded bytes by its binding;
- a registered atom from a closed set;
- an optional value, represented as absent or present rather than by a
  fabricated sentinel;
- a bounded ordered sequence;
- a bounded canonical set whose members are unique and deterministically
  ordered;
- a closed record with named fields; or
- a closed tagged choice with exactly one selected alternative.

Bindings MUST reject duplicate record fields, duplicate set members, unknown
closed fields, unknown closed alternatives, invalid atom values, and values
outside their declared bounds. An extension point is open only when its owning
specification explicitly says so and supplies its own namespace and bounds.

Opaque identifiers are positive provider-selected integers unless a binding
defines an equally strict representation. Zero may be used only where the
information model explicitly defines an initial cursor, generation, or
sequence value. Identifiers never imply a process, X11, Wayland, operating-
system, or application identity.

## Session model

### Opening

A peer proposes one exact protocol revision, descriptive peer metadata, and a
canonical set of requested capabilities. The provider authenticates the
transport peer, selects that exact revision or refuses it, evaluates policy,
and returns:

- a fresh nonzero session identity;
- descriptive provider metadata;
- backend and assurance atoms;
- the exact feature set;
- the exact granted capability subset; and
- the binding limits that apply to the session.

Peer metadata is never authentication evidence. A feature describes available
provider evidence or behavior; a capability permits a class of calls; a grant
is the provider's current authority decision. None implies either of the
others.

### Lifetime

Every target handle, generation, observation sequence, event cursor, launch
token, and page cursor is local to one opened session. Reconnection creates a
new policy decision and invalidates every prior session-local value.

A session accepts a bounded number of requests and bounded concurrent work. A
provider may narrow or revoke grants. It cannot broaden a grant silently. A
terminal close invalidates all session-local values and outstanding work.

## Desktop model

### Scope and identity

Scope is a provider-owned predicate applied before a desktop object or
sensitive fact enters the session model. A direct lookup cannot distinguish a
nonexistent object from an existing object hidden by scope.

A visible desktop client has an opaque handle and generation. When it leaves
scope, the handle becomes invalid. If the same backend object later returns,
the provider supplies fresh identity or generation evidence so an old mutation
cannot apply to it accidentally.

A workspace has a session-valid identifier and optional bounded descriptive
facts. Titles, application labels, geometry, states, and actions are untrusted
descriptions. Their presence depends on the applicable grant and backend
profile.

### Observation

A snapshot is a bounded provider observation with one sequence value. It
contains only in-scope objects and facts. The selected profile declares whether
that observation is authoritative, sampled, or convergent.

An event subscription begins at a provider cursor and yields bounded,
monotonically ordered changes. If a complete continuation is unavailable, the
provider returns a resynchronization requirement. The peer then discards its
derived model and takes a fresh snapshot. Missing facts remain absent.

## Operation model

Every request has a nonzero peer-selected identifier and one registered call.
The provider returns exactly one response carrying the same identifier, unless
the selected binding terminates before a response can be encoded.

Before realization, the provider revalidates:

1. the live session and grant;
2. current scope;
3. target identity and generation;
4. request-specific freshness; and
5. evidence required by the selected backend profile.

Failure before the profile's send boundary is a typed no-send result. The
provider cannot perform the operation and then describe it as refused, stale,
or unsupported.

### Observation and management

Observation calls return bounded current evidence. Management calls request a
public desktop-state transition. Their results distinguish at least:

- the desired public state was observed;
- a bounded request was sent but the state was not observed by its deadline;
- the target disappeared after send and the result is unknown; and
- realization never began because policy, scope, freshness, arguments, or
  support rejected it.

No result claims an application internally accepted or understood a request.

### Launch

Application discovery returns a bounded, deterministically ordered policy view
of launchable application identifiers. Launch re-resolves the current winning
entry and current policy; page metadata is never executable authority. A launch
token is session-local correlation evidence, not proof of process identity or
causality. Untrusted launch metadata never crosses a shell boundary.

### Interruptible actions

An interruptible action additionally defines a trusted interruption source,
the evidence sampled before realization, the smallest atomic action that may
overlap the first interrupting event, the terminal synchronization point, and
cleanup after partial progress.

Loss, ambiguity, overflow, restart, or stale interruption evidence is
unavailable. Asynchronous interruption is never described as a zero-race
guarantee. Each optional input, capture, or accessibility operation belongs to
a separately approved profile; core observation and management do not imply
it.

## Outcome and error model

Successful operation bodies are closed tagged choices registered by the
selected revision. Mutation semantics use these common outcome classes where
applicable:

| Class | Knowledge represented |
| --- | --- |
| `no-send` | Realization did not begin. |
| `observed` | The requested public state was subsequently observed. |
| `sent-unobserved` | A bounded request was sent or queued; the desired public state was not observed by the deadline. |
| `target-lost` | The target disappeared after send; the effect is unknown. |
| `interrupted` | Required evidence changed before the profile's terminal boundary. |
| `unavailable` | Required authority or evidence cannot currently serve the operation. |

Errors carry a stable registered code, stable retry guidance, and optional
bounded context. Human-readable diagnostics never select control flow. A
binding defines which malformed input closes the session and which application
errors receive ordinary responses.

## Binding requirements

A conforming binding specification provides all of the following without
changing the information-model meaning:

- an immutable positive revision identifier and exact selection procedure;
- permitted local transports and peer authentication;
- encoding, framing, end-of-stream, and truncation behavior;
- exact field and atom spellings;
- maximum frame, text, collection, wait, request, and retained-state sizes;
- canonical ordering and strict-decoding rules;
- the complete call, reply, event, feature, capability, backend, assurance,
  error, retry, profile, and extension registries; and
- public black-box fixtures for every required and failure path.

The current binding is the pathname Unix-stream, length-prefixed strict JSON
contract in [`specification.md`](specification.md). MCP stdio is an
agent-facing translation and is not an Agent Seat transport binding.

## Profile requirements

A conforming backend profile defines its authority inventory, trusted inputs,
scope and freshness realization, observation consistency, mutation send
boundary, interruption and race semantics, lifecycle failure behavior,
resource bounds, negative-authority tests, hostile fixtures, known limitations,
and exact assurance claim.

An implementation advertises only profiles it has passed on its actual
backend. Sharing call names, source code, libraries, or visible behavior does
not transfer evidence from another profile.
