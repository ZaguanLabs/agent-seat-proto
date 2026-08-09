# Standalone X11 provider

Status: T3 Tier 0 core plus an experimental T5 pointer slice.
`agent-seat-x11` 0.1.20 owns lifecycle, policy, local
authentication, X11 discovery, bounded EWMH observation, supported management,
and controlled desktop-entry launch without moving authority into the MCP
companion. The current implementation target is Linux X11 and its `SO_PEERCRED`
contract.

## Goals

- Start only from an explicit, valid, enabled configuration.
- Admit only the provider user's kernel-authenticated local socket peers.
- Compute a configured capability grant and recheck it on every request.
- Own one X11 screen without racing or replacing another conforming provider.
- Keep socket paths, frames, sessions, requests, waits, and configuration
  bounded.
- Expose only configured client scope, and expose titles only when both policy
  and the session grant permit them.
- Use per-session opaque client identities, generations, sequences, filtered
  diffs, and explicit resynchronization.
- Recheck scope, freshness, and exact advertised support immediately before an
  EWMH send, then report only what the provider subsequently observes.
- Discover a bounded XDG catalog in standard preference order, expose only
  policy-visible launchable entries, and execute parsed argument vectors
  without a shell.
- Treat client correlation as optional evidence: return a handle only for a
  newly visible scoped client with the exact launch startup ID.
- Remove the private socket and advertisement on clean shutdown while leaving
  Openbox or another window manager independent.
- Start every provider instance with a volatile disabled seat, admit no Agent
  Seat session until an explicit local enable, and revoke the current session
  generation on disable.

## Non-goals

- Treating a peer-supplied name, version, purpose, PID, title, or X11 property
  as authorization identity.
- Listening on a network or abstract socket, admitting another OS user, or
  daemonizing itself.
- Treating a sequence of independently sampled EWMH properties as an atomic
  window-manager transaction.
- Implementing arbitrary commands, terminal wrapping, D-Bus activation,
  capture, input, or semantics in the Tier 0 core.
- Killing a client, synthesizing input, or claiming a foreign WM accepted an
  internally delivered request.
- Treating the volatile operator gate as same-UID isolation, a trusted lock
  transition, a consent window, or Tier 1 window-manager authority.

## Configuration

The default file is `$XDG_CONFIG_HOME/agent-seat/config.toml`, falling back to
`$HOME/.config/agent-seat/config.toml`. It must be an absolute-path regular
file owned by the provider's effective UID and must not be writable by group
or others. The file is UTF-8, at most 65,536 bytes, and rejects unknown fields.

### First run

Run `agent-seat-x11` with no options from the intended provider account. If
the default file does not exist, the command:

1. creates the `agent-seat` configuration directory where necessary;
2. creates `config.toml` with mode 0600 without replacing any existing file;
3. fills `grant.uid` from the process's effective UID;
4. writes a commented template describing every setting and capability; and
5. exits successfully without connecting to X11 or creating a socket.

```sh
agent-seat-x11
# Created first-run configuration at /home/example/.config/agent-seat/config.toml.
# The provider has not started. Review the documented policy and run
# `agent-seat-x11 --check-config`. When ready, set enabled = true, validate again,
# then start the provider.
```

The generated template is deliberately disabled. Its uncommented policy grants
only title-free structure observation on the current workspace after the user
changes `enabled = false` to `enabled = true`. Observation of titles or events,
window management, and application launch remain commented out. Each optional
capability is documented next to the corresponding entry.

Review and validate the disabled file first. Enable only the required
permissions when ready, then validate it again before starting:

```sh
${EDITOR:-vi} "${XDG_CONFIG_HOME:-$HOME/.config}/agent-seat/config.toml"
agent-seat-x11 --check-config
# Change enabled = false to enabled = true when the policy is ready.
agent-seat-x11 --check-config
agent-seat-x11
# In another terminal, only when Agent Seat access is wanted:
agent-seat-x11 seat enable
```

First-run creation applies only to a no-option invocation and the discovered
default path. `--check-config` remains read-only, while `--config PATH`
requires an absolute path to an existing file; neither creates or overwrites a
configuration. Subsequent ordinary runs also never modify an existing default
configuration.

Every successful provider start has a second, volatile gate that begins
disabled independently of the saved policy. See the
[Tier 0.5 seat-gate contract](tier-0.5-seat-gate.md). `seat enable` applies
only to the current provider process; `seat disable`, provider exit, X11 loss,
logout, or restart removes that authorization. No MCP call can change it.

`--check-config` validates the complete policy independently of activation. A
valid file reports either `valid and enabled` or `valid and disabled` and exits
successfully without touching X11. An ordinary provider start still rejects a
disabled policy. This separation lets a person or settings application safely
validate a staged disabled policy before enabling it.

### Settings integration

The `agent-seat-x11` library exposes validated policy snapshots and independent
typed drafts for the Settings application. Drafts provide grouped,
all-or-nothing edits for activation, resource limits, grants, observation, and
launch policy. They never add capability dependencies implicitly. Rendering
preserves existing comments and unaffected layout, then passes the exact
result back through the provider's bounded parser before it can be submitted
for replacement. The library also exposes default-policy creation and the
paired recovery path so an editor does not reproduce XDG or filename rules.

The same library exposes a read-only installed-application catalog containing
at most 4,096 entries. It uses the runtime provider's XDG search order,
desktop-entry parser, localization, executable checks, user-entry shadowing,
and canonical desktop IDs, but does not apply the saved allow/deny policy. This
lets an editor present only entries the provider can actually launch without
connecting to X11 or a running provider.

A replacement succeeds only when the target still has the exact inode and
contents that the editor originally read. The candidate is bounded and
validated by the provider's parser before any write.

On Linux, replacement uses an atomic filesystem exchange under a private
non-blocking settings lock. The new target is mode 0600, the prior policy is
retained as `config.toml.previous`, and the containing directory is
synchronized before success is reported. Symlink, non-regular, wrong-owner,
unsafe recovery, invalid candidate, stale snapshot, concurrent writer, and
write-infrastructure failures are refused without replacing the reviewed
policy. This API edits saved policy only; a running provider does not reload it.

After X11 ownership succeeds, a provider also attempts to publish a private,
mode-0600 active-policy marker under `$XDG_RUNTIME_DIR/agent-seat`. The process
holds an exclusive advisory lock on that marker for its lifetime and records
the exact policy path and source loaded at startup. Settings can therefore
distinguish a reported matching policy from a changed saved file without
connecting to X11 or the provider socket. Unlocked crash-stale markers are
ignored. Missing or unavailable evidence is reported as unknown rather than
as proof that the provider is stopped; this best-effort channel grants no
authority and is not a same-user security boundary.

The library's separate Tier 0.5 control API is intentionally not part of those
saved-policy and marker APIs. It validates the current X11 selection-bound
advertisement and performs one fixed, bounded status, Enable, or Disable
request against the derived provider control socket. The Settings GTK shell
uses this typed boundary for its runtime panel; its display-independent model
and terminal commands do not call it. The API exposes neither private framing
nor an independent grant authority.

### Policy reference

The smallest enabled, deny-by-default configuration is:

```toml
enabled = true
```

It publishes a provider but admits no session because no grant exists. A
same-user grant is explicit:

```toml
enabled = true
max_sessions = 4
max_requests_per_session = 1024
io_timeout_ms = 2000

[grant]
uid = 1000
capabilities = [
  "observe_structure",
  "observe_titles",
  "observe_events",
  "manage_activate",
  "manage_close",
  "manage_workspace",
  "manage_state",
  "manage_geometry",
  "launch_list",
  "launch_execute",
  # "input_pointer",
]

[observation]
clients = "current_workspace"
titles = false

[launch]
mode = "allow_listed"
allow = ["org.example.Editor.desktop"]
deny = []
allow_user_entries = false

# Input remains unavailable without an administrator-enrolled broker and a
# separately trusted eligibility producer.
[input]
# broker_socket = "/run/agent-seat/1000/activity.sock"
# PID 1 owns the socket-activated listener in the system deployment.
# broker_peer_uid = 0
# Enable only with the private-device user service documented below.
provider_private_devices = false
```

`grant.uid` must equal the provider's effective UID. The private runtime
directory enforces the same-user boundary and the accepted socket's
`SO_PEERCRED` UID selects the grant; `hello.peer` remains descriptive.
Capabilities must be unique and are intersected with the peer's canonically
ordered request. `observe_titles`, `observe_events`, and every management
capability and `input_pointer` require `observe_structure`; `launch_execute` requires
`launch_list`. Calls recheck those dependencies in the live session. An
omitted grant denies every peer.

| Setting | Default | Accepted bound |
| --- | ---: | ---: |
| `max_sessions` | 4 | 1..32 |
| `max_requests_per_session` | 1024 | 1..4096 |
| `io_timeout_ms` | 2000 | 50..10000 |
| grant capabilities | empty | at most 11 unique atoms |
| `observation.clients` | `none` | `none`, `current_workspace`, `all_workspaces` |
| `observation.titles` | `false` | boolean |
| `launch.mode` | `deny` | `deny`, `allow_listed`, `allow_installed` |
| `launch.allow` | empty | at most 256 unique canonical desktop IDs; consulted only with `allow_listed` |
| `launch.deny` | empty | at most 256 unique canonical desktop IDs |
| `launch.allow_user_entries` | `false` | boolean |
| `input.broker_socket` | absent | bounded absolute pathname, paired with peer UID |
| `input.broker_peer_uid` | absent | numeric socket-listener UID observed through `SO_PEERCRED` |
| `input.provider_private_devices` | `false` | boolean; `true` requires a complete broker endpoint and the private-device service |

`deny` exposes and launches nothing. `allow_listed` admits only IDs in
`allow`, after applying `deny`. `allow_installed` admits each valid discovered
entry except IDs in `deny`. An inactive `allow` list remains saved when another
mode is selected but grants nothing until `allow_listed` is selected again. In
every mode, a winning entry from
`$XDG_DATA_HOME/applications` remains refused unless
`allow_user_entries = true`. User entries retain XDG precedence, so a denied
user override also shadows a lower-priority system entry with the same ID.

Validate configuration without touching X11 or creating a socket:

```sh
agent-seat-x11 --check-config
```

## Startup and ownership

The foreground process connects to the selected `DISPLAY`, creates a private
pathname socket, then claims `_AGENT_SEAT_S<screen>` while holding an X server
grab. It checks for an owner before setting itself and verifies ownership
before releasing the grab. Thus two conforming providers cannot both observe
an empty selection and overwrite one another.

The selected root and the dedicated owner window receive byte-identical
revision-4 `_AGENT_SEAT` advertisements only after ownership succeeds. A
second provider refuses to compete. Losing the selection terminates the
provider. Missing or mismatched properties remain undiscoverable rather than
falling back to a conventional filename.

The default socket is a display-derived name below
`$XDG_RUNTIME_DIR/agent-seat`, whose directory must be owned by the provider
UID with mode 0700. The socket is mode 0600. `--socket ABSOLUTE_PATH` is
available for controlled service layouts but its parent must meet the same
private-directory rule. A dead, same-owner socket inode is recoverable; a live
socket, symlink, regular file, or foreign inode is never replaced.

## Session behavior

Each admitted connection has fixed read/write deadlines and one sequential
request stream; there is no per-session request queue. The provider refuses
capacity beyond `max_sessions`, evicts a peer that does not complete framing
before its deadline, and ends a session at its request bound. Frames retain
the revision-4 direction limits.

The provider advertises `ewmh_observation`, `ewmh_management`, and
`desktop_launch`. It implements `seat.status` and `desktop.snapshot` with
`observe_structure`, `events.subscribe` and `events.poll` with
`observe_events`, and application list/launch with their separate capabilities.
Every request is checked against the grant first; missing authority returns
`refused`.

When `input_pointer` and a complete `[input]` endpoint are configured, the
provider additionally advertises `input_injection` and `human_activity` and
accepts revision-4 `pointer.move`. Configuration does not install, start, arm,
or enroll a broker. Each movement reconnects to the one persistent broker
instance, verifies its socket-activation peer UID, rechecks the fresh target
and visible destination under an X server grab, compares the activity epoch,
queues at most one XTEST movement, synchronizes, and checks the same broker
instance/epoch again. Changed evidence reports `interrupted`; it never forces
focus or claims application handling. Click and keyboard calls are absent.

Each session owns a separate X11 observer and opaque client-ID namespace. A
snapshot samples validated EWMH workspace, client, active-window, geometry,
state, and allowed-action properties under fixed public bounds. Raw XIDs never
cross the wire. `observation.clients = "current_workspace"` filters clients
before titles are read; a client leaving that scope is removed, and returning
later receives a fresh opaque ID. `observation.titles = true` still requires an
`observe_titles` session grant.

Events are monotonic diffs between bounded samples, not pushed window-manager
transactions. An empty subscription filter selects every event class; a
nonempty filter selects only those classes. Polls are bounded to 1,024 events
and 30 seconds. A stale, future, or evicted cursor returns `resync_required`
with the provider's current sequence; taking a snapshot establishes current
state again. Missing or malformed optional client properties are redacted,
while absent required EWMH workspace facts fail the observation.

## Management behavior

Management supports activation, polite close, workspace switch/send,
non-hidden state changes, and frame move/resize. The final observation refresh,
opaque-ID/generation or sequence check, workspace validation, exact
`_NET_SUPPORTED` and `_NET_WM_ALLOWED_ACTIONS` check, and `SendEvent` are
performed while the provider holds a short X server grab. No failure before
the send is represented as success.

Frame geometry is public outer geometry. The provider reads the target's
bounded frame extents and converts to the client rectangle used by
`_NET_MOVERESIZE_WINDOW` with `StaticGravity`; overflow, underflow, or an empty
client extent is `invalid_argument`. `_NET_WM_STATE_HIDDEN` is observation-only
because EWMH says a WM should ignore attempts to toggle it. Close additionally
requires `WM_DELETE_WINDOW`, and the provider never falls back to
`XKillClient`.

After a successfully sent request, the provider samples for one second. A
`management` reply is `observed` when the desired visible state appears,
`target_gone` when a non-close target disappears first, or `timed_out` when the
deadline expires. For close, visible disappearance is the desired observation.
These values do not claim internal acceptance by Openbox or another WM.

## Launch behavior

Application discovery reads `$XDG_DATA_HOME/applications` first, then the
preference-ordered `$XDG_DATA_DIRS/applications` roots. Empty variables use the
XDG defaults. Roots, directory depth, visited paths, file bytes, parsed keys,
catalog entries, page entries, argument count, and active child processes all
have fixed bounds. Directory symlinks are not traversed; regular desktop files
and symlinked desktop-file leaves are opened through bounded file handles.
Hidden, `NoDisplay`, non-Application, malformed, incompatible
`OnlyShowIn`/`NotShowIn`, terminal, unavailable `TryExec`, and otherwise
unlaunchable entries are absent. The first XDG-precedence entry with a desktop
ID shadows later entries even when hidden, malformed, or refused.

The provider parses only the main `[Desktop Entry]` group. It applies the
Desktop Entry 1.5 string, quoting, and field-code rules, removes the standard
file/URL and deprecated field codes because launch accepts no document, expands
`%c`, `%i`, `%k`, and `%%`, and rejects unknown or ambiguous codes. It invokes
the executable directly with `std::process::Command`; no shell, command string,
or peer argument is involved. Under the private-device input profile, it
instead invokes fixed `/usr/bin/systemd-run` with a generated argument vector.
The user manager creates one transient application service outside the
provider's private device namespace. The waiting `systemd-run` child remains
inside the same 64-child supervision bound until that application service
ends. `Terminal=true` is unsupported because silently
choosing a terminal would add another executable outside policy. This provider
does not implement D-Bus activation and therefore uses the required compatible
`Exec` fallback when `DBusActivatable=true`.

A successful spawn returns a provider-unique token. At most 64 live launched
children are supervised and reaped. When the same session also has
`observe_structure` and a nonempty observation scope, the provider may wait up
to one second for a newly visible client whose own or `WM_CLIENT_LEADER`'s
`_NET_STARTUP_ID` exactly equals the `DESKTOP_STARTUP_ID` supplied to the
child. That exact match returns the session's opaque client handle. Missing,
late, filtered, absent, or spoofed metadata produces `client = null`; the
provider never guesses from PID, title, class, timing, or “only new window.”
The ID is same-user X11 evidence, not an authentication or causality guarantee.

## Optional private-device input service

An input-enabled provider must not inherit the desktop user's evdev or uinput
permissions. Set `input.provider_private_devices = true` only after installing
the source unit
`contrib/systemd/user/agent-seat-x11-input.service` as
`agent-seat-x11-input.service` in the system or user systemd unit search path.
The packaged location is `/usr/lib/systemd/user/`; a source-checkout test can
install the same bytes for only the current user:

```sh
install -Dm0644 contrib/systemd/user/agent-seat-x11-input.service \
  "$HOME/.config/systemd/user/agent-seat-x11-input.service"
systemctl --user daemon-reload
```

The unit is deliberately missing `[Install]`: it cannot be enabled and no
package preset starts it. In the X11 session that will run the provider, import
the display variables actually present, validate the policy, stop any existing
foreground provider, and start one explicit cycle:

```sh
systemctl --user import-environment DISPLAY XAUTHORITY XDG_CURRENT_DESKTOP
agent-seat-x11 --check-config
systemctl --user start agent-seat-x11-input.service
systemctl --user status agent-seat-x11-input.service
agent-seat-x11 seat enable
```

The service keeps local `AF_UNIX` access for X11 and the user manager, but
applies `PrivateDevices=yes`, a strict device policy, no capabilities,
`NoNewPrivileges=yes`, syscall and filesystem restrictions, and fixed resource
bounds. Its private temporary directory re-exposes only the host's read-only
X11 socket directory, and only its private mode-0700 runtime directory remains
writable under the read-only filesystem view. When the configuration switch is
true, the provider independently checks that both `/dev/input` and
`/dev/uinput` are absent before connecting to X11. Missing isolation is a
startup error; there is no warning-only fallback.

This unit binds the provider at the fixed private pathname
`$XDG_RUNTIME_DIR/agent-seat/x11-input.sock`. That makes the separately
confined companion registration deterministic without giving the companion
X11 discovery. After the provider is active, run
`agent-seat-mcp --print-private-mcp-config` and register the emitted JSON with
the MCP host; see the
[private companion profile](mcp.md#private-companion-profile-for-optional-input).

Admitted applications do not inherit this namespace. The provider delegates
their already parsed absolute executable and bounded arguments directly to a
uniquely named transient user service, with no shell. A live hostile gate
proves that the provider cannot see either input path even when the user can
open uinput, while the delegated application retains exactly that user's
baseline device namespace:

```sh
cargo test -p agent-seat-x11 --test systemd_input_confinement \
  provider_loses_input_devices_while_delegated_application_keeps_baseline \
  -- --ignored
```

Neither this user service nor the desktop user needs membership in `input`.
The separate broker enrollment remains the only root-controlled operation and
passes exact reviewed descriptors to its own unprivileged process. This
provider service closes one negative-authority gate; it does not pass the
remaining physical-device and trusted-lock gates or make generic Openbox input
supported.

## Running beside Openbox

Start the provider as a separate process from Openbox autostart:

```sh
agent-seat-x11 &
```

The provider starts disabled. Run `agent-seat-x11 seat enable` explicitly when
you want to admit sessions, and `agent-seat-x11 seat disable` to revoke the
current generation. Do not place the enable command in Openbox autostart.

SIGINT or SIGTERM performs clean withdrawal and socket removal. A crash may
leave a recoverable socket or stale root property, but it cannot terminate or
block Openbox; discovery requires a current selection owner and matching owner
property, so stale root bytes alone are not live authority.

## T3 end result

The provider is an independently failing, bounded, same-user policy process
whose scoped Openbox snapshots and filtered diffs converge across client
creation, title, state, workspace, and destruction changes. It does not expose
raw XIDs or read titles for filtered-out clients, and it does not claim that
standalone X11 observation is atomic or a strong isolation boundary. Supported
management is additionally freshness-checked before send and reports ignored
or ambiguous terminal outcomes without elevating them to acceptance. The
complete Tier 0 core additionally exposes only policy-approved XDG entries and
launches them without shell interpretation. Capture, input, accessibility, and
persistent coordinate/workflow memory remain unsupported optional profiles.
