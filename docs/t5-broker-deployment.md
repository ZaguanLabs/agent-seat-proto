# T5 activity broker deployment contract

Status: candidate deployment review, 2026-08-08. This document does not
approve the broker, allocate a protocol revision, or authorize implementation.
It refines the privileged candidate in
[`t5-input-reconsideration.md`](t5-input-reconsideration.md) and records the
remaining stop condition.

## Verdict

An unprivileged runtime broker can plausibly receive an exact, administrator-
enrolled set of read-only evdev descriptors from the system service manager.
That avoids retained root, broad input-group membership, and provider access
to `/dev/input`. Device changes can fail closed rather than dynamically
expanding the set.

The deployment gate still does not pass for generic Openbox. systemd-logind's
locked state is a desktop-provided hint, not an independently enforced lock
signal. Stock Openbox does not own a screen locker or provide a stronger
contract. A false unlocked hint would leave the broker armed across an
automatic lock. Until a separately trusted lock-state integration is specified
and tested, no broker or Tier 0 input implementation begins.

## Supported deployment candidate

The candidate is deliberately narrower than the Tier 0 core:

- Linux with systemd 253 or newer, evdev, udev, and a local system manager;
- one explicitly enrolled local physical seat;
- one active, non-remote `x11` session of class `user` for the enrolled UID;
- an independently trusted lock-state source that fails closed; and
- released Openbox only after that complete deployment is entered in the
  compatibility matrix.

Non-systemd systems, remote sessions, nested X servers, software keyboards,
accessibility input, ambiguous seats, and sessions without trusted lock state
remain unsupported. This optional baseline does not change the core product's
portable revision-3 behavior.

## Components and authority

### Administrator enrollment tool

A separately packaged, short-lived tool runs only through an explicit root
invocation. It may:

- enumerate udev metadata and evdev capabilities without reading event data;
- create or replace a root-owned enrollment record and runtime service
  definition;
- ask the system manager to reload, start, stop, or restart only the broker
  units; and
- report the exact enrolled seat, UID, stable device identities, and unmet
  requirements to the administrator.

It must not read evdev events, connect to X11, launch desktop applications,
edit Agent Seat provider policy, grant an Agent Seat capability, enable itself
from package presets, or remain resident. Settings, the provider, the MCP
companion, and the first-run workflow never invoke it.

### System service manager

PID 1 is the existing trusted OS component that opens each enrolled event node
read-only with `OpenFile=` and passes the descriptors to the service. The same
mechanism may pass a pre-connected, policy-restricted session-state channel and
the socket unit passes the provider listener. Passed descriptors do not grant
the broker permission to open another device or socket.

`OpenFile=` was added in systemd 253. The profile must refuse older managers;
it must not fall back to root execution, an input group, broad ACLs, or direct
provider device access.

### Runtime activity broker

The long-running broker uses a dedicated static system account with no login
shell, home directory, supplementary groups, or capabilities. It receives
only:

- the exact enrolled read-only evdev descriptors;
- one socket-activated provider listener; and
- the minimum read-only session-state source approved by the lock-state gate.

It reads events only to advance an epoch or enter a stopped/unavailable state.
It never sends or stores event type, code, value, coordinate, device identity,
or event timestamp. It has no X11, MCP, Agent Seat wire, launch, grant, policy,
uinput, XTEST, or administrative surface.

### Existing Agent Seat processes

`agent-seat-x11` receives readiness, instance, epoch, and stop state only. It
never receives an evdev descriptor and cannot clear a stopped broker. The MCP
companion and harness have no broker protocol. The existing companion's X11
selection discovery does not expand.

## Explicit installation and enablement

The broker is a separate optional package, not a dependency of the Tier 0 core
packages. Installation creates disabled unit templates, the dedicated system
account, root-owned configuration directories, and documentation. It applies
no enable preset, udev permission rule, input-group membership, provider grant,
or Settings change.

Enrollment is a distinct administrator action that names exactly:

- one seat, initially restricted to `seat0`;
- one numeric session-owner UID;
- the complete reviewed device set; and
- the approved lock-state integration.

The tool shows the complete candidate record before replacement. Confirmation
is never inferred from package installation, an MCP call, a provider policy,
or a prior enrollment for another boot or device set.

Starting or rearming the broker is also an administrator action. The initial
profile does not automatically rearm after physical activity, failure, lock,
session loss, or reboot. This is intentionally inconvenient: automatic rearm
would weaken the local emergency-stop contract before a separately reviewed
human consent mechanism exists.

## Device enrollment and completeness

### Candidate set

Enrollment enumerates current `event*` nodes from the udev database. A node is
conservatively relevant when it has `ID_INPUT=1` and any of these properties:

- `ID_INPUT_KEYBOARD=1` or `ID_INPUT_KEY=1`;
- `ID_INPUT_MOUSE=1` or `ID_INPUT_TOUCHPAD=1`;
- `ID_INPUT_TOUCHSCREEN=1`; or
- `ID_INPUT_TABLET=1`.

`ID_SEAT` selects the physical seat; an absent value means `seat0`, matching
libinput's documented default. Power, consumer-control, and virtual devices
may be included conservatively. An extra event can only stop the broker. A
device cannot be excluded merely because its name, vendor, bus, or udev
metadata suggests that it is virtual or uninteresting.

This set covers only local kernel-seat activity. Input that does not traverse
an enrolled evdev device is outside the candidate feature and keeps the
deployment unsupported if it can operate the local session.

### Stable enrollment

The root-owned enrollment record stores a bounded canonical sysfs path and the
minimum immutable identity needed to detect replacement for every node. Raw
event data, current key state, human-readable device names, and `/dev/input`
minor numbers are not stored as identity. The exact record format remains
unallocated until this deployment contract is approved.

At every arm or restart, the administrator tool resolves the enrolled records
against a fresh enumeration. The current relevant set must equal the enrolled
set exactly. Missing, additional, replaced, ambiguous, relative, symlink-raced,
or over-bound entries stop startup. The tool then creates a runtime-only unit
definition with one read-only `OpenFile=` entry per resolved event node.

The broker validates every inherited descriptor before readiness:

- descriptor count and names equal the enrollment record;
- each descriptor is read-only, nonblocking, and a Linux input event device;
- current sysfs and udev identity still matches enrollment;
- evdev capability bits still classify the device as relevant; and
- initial key/button state and input queues are safe and synchronized.

The broker has `DevicePolicy=strict` with no `DeviceAllow=` entry. It therefore
cannot open an event node itself even if discretionary file permissions would
otherwise allow it. The inherited descriptors are the complete authority.

### Hotplug and replacement

The broker never adds a descriptor at runtime. It watches the bounded udev and
sysfs view needed to detect changes and periodically resamples it before a
status reply. Any add, remove, change, identity mismatch, descriptor hangup,
read failure, or set mismatch advances the epoch, makes state unavailable, and
closes all event descriptors.

Recovery requires a fresh administrator arm. If the same enrolled identities
resolve safely, the service manager opens a new exact set. Otherwise explicit
reenrollment is required. A udev rule never grants the broker a broader group
or automatically restarts it into readiness.

## Session and lock binding

The broker is eligible only while all session evidence is simultaneously
true:

- the provider peer UID is the enrolled UID;
- the named session belongs to that UID and exact seat;
- session type is `x11`, class is `user`, state is active, and remote is false;
- the seat's active session is that same session;
- system sleep or shutdown is not being prepared; and
- the trusted lock integration states that the session is unlocked.

Unknown fields, new enum values, lookup errors, notification loss, conflicting
sessions, inactive state, VT switch, logout, suspend, shutdown, lock, or lock-
source loss immediately stop the broker and close device descriptors.

logind `TakeControl()` and `TakeDevice()` remain forbidden. They would compete
with the display server's exclusive session controller. Reading `Active` and
other session metadata does not grant device control.

`LockedHint=false` is insufficient by itself. logind documents the value as a
hint set by the desktop environment, and Openbox supplies no general locker
contract. The deployment remains stopped until an independently trusted source
can prove both lock and unlock transitions, survive automatic locking, and be
made unavailable on disconnect or ambiguity. A wrapper around one preferred
lock command is insufficient because another lock path could bypass it.

## Broker state machine

The only public states are:

- `unavailable`: deployment, device, session, lock, IPC, or internal evidence
  is incomplete;
- `arming`: the exact device and session set is valid, but the bounded initial
  quiet/synchronization interval has not completed;
- `ready`: one provider may use the current instance and epoch as input-gate
  evidence; and
- `stopped`: physical activity or a safety transition latched the instance.

Each service start creates an unpredictable instance identifier and epoch 1.
The identifier is not reused across restart. Any meaningful evdev packet while
ready advances the epoch exactly once and latches `stopped` before another
status reply. `SYN_DROPPED`, partial packets, impossible state, queue overflow,
or timestamp/order ambiguity latch `unavailable`; the broker never attempts to
reconstruct what happened for Agent Seat.

Activity during `arming` restarts the quiet interval. No event contents leave
the process. Once `stopped` or `unavailable`, provider IPC cannot rearm, clear,
or downgrade the state. Administrative restart is the only initial recovery
path.

## Provider IPC

The IPC is a private pathname `AF_UNIX` socket created by a system socket unit.
Enrollment sets its owner to the enrolled UID and its mode to 0600. The broker
accepts one bounded provider connection; kernel peer credentials must match
the enrolled UID. Same-user X11 is not an isolation boundary, so process names,
executable paths, environment, and client-supplied PIDs do not authenticate.

The protocol is separately versioned from Agent Seat wire revision 3. It has a
small fixed frame bound and only two operations:

- open with the claimed opaque session identifier; and
- read current broker instance, state, epoch, and a coarse closed reason.

There is no event subscription, device list, activity timestamp, raw value,
history, clear, rearm, configure, enumerate, launch, or injection operation.
Unknown revisions, fields, operations, enum values, oversized frames, extra
connections, or session changes close the connection and make provider input
unavailable.

Coarse reasons distinguish administrator action needed, session unavailable,
device evidence lost, activity stopped, and internal failure. They never
identify a device or physical event. The broker logs startup, terminal reason,
and bounded diagnostics only; it does not log activity packets, epochs, device
names, session activity timing, or successful status queries.

## Runtime confinement contract

The service definition must name and test each mechanism rather than rely on a
hardening score. The candidate minimum is:

- `User=` and `Group=` set to the dedicated account, with no supplementary
  groups, capabilities, setuid transition, or writable home;
- exact read-only `OpenFile=` descriptors supplied by PID 1;
- `DevicePolicy=strict` and no device allow-list;
- all network families disabled and only pre-opened/socket-activated local
  descriptors available;
- a read-only filesystem namespace exposing only the executable, dynamic
  loader/libraries, bounded udev/sysfs/session evidence, and runtime socket;
- `/home`, Xauthority locations, X11 filesystem and abstract sockets, other
  users' runtime directories, ordinary `/proc` process metadata, kernel
  interfaces, and unrelated `/run` state inaccessible;
- no writable executable mapping, executable temporary storage, arbitrary
  `execve`, ptrace, mount, namespace creation, BPF, perf, module, keyring,
  reboot, clock, raw-I/O, or device-creation path;
- native syscall architecture, a minimal syscall allow-list, no new
  privileges, an empty capability bounding and ambient set, and locked
  personality; and
- fixed memory, task, descriptor, frame, queue, CPU, and watchdog bounds with
  core dumps disabled.

The exact unit directives depend on the approved session/lock channel and must
be exercised on every supported systemd/kernel combination. Namespace or
seccomp setup failure is a service-start failure, never a warning followed by
reduced confinement.

Negative tests run the installed unit and prove that it cannot create or
connect an X11 socket, open any input node, execute a supplied fixture, read an
ordinary user's process/environment/home/runtime metadata, create a network or
device socket, write outside its private runtime state, or send raw event data.
Tests also prove that inherited event descriptors remain readable despite the
device-open denial and that no unexpected descriptor survives startup.

## Upgrade and removal

An upgrade stops and leaves the broker stopped before replacing any binary,
unit, or policy. It validates the old enrollment against the new exact format
but never automatically rearms. Unknown or migrated policy requires a new
administrator review. Runtime instance IDs, sockets, descriptors, and generated
unit fragments are never preserved across upgrade.

Ordinary package removal stops and disables the units, removes runtime state,
and removes the dedicated account only when the package manager can do so
safely. Root-owned enrollment remains inert for review. An explicit purge may
remove it after showing the exact path. Removal never edits provider policy or
another package's udev rules and never leaves group membership or device ACLs.

## Approval ledger

- **Authority split:** candidate pass. Runtime evdev stays outside the provider
  and the broker is unprivileged with exact inherited descriptors.
- **Administrator action:** candidate pass. Package install, enrollment, arm,
  rearm, and reenrollment are distinct and never driven by Agent Seat peers.
- **Device completeness:** candidate, needs deterministic enumeration and
  hotplug tests. Exact-set enrollment is fail closed but not yet proven.
- **Retained privilege and confinement:** candidate, needs an installable unit
  and negative tests on the compatibility matrix.
- **IPC minimization:** candidate pass at design level; schema remains
  unallocated and implementation is forbidden.
- **Session activity and VT lifecycle:** candidate, needs deterministic logind
  tests without `TakeControl()`.
- **Lock lifecycle:** fail. Generic Openbox has no independently trusted lock
  state satisfying the contract.
- **Overall deployment gate:** fail until every item passes. No code milestone
  is authorized.

## Standards consulted

- Linux evdev event and loss semantics:
  <https://docs.kernel.org/input/event-codes.html>
- udev dynamic device and permission model:
  <https://www.freedesktop.org/software/systemd/man/latest/udev.html>
- libinput udev seat and input properties:
  <https://wayland.freedesktop.org/libinput/doc/latest/device-configuration-via-udev.html>
- systemd service `OpenFile=` descriptor passing:
  <https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html>
- systemd device access controls:
  <https://www.freedesktop.org/software/systemd/man/latest/systemd.resource-control.html>
- systemd process and filesystem confinement:
  <https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html>
- systemd-logind session objects, lock hints, and device control:
  <https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.login1.html>
