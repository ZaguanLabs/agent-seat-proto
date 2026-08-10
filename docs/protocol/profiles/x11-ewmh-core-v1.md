# Agent Seat profile: standalone X11/EWMH core v1

Identifier: `agent-seat.x11-ewmh-core.v1`

Status: experimental repository profile, 2026-08-09. The profile is complete
enough for independent implementation and black-box review, but has not yet
met the pre-RFC maturity requirement for provisional status.

Owning specification: the Agent Seat repository pre-RFC. Supported binding:
local JSON wire revisions 5 through 9. Backend atom: `x11_ewmh`. Assurance
atom: `tier0`.

## 1. Claim

This profile describes a standalone Agent Seat provider operating beside an
unmodified EWMH window manager. The window manager remains the desktop
authority. The provider supplies bounded, scope-filtered, convergent
observation; sends only currently advertised EWMH management requests; and
launches only currently policy-admitted desktop entries.

The profile does not claim an atomic window-manager snapshot, exclusive desktop
control, application identity, application acceptance, human-priority input,
screen lock enforcement, capture, accessibility, or isolation from another
same-user X11 client.

## 2. Supported revision and surface

Revision 5 through 9 sessions claiming this profile advertise exactly:

- backend `x11_ewmh`;
- assurance `tier0`; and
- the implemented subset of `ewmh_observation`, `ewmh_management`, and
  `desktop_launch`.

The profile covers these capabilities:

- `observe_structure`, `observe_titles`, and `observe_events`;
- `manage_activate`, `manage_close`, `manage_workspace`, `manage_state`, and
  `manage_geometry`; and
- `launch_list` and `launch_execute`.

Each capability remains separately requested, granted, and checked. A provider
may implement a strict subset and advertise/grant only that subset. The
`input_pointer`, `input_keyboard`, and `capture_obscured` capabilities and every capture, input,
human-activity, and
accessibility feature are outside this core profile. Matching this profile does
not satisfy their separate gates.

## 3. Actors and authority inventory

```text
requesting peer
    required: one authenticated local Agent Seat connection
    untrusted: metadata, requested grants, handles, freshness, arguments
    profile claim: no authority beyond provider-returned grants

standalone provider
    required: local peer authentication, policy, X11 observation and
              qualified EWMH requests, bounded XDG launch resolution
    prohibited by this profile: raw backend identifiers on the wire,
              shell interpolation, unscoped facts, unsupported emulation

foreign EWMH window manager
    required: owns desktop state and interprets advertised EWMH requests
    untrusted to acknowledge, complete, order, or preserve requested state

target applications and other X11 clients
    untrusted: titles, classes, PIDs, properties, startup metadata, timing
    ambient limitation: same-user clients may inspect, spoof, or mutate X11
```

The binding authenticates the local transport peer before the provider returns
grants. Descriptive peer names, executable paths, X11 properties, application
metadata, and target metadata never authenticate a peer or select policy.

This is not an OS sandbox profile. A process that independently possesses the
same X11 authorization may bypass Agent Seat. Companion or harness confinement
is a separate deployment claim and cannot be inferred from this profile.

## 4. Trusted and untrusted evidence

The provider trusts its own configuration, local peer-credential result,
session-local identity map, limits, and implementation state. It treats the X
server as the source of sampled public X11 state, but does not treat mutable
client-owned properties as authentication.

EWMH root and client properties are backend evidence only after strict type,
format, length, and value validation. Missing, malformed, over-bound, or
internally inconsistent properties become absent facts, typed unsupported
behavior, resynchronization, or unavailability as specified below. They are
never repaired by guessing.

## 5. Object identity, scope, and freshness

The provider maps every visible X11 client to a nonzero session-local handle
and generation. An XID never crosses the Agent Seat boundary. Direct lookup of
an invalid, hidden, or out-of-scope handle returns the same `no_such_client`
class.

Scope filtering occurs before title or other optional facts are allocated to a
response. At minimum, an implementation may define all-workspace and current-
workspace scope. In current-workspace scope, leaving scope invalidates the
handle. Returning later receives fresh identity or generation evidence.

Every client mutation supplies the current handle and generation. A workspace
switch supplies a current observation sequence. Immediately before sending an
EWMH request, the provider rechecks the live session, grant, scope, target,
generation or sequence, requested support, and operation-specific arguments.
A failed recheck sends nothing.

## 6. Observation and resynchronization

Observation is sampled and convergent, not atomic. A complete observation pass
reads bounded EWMH root state, then bounded properties for the clients selected
by that root state. The result may reflect legal changes made by the foreign
window manager during the pass. The provider assigns a monotonic sequence to
its own visible model and converges through later sampling and X11 events.

A snapshot contains at most the revision limits and satisfies all cross-field
invariants: unique workspaces and clients, in-range current workspace, and an
active client only when that client is visible in the same snapshot. Missing
titles, geometry, work areas, states, actions, or active-client evidence remain
absent.

Event subscriptions use bounded provider cursors. Events are scope-filtered,
strictly ordered, and no later than the returned cursor. Overflow, discarded
history, or inability to construct a complete continuation returns
`resync_required`; the peer discards its derived model and requests a fresh
snapshot.

## 7. Management semantics

### Common preconditions

A management operation begins realization only after every check in section 5
passes. The relevant EWMH atom must be advertised by the window manager and,
where applicable, by the target's allowed-action set. The provider does not
substitute XTEST, shell commands, direct client protocol messages, private
window-manager APIs, or property writes for an unsupported EWMH request.

### Send boundary and result

The send boundary is the successful queueing of the one bounded EWMH client
message or protocol request defined for the operation. Before that boundary,
refusal, unsupported behavior, stale evidence, invalid arguments, and hidden
targets are typed no-send results.

After send, the provider synchronizes with X11 and observes public state until
one of these terminal results:

- `observed`: the requested public state became true;
- `timed_out`: the fixed deadline elapsed without that observation; or
- `target_gone`: the target disappeared after send and the effect is unknown.

The provider never reports application or window-manager acceptance. An X11
request error is `internal` or another precisely specified typed error; it is
not rewritten as `observed`.

### Operations

- Activation requests the target through the EWMH active-window mechanism and
  observes the public active client.
- Polite close requests EWMH close and observes target disappearance.
- Workspace switch requests the advertised desktop index and observes the
  public current workspace.
- Client workspace reassignment requests the advertised desktop and observes
  the target's public workspace.
- State changes request one advertised state add, remove, or toggle and observe
  the resulting public state.
- Geometry changes require a nonempty requested rectangle and advertised move
  or resize support, then observe public frame geometry.

An implementation documents the exact EWMH source-indication and timestamp
values it sends. Those fields do not increase assurance.

## 8. Launch semantics

Application discovery follows the XDG application-directory precedence defined
by the selected deployment. It accepts only bounded, regular desktop entries
with a canonical desktop identifier, a supported application entry type, and a
strictly parsed executable field. Hidden, duplicate-losing, malformed,
nonlaunchable, and policy-denied entries do not appear.

The provider applies one explicit admission mode: deny all, allow only selected
identifiers, or allow all except selected identifiers. Inactive allow and deny
lists may be preserved across mode changes, but only the active mode decides
the current result.

Launch re-resolves the winning entry and policy at execution time. It expands
only specified desktop-entry field codes into a bounded argument vector and
invokes the resolved absolute executable without a shell. Concurrent waiting
children, arguments, strings, directories, and catalog pages are bounded.

A successful launch returns a fresh session-local token. A visible client is
attached only when bounded, in-scope startup-notification evidence matches; an
absent correlation is ordinary success. Same-user startup metadata is not
process identity or proof that the new client was caused by the launch.

## 9. Interruption and lifecycle

The core profile makes no human-priority interruption claim. It has no physical
activity source and no atomic-action race statement. Input operations remain
unsupported even if another same-user process can inject X11 events.

Provider startup proves ownership of its own advertisement and runtime socket
without replacing a live or foreign owner. A second provider refuses to start.
Losing advertisement ownership, losing the X connection, or an unrecoverable
listener failure stops the provider and removes only its own runtime socket.
Provider failure does not terminate, replace, or patch the window manager.

Suspend, inactive sessions, VT switching, and screen locks do not create a
trusted boundary in this profile. A deployment may conservatively stop the
provider, but a provider that remains connected makes only the same qualified
X11 core claims. It MUST NOT infer user presence, unlocked state, or input
safety from continued X11 access. Restart creates a new Agent Seat session and
invalidates every old handle and cursor.

## 10. Resource and denial-of-service bounds

The selected binding's frame, string, list, snapshot, event, page, and wait
limits all apply. The provider additionally publishes finite session, request, catalog,
launch-child, filesystem-depth, X11-property, and I/O deadlines. It evicts
peers that do not complete opening or a frame within the configured deadline
and recovers the bounded session slot.

Limits are not silently raised from peer input. Exhaustion returns a typed
error or closes the affected session without taking down the foreign window
manager.

## 11. Black-box conformance fixtures

The required suite uses an isolated X server and a released EWMH window
manager. It observes public wire, socket, filesystem, process, and X11 behavior;
logs alone cannot pass a case. The suite proves:

- `opening.strict`: exact opening, peer-credential grant intersection, strict
  decoding, bounds, timeout eviction, and reconnect invalidation;
- `observation.desktop`: empty, populated, changing, and malformed public
  desktop observations;
- `scope.identity`: title denial, scope filtering before allocation,
  leave-scope invalidation, and fresh identity on return;
- `events.resynchronization`: ordered event convergence and deterministic
  overflow resynchronization;
- `management.outcomes`: every management no-send branch plus observed,
  timed-out, target-gone, X11 error, and provider-loss branches;
- `management.no-emulation`: unsupported EWMH operations emit no substitute
  request;
- `launch.controlled`: catalog precedence, strict desktop-entry parsing, all
  admission modes, execution-time re-resolution, shell-free launch, bounded
  children, and qualified correlation;
- `lifecycle.independent`: live-owner refusal, stale-owned-socket recovery,
  foreign-path refusal, ownership loss, provider crash, and window-manager
  survival; and
- `boundary.no-extra-surface`: absence of raw backend identifiers, ungranted
  titles, capture, input, accessibility, broker administration, and claims of
  application handling at the public protocol boundary.

All nine IDs are required. A conforming report uses
[`agent-seat.conformance-report/1`](../conformance-report.md) or a later
registered format and includes each ID exactly once.

Same-user X11 bypass and compromise of a process already holding ambient
desktop authorization are limitations to report, not fixture failures that the
profile can pretend to prevent.

## 12. Known limitations and prohibited claims

An implementation claiming this profile states all of the following:

- observations are convergent samples beside a foreign authority;
- EWMH management is a qualified request followed by public observation;
- target and application metadata are not authenticated identities;
- launch correlation may be absent and is never causality proof;
- lock state, physical activity, and user presence are not established;
- another same-user X11 client can bypass Agent Seat policy independently;
- input, capture, accessibility, and integrated-authority assurance are absent;
  and
- `tier0` does not inherit `tier1` guarantees from matching call names.

## Non-normative reference evidence

The repository's current Xvfb/Openbox lifecycle, policy-transaction, catalog,
discovery, malformed-process, and companion tests exercise the listed public
branches. That implementation evidence does not make this profile provisional;
the maturity gate still requires a genuinely independent implementation or
independent black-box conformance harness.
