# Generic MCP companion

`agent-seat-mcp` implements MCP `2025-11-25` over newline-delimited JSON-RPC
stdio. Its lifecycle and tool result shape follow the official
[MCP lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)
and [tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
contracts.

## Static lifecycle

`initialize`, `notifications/initialized`, `ping`, and `tools/list` do not
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

The companion publishes twelve closed-object schemas:

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

Each maps one-to-one to a typed revision-3 call. Results contain matching JSON
in `structuredContent` and a text block for clients that do not consume
structured results. A wire error sets `isError: true` without converting its
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
