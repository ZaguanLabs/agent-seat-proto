# Agent Seat protocol: pre-RFC draft

Status: repository draft, 2026-08-09. This document has no IETF, freedesktop.org,
or other external standards status. The normative contract implemented by the
current product remains [wire revision 4](specification.md).

## Abstract

Agent Seat is a local protocol between an authority-free agent companion and a
desktop provider that owns policy and realizes desktop operations. It exposes
only bounded, explicitly granted capabilities; identifies desktop objects with
provider-local handles; makes stale evidence and incomplete outcomes visible;
and requires every implementation to advertise only the assurance it can
actually prove.

This pre-RFC separates three layers:

1. a display-server-neutral core model;
2. independently testable backend conformance profiles; and
3. non-normative implementations, deployment recipes, and test fixtures.

It records the contract that has survived the current reference implementation
and its hostile tests. It does not standardize X11, Linux evdev, systemd, MCP,
or any particular window manager as part of Agent Seat.

## 1. Conventions and document scope

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**,
**SHOULD**, **SHOULD NOT**, **RECOMMENDED**, **NOT RECOMMENDED**, **MAY**, and
**OPTIONAL** are to be interpreted as described by BCP 14 when, and only when,
they appear in capitals.

This draft defines an abstract protocol contract. A concrete wire revision MUST
define the exact encoding, framing, bounds, message grammar, registries, and
revision identifier needed to implement that contract. Conformance to this
draft does not imply wire compatibility with revision 3, revision 4, or any
future revision.

The current repository wire specification takes precedence if this draft and
revision 4 differ. Resolving such a difference requires an explicit future wire
revision; it MUST NOT silently change a released revision.

## 2. Goals and non-goals

The protocol provides:

- a narrow boundary between an untrusted requester and a policy authority;
- explicit capability negotiation and feature evidence;
- provider-local target identity, scope, and freshness;
- bounded requests, responses, collections, waits, and diagnostics;
- typed no-send, sent-but-unobserved, interruption, and terminal outcomes; and
- comparable but non-transferable assurance claims.

The protocol does not:

- make an agent, model, translator, peer name, or executable path trusted;
- require MCP or any other agent-facing API;
- make X11, Wayland, Linux, a service manager, or a compositor mandatory;
- grant capabilities because a backend advertises a feature;
- turn same-user desktop metadata into authenticated identity;
- promise application handling merely because an action was queued; or
- allow a backend to inherit another backend's assurance by matching its call
  names or visible behavior.

## 3. Actors and authority

### 3.1 Companion

The companion translates an agent-facing request into Agent Seat wire calls.
It has no desktop policy authority. A provider MUST treat its name, version,
purpose, requested capabilities, and translated arguments as untrusted input.

### 3.2 Provider

The provider authenticates the local peer, evaluates grants, owns target scope,
and realizes accepted operations through a backend. It is the sole Agent Seat
policy authority for its sessions. It MUST revalidate the applicable grant,
scope, target, freshness, and backend evidence at the point required by the
selected conformance profile.

### 3.3 Backend

The backend is the desktop authority or integration used by the provider. It
may be an integrated compositor/window manager, a standalone observer beside a
foreign window manager, or a future implementation. Backend-specific object
identifiers and metadata MUST NOT cross the core protocol unless a separately
registered extension explicitly defines them.

### 3.4 Person and physical seat

The person using the physical seat is outside the agent trust domain. A profile
that claims human-priority interruption MUST define the trusted activity and
seat-state evidence, the ordering boundary, the remaining race, and every
condition that makes the operation unavailable.

### 3.5 Target application

A target application and all of its metadata are untrusted. A provider MUST
NOT use titles, process identifiers, application classes, timing, or other
mutable self-asserted metadata as authentication unless a profile explicitly
defines the limited evidence and resulting assurance.

## 4. Core protocol model

### 4.1 Local authenticated session

Agent Seat is a local provider protocol. A wire revision MUST define its
permitted transports and peer-authentication mechanism. Authentication MUST
complete before grants are returned. Descriptive peer metadata MUST NOT affect
authentication.

A session has one provider-selected, nonzero identity. All target handles,
generations, cursors, launch tokens, and other session values are meaningful
only within that session unless a concrete revision explicitly states
otherwise. Reconnection creates a new authority decision and MUST invalidate
prior session-local values.

### 4.2 Exact revision selection

Each endpoint MUST select one exact wire revision before ordinary requests.
An implementation MUST NOT infer compatibility by accepting a message that
resembles a different revision. A mismatch ends opening with a typed
incompatible-revision result.

### 4.3 Capabilities, features, and grants

A **capability** authorizes a class of protocol calls. A **feature** reports
backend behavior or evidence available to the provider. They are independent:

- requesting a capability does not grant it;
- a feature does not grant a capability;
- a grant does not prove a feature; and
- an unadvertised optional feature produces typed unsupported behavior.

The provider returns the exact granted subset during session opening. It MAY
grant fewer capabilities than requested. It MUST NOT grant an unknown
capability, and it MUST reject or close on grammar that the selected revision
defines as malformed.

A live grant can be narrowed or revoked. A request affected before realization
MUST produce a typed no-send or revoked outcome. A revision that permits
grant-change events MUST define their ordering and resynchronization rules.

### 4.4 Scope

Scope is the provider-owned rule determining which desktop objects and facts a
session may observe or affect. Scope filtering MUST happen before optional
sensitive metadata is allocated or returned. Direct lookup MUST NOT reveal
whether an absent target is nonexistent, hidden, or outside scope.

Leaving scope invalidates the session-local target identity. If the same
backend object later returns to scope, the provider MUST assign fresh identity
or generation evidence sufficient to reject cached mutation requests.

### 4.5 Targets and freshness

Desktop targets use opaque provider-selected handles, never raw backend object
identifiers. Every mutation target MUST carry freshness evidence defined by
the concrete revision, such as a target generation or snapshot sequence.

Immediately before realization, the provider MUST recheck:

1. the session and grant;
2. target visibility and scope;
3. target identity and generation;
4. request-specific freshness; and
5. profile-required backend evidence.

If any precondition changed, the provider MUST NOT realize the operation and
MUST return a typed no-send outcome.

### 4.6 Bounds and strict decoding

Every wire revision MUST publish finite limits for frame bytes, collection
items, strings, nesting, outstanding work, waits, retries, and provider-held
state. Receivers MUST reject an over-bound encoded length before allocating
the claimed body.

Message schemas are closed unless a revision explicitly marks an extension
point. Unknown fields, duplicate fields, unknown enum values, invalid types,
noncanonical sets, and trailing data MUST be rejected rather than guessed.

### 4.7 Observation and resynchronization

An observation is evidence sampled or owned by the provider; it is not
implicitly an atomic desktop snapshot. Each profile MUST state whether its
observations are authoritative, sampled, or convergent.

Incremental events MUST have a bounded monotonic cursor. If the provider can no
longer supply a complete continuation, it MUST return a typed resynchronization
requirement. The peer then discards its derived model and obtains a fresh
snapshot.

Missing optional facts are absent. A provider MUST NOT fabricate placeholder
identities, geometry, state, or metadata to make a response appear complete.

### 4.8 Mutation outcomes

Mutation results MUST distinguish at least these semantic classes:

| Class | Meaning |
| --- | --- |
| no-send | Policy, scope, freshness, arguments, or support prevented realization. |
| observed | The requested public state was subsequently observed. |
| sent-unobserved | A bounded request was sent or queued, but the desired public state was not observed before the deadline. |
| target-lost | The target disappeared after send; the result is unknown. |
| interrupted | Required evidence changed before the profile's terminal boundary. |
| unavailable | The provider or required evidence source cannot currently serve the operation. |

A result MUST claim only what the provider knows. Sending, queueing,
synchronizing, or observing a public state MUST NOT be described as proof that
an application internally handled an action.

A concrete call MAY omit inapplicable classes, but it MUST define every point
at which realization is considered to have begun and every result possible
after that point.

### 4.9 Interruption

An interruptible operation MUST define:

- the event or state change that triggers interruption;
- the trusted component that observes it;
- the evidence value checked before realization;
- the smallest atomic action that may overlap the first interrupting event;
- the terminal synchronization boundary; and
- cleanup and reporting after partial progress.

Loss, ambiguity, overflow, restart, or stale interruption evidence MUST fail
closed. Implementations MUST NOT describe asynchronous notification as
zero-race preemption.

### 4.10 Errors

Errors use stable machine-readable codes and retry guidance. Human-readable
diagnostics are bounded and MUST NOT select control flow. A concrete revision
MUST distinguish malformed input, incompatible revision, refusal, unsupported
behavior, stale evidence, unavailable authority, resynchronization, revocation,
and terminal session closure.

## 5. Assurance and conformance

### 5.1 Core conformance

An implementation is **core conformant for revision R** only when it:

- implements the complete required grammar and lifecycle for R;
- enforces every published bound and strict-decoding rule;
- authenticates before granting;
- keeps backend identifiers behind opaque session-local values;
- implements typed freshness, scope, and outcomes; and
- passes the revision's required public-boundary fixtures.

Partial implementations MUST report a compatible subset through the selected
revision's feature and grant mechanisms. They MUST NOT claim complete revision
conformance if required calls or error branches are absent.

### 5.2 Profile conformance

A backend profile is a separately versioned set of evidence requirements and
hostile tests. A profile MUST define:

- its authority and trust inventory;
- required backend primitives and their ordering;
- observation and mutation semantics;
- target and human-priority validation, where applicable;
- fail-closed conditions and known races;
- negative-authority tests for every participating process; and
- the exact assurance label an implementation may advertise after passing.

Implementing the same tools, using the same library, or copying a reference
deployment is not profile conformance. Conformance requires the stated
evidence and tests on the claiming implementation.

### 5.3 Assurance vocabulary

This draft reserves two initial assurance families without making either a
universal security boundary:

| Family | Minimum claim |
| --- | --- |
| `tier0` | A standalone provider beside a desktop authority supplies bounded, independently revalidated observations and qualified requests. It does not own the desktop event loop and MUST disclose races and foreign-authority limits. |
| `tier1` | An integrated desktop authority supplies the profile's state and ordering from the event loop or object model it owns. It MUST still define policy, scope, application trust, and OS/session limits. |

`tier1` is not automatically “secure,” and `tier0` is not automatically
conformant. Assurance is the combination of an exact wire revision, backend
profile/version, advertised features, and passed evidence. A new backend MUST
use the narrowest existing claim it proves or register a new profile.

## 6. Backend conformance profile template

Every proposed profile SHOULD use this template:

1. profile name, version, status, and owning specification;
2. supported wire revisions and capability subset;
3. process and authority inventory;
4. trusted and untrusted inputs;
5. object identity, scope, and freshness realization;
6. observation consistency and resynchronization;
7. mutation preconditions, send boundary, and terminal outcomes;
8. interruption source, action granularity, and race bound;
9. lock, logout, suspend, seat-switch, hotplug, and restart behavior;
10. resource bounds and denial-of-service behavior;
11. negative-authority and hostile conformance fixtures; and
12. known limitations and prohibited claims.

An optional profile MUST remain unadvertised and return typed unsupported or
unavailable outcomes until every required gate passes. Failure to satisfy a
gate is not permission to weaken the profile silently.

## 7. Revision and extension governance

### 7.1 Wire revisions

Wire revisions are unsigned positive integers allocated by this repository
until an external registry is approved. A revision is immutable after release.
Any incompatible grammar, enum, framing, bound interpretation, or semantic
change requires a new revision.

A compatible implementation bug fix does not allocate a revision. Crate,
binary, package, and product versions are independent from wire revisions.

### 7.2 Registry ownership

The repository maintains these logical registries in the normative wire
specification:

- protocol and revision identifiers;
- capability atoms;
- feature atoms;
- backend and assurance identifiers;
- call and event names;
- error codes and retry guidance; and
- extension namespaces and profile identifiers.

Names beginning `agent-seat.` are reserved for the core specification. An
experimental extension SHOULD use a collision-resistant reverse-domain
namespace controlled by its author. An extension specification MUST name its
owner, status, supported revisions, bounds, authority change, privacy effect,
and failure semantics.

### 7.3 Extension maturity

Extensions progress through these repository states:

1. **experimental** — threat model and wire allocation exist; no portability or
   deployment claim;
2. **provisional** — at least two independent implementations or one
   implementation plus a complete independent black-box conformance harness;
3. **normative** — stable semantics, deterministic hostile tests, documented
   compatibility, and maintainer/community approval; or
4. **withdrawn** — unsafe, unimplementable, or superseded; its names remain
   reserved to prevent ambiguous reuse.

Moving between states requires a reviewed specification change. Runtime usage
alone does not mature an extension.

## 8. Security and privacy considerations

The companion, model, harness, target application, and descriptive metadata
are untrusted. The provider is responsible for policy and must remain safe when
any of them is compromised.

Desktop protocols frequently expose ambient same-user authority. Agent Seat
can reduce accidental overreach and constrain an untrusted companion, but it
cannot revoke authority held independently by another process. Each profile
MUST state its OS-user, session, display-server, and peer-isolation assumptions.

Titles, application identifiers, process metadata, geometry, screenshots,
accessibility trees, and input activity can disclose private information.
Sensitive observations require separate grants and scope. An activity source
with raw keyboard or pointer access has keylogging-grade authority even if its
outbound protocol reports only an epoch; deployment and confinement are part
of its profile evidence.

Launch authorization applies to the currently resolved application entry, not
an immutable binary, unless a profile explicitly defines stronger evidence.
The provider MUST NOT route untrusted launch metadata through a shell.

Capture, input, and accessibility change the authority and privacy model. They
MUST be separate optional profiles with dedicated threat models. They MUST NOT
be inferred from core observation or management conformance.

## 9. Current profile status

The following table is descriptive, not a new compatibility promise:

| Profile | Current status | Claim |
| --- | --- | --- |
| revision 3 core and the retained revision 4 core | implemented | Strict local lifecycle, bounded observation, qualified EWMH management, and controlled desktop-entry launch. |
| standalone X11/EWMH Tier 0 | implemented for the core | Convergent observation beside a foreign window manager; no atomic WM-state claim. |
| revision-4 pointer movement | experimental, deployment gate open | One target-relative action with broker epoch and live destination checks; generic lock-state evidence is insufficient. |
| pointer click | unsupported | No approved wire call or passed profile. |
| keyboard input | unsupported | No approved wire call or passed profile. |
| capture | unsupported | No approved wire call or passed profile. |
| accessibility semantics | unsupported | No approved wire call or passed profile. |
| integrated Tier 1 backend | profile not yet specified here | No implementation may claim it from tool compatibility alone. |

## 10. Non-normative reference mapping

The current repository maps the abstract actors as follows:

```text
agent harness
    -> agent-seat-mcp (authority-free companion)
    -> Agent Seat local wire
    -> agent-seat-x11 (policy-owning provider)
    -> X11/EWMH or an explicitly gated optional realization
```

`agent-seat-proto` contains display-neutral wire values and framing.
`agent-seat-mcp` translates MCP without owning grants. `agent-seat-x11` owns
policy, scope, peer verification, and realization. The optional Linux activity
broker is deployment machinery for one experimental profile; it is not part of
the core protocol and an integrated backend is not required to reproduce it.

Reference tests use public process, filesystem, socket, and display behavior.
Logs alone do not prove conformance. A future reusable conformance suite SHOULD
express expected requests, externally visible outcomes, timing bounds, and
negative authority without importing implementation internals.

## 11. Open work before external publication

This draft is ready for independent review, not external standards submission.
The next revision SHOULD:

- split the abstract core into a serialization-neutral information model and a
  concrete transport binding;
- publish machine-readable registries without making generated artifacts the
  source of truth;
- specify at least one complete backend conformance profile independently from
  the reference implementation;
- define a portable black-box conformance report format;
- obtain review from integrated compositor/window-manager maintainers; and
- resolve governance, registry custody, change control, and the venue for any
  external standard.

## 12. References

Normative terminology:

- RFC 2119, “Key words for use in RFCs to Indicate Requirement Levels”:
  <https://www.rfc-editor.org/rfc/rfc2119>
- RFC 8174, “Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words”:
  <https://www.rfc-editor.org/rfc/rfc8174>

Repository contracts and evidence:

- [Agent Seat wire revision 4](specification.md)
- [Architecture](architecture.md)
- [Security model](security-model.md)
- [Optional-profile decisions](optional-profiles.md)
- [T5 input reconsideration](t5-input-reconsideration.md)
- [T5 broker deployment](t5-broker-deployment.md)
- [T5 lock-state study](t5-lock-state-study.md)
