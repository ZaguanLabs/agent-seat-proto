# Tier 0.5 volatile seat gate

Status: experimental provider profile, 2026-08-09. This is a working local
operator gate and the runtime prerequisite for the revision-5 X11 input
surface, not a new wire assurance value or a physical-user priority claim.

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

Every pointer or keyboard action additionally rechecks this generation after
fresh target validation while the X server is grabbed. It checks again after
synchronization before reporting `queued`. A concurrent disable can therefore
turn the result into `interrupted`; a text request checks between its bounded
character actions and may report a partial count.

The gate does not replace target validation and is not evidence of an unlocked
session or physical-user inactivity. Ordinary X11 cannot reliably distinguish
XTEST from physical input, so a physical event can overlap an agent action. An
automatic lock must not be treated as safe merely because some desktop
component requested `seat disable`.

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

## Settings integration

`agent-seat-settings` 0.1.7 shows the selected provider's volatile status in a
fourth `RUNTIME SEAT` state-rail node and in a dedicated Overview panel. The
panel provides manual Refresh, **Enable for this instance**, and immediate
**Disable now** controls. Enable requires a confirmation that names the current
provider lifetime; Disable revokes the generation without an extra dialog so
the operator stop remains prompt.

The GTK shell calls one typed `agent-seat-x11` library boundary. That boundary
performs the same selection-bound advertisement validation, private-socket
derivation, bounded I/O, and provider peer authentication as the command. It
does not duplicate the fixed control protocol, invoke a shell, or spawn
`agent-seat-x11`.

The Access page separately grants `input_pointer` and `input_keyboard`; neither
grant enables the runtime seat. Saved policy, active-policy evidence, and
volatile seat state remain separate facts. Opening Settings performs only a
status request. Opening, saving,
reloading, restoring, logging in, and unlocking never send Enable. The
display-independent policy model and terminal recovery commands never
initialize GTK, inspect X11, or contact the control plane. The command
interface remains available as the smallest auditable operator path.
