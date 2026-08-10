# Generic MCP companion

`agent-seat-mcp` 0.1.8 implements both MCP `2026-07-28` and `2025-11-25` over
newline-delimited JSON-RPC stdio. A modern host should use `server/discover`;
an existing host can continue using the legacy `initialize` lifecycle without
registration changes or changes to any pre-existing tool schema.

## Modern MCP 2026-07-28

The modern protocol has no initialization handshake. Every request supplies
`io.modelcontextprotocol/protocolVersion` and
`io.modelcontextprotocol/clientCapabilities` in `_meta`; client information is
accepted there as well. The companion validates those fields independently on
every request and returns the specified `-32022` error with the requested and
supported versions when the version is unsupported.

`server/discover` reports both supported MCP versions, the static tools
capability, server identity, and a short instruction budget. Discovery and
`tools/list` return `resultType: "complete"`; their caller-independent results
are deterministic and marked `cacheScope: "public"` with a one-hour `ttlMs`.
They do not inspect `DISPLAY`, resolve an Agent Seat socket, or connect to a
provider. Tool results also carry the complete result type and server identity,
but are not marked cacheable.

This path follows the official MCP
[discovery](https://modelcontextprotocol.io/specification/2026-07-28/server/discover),
[stdio compatibility](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio),
[tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools),
and [caching](https://modelcontextprotocol.io/specification/2026-07-28/server/utilities/caching)
contracts. It does not implement or claim Streamable HTTP, MCP authorization,
extensions or subscriptions.

## Legacy MCP 2025-11-25 compatibility

The legacy lifecycle and every pre-existing tool remain backward compatible.
`initialize`, `notifications/initialized`, `ping`, and `tools/list` remain
desktop-free. The server advertises only the static tools capability and
returns `2025-11-25` when that revision is requested; for another requested
revision it returns its legacy revision so the host can accept it or disconnect. Legacy clients keep
the original 16 tool names and schemas, implicit provider connection, and
result shapes. Revision 7 added `keyboard_key` as a seventeenth closed-schema
tool. Revision 8 adds two wire-backed tools and three session-local pointer
tools without changing an earlier name or schema; no modern `resultType`,
cache field, context argument, or `seat_release` is inserted into legacy
results.

This path follows the official legacy
[lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)
and [tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
contracts.

## JSON-RPC boundary

Each JSON-RPC line is at most 1,048,576 bytes. Oversized input terminates the
stdio process before JSON parsing. Malformed JSON gets `-32700`, an invalid
request `-32600`, an unknown method `-32601`, and malformed method parameters
`-32602`. A null request ID is rejected. Tool argument corrections are tool
execution errors so a model can act on their structured fields.

## Tools

The legacy companion publishes twenty-two closed-object schemas:

- `seat_status`
- `desktop_snapshot`
- `events_subscribe`
- `events_poll`
- `client_activate`
- `client_close`
- `workspace_switch`
- `client_workspace`
- `client_state`
- `client_geometry`
- `applications_list`
- `application_launch`
- `pointer_move`
- `pointer_click`
- `pointer_slot_save`
- `pointer_slot_replay`
- `pointer_slots_list`
- `keyboard_type`
- `keyboard_key`
- `keyboard_write`
- `capture_obscured`
- `capture_region`

The modern list has the same twenty-two tools plus `seat_release`. `seat_status`
opens one of at most eight provider contexts and returns its positive integer `context`.
Pass that context to every later modern tool call and call `seat_release` when
finished. A context lasts until explicit release, provider transport failure,
or companion exit; identifiers are never reused by a running process. They are
local bookkeeping names, not authorization capabilities: the provider still
authenticates the companion peer, owns grants and scope, and rechecks each wire
call. A private-profile companion has only its one inherited provider
connection, so only the first modern context can consume it.

Every provider-backed desktop tool maps one-to-one to a typed revision-8 call.
The three pointer-slot tools are the only exceptions. They keep at most 32
named `pointer_click` argument sets inside the current provider session. Slots
disappear on context release, provider failure, or companion exit. They confer
no authority, retain no pixels or application meaning, and cannot form a
sequence. `pointer_slot_replay` requires the target's freshly observed
`generation` and sends one ordinary `pointer.click`, so the provider still
rechecks the grant, seat, target generation, current geometry, visible
hit-test ancestry, and destination. Save a slot only after observing
that the original click reached the intended control; reobserve before replay
after layout or UI changes.

Ordinary results
contain matching JSON in `structuredContent` and a text block for clients that
do not consume structured results. A successful capture instead contains one
`image/png` block; its structured result retains target, dimensions, and format
but omits the large base64 field so image data is not duplicated. A wire error
sets `isError: true` without converting its stable code or retry action into
English control flow.

Use `keyboard_key` for one conventional focused command such as `page_down`,
Control+L, Control+F, Control+W, or Alt+Left. Its `key` comes from the finite
published enum; optional `modifiers` are unique and ordered `control`, `alt`,
`shift`, `super`. Prefer this path before a coordinate-based pointer action,
and prefer titles and other metadata before requesting pixels.
`keyboard_type` remains the short text path, limited to 256 Unicode scalar
actions and 1,024 UTF-8 bytes. Use `keyboard_write` for long-form text: it
accepts at most 4,096 scalar actions and 16 KiB, preserves newlines and tabs,
and preflights every character against the live XKB layout before emitting the
first action. It still types one complete character action at a time, requires
the target to retain focus, can be interrupted by seat or target changes, and
reports exact completed/requested counts. It is not clipboard paste and cannot
produce a character absent from the active layout.

Use `capture_region` instead of `capture_obscured` when a small
client-relative area is sufficient. Each side is at most 1,024 pixels and the
area is at most 262,144 pixels. The image still comes from the freshly scoped
target's Composite storage and uses the same separate capture grant.

## Lazy provider boundary

The first legacy tool call, or the modern `seat_status` creation call, resolves
the exact source precedence and opens the local socket. The companion requests
capabilities but grants none: the provider authenticates peer credentials,
selects the grant, reports features and assurance, and rechecks every call.

A dead legacy connection is discarded and the next call resolves a provider
again. A modern provider failure discards its context and a new `seat_status`
call is required. The companion does not retain a stale automatically
discovered path. Read and write operations have a fixed ten-second transport
deadline in addition to provider operation deadlines. Only `keyboard_write`
extends the response-read deadline to a bounded 120 seconds; the ordinary
deadline is restored after the response.

Register it with an MCP host using the equivalent of:

```json
{
  "mcpServers": {
    "agent-seat": {
      "command": "agent-seat-mcp"
    }
  }
}
```

Pass `DISPLAY` when the host sanitizes its environment, or provide `--socket`
or `AGENT_SEAT_SOCKET`. `--print-mcp-config` prints the minimal registration.

## Optional private companion hardening

Ordinary Tier 0.5 input works with the normal companion and needs no device
permission. A stronger optional registration can additionally prevent the
companion from inheriting the desktop user's ambient device and X11 access. With
the private-device provider user service running, print the exact registration:

```sh
agent-seat-mcp --print-private-mcp-config
```

The command emits JSON ready for the MCP host. By default it names the fixed
`$XDG_RUNTIME_DIR/agent-seat/x11-input.sock` created by
`agent-seat-x11-input.service`; `--socket ABSOLUTE_PATH` can select a deliberate
custom deployment. The emitted command uses installed `/usr/bin` paths and
requires a systemd user manager with `OpenFile=` support (systemd 253 or newer).

The user manager connects that one pathname before service isolation and gives
the worker exactly one descriptor named `agent-seat-provider`. The worker
strictly verifies `LISTEN_PID`, `LISTEN_FDS`, `LISTEN_FDNAMES`, descriptor count,
and name before opening the Agent Seat session. It never discovers X11 or
opens another Unix socket. Because the descriptor is already connected,
`PrivateNetwork=yes` can isolate filesystem and abstract X11 sockets without
breaking provider IPC.

The transient worker also has a private device view, no IP families, an empty
read-only `/run`, a private `/tmp`, inaccessible home and other-process views,
no capabilities, `NoNewPrivileges=yes`, a system-call filter, no arbitrary
executable path, cleared desktop/socket/path environment variables, and fixed
task, descriptor, memory, CPU, and core-dump bounds. The MCP host retains only
the stdio side of `systemd-run --pipe`; neither the provider descriptor nor
broker IPC crosses the MCP boundary.

This mode connects and completes the Agent Seat opening handshake while the
worker starts, before either MCP-era request flow begins, so a missing provider
fails the MCP process immediately. In the input profile, the provider permits
exactly one successfully authenticated and granted session to wait idle between
complete frames. Initial `Hello` and partial-frame reads retain the configured
I/O deadline, additional sessions retain the ordinary deadline, and provider
shutdown interrupts the idle wait. This is an availability rule based on the
provider's existing UID grant; it is not evidence that systemd confined the
peer. The emitted unit and hostile gate establish that separate client-side
boundary. The ordinary mode remains lazy and desktop-free through modern
discovery, legacy initialization, and tool listing.

Run the emitted-profile hostile gate explicitly:

```sh
cargo test -p agent-seat-mcp --test systemd_private_companion \
  emitted_private_profile_exposes_only_the_provider_channel \
  -- --ignored
```

The gate begins with a UID that can open host uinput, then proves the worker
cannot see input nodes, X11, IP networking, a broker-like socket, home or parent
process data, sensitive environment, or an unapproved executable. It also
proves systemd connected only the named provider fixture. All units are
transient and collected; no desktop or policy is changed.
