# T5 activity broker deployment contract

Status: experimental authority and implementation approved, 2026-08-08;
deployment remains gated. This document refines the privileged candidate in
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
automatic lock. The separate
[`t5-lock-state-study.md`](t5-lock-state-study.md) identifies a narrow
display-manager foreground-session transition as a candidate, but it remains
unproven. The experimental broker therefore requires a separate inherited
eligibility channel and permanently fails closed if that evidence is absent,
changes, or is lost. No supported Tier 0 input deployment begins until a
trusted producer for that channel passes the remaining gates.

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
released revision-3 core behavior. The experimental source extension allocates
revision 4 rather than changing revision 3 in place.

## Components and authority

### X11 provider private-device profile

The provider never opens an evdev or uinput node. The optional input profile
adds a separate systemd user unit with `PrivateDevices=yes`, strict device,
syscall, filesystem, capability, and resource bounds, while retaining only
local `AF_UNIX` connectivity needed for X11 and user-manager IPC. An explicit
`input.provider_private_devices = true` policy switch makes startup verify that
both `/dev/input` and `/dev/uinput` are absent. Namespace setup failure or a
visible path is terminal; direct foreground startup cannot silently retain the
same UID's device ACLs.

Controlled application launch cannot inherit this restriction. After applying
the existing desktop-entry policy and strict shell-free parser, the confined
provider runs fixed `/usr/bin/systemd-run --user --wait --collect` with one
bounded argument vector. The user manager creates a uniquely named transient
application unit outside the provider's private device namespace. The waiting
launcher remains counted by the provider's existing 64-child bound. No peer
selects a unit property, command path, environment key, or service name.

The source user unit has no `[Install]` section and is never enabled by package
preset. Starting it, like enabling the configuration switch, is a separate
user action. It requires neither root nor membership in the group owning event
nodes. Root remains confined to the separately reviewed broker enrollment and
lifecycle transactions.

### MCP companion private profile

The optional reference deployment does not run the translator with the
desktop user's ambient X11 and device view. `agent-seat-mcp
--print-private-mcp-config` emits a copyable MCP registration whose command is
fixed `/usr/bin/systemd-run`. The user manager uses `OpenFile=` to connect the
provider's fixed mode-0600 socket before namespace construction and passes that
connection as exactly one descriptor named `agent-seat-provider`.

The worker validates the activation PID, descriptor count, complete name, and
owned descriptor set before completing the Agent Seat handshake. It runs with
private network/device/tmp views, an empty read-only `/run`, hidden home and
other-process data, no desktop or socket-discovery environment, no capabilities
or arbitrary executable, seccomp, and fixed task, descriptor, memory, CPU, and
core-dump bounds. The already-connected descriptor remains usable after
`PrivateNetwork=yes`; X11 filesystem and abstract sockets and every other host
Unix socket do not. The harness owns only the MCP stdio pipes created by
`systemd-run --pipe`.

An inherited connected socket does not convey the transient unit's confinement
properties in its peer credentials. The provider does not infer them. After a
valid `Hello` and UID grant, the input profile assigns one session an idle slot
so MCP hosts may pause between calls without consuming the ordinary frame I/O
deadline. Initial handshakes and partial frames are still deadline-bound,
additional sessions receive no idle exception, and the provider checks its
shutdown flag at 100-millisecond intervals while waiting. Unit inspection and
the hostile gate below provide the negative-authority evidence.

### Administrator enrollment tool

A separately packaged, short-lived tool exposes read-only inspection and inert
unit rendering without privilege. Its installation and lifecycle commands run
only through an explicit root invocation. The tool may:

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

### Current inspection, rendering, and verification preflight

`agent-seat-activity-enroll inspect --seat seat0`, `render`, and `verify`
implement the unprivileged review portion of this boundary. They enumerate
canonical `eventN` entries from `/sys/class/input`, verify the matching direct
character nodes and stable udev/sysfs identity twice, and ask the fixed
`/usr/bin/udevadm` for only the bounded input-class, seat, topology, and
hardware-identity properties below.
Device count, command runtime, command output, property set, paths, values,
and report contents are bounded and strictly parsed.

The `inspect` command prints a deterministic review candidate containing
device nodes, canonical sysfs paths, coarse classes, and whether identity is
serial-backed or topology-only. The `render` command
requires an explicit nonzero UID, bounded logind session ID, and normalized
absolute path that does not already exist. It writes the exact current set as
four inert unit files, `initial-input-set.v1`, `enrolled-device-set.v1`, and
`REVIEW.txt` in a new mode-0700 directory; files are mode 0600 and are never
overwritten. It fixes
the installed executable paths to `/usr/bin`, maps only the reviewed relevant
nodes to broker `OpenFile=` entries, and records every bounded canonical
`/sys/class/input/event*` mapping in the separate human-readable manifest. The
manifest contains paths only, never event packets or device names.

The `verify` command regenerates the expected bundle from a fresh double-
checked seat enumeration. It requires a direct current-user-owned mode-0700
directory containing exactly the seven expected UTF-8 filenames. Every entry
must be one-link, direct, current-user-owned, mode 0600, within the fixed size
bound, unchanged while read, and byte-identical to the newly rendered UID,
session, device set, and unit sources. Extra, missing, replaced, symlinked,
permission-weakened, stale, or modified content fails verification. The
enumeration itself stops as soon as the fixed file-count bound is exceeded.

None of these three preflight commands opens a device, reads an event, retains
raw values, installs a unit, starts a service, changes permissions, or edits
provider policy. A successful inspection, render, or verification is therefore
evidence for review, not authorization to arm input. The relevant-device
record is bounded to 64 KiB and 32 sorted entries and has a strict canonical
parser.

`install --confirm-install` is a separate root-only transaction. It repeats the
current-seat inspection, verifies the review as owned by the enrolled UID,
requires the packaged broker and guard executables to be direct, root-owned,
one-link, executable, and not group/world writable, and publishes only new
mode-0600 root-owned files. No existing unit or enrollment file is replaced.
Publishing uses no-follow exclusive temporary files, synchronized contents,
no-replace renames, and exact rollback of files created by that transaction.
It does not reload, start, or enable anything.

`arm --confirm-arm` again inspects the seat, reconstructs the expected bytes,
and requires the installed three-file enrollment and four unit files to match
exactly before a bounded `daemon-reload`. It then stops the exact prior unit set
and starts one fresh broker-service cycle, so an active terminal broker cannot
be mistaken for a successful rearm. It does not enable startup. `stop` names
only those four UID-bound units. `purge
--confirm-purge` stops them, refuses unexpected enrollment content, removes
only the seven exact root-owned files and now-empty UID directory, then reloads
the manager. These transactions have passed isolated filesystem fault and
rollback tests. The new-only install and freshly verified arm paths have also
run against the current systemd-258 host and reached broker `Ready`; that host
was not purged merely to turn a destructive cleanup path into test evidence.

## Permission model

The inspection command and running eligibility guard need no elevated
privilege and no input-device group. The installed guard does require PID 1 to
pass the reviewed root-owned mode-0600 input-class manifest read-only; it never
opens that pathname or an event node itself. On systems where event nodes are
`root:input` mode 0660, the enrolled desktop user deliberately remains outside
`input`. Adding that
user would give every process in the desktop session continuous access to all
keyboard and pointer events, including events unrelated to Agent Seat.

The dynamic broker identity also has no `input` supplementary group. The system
manager performs the one administrative operation that ordinary Unix
permissions forbid: it opens each reviewed event node read-only and passes only
those exact descriptors with `OpenFile=`. Because systemd applies the service's
device cgroup while preparing those descriptors, the rendered unit also names
each reviewed node in an exact read-only `DeviceAllow=` entry. This permits the
manager-side open; it does not bypass DAC for the dynamic UID. The broker cannot
open the same node again, and `PrivateDevices=yes` removes the host nodes from
its namespace.

For isolated developer experiments, an administrator could place a dedicated,
non-login broker account in `input` or make `input` its primary group. That is
not the supported profile: it grants broad retained device authority, defeats
exact-descriptor confinement, still requires separate descriptor plumbing,
and must never be suggested for the desktop user's account. Removing the group
membership and starting a fresh login session are required after such an
experiment.

Concretely, do **not** run `usermod -aG input "$USER"`. If a developer chooses
the unsupported fallback, distribution-specific account tools must create a
separate system account with no login shell or home and add only that account
to the group owning `/dev/input/event*` (commonly `input`). Root access is
required for that account/group change. The broker must run as that dedicated
identity, and the administrator must remove the membership after the test.
This fallback is not available to the normal `DynamicUser=yes` profile and is
not a substitute for systemd 253 `OpenFile=` support.

## Current eligibility guard

`agent-seat-eligibility-guard` is a separate unprivileged process with no
input-event-node or X11 access. It accepts one socket-activated private local
eligibility listener, authenticates the connecting service-manager UID,
subscribes to logind signals
and the kernel kobject-uevent group, reconciles every live canonical
`/sys/class/input/event*` mapping against the bounded root-owned manifest, and
then checks the exact enrolled session UID and seat, active-seat identity,
local X11 user class/state, lock hint, and system sleep/shutdown state. It emits
only `eligible` or one terminal `ineligible` frame.

Any login1 property signal in the bounded namespace latches the guard off
conservatively. login1 owner replacement or D-Bus loss closes the channel and
makes broker evidence unavailable. Any valid input-subsystem uevent after the
subscription latches `ineligible`; truncated, malformed, non-kernel, oversized,
or unreadable uevent evidence closes the channel. The monitor is nonblocking,
bounded to 16 KiB and 128 fields, and checked after each at-most-10-millisecond
logind polling interval. It receives device lifecycle metadata, never evdev
event packets, keys, buttons, or coordinates.

The manifest is capped at 64 KiB and 256 sorted mappings. The guard accepts it
only through an inherited descriptor for a one-link mode-0600 regular file
owned by the already authenticated service-manager peer. The rendered system
profile fixes that peer to UID 0, so its installed manifest is root-owned; an
ordinary-user owner is accepted only when that UID is explicitly selected for
a non-production local test. The guard verifies the file unchanged while
reading and strictly rejects unknown revisions, duplicate or noncanonical
event numbers, malformed paths, trailing data, and bounds violations. A
persistent add, remove, or event reindex between review and guard startup
changes the mapping; a change concurrent with the scan is caught by the
already-open uevent monitor.

The current manifest-reconciling guard passed a live active-session handshake
as an explicitly ordinary-user local test. It now also passes that handshake
under the hardened transient profile: the service consumed the freshly
rendered manifest at fd 3, opened the real kernel uevent subscription, reached
the one re-exposed system-bus socket, and evaluated the real local X11 session.
The root-owned installed profile has now reached broker `Ready` with the
dynamic broker and locked static guard identities, exact named descriptors,
zero capabilities, seccomp, private device views, and no supplementary groups.
An installed no-event uinput relative device then produced a real kernel input
add notification. The guard exited, the broker reported
`Stopped`/`EligibilityChanged` at the next epoch, and inspection found zero
remaining event descriptors. After the device disappeared and udev settled,
the freshly verified `arm` path replaced that resident terminal broker with a
different ready instance at epoch one. No input event was emitted and no
physical device was changed. This proves installed synthetic lifecycle
delivery, not physical replacement compatibility. A same-host live check also
found XScreenSaver reporting the seat locked while logind still reported
`LockedHint=no`; the provider's topmost-input-window proof correctly refused
the pointer action. This does not make generic Openbox supported: the trusted
lock transition and hostile lock/unlock tests below remain open.

### System service manager

PID 1 is the existing trusted OS component that opens each enrolled event node
read-only with `OpenFile=`. It passes the exact installed relevant-device
record and event descriptors with bounded enrollment-derived names. It
separately passes the exact installed input-class manifest read-only to the
guard. For the broker, PID 1 connects the guard endpoint to standard input with
`StandardInput=file:` and places the socket-activated provider listener on
standard output with `StandardOutput=socket`. Each process validates systemd's
PID/count/name activation environment and safely takes ownership of the named
descriptors without reopening `/proc/self/fd/*` or accepting numeric descriptor
arguments. The broker safely duplicates only its connected standard streams
and never reconstructs a connected socket from an arbitrary raw descriptor.
Passed descriptors do not grant either process
permission to open another device or socket.

`OpenFile=` was added in systemd 253. The profile must refuse older managers;
it must not fall back to root execution, an input group, broad ACLs, or direct
provider device access.

### Runtime activity broker

The long-running broker uses a service-manager allocated dynamic UID with no
login shell, home directory, supplementary groups, or capabilities. It
receives only:

- the exact enrolled read-only evdev descriptors;
- one preconnected eligibility stream on standard input; and
- one socket-activated provider listener on standard output.

The eligibility stream must deliver one complete fixed frame within two
seconds. Timeout, partial data, closure, poll failure, or malformed data aborts
startup. After a usable initial decision, any later channel ambiguity is
terminal and cannot reassert readiness.

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
packages. Installation creates disabled unit templates, root-owned
configuration directories, a locked `agent-seat-guard` system identity, and
documentation. The evdev broker identity is allocated by `DynamicUser=yes`;
the provider pins UID 0 because PID 1 owns its socket-activated listener. The
guard needs a stable identity because system-bus policy does not reliably
authenticate dynamic users. The static guard has no home, login shell,
supplementary group, or owned state. Installation applies no
enable preset, udev permission rule, input-group membership, provider grant, or
Settings change.

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

The candidate `enrolled-device-set.v1` record stores each relevant event
number, bounded canonical sysfs path, coarse classes, udev `ID_PATH`, selected
bus/vendor/model/revision IDs, the kernel's complete event-capability and input-
property bitmaps, and `ID_SERIAL_SHORT` when available. Raw event
data, current key state, human-readable device names, derived non-unique serial
labels, and `/dev/input` minor numbers are not stored as identity.

Serial-backed entries can additionally detect a changed device identifier at
the same topology. Entries without `ID_SERIAL_SHORT` remain topology-only for
hardware identity, but the input promise requires coverage rather than device
attestation: current topology, classes, all kernel event-capability bitmaps,
and the complete event-node set must still match. A serial-less replacement
with any observable input difference is rejected; an exact clone is coverage-
equivalent and does not reduce the broker's ability to stop on its events.

At every arm or restart, the administrator tool resolves the enrolled records
against a fresh enumeration. The current relevant set must equal the enrolled
set exactly. Missing, additional, replaced, ambiguous, relative, symlink-raced,
or over-bound entries stop startup. The tool then creates a runtime-only unit
definition with one read-only `OpenFile=` entry per resolved event node.

The deployment must validate every inherited descriptor before readiness:

- descriptor count and names equal the enrollment record;
- each descriptor is read-only, nonblocking, and a Linux input event device;
- current sysfs and udev identity still matches enrollment;
- evdev capability bits still classify the device as relevant; and
- initial key/button state and input queues are safe and synchronized.

The current preflight renders and byte-verifies the bounded device identity
and capability record, including changed same-path serial or capability
evidence, and resamples relevant evidence before returning. Installation and
arming freshly bind that evidence to exact installed bytes. The broker reads
the named installed record descriptor and verifies that each inherited descriptor
is an evdev device with exactly the recorded capability and property bitmaps,
checks initial key/button state, drains its initial queue, and enters a bounded
quiet interval. The guard separately validates the complete current input-class
mapping. Descriptor identity and placement now pass under the real manager.
A same-host production-process inspection confirms the intended identities,
one task each, zero capabilities, no supplementary groups,
`NoNewPrivileges`, seccomp, private device views, and the exact inherited
descriptors. A repeatable hostile test also passes under the real system manager
with the production dynamic broker and static guard identity models. An exact
installed-unit-derived hostile fixture now passes for both identities while
changing only executable and inherited-channel test plumbing. Physical
replacement lifecycle tests remain. Until they pass, the service source is not
a supported deployment.
The broker has `DevicePolicy=strict` with one read-only allow entry per enrolled
node so systemd can prepare the corresponding `OpenFile=` descriptor. The
dynamic UID has no matching DAC permission and its private `/dev` contains no
host event node, so the inherited descriptors remain its complete device
authority.

### Hotplug and replacement

The broker never adds a descriptor at runtime. Descriptor hangup or read
failure advances the epoch, makes state unavailable, and closes all event
descriptors. Independently, the eligibility guard now subscribes to kernel
kobject uevents before calculating initial eligibility. Any input-subsystem
add, remove, change, or other valid lifecycle action observed after that point
latches eligibility off. Notification truncation, malformed framing, a
non-kernel sender, receive failure, or notification loss fails closed.

The guard now also rejects a current complete class mapping that differs from
the rendered manifest, after opening its notification subscription and before
initial eligibility. This catches persistent set changes before startup and
notifications during the scan. It does not prove immutable identity when a
device was replaced with the same eventual mapping before subscription.
Installation and arming now transactionally bind fresh bundle verification to
installed bytes, but PID 1 still opens descriptors only during the subsequent
service start. The installed no-event uinput transition proves that real kernel
input uevents reach the confined guard, cause a terminal broker state, and
close the inherited event descriptors. Physical add/remove/replacement cases
and the isolated compatibility matrix remain. Until those remaining checks
pass, runtime device completeness is unsupported.

The exact participant-facing removal, addition, serial change, capability
change, topology-clone, complete-set, notification-loss, and verification-to-
start race fixtures are enumerated in
[`t5-participation-contract.md`](../verification/t5-participation-contract.md). They require a
hypervisor-controlled virtual device topology; the live-host hotplug fixture
below proves only the narrower already recorded transition.

The repository's explicit `t5_hotplug_fixture` creates one relative-capability
uinput device for two seconds and emits no input event. It requires root access
to `/dev/uinput`, changes the host input class, and intentionally makes an armed
broker terminal; it is never run by the ordinary test suite. Prefer an isolated
system. On a reviewed development host, build and invoke it explicitly, wait
for udev to settle after removal, inspect the terminal broker state and open
descriptors, then perform a fresh administrator arm:

```sh
cargo build --release -p agent-seat-activity-broker \
  --example t5_hotplug_fixture
sudo ./target/release/examples/t5_hotplug_fixture
udevadm settle --timeout=5
sudo agent-seat-activity-enroll arm \
  --uid "$(id -u)" --session "$XDG_SESSION_ID" --confirm-arm
```

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

The candidate lock contract is specified in
[`t5-lock-state-study.md`](t5-lock-state-study.md). It requires a supported
display manager to make the enrolled user session inactive and activate a
distinct greeter or lock-screen session before credentials can be entered.
Any session transition or ambiguity latches the broker off, and returning to
the user session never rearms it. Generic Openbox and in-session lockers do not
satisfy that contract.

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

Activity during `arming` latches `stopped`; ambiguous or lost evidence latches
`unavailable`. No event contents leave the process. Once `stopped` or
`unavailable`, provider IPC cannot rearm, clear, or downgrade the state.
Administrative restart is the only initial recovery path.

## Provider IPC

The IPC is a private pathname `AF_UNIX` socket created by a system socket unit.
Enrollment sets its owner to the enrolled UID and its mode to 0600. The broker
accepts one bounded provider connection; kernel peer credentials must match
the enrolled UID. Same-user X11 is not an isolation boundary, so process names,
executable paths, environment, and client-supplied PIDs do not authenticate.

The protocol is separately versioned from Agent Seat wire revision 4. It has a
small fixed frame bound and one operation: read current broker instance, state,
epoch, and a coarse closed reason. The current experiment authenticates the
provider's enrolled numeric UID with kernel peer credentials. Binding the
broker to a separately verified opaque session identity remains part of the
eligibility-producer deployment gate.

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

- `DynamicUser=yes` for the evdev broker, and a locked dedicated static identity
  for the D-Bus eligibility guard; neither has supplementary groups,
  capabilities, a setuid transition, or a writable home;
- exact read-only `OpenFile=` descriptors supplied by PID 1;
- `DevicePolicy=strict` and an exact read-only device allow-list equal to the
  inherited enrolled nodes, needed only for manager-side `OpenFile=` setup;
- for the activity broker, all network families disabled and only
  the preconnected standard-input eligibility stream, standard-output
  socket-activated provider listener, and exact event descriptors available;
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

The current units place a read-only empty filesystem over `/run`; the guard
then bind-mounts only `/run/dbus/system_bus_socket` back into its namespace.
Both mark the entire filesystem non-executable and reopen only the packaged
executable and system library directories needed by the dynamic loader. The
steady-state processes are single-task, but `TasksMax=2` is required because
systemd briefly needs a second task to construct an unprivileged namespace;
the value 1 failed startup with `status=226/NAMESPACE` on systemd 258.

The exact unit directives depend on the approved session/lock channel and must
be exercised on every supported systemd/kernel combination. Namespace or
seccomp setup failure is a service-start failure, never a warning followed by
reduced confinement.

The eligibility guard is a separate authority with `AF_UNIX` for its fixed
local channel and `AF_NETLINK` only for the kernel kobject-uevent group. It
runs in a private network namespace and receives no internet address family,
input-event descriptor, or raw input packet. Its installed negative tests must
prove those distinctions rather than treating the broker's stricter network
rule as if it applied to both processes.

Negative tests run the installed unit and prove that it cannot create or
connect an X11 socket, open any input node, execute a supplied fixture, read an
ordinary user's process/environment/home/runtime metadata, create a network or
device socket, write outside its private runtime state, or send raw event data.
Tests also prove that inherited event descriptors remain readable despite the
device-open denial and that no unexpected descriptor survives startup.

The current explicit rootless systemd-258 probe covers both profiles with a
single-process hostile executable. It proves exact inherited read-only evidence
survives while home, unrelated runtime, other-process, input-node, host-socket,
new network-socket, desktop-environment, and direct-exec attempts fail. For the
guard it additionally proves that only the explicitly re-exposed system-bus
socket remains reachable. This is useful evidence for the directives and found
a real `TasksMax=1` startup defect, but it does not exercise the production
identities or system manager. A second explicitly privileged variant now runs
the same hostile executable as transient system units with the production
dynamic broker and static guard identities. It preserves only the exact
inherited read-only descriptor and guard system-bus channel while denying the
same unowned authority. It changes no persistent unit, enrollment, or running
provider.

A third explicit gate reads the root-owned installed service bytes, retains
every confinement directive, replaces only the executable, dependency,
standard-stream, and inherited-file plumbing needed by the hostile fixture,
and installs a uniquely named mode-0600 unit under `/run/systemd/system`. Both
production identity models pass. The test removes that exact volatile unit,
reloads the manager, and verifies separately that the live broker remains
untouched. It does not claim that a substituted executable is the production
binary or expand the installed runtime's authority.

Run the two probes explicitly; ordinary workspace tests ignore both:

```sh
cargo test -p agent-seat-activity-broker --test systemd_confinement \
  runtime_profiles_deny_unowned_authority_but_preserve_exact_inherited_access \
  -- --ignored
cargo test -p agent-seat-activity-broker --test systemd_confinement \
  production_identities_deny_unowned_authority_under_the_system_manager \
  -- --ignored
cargo test -p agent-seat-activity-broker --test systemd_installed_confinement \
  -- --ignored
```

The second command invokes fixed `/usr/bin/sudo -n /usr/bin/systemd-run`
internally and therefore requires an explicit passwordless administrator
authorization. It creates only bounded transient collected units.
The provider boundary has its own rootless gate:

```sh
cargo test -p agent-seat-x11 --test systemd_input_confinement \
  provider_loses_input_devices_while_delegated_application_keeps_baseline \
  -- --ignored
```

It captures the current user's uinput baseline, runs a hostile provider fixture
under the source unit's confinement properties, proves both input paths are
absent there, then delegates a second process through the user manager and
proves that process sees the ordinary host paths and the same baseline access.
Both services are transient and collected; no policy, installed unit, desktop,
or input event is changed.

The emitted companion profile also has a rootless hostile gate:

```sh
cargo test -p agent-seat-mcp --test systemd_private_companion \
  emitted_private_profile_exposes_only_the_provider_channel \
  -- --ignored
```

It requires a test UID that can open host uinput so device denial is meaningful.
Starting from the exact emitted arguments, it substitutes only a bound hostile
executable, proves the one named provider socket was connected, and denies
uinput/evdev visibility, a broker-like socket, X11, IP networking, home and
parent-process data, sensitive environment, and unapproved execution. The
transient unit is collected and the host sockets and filesystem are unchanged.

An additional explicit live guard probe passes under the same hardened
rootless profile against the current sysfs mapping, kernel uevent group, and
logind session. Every individual owner, seat, foreground, active, local, type,
class, state, unlocked, awake, and not-shutting-down predicate also has a
direct fail-closed unit case. Destructive transitions and the production
identity remain reserved for the isolated full-system suite.

## Upgrade and removal

An upgrade stops and leaves the broker stopped before replacing any binary,
unit, or policy. It validates the old enrollment against the new exact format
but never automatically rearms. Unknown or migrated policy requires a new
administrator review. Runtime instance IDs, sockets, descriptors, and generated
unit fragments are never preserved across upgrade.

Ordinary package removal stops and disables the units, removes runtime state,
and removes the packaged sysusers definition. The locked guard account may be
retained by the operating system's ordinary system-account policy; it owns no
Agent Seat state and belongs to no supplementary group. Root-owned enrollment
remains inert for review. An explicit purge may remove it after
showing the exact path. Removal never edits provider policy or another
package's udev rules and never leaves group membership or device ACLs.

## Approval ledger

- **Authority split:** candidate pass. Runtime evdev stays outside the provider
  and the broker is unprivileged with exact inherited descriptors.
- **Administrator action:** candidate pass. Package install, enrollment, arm,
  rearm, and reenrollment are distinct and never driven by Agent Seat peers.
- **Device completeness:** candidate. Bounded deterministic current-set
  inspection, hostile metadata parsing, exact inert unit rendering, and
  byte-exact current-set verification are implemented. New-only privileged
  publication, exact installed-byte arm verification, rollback, and scoped
  purge are fixture-tested. The guard compares the complete initial input-class
  manifest after subscribing and latches off on bounded later input-subsystem
  kernel notifications. Runtime descriptor placement and an installed no-event
  synthetic hotplug pass; physical replacement matrix tests remain.
- **Retained privilege and confinement:** candidate. Rootless hostile probes
  pass both profiles and preserve only their intended inherited/system-bus
  channels. The production authority inventory and production-identity
  system-manager hostile probes pass. Hostile probes derived from the exact
  installed broker and guard service bytes also pass without touching the live
  services. The provider-private-device hostile gate also passes while
  preserving ordinary application device access through bounded launch
  delegation. The emitted companion-profile gate passes with only its named
  provider descriptor; a live installed worker reached `seat_status` through
  that descriptor under the same confinement. Harness/product boundary
  evidence remains part of the complete compatibility matrix.
- **IPC minimization:** experimental implementation pass. Fixed status and
  eligibility frames expose no event, device, coordinate, or timestamp data.
- **Session activity and VT lifecycle:** candidate, needs deterministic logind
  tests without `TakeControl()`.
- **Lock lifecycle:** fail. A display-manager foreground-session transition is
  a design candidate, but its ordering and failure behavior have not passed
  isolated full-system tests. Generic Openbox remains unsupported.
- **Overall deployment gate:** fail until every item passes. Experimental code
  and explicit administrator transactions do not authorize a support claim.

## Standards consulted

- Linux evdev event and loss semantics:
  <https://docs.kernel.org/input/event-codes.html>
- udev dynamic device and permission model:
  <https://www.freedesktop.org/software/systemd/man/latest/udev.html>
- Linux netlink kobject-uevent transport and loss semantics:
  <https://man7.org/linux/man-pages/man7/netlink.7.html>
- Linux kernel kobject-uevent broadcast implementation:
  <https://github.com/torvalds/linux/blob/master/lib/kobject_uevent.c>
- libinput udev seat and input properties:
  <https://wayland.freedesktop.org/libinput/doc/latest/device-configuration-via-udev.html>
- systemd service `OpenFile=` descriptor passing:
  <https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html>
- systemd standard-stream and socket-activation descriptor placement:
  <https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html>
  and <https://www.freedesktop.org/software/systemd/man/latest/systemd.socket.html>
- systemd device access controls:
  <https://www.freedesktop.org/software/systemd/man/latest/systemd.resource-control.html>
- systemd process and filesystem confinement:
  <https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html>
- systemd system-account declarations:
  <https://www.freedesktop.org/software/systemd/man/latest/sysusers.d.html>
- systemd upstream discussion of D-Bus policy and dynamic identities:
  <https://github.com/systemd/systemd/issues/9503>
- systemd-logind session objects, lock hints, and device control:
  <https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.login1.html>
