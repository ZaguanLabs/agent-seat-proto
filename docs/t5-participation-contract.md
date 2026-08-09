# T5 participant and full-system runner contract

Status: candidate test-integration contract, 2026-08-09. This document does
not pass the T5 deployment gate, register an input profile, or authorize click
or keyboard implementation. It defines the exact evidence another harness or
isolated-system maintainer must supply so the remaining gates can be reviewed
without adopting the reference implementation.

## Purpose

Three claims remain deliberately unproven by the current repository evidence:

1. an external agent harness has no desktop or broker authority except through
   its MCP channel;
2. physical-device removal, addition, and replacement preserve complete
   activity coverage or fail closed; and
3. every supported lock path removes the enrolled user session from the
   physical seat before a credential surface can receive input.

These claims require deployment-specific evidence. Source inspection, Xvfb,
the companion sandbox, a visible greeter, `LockedHint`, and the existing
synthetic hotplug case are insufficient substitutes.

The participant supplies one adapter for its real harness launcher and one
adapter for a disposable full-system runner. Both report stable fixture IDs
and content-digested public evidence. The Agent Seat provider, broker, and wire
contracts are not patched to accommodate the adapters.

## Common safety rules

- Tests run only in a disposable VM or an equivalently isolated full boot.
  Containers, Xvfb alone, and the person's live seat cannot pass lock or
  physical-seat cases.
- No host input device, display socket, Xauthority file, broker socket, or
  credential is passed into the guest accidentally.
- Synthetic devices emit no meaningful event unless a fixture explicitly
  requires one; credential-like strings are never used.
- Every case starts from a clean snapshot or proves equivalent complete state
  restoration. A stopped broker is never silently reused.
- Evidence is public process, descriptor, socket, filesystem, logind, X11, and
  synthetic-device behavior. Logs may explain but cannot decide a result.
- A timeout, lost observer, ambiguous state, missing artifact, or adapter crash
  is `error` or `incomplete`, never `pass`.

## Harness adapter

### Required topology

The real harness runs inside the boundary being claimed. The MCP companion and
policy-owning provider run outside that boundary. The harness receives one
bounded MCP request/response channel and any non-desktop resources its product
requires, such as an explicitly reviewed project tree or remote API network.

The adapter cannot pass the provider descriptor, provider pathname, broker
pathname, X11 socket, Xauthority cookie, user-manager control socket, evdev or
uinput descriptor, or a writable host runtime directory into the harness.
Passing a broad systemd user-manager connection is not an MCP channel: an
untrusted harness could use it to create a less-confined service.

Networked harnesses need a boundary that permits their reviewed remote network
surface while denying host-local Unix authority. `PrivateNetwork=yes` is a
valid proof only for an offline harness; dropping it merely because the harness
needs an API is not. A participating launcher must name the enforcement used
for filesystem and abstract X11 sockets, local service sockets, process
inspection, inherited descriptors, and child-process escape.

### Stable fixtures

All fixtures execute through the participant's actual production launcher:

- `harness.mcp-round-trip`: one nonsecret JSON-RPC request identifier crosses
  the MCP channel on a bounded `ping` or static tool request and the matching
  response returns; no second desktop channel exists.
- `harness.no-provider-wire`: attempts to discover and connect to every
  provider advertisement/path available outside the boundary fail.
- `harness.no-broker-control`: provider-status, eligibility, enrollment,
  system-manager, and broker runtime endpoints are absent or denied.
- `harness.no-x11-filesystem`: every mounted X11 filesystem socket is absent or
  denied, even when `DISPLAY` is supplied maliciously.
- `harness.no-x11-abstract`: a direct connection attempt to the host X11
  abstract address is denied independently of pathname hiding.
- `harness.no-xauthority`: the live cookie, home source, environment variable,
  and inherited authorization descriptors are absent.
- `harness.no-input-devices`: `/dev/input`, `/dev/uinput`, and inherited input
  descriptors are absent even when the launcher account could access them
  outside the boundary.
- `harness.no-process-escape`: parent/session processes and their descriptors
  cannot be inspected or reused to recover desktop authority.
- `harness.child-preserves-boundary`: an arbitrary child created through every
  supported harness execution path retains the same denials.
- `harness.no-manager-escape`: the harness cannot submit a transient unit,
  container, portal request, or equivalent operation that regains the denied
  authority.

Each negative fixture begins from a baseline demonstrating that the hostile
probe could reach the target authority without the participant boundary. A
denial is otherwise not meaningful.

The repository supplies a dependency-free safe-Rust
[`harness-authority-probe`](../contrib/t5/README.md) for the baseline/denial
portion. It checks one selected event node, uinput, Xauthority, provider,
broker, user-manager connection and transient-unit submission, filesystem and
abstract X11 sockets, parent-process visibility, and inherited input
descriptors. The probe does not create a sandbox, know MCP, or make a
participant result portable by itself.

### Harness evidence

The adapter records the launcher/version/configuration digest, harness binary
digest, sandbox identity, visible descriptor names and classes, mount/network
namespace identities, peer credentials at the MCP endpoint, and each attempted
denial. It does not record tokens, environment contents, window metadata, raw
socket payloads, or project data.

Harness evidence uses the `agent-seat.conformance-report/1` data model. Until
an input profile and these fixture IDs are registered, the bundle is explicitly
an incomplete candidate report rather than a schema-and-registry-valid
conformance report. Registration is not inferred from collecting evidence.

## Full-system runner adapter

### Image and reset manifest

The runner boots a real init, logind, display manager, greeter, X server,
released Openbox, Agent Seat services, and virtual terminal stack. Its manifest
pins:

- base image and package-manager snapshot digests;
- kernel, init/logind, display-manager, greeter, X server, Openbox, and Agent
  Seat build/package versions;
- display-manager, greeter, PAM test-module, session, idle-lock, suspend, VT,
  and service unit configuration digests;
- virtual keyboard/pointer controller and device descriptors;
- firmware/machine type and virtual seat topology; and
- the clean snapshot identifier used before every destructive case.

The greeter/input-dispatch probe accepts only a fixed noncredential synthetic
signal delivered by the runner. It must expose the earliest instant at which
the released stack can deliver an authentication input event without storing
key content or delaying that instant. A PAM callback is too late by itself; a
visible field or successful display-manager method is not the input boundary.
If nonintrusive observation is impossible, the exact supported deployment must
provide an independently enforced production pre-input barrier. Test-only
delay cannot make an otherwise unsafe production stack pass.

### Device topology

The runner controls virtual USB or equivalent devices outside the guest. It
can attach, detach, and replace a keyboard or pointer at one stable virtual
port while the guest observes ordinary kernel, udev, and evdev behavior.
Agent Seat never controls the hypervisor channel.

The initial matrix includes:

- a serial-backed keyboard and pointer;
- topology-only keyboard and pointer entries;
- a replacement at the same port with a different serial;
- a replacement at the same port with changed capabilities/properties;
- a coverage-equivalent topology-only clone;
- an extra relevant device;
- a missing relevant device; and
- replacement between verification, unit publication, descriptor opening, and
  broker readiness.

### Replacement fixtures

- `replacement.remove-ready`: removing one enrolled device latches the broker
  off and closes every event descriptor.
- `replacement.add-ready`: adding one relevant device latches the broker off;
  it is never adopted into the current instance.
- `replacement.serial-change`: changed serial evidence at the same topology is
  rejected before readiness.
- `replacement.capability-change`: any changed class, capability, property, or
  complete-set evidence is rejected before readiness.
- `replacement.topology-clone`: a topology-only coverage-equivalent clone is
  accepted only after a fresh administrator arm and complete current-set
  validation; the stopped instance never rearms.
- `replacement.extra-device` and `replacement.missing-device`: unequal
  complete relevant sets are unavailable.
- `replacement.verify-start-race`: every replacement injected at a controlled
  boundary between verification and readiness is either bound to the exact
  inherited descriptor set or detected and latched unavailable.
- `replacement.notification-loss`: lost, malformed, truncated, or overflowing
  lifecycle notification fails closed without a grace interval.

Every fixture proves descriptor closure through the broker process descriptor
table and proves that no new event descriptor appears until a fresh explicit
arm.

### Lock fixtures

- `lock.manual-order`: the enrolled user session becomes inactive, the distinct
  greeter/lock-screen session becomes active, and the broker closes event
  descriptors before the greeter input barrier opens.
- `lock.idle-order` and `lock.suspend-order`: automatic idle and suspend paths
  reach the identical ordering without an in-session fallback.
- `lock.vt-switch`, `lock.other-user`, `lock.logout`, `lock.greeter-crash`,
  `lock.display-manager-restart`, and `lock.logind-restart`: each transition
  closes descriptors and leaves the instance terminal.
- `lock.ipc-loss`: bus loss, owner change, signal loss, conflicting properties,
  and unknown enum values fail closed.
- `lock.hint-untrusted`: false, true, absent, delayed, or same-user-modified
  `LockedHint` cannot authorize readiness or prevent stopping.
- `lock.same-user-signals-untrusted`: saver, DPMS, focus, stacking, lock-window,
  and same-user locker service changes cannot authorize readiness.
- `lock.no-rearm`: unlock, greeter exit, and return to the user's VT leave the
  broker terminal until a fresh administrator arm.
- `lock.failed-request` and `lock.partial-transition`: a rejected or incomplete
  lock cannot restore a stopped instance or leave descriptors open at a
  credential surface.
- `lock.activity-boundaries`: synthetic activity immediately before, during,
  and after the transition preserves the one-atomic-action maximum overlap and
  exposes no event content.

### Ordering evidence

The runner owns a monotonic host-side sequence counter and causal barriers.
Guest observers acknowledge bounded state transitions before the runner may
advance; the hypervisor-side device controller uses that same coordinator.
Arrival order on independent asynchronous channels is not treated as event
order. This avoids comparing unrelated guest wall clocks or relying on
transport timing. The retained evidence establishes this strict order:

```text
user session no longer eligible
    < broker terminal state published
    < every evdev descriptor closed
    < greeter authentication-input barrier open
```

If the runner cannot place all four observations on one deterministic order,
`lock.manual-order` is incomplete. It is not approximated with timestamps or
sleep intervals. Instrumentation must either observe the earliest released-
stack input dispatch without delaying it or exercise the exact production
pre-input barrier whose enforcement is part of the compatibility claim.

## Participant submission

A review bundle contains:

1. the adapter source and license;
2. exact build and configuration manifests;
3. one `agent-seat.conformance-report/1` document;
4. content-addressed, privacy-reviewed public evidence for every fixture;
5. a statement of every authority the adapter itself holds; and
6. reproducible commands starting from the named clean image snapshot.

The bundle cannot vendor or inspect Agent Seat implementation internals to make
a behavioral assertion pass. It may use documented commands and public
process, socket, filesystem, X11, logind, and device behavior.

## Gate result

The gate passes only after one reviewed harness adapter passes all harness
fixtures and one isolated full-system runner passes every applicable
replacement and lock fixture for an exact compatibility row. A different
harness, display manager, greeter, package build, or lock mechanism does not
inherit the result.

Until then:

- the harness negative-authority gate is incomplete;
- physical replacement compatibility is incomplete;
- the LightDM lock candidate is unsupported; and
- pointer click and keyboard operations remain absent.
