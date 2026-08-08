# Standalone X11 provider

Status: T3 Tier 0 core. `agent-seat-x11` 0.1.7 owns lifecycle, policy, local
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
- Providing a consent window or claiming Tier 1 window-manager authority.

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
```

First-run creation applies only to a no-option invocation and the discovered
default path. `--check-config` remains read-only, while `--config PATH`
requires an absolute path to an existing file; neither creates or overwrites a
configuration. Subsequent ordinary runs also never modify an existing default
configuration.

`--check-config` validates the complete policy independently of activation. A
valid file reports either `valid and enabled` or `valid and disabled` and exits
successfully without touching X11. An ordinary provider start still rejects a
disabled policy. This separation lets a person or settings application safely
validate a staged disabled policy before enabling it.

### Settings transaction foundation

The `agent-seat-x11` library exposes validated policy snapshots for the future
Settings application. A replacement succeeds only when the target still has
the exact inode and contents that the editor originally read. The candidate is
bounded and validated by the provider's parser before any write.

On Linux, replacement uses an atomic filesystem exchange under a private
non-blocking settings lock. The new target is mode 0600, the prior policy is
retained as `config.toml.previous`, and the containing directory is
synchronized before success is reported. Symlink, non-regular, wrong-owner,
unsafe recovery, invalid candidate, stale snapshot, concurrent writer, and
write-infrastructure failures are refused without replacing the reviewed
policy. This API edits saved policy only; a running provider does not reload it.

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
]

[observation]
clients = "current_workspace"
titles = false

[launch]
mode = "allow_listed"
allow = ["org.example.Editor.desktop"]
deny = []
allow_user_entries = false
```

`grant.uid` must equal the provider's effective UID. The private runtime
directory enforces the same-user boundary and the accepted socket's
`SO_PEERCRED` UID selects the grant; `hello.peer` remains descriptive.
Capabilities must be unique and are intersected with the peer's canonically
ordered request. `observe_titles`, `observe_events`, and every management
capability require `observe_structure`; `launch_execute` requires
`launch_list`. Calls recheck those dependencies in the live session. An
omitted grant denies every peer.

| Setting | Default | Accepted bound |
| --- | ---: | ---: |
| `max_sessions` | 4 | 1..32 |
| `max_requests_per_session` | 1024 | 1..4096 |
| `io_timeout_ms` | 2000 | 50..10000 |
| grant capabilities | empty | at most 10 unique atoms |
| `observation.clients` | `none` | `none`, `current_workspace`, `all_workspaces` |
| `observation.titles` | `false` | boolean |
| `launch.mode` | `deny` | `deny`, `allow_listed`, `allow_installed` |
| `launch.allow` | empty | at most 256 unique canonical desktop IDs; only with `allow_listed` |
| `launch.deny` | empty | at most 256 unique canonical desktop IDs |
| `launch.allow_user_entries` | `false` | boolean |

`deny` exposes and launches nothing. `allow_listed` admits only IDs in
`allow`, after applying `deny`. `allow_installed` admits each valid discovered
entry except IDs in `deny`. In every mode, a winning entry from
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
revision-3 `_AGENT_SEAT` advertisements only after ownership succeeds. A
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
the revision-3 direction limits.

The provider advertises `ewmh_observation`, `ewmh_management`, and
`desktop_launch`. It implements `seat.status` and `desktop.snapshot` with
`observe_structure`, `events.subscribe` and `events.poll` with
`observe_events`, and application list/launch with their separate capabilities.
Every request is checked against the grant first; missing authority returns
`refused`.

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
or peer argument is involved. `Terminal=true` is unsupported because silently
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

## Running beside Openbox

Start the provider as a separate process from Openbox autostart:

```sh
agent-seat-x11 &
```

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
