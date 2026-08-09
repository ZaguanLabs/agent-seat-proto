# Tier 0.5 volatile seat gate

Status: experimental provider profile, 2026-08-09. This is a working local
operator gate, not a new wire assurance value and not approval of the complete
T5 input deployment.

## Purpose

`agent-seat-x11` is already the process through which every Agent Seat request
for an X11 desktop must pass. The Tier 0.5 profile makes that dependency an
explicit, deny-by-default runtime switch:

- no provider means no advertised socket and no Agent Seat operation;
- every new provider process starts with its runtime seat disabled;
- a disabled provider completes no Agent Seat handshake;
- enabling is explicit, volatile, and local to that provider instance;
- disabling revokes the current generation; and
- provider or X11-display death destroys the enabled state.

Saved `enabled = true` policy means only that the reviewed provider policy may
start. It does not enable the runtime seat. Runtime seat state is never written
to configuration, restored after restart, or inferred from login, unlock,
broker readiness, or an MCP request.

## Operator commands

Run these inside the same X11 login environment as the provider:

```sh
agent-seat-x11 seat status
agent-seat-x11 seat enable
agent-seat-x11 seat disable
```

The command validates the live selection-bound X11 advertisement, then derives
the provider's private control socket below `$XDG_RUNTIME_DIR/agent-seat` from
that advertised provider pathname. Equivalent `DISPLAY` spellings therefore
resolve the same provider. The socket is mode 0600 and the provider checks
the connecting process's kernel peer UID. The control protocol is private to
the X11 reference provider. It is not part of the Agent Seat wire protocol and
is never exposed as an MCP tool.

An authenticated Agent Seat session is bound to the enabled generation in
which it opened. Disable followed by enable does not revive it: its next
request receives `revoked`/`reconnect`, and a new handshake is required. A
peer that attempts a handshake while disabled receives `refused`.

## Exact guarantee and remaining race

The provider checks the gate before admitting a session and before dispatching
each request. A disable acknowledgement means later requests cannot begin in
the old generation. One request that was already executing may still finish;
the runtime switch is not process cancellation and does not claim otherwise.

The experimental pointer action has an additional gate check after target and
broker validation while the X server is grabbed. It also rechecks the gate
after synchronization before reporting `queued`. A concurrent disable can
therefore turn the result into `interrupted`, but the already documented bound
of one atomic action remains.

The gate does not replace the physical-activity broker, target validation,
session eligibility, device-loss handling, or the unresolved credential-
surface ordering test. In particular, an automatic lock must not be treated as
safe merely because some desktop component requested `seat disable`.

## Authority boundary

The private MCP companion receives one already-connected provider descriptor.
Its confinement profile has no X11 discovery, runtime-directory view, or
arbitrary execution authority, so it cannot reach this control plane.

The control socket is not isolation from an arbitrary malicious process
already running as the desktop UID. Such a process can authenticate to it, and
on ordinary X11 it commonly has other ways to affect the desktop anyway. The
Tier 0.5 claim is therefore a practical operator consent and kill switch for
the confined-companion deployment, not a same-UID security boundary.

## Login and launcher lifecycle

The contract is display-manager neutral. LightDM is one compatibility case,
not a protocol dependency. A session launcher participates correctly when:

1. it starts `agent-seat-x11` only inside the intended local X11 login;
2. it never runs `agent-seat-x11 seat enable` automatically;
3. logout terminates the provider or its X connection, removing its provider
   and control sockets; and
4. a later login creates a new provider process whose seat is disabled.

The reference private-device user unit has `Restart=no` and no `[Install]`
section. It does not persist an enabled latch and cannot be package-enabled.
Provider loss remains fail-closed even when a user manager survives logout,
because loss of the owned X11 display terminates the provider.

For LightDM, the first live compatibility check records the provider PID and
disabled/enabled status, logs out normally, logs in again, starts the provider
through the intended Openbox session path, and verifies a different PID plus
`Seat disabled (generation 0)`. It does not enter a password into an observed
greeter and does not claim the separate T5 lock-ordering gate passed.

Other display managers, `startx`, session supervisors, and compositor-hosted
future providers can implement the same lifecycle without copying a LightDM
hook. Each launcher still needs a process-level logout/relogin test because
similar startup conventions do not prove identical lifetime behavior.

## Future Settings integration

The Settings application may show provider presence, the volatile state, and
explicit Enable and Disable controls. It must distinguish saved policy state
from runtime seat state, warn that enable lasts only for the current provider
instance, and never enable automatically while opening, saving, logging in, or
unlocking. Adding that UI is a later milestone; the command interface is the
current auditable operator path.
