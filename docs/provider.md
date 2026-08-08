# Standalone X11 provider

Status: T1 observation. `agent-seat-x11` 0.1.2 owns lifecycle, policy, local
authentication, X11 discovery, and bounded EWMH observation. T2--T3 add
management and launch without moving authority into the MCP companion.
The current implementation target is Linux X11 and its `SO_PEERCRED` contract.

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
- Remove the private socket and advertisement on clean shutdown while leaving
  Openbox or another window manager independent.

## Non-goals

- Treating a peer-supplied name, version, purpose, PID, title, or X11 property
  as authorization identity.
- Listening on a network or abstract socket, admitting another OS user, or
  daemonizing itself.
- Treating a sequence of independently sampled EWMH properties as an atomic
  window-manager transaction.
- Implementing management, launch, capture, input, or semantics in T1.
- Providing a consent window or claiming Tier 1 window-manager authority.

## Configuration

The default file is `$XDG_CONFIG_HOME/agent-seat/config.toml`, falling back to
`$HOME/.config/agent-seat/config.toml`. It must be an absolute-path regular
file owned by the provider's effective UID and must not be writable by group
or others. The file is UTF-8, at most 65,536 bytes, and rejects unknown fields.

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
capabilities = ["observe_structure", "observe_titles", "observe_events"]

[observation]
clients = "current_workspace"
titles = false
```

`grant.uid` must equal the provider's effective UID. The private runtime
directory enforces the same-user boundary and the accepted socket's
`SO_PEERCRED` UID selects the grant; `hello.peer` remains descriptive.
Capabilities must be unique and are intersected with the peer's canonically
ordered request. `observe_titles` and `observe_events` require
`observe_structure` in the configured grant. An omitted grant denies every
peer.

| Setting | Default | Accepted bound |
| --- | ---: | ---: |
| `max_sessions` | 4 | 1..32 |
| `max_requests_per_session` | 1024 | 1..4096 |
| `io_timeout_ms` | 2000 | 50..10000 |
| grant capabilities | empty | at most 10 unique atoms |
| `observation.clients` | `none` | `none`, `current_workspace`, `all_workspaces` |
| `observation.titles` | `false` | boolean |

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

The provider advertises `ewmh_observation` and implements `seat.status` and
`desktop.snapshot` with `observe_structure`, plus `events.subscribe` and
`events.poll` with `observe_events`. Every request is checked against the grant
first: missing authority returns `refused`; an authorized call reserved for a
later milestone returns `unsupported`.

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

## Running beside Openbox

Start the provider as a separate process from Openbox autostart:

```sh
agent-seat-x11 &
```

SIGINT or SIGTERM performs clean withdrawal and socket removal. A crash may
leave a recoverable socket or stale root property, but it cannot terminate or
block Openbox; discovery requires a current selection owner and matching owner
property, so stale root bytes alone are not live authority.

## T1 end result

The provider is an independently failing, bounded, same-user policy process
whose scoped Openbox snapshots and filtered diffs converge across client
creation, title, state, workspace, and destruction changes. It does not expose
raw XIDs or read titles for filtered-out clients, and it does not claim that
standalone X11 observation is atomic or a strong isolation boundary.
