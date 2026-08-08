# T5 input reconsideration

Status: threat-model review, 2026-08-08. The ordinary-X11 stop decision still
holds. This review identifies one candidate architecture, but does not approve
its new privileged authority or allocate a wire revision.

This document is a stop sign with approval gates, not an implementation plan.
Neither interest in T5 nor acceptance of this review authorizes broker code,
new device permissions, XTEST calls, protocol allocation, or an install-time
service. Observation, management, and launch remain the shippable Tier 0
profile if the gates cannot be passed without weakening the promise below.

## User outcome

The useful outcome is not merely that an agent can make X11 synthesize an
event. A qualifying input profile must:

- act only on a freshly scoped target;
- stop promptly when the local person uses a seat input device;
- bound how much input can race with that stop;
- fail closed when activity evidence, target evidence, or backend state is
  incomplete; and
- state only what was queued and observed, never that an application accepted
  or understood the input.

Mouse movement, clicks, keys, and text are absent until this complete contract
can be met. Opening a client or changing its workspace does not imply input
authority.

These promises are non-negotiable release criteria. Failure to realize them on
Openbox/X11 stops T5; it is not a reason to weaken the contract, rename an
uncertain result as success, or loosen a test.

## Threat model

The session owner explicitly enables the optional profile and chooses its
grants. The provider, its strict policy, and any separately installed activity
broker are trusted. The MCP companion, model, harness, launched applications,
other X11 clients, and all of their metadata are untrusted.

Same-user X11 remains unable to isolate clients from one another. The profile
must nevertheless prevent accidental input from being redirected to a client
outside the granted scope during the provider's own operation. It does not
claim to stop another same-user X11 client from independently injecting input
or changing desktop state.

The new sensitive asset is raw kernel seat input. Access to an evdev descriptor
can reveal keys even when Agent Seat only needs an activity bit. Granting that
access to the existing user process would enlarge its authority beyond the X11
session and could remain effective at a lock screen or another virtual
terminal. Device names, vendor IDs, udev properties, XInput source IDs, and
timing are descriptive data, not proof that a person generated an event.

## Process authority inventory

This inventory describes authority granted or exercised by Agent Seat. It is
not a claim that same-user X11 is an OS sandbox: a separately written process
that already has the session cookie can bypass Agent Seat entirely. The MCP
companion's existing X11 selection lookup is discovery only and must not grow
into desktop observation or control.

```text
agent harness:
    required: MCP calls to the companion
    forbidden: Agent Seat X11 realization, evdev, broker control

agent-seat-mcp:
    required: static translation, provider IPC, selection-only X11 discovery
    forbidden: desktop realization, XTEST, evdev, grants, launch realization

agent-seat-x11:
    required: X11 observation/manage, controlled launch
    candidate: XTEST only while an approved input profile is active
    forbidden: evdev, uinput, broker administration, raw activity details

candidate activity broker:
    required: configured seat evdev, readiness evaluation, minimal IPC
    forbidden: X11, MCP, Agent Seat wire, launch, injection, policy grants
```

Any feature that moves an authority from one row into another requires a new
architecture and threat-model review. In particular, no convenience fallback
may let the provider open `/dev/input/event*` or let the broker connect to the
X server.

## Evidence and rejected shortcuts

### XTEST alone

XTEST can synthesize core key, button, and pointer events, and those events
take part in propagation and grabs like device input. The resulting event does
not carry an authenticated origin marker. X RECORD or XInput observation
cannot turn ordering, timing, or a server device ID into trusted human-activity
evidence. This remains unsuitable for the suppression promise.

### Direct evdev access in `agent-seat-x11`

Linux evdev supplies device event packets independently of X11. It can
therefore observe kernel-seat activity without confusing it with XTEST output.
It also exposes the raw event stream to the reader. Adding the provider user to
an input group, granting broad ACLs, or retaining device descriptors across
session changes would silently create keylogging-grade authority. This is not
an acceptable default or first-run side effect.

An evdev reader must also treat `SYN_DROPPED` as loss of evidence, discard the
incomplete packet, resynchronize device state, and keep injection disabled
until a complete safe state is established. Device removal, hotplug, read
failure, and an unknown seat are equivalent fail-closed conditions.

### logind device ownership

The logind `TakeDevice()` path provides active-session revocation, but only to
the session controller. `TakeControl()` is exclusive: one D-Bus connection can
control a session, and an existing controller cannot be displaced without a
root-only forced takeover. Beside an unmodified Xorg session, Agent Seat must
not replace or disrupt the display server's controller merely to observe
activity.

### uinput injection

Linux uinput can create a virtual keyboard or pointer whose events flow to
kernel and userspace consumers. It needs additional device authority and does
not solve target binding: the resulting input still follows the display
server's current focus and hit testing. T5 therefore has no reason to add
uinput authority when XTEST already supplies the narrower X11 realization.

## Candidate: privileged activity broker

The only candidate found in this review keeps XTEST realization in
`agent-seat-x11` and places raw evdev access in a new, separately installed
system broker. This is a new product authority, not an implementation detail
or an automatic upgrade.

The broker contract would be deliberately smaller than evdev:

- an administrator, not Settings or an MCP peer, installs and enables it;
- it opens the complete configured keyboard and pointer set for one physical
  seat and validates the requesting provider's local credentials and active
  graphical session;
- it sends only a monotonic activity epoch, readiness state, and latched stop
  state—never event type, key code, button, coordinate, device name, or timing
  history;
- any qualifying physical activity advances the epoch; Agent Seat's XTEST
  output does not traverse this broker;
- unknown session state, inactive session, missing device coverage, hotplug,
  overflow, descriptor loss, broker restart, or IPC loss disables injection;
  and
- it has no X11 connection, Agent Seat wire listener, MCP surface, policy
  grant, launch authority, or input-injection device.

Even this split increases installation and compromise risk: the broker process
can read raw input internally, regardless of how little it sends. Its service
model, device enumeration, session binding, privilege dropping, confinement,
package ownership, upgrade behavior, and uninstall path need explicit
maintainer approval and a separate deployment review before code is added.

Remote desktops, nested servers, software keyboards, accessibility input, and
devices not represented by the broker's physical seat set remain unsupported.
The feature must describe `kernel_seat_activity`, not claim generic proof of
all human activity.

## Unresolved deployment decisions

The IPC shape is not the unresolved part: only readiness, a monotonic activity
epoch, and a latched stop state may cross it. The following questions block
implementation:

- **Installation:** define a deliberate administrator-controlled package and
  enablement action. The provider, companion, Settings, and first-run workflow
  must never install, enable, or elevate the broker.
- **Device coverage:** define how the broker proves it has the complete
  keyboard and pointer set for exactly one physical seat. A partial,
  ambiguous, remote, nested, or changing set is unavailable, not degraded.
- **Lifecycle:** specify detection of hotplug, suspend, lock, VT switch,
  inactive session, logout, broker restart, overflow, and device loss. Each
  transition immediately makes input unavailable and latches it off until a
  complete safe state is re-established.
- **Retained privilege:** define opening ownership, UID/GID changes,
  capabilities, namespaces, filesystem and `/proc` visibility, executable
  access, resource limits, syscall restrictions, crash behavior, and log data.
- **IPC exposure:** define the private pathname, owner and mode, peer-credential
  checks, session binding, framing bounds, reconnect rules, and denial of raw
  event fields. No peer chooses devices or clears a latched stop remotely.
- **Maintenance:** define which package owns policy and service files, how
  upgrades preserve fail-closed state, and how uninstall removes authority and
  stale runtime objects.

Answers belong in a separately approved deployment contract. An
implementation cannot be used to discover the answers after the fact.

## Candidate X11 action contract

If the broker authority is approved, every input request must still pass a
fresh action gate in the provider:

1. Require separate input and target-observation grants, broker readiness, an
   unchanged activity epoch, and a configured quiet interval.
2. Acquire the X server grab and resample the target, scope, generation,
   active client, focus ancestry, geometry, and relevant hit-test evidence.
3. Recheck the activity epoch. Refuse without XTEST if any evidence changed.
4. Send at most one bounded atomic action, synchronize the X connection, and
   release the server grab.
5. Recheck broker state and report `queued`, `interrupted`, `target_gone`,
   `stale`, `refused`, `unsupported`, or `failed` without claiming application
   acceptance.

A key action is one press/release pair with bounded modifiers. A button action
is one press/release pair. A pointer move is one destination. Text, if later
approved, is a bounded sequence of independently gated key actions and may
return a partial count. No request may leave a key, modifier, or button
logically pressed after failure or cancellation.

The server grab prevents other X11 clients from changing focus or stacking
through requests during the provider's check/send boundary. It does not stop
physical input or prove application handling. Because activity notification
is asynchronous, one atomic action may overlap with the person's first event;
the profile must say so. Larger batches, holds, drags, workflows, remembered
coordinates, and background input are outside this candidate.

Pointer actions additionally require a current, unobscured destination whose
X11 hit-test ancestry is the scoped target. A target-relative rectangle alone
is insufficient because another top-level or override-redirect window may
cover it. Keyboard actions require the actual input focus to be the target or
one of its descendants; the provider must not force focus as a fallback.

## Verification before implementation

The first code milestone, if the authority and deployment design are approved,
is the hostile test boundary—not XTEST realization. It must establish both
positive behavior and absence of authority.

Negative-authority tests must show that:

- the broker's deployed confinement cannot reach an X11 socket, execute an
  arbitrary program, inspect ordinary application metadata, open unrelated
  files, inject input, or expose raw input fields over IPC;
- the provider cannot open evdev or uinput devices, including when the test
  account would otherwise have filesystem permission;
- the companion cannot access broker IPC, XTEST, raw input, grants, or launch
  realization, and its X11 use remains selection-only discovery; and
- the harness reaches desktop authority only through the companion/provider
  protocols and cannot administer the broker through either one.

The enforcement mechanism for each `cannot` must be named and exercised. A
source-code convention, missing dependency, or currently restrictive file mode
is useful defense in depth but is not by itself proof of runtime confinement.

The hostile action suite must deterministically cover:

- missing or partial device coverage, `SYN_DROPPED`, hotplug, device removal,
  broker restart, IPC loss, suspend, lock, logout, inactive session, and VT
  switch;
- activity before the first epoch check, between both epoch checks, during the
  single action, and immediately after synchronization;
- scope, generation, focus, stacking, geometry, and hit-test changes at every
  check boundary, including a new override-redirect window over the point;
- target destruction during the server grab and X11 errors at every send and
  synchronization boundary;
- press/release failure at each partial modifier, key, and button state, with
  observable cleanup and no logically held input; and
- exact typed results and the documented maximum of one overlapping atomic
  action, without accepting logs as pass evidence.

Tests use isolated synthetic evdev fixtures and Xvfb/Openbox, never the
person's live display or input devices. If a race cannot be made deterministic,
the contract is not implementable yet and T5 remains stopped.

## Approval gates

No revision-4 schema or implementation begins until all of these are accepted:

- **Authority gate:** approve or reject the new system broker and its raw-input
  risk; ordinary user-group or broad ACL access is not the substitute.
- **Deployment gate:** specify session binding, complete device coverage,
  confinement, install/upgrade/uninstall ownership, and inactive-session
  revocation behavior.
- **Contract gate:** approve the one-action race bound, local physical-seat
  limitation, typed outcomes, and separate capability/feature names.
- **Target gate:** prove focus ancestry and unobscured pointer hit testing in
  isolated Xvfb/Openbox tests without inspecting unrelated client content.
- **Interruption gate:** prove pre-action refusal, between-action cancellation,
  overflow/hotplug/disconnect latching, cleanup of pressed state, and the
  documented worst-case single-action overlap.
- **Negative-authority gate:** name and test the confinement that prevents each
  process from acquiring the forbidden authorities in the inventory.
- **Test-first gate:** approve deterministic hostile fixtures and land their
  failure contracts before XTEST realization.

If any gate cannot be met, T5 remains unsupported. The existing revision-3
feature list and MCP surface do not change.

The governing rule is: Tier 0 must never acquire extra machine authority merely
to imitate a guarantee that Tier 1 gets naturally from owning the display
server. A useful Tier 0 release does not depend on input support.

## Standards consulted

- XTEST extension protocol:
  <https://www.x.org/releases/X11R7.7/doc/xextproto/xtest.html>
- Linux evdev event codes and loss handling:
  <https://docs.kernel.org/input/event-codes.html>
- Linux uinput interface:
  <https://docs.kernel.org/input/uinput.html>
- systemd-logind session control and device access:
  <https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.login1.html>
