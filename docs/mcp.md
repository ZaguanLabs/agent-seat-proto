# Generic MCP companion

`agent-seat-mcp` implements MCP `2025-11-25` over newline-delimited JSON-RPC
stdio. Its lifecycle and tool result shape follow the official
[MCP lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)
and [tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
contracts.

## Static lifecycle

In the ordinary lazy-discovery mode, `initialize`, `notifications/initialized`,
`ping`, and `tools/list` do not
inspect `DISPLAY`, resolve a socket, or connect to a provider. The server
advertises only the static tools capability and returns the requested protocol
version when it is `2025-11-25`; otherwise it returns the one revision it
supports so the client can accept it or disconnect.

Each JSON-RPC line is at most 1,048,576 bytes. Oversized input terminates the
stdio process before JSON parsing. Malformed JSON gets `-32700`, an invalid
request `-32600`, an unknown method `-32601`, and malformed method parameters
`-32602`. Tool argument corrections are tool execution errors so a model can
act on their structured fields.

## Tools

The companion publishes sixteen closed-object schemas:

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
- `keyboard_type`
- `capture_obscured`

Each maps one-to-one to a typed revision-6 call. Ordinary results contain matching JSON
in `structuredContent` and a text block for clients that do not consume
structured results. A successful capture instead contains one `image/png`
block; its structured result retains target, dimensions, and format but omits
the large base64 field so image data is not duplicated. A wire error sets
`isError: true` without converting its
stable code or retry action into English control flow.

## Lazy provider boundary

The first `tools/call` resolves the exact source precedence and opens the local
socket. The companion requests capabilities but grants none: the provider
authenticates peer credentials, selects the grant, reports features and
assurance, and rechecks every call.

A dead connection is discarded. The next tool call resolves a provider again;
the companion does not retain a stale automatically discovered path. Read and
write operations have a fixed ten-second transport deadline in addition to
provider operation deadlines.

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
worker starts, before MCP initialization, so a missing provider fails the MCP
process immediately. In the input profile, the provider permits exactly one
successfully authenticated and granted session to wait idle between complete
frames. Initial `Hello` and partial-frame reads retain the configured I/O
deadline, additional sessions retain the ordinary deadline, and provider
shutdown interrupts the idle wait. This is an availability rule based on the
provider's existing UID grant; it is not evidence that systemd confined the
peer. The emitted unit and hostile gate establish that separate client-side
boundary. The ordinary mode remains lazy and desktop-free through
initialization and tool listing.

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
