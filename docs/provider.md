# Standalone X11 provider

Status: T0 foundation. `agent-seat-x11` 0.1.1 owns lifecycle, policy, local
authentication, bounds, and X11 discovery. T1--T3 add observation, management,
and launch behavior without moving authority into the MCP companion.
The current implementation target is Linux X11 and its `SO_PEERCRED` contract.

## Goals

- Start only from an explicit, valid, enabled configuration.
- Admit only the provider user's kernel-authenticated local socket peers.
- Compute a configured capability grant and recheck it on every request.
- Own one X11 screen without racing or replacing another conforming provider.
- Keep socket paths, frames, sessions, requests, waits, and configuration
  bounded.
- Remove the private socket and advertisement on clean shutdown while leaving
  Openbox or another window manager independent.

## Non-goals

- Treating a peer-supplied name, version, purpose, PID, title, or X11 property
  as authorization identity.
- Listening on a network or abstract socket, admitting another OS user, or
  daemonizing itself.
- Implementing observation, management, launch, capture, input, or semantics
  in the T0 foundation.
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
capabilities = ["observe_structure"]
```

`grant.uid` must equal the provider's effective UID. The private runtime
directory enforces the same-user boundary and the accepted socket's
`SO_PEERCRED` UID selects the grant; `hello.peer` remains descriptive.
Capabilities must be unique and are intersected with the peer's canonically
ordered request. An omitted grant denies every peer.

| Setting | Default | Accepted bound |
| --- | ---: | ---: |
| `max_sessions` | 4 | 1..32 |
| `max_requests_per_session` | 1024 | 1..4096 |
| `io_timeout_ms` | 2000 | 50..10000 |
| grant capabilities | empty | at most 10 unique atoms |

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

The T0 foundation implements `seat.status` when `observe_structure` was
granted. It reports `x11_ewmh`, `tier0`, the exact grant, no implemented backend
features yet, and sequence zero. Every request is checked against the grant
first: missing authority returns `refused`; an authorized call reserved for a
later milestone returns `unsupported`. T1 changes only the implemented feature
set and operation realization, not this authority order.

## Running beside Openbox

Start the provider as a separate process from Openbox autostart:

```sh
agent-seat-x11 &
```

SIGINT or SIGTERM performs clean withdrawal and socket removal. A crash may
leave a recoverable socket or stale root property, but it cannot terminate or
block Openbox; discovery requires a current selection owner and matching owner
property, so stale root bytes alone are not live authority.

## T0 end result

The foundation is an independently failing, bounded, same-user policy process
with atomic per-screen ownership and an authenticated revision-3 handshake.
It is ready for T1 EWMH observation without granting the companion authority
or claiming that standalone X11 is a strong isolation boundary.
