# T5 lock-state integration study

Status: candidate integration review, 2026-08-08. Broker experiments are now
approved, but this document does not approve a lock integration or supported
input deployment. It refines the lock-lifecycle stop condition in
[`t5-broker-deployment.md`](t5-broker-deployment.md).

## Verdict

Generic Openbox does not provide a trustworthy lock boundary for Tier 0
input. An X11 window, screen-saver state, desktop D-Bus service, or logind
`LockedHint` can describe an in-session lock but cannot prove that the broker
lost access before credentials were entered. Those signals fail this gate.

One narrow candidate remains: a supported display manager must move the
physical seat away from the enrolled Openbox user session and activate a
distinct greeter or lock-screen session. The broker trusts the logind
foreground-session transition, not a lock window or unlocked hint. It closes
all evdev descriptors and latches off as soon as the enrolled session stops
being the unambiguous active session. Returning to the user session never
rearms it.

LightDM exposes a seat-lock command that switches to a greeter, so an exact
released LightDM, greeter, systemd, and Openbox combination is a plausible
first compatibility candidate. This is not yet evidence that every lock path
has the required ordering. The lock gate remains failed until isolated
full-system tests prove it.

## Required guarantee

The broker may be ready only while one enrolled local X11 user session is the
sole foreground session of its physical seat. Before a lock surface can
accept authentication input, the system must make that user session inactive
and make a different greeter or lock-screen session active. The broker must
observe the loss of eligibility, latch a terminal state, and close every raw
input descriptor.

Session change notification is asynchronous. The existing one-action contract
therefore still permits at most one already-gated atomic agent action to
overlap the first transition away from the user session. It does not permit
continued input, a batch, a held key or button, or rearming on return.

The same asynchrony creates the candidate's central unresolved question:
changing logind state does not by itself prove that the broker processed the
change before the greeter became input-capable. The compatibility test must
establish that ordering or identify an independently enforced pre-input
barrier. If the broker can still hold evdev descriptors when the greeter can
receive a credential event, the candidate fails even though the user session
is already inactive.

This study does not claim that the broker can prevent a system administrator
or the enrolled user from replacing the configured locker. It defines the
only lock architecture eligible for a supported deployment. Installing or
using another lock route makes that deployment unsupported and must fail the
compatibility checks where it can be detected.

## Evidence that does not establish a lock boundary

### logind lock requests and hints

logind's `LockSession()` emits a `Lock` signal for a session manager to act
on; it does not itself create an authentication boundary. `LockedHint` is set
by the desktop environment and is explicitly a hint. A missing, false, stale,
or forged value must not make the broker ready. `Lock` and `Unlock` signals
may stop an already armed broker conservatively, but neither may authorize or
rearm it.

### X11 screen-saver, DPMS, and lock windows

The X11 Screen Saver Extension reports disabled, off, or on saver state,
whether blanking is internal or external, and idle time. It does not report
authentication state. DPMS reports display power state. A mapped full-screen
or override-redirect window is also only same-session X11 state. Same-user X11
clients can influence or imitate these observations, so they cannot preserve
raw-input readiness at a password surface.

The provider's live focus, stacking, geometry, and pointer hit tests remain
necessary action-target checks. They are not a substitute for revoking broker
readiness on lock.

### same-user D-Bus locker services

An `org.freedesktop.ScreenSaver`-style service is controlled by an ordinary
session process and may disappear, restart, report late, or be replaced by a
same-user peer. It is useful desktop integration, not an independent boundary
for a process holding keylogging-grade descriptors.

### command wrappers, hooks, and authentication callbacks

A wrapper around one lock command cannot cover another manual command,
automatic idle lock, suspend-triggered lock, or a replacement locker. A PAM
or greeter authentication callback is too late: credentials may already have
generated evdev events. A successful command return also says only that a
request was accepted, not that the seat transition completed safely.

## Candidate foreground-session contract

The initial candidate is eligible only when an administrator enrolls an exact
display-manager integration and every condition below holds:

- the enrolled session has the expected UID, physical seat, `x11` type,
  `user` class, local origin, and non-empty virtual terminal;
- logind identifies that exact session as active and as the seat's sole active
  session;
- no greeter, lock-screen, conflicting user, remote, or ambiguous session is
  simultaneously eligible for the enrolled seat;
- the supported lock operation changes the seat's active session to a
  distinct local session whose class is `greeter` or `lock-screen`, while the
  enrolled user session becomes inactive;
- that transition completes before the greeter can accept authentication
  input; and
- every supported manual, idle, suspend, and policy-driven lock path reaches
  the same transition.

The broker watches the enrolled session and seat through the separately
approved, read-only, fail-closed logind channel. It does not call
`TakeControl()`, `TakeDevice()`, `ActivateSession()`, `LockSession()`, or any
display-manager method. The display manager remains responsible for session
switching and authentication.

Any of these observations is terminal for the current broker instance:

- the enrolled session becomes inactive, closing, missing, remote, or changes
  seat, class, type, owner, or virtual terminal;
- the seat's active-session identity changes, disappears, conflicts with
  session state, or cannot be read atomically enough to resolve;
- a greeter, lock-screen, or another user session becomes active;
- a VT switch, logout, suspend preparation, shutdown preparation, display-
  manager restart, logind restart, bus disconnect, notification loss, or
  property read failure occurs; or
- the configured display-manager identity or compatibility evidence no longer
  matches enrollment.

The broker advances its epoch, enters `stopped` for an expected safety
transition or `unavailable` for lost evidence, and closes all evdev
descriptors before serving another ready response. State disagreement is not
resolved by waiting for a preferred value. Reconnection may improve the
diagnostic reason but cannot restore readiness in the same instance.

Unlock, greeter exit, return to the user's VT, or `Active=true` does not rearm
the broker. A fresh administrator arm must revalidate the exact deployment,
session, device set, and quiet interval.

## LightDM compatibility candidate

LightDM's documented `dm-tool lock` operation locks the current seat, while
`switch-to-greeter` switches to a greeter. Upstream release history records
both correct logind session-class handling and optional support for greeters
running inside sessions. That variability is why the exact session topology
must be observed rather than inferred from the product name. The documented
seat operations justify testing LightDM first; they do not make all LightDM
configurations supported.

An entry in the compatibility matrix must pin at least:

- the released LightDM version and package build;
- the released greeter and its configuration;
- the systemd-logind, kernel, and X server versions;
- the Openbox version and session launcher;
- the exact manual lock command and automatic idle-lock mechanism; and
- suspend/resume and VT-switch policy.

The candidate is rejected if the greeter runs inside the enrolled user's
logind session, the user session remains active while credentials are accepted,
the greeter session lacks an unambiguous seat/class/VT identity, any supported
lock path uses an in-session locker, or failure falls back to such a locker.
`LockedHint`, a LightDM D-Bus reply, or a visible greeter is never sufficient
pass evidence.

Generic Openbox without this exact display-manager integration remains
unsupported. Other display managers require their own independent contract
and compatibility evidence; similarity to LightDM is not inherited.

## Pre-implementation verification

These tests require an isolated VM or equivalent full boot with real logind
session and VT switching. Xvfb alone cannot prove this gate. Synthetic evdev
devices may be used only inside that isolated system. Tests never attach to a
person's live display or physical input devices.

The fixture must observe public session, seat, process, descriptor, socket,
and input behavior. Diagnostic logs can help explain a failure but cannot make
a test pass. Before broker implementation, an unprivileged recorder can prove
the display-manager ordering; later tests must exercise the installed broker
under its real confinement.

The suite must deterministically establish:

1. A manual lock makes the enrolled session inactive and the expected distinct
   greeter or lock-screen session active before the authentication field can
   receive a synthetic key event.
2. Automatic idle lock and suspend-triggered lock reach the identical boundary
   without an in-session fallback.
3. VT switch, another user session, logout, suspend, display-manager restart,
   greeter crash, logind restart, and session-state IPC loss each close every
   event descriptor and latch readiness off.
4. A false, true, missing, delayed, or same-user-modified `LockedHint` cannot
   authorize readiness and cannot prevent a conservative stop.
5. Screen-saver, DPMS, same-user locker D-Bus, focus, stacking, and lock-window
   changes cannot authorize readiness.
6. Unlock and return to the enrolled session do not rearm; only a fresh
   administrator arm can create a new instance after full validation.
7. Human activity immediately before, during, and after the session transition
   stops the broker, exposes no event content, and preserves the documented
   maximum overlap of one bounded atomic agent action.
8. Conflicting or reordered logind properties, signal loss, owner changes, and
   new enum values fail closed without a grace interval.
9. A lock request that fails before switching sessions leaves the desktop
   visibly unlocked but never restores an already stopped broker; a lock
   request that partially switches cannot leave input ready.
10. Package or configuration changes invalidate compatibility evidence and
    require administrator review rather than automatic rearm.

If the ordering cannot be observed deterministically, or if any credential
event can reach an authentication surface while the broker still owns evdev
descriptors, this candidate fails. Tests are not loosened to accommodate the
implementation.

The full-system image/reset manifest, greeter input barrier, virtual-device
controller, stable lock/replacement fixture IDs, and required evidence ordering
are defined in the
[`T5 participant contract`](t5-participation-contract.md). A container or
nested X server cannot submit those fixtures as full-system evidence.

## Decision ledger

- **Generic Openbox lock state:** fail. Openbox has no independent locker or
  authenticated lock-state contract.
- **`LockedHint` or lock-request signals:** fail as authorization evidence;
  acceptable only as additional stop triggers.
- **X11, DPMS, and same-user locker evidence:** fail.
- **Display-manager foreground-session transition:** candidate at design level;
  exact ordering and failure behavior remain unproven.
- **LightDM plus a separate greeter session:** first test candidate, not yet a
  supported profile.
- **Rearm after unlock:** forbidden. A new administrator arm is required.
- **Overall lock lifecycle gate:** fail until the full-system compatibility
  suite passes and the authority/deployment design receives explicit approval.

No broker schema, service, enrollment format, XTEST path, capability, or wire
revision is authorized by this study.

## Standards and upstream documentation consulted

- systemd-logind session/seat properties, classes, lock signals, and hints:
  <https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.login1.html>
- X11 Screen Saver Extension states and events:
  <https://www.x.org/releases/X11R7.7/doc/scrnsaverproto/saver.pdf>
- LightDM project and greeter model:
  <https://github.com/canonical/lightdm>
- LightDM `dm-tool` manual:
  <https://github.com/canonical/lightdm/blob/main/data/dm-tool.1>
- LightDM release history, including logind class and in-session greeters:
  <https://github.com/canonical/lightdm/blob/main/NEWS>
