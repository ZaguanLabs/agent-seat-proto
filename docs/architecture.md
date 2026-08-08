# Architecture

The product separates data from authority and backend realization.

## `agent-seat-proto`

This library owns bounded wire values, framing, revision identity, and strict
serialization only. It depends on no display server, transport, policy engine,
MCP library, or application catalog. Its eventual runtime dependency budget is
Serde plus the minimum serialization support justified by the wire format.

## `agent-seat-mcp`

This executable translates a static MCP tool surface into Agent Seat calls. It
may discover and connect to a provider lazily, but it owns no grant, consent,
scope, or desktop policy. Initialization and tool listing do not require a
desktop connection. The provider distrusts and revalidates every call.

## `agent-seat-x11`

This executable is a standalone Tier 0 provider beside an unmodified EWMH
window manager. It owns X11 discovery, one private local socket, verified peer
identity, strict configuration, grants, scopes, observation, EWMH requests,
desktop-entry policy, and bounded failure isolation. It never becomes an
Openbox plugin or window-manager dependency.

The core profile is observation, supported management, and controlled launch.
Capture, input, and accessibility remain separately reviewed optional
profiles. Missing profiles return typed unsupported results.

## Dependency direction

```text
agent-seat-mcp ──> agent-seat-proto <── agent-seat-x11
                                           │
                                           ├── X11/EWMH realization
                                           ├── grants and scope
                                           └── XDG launch policy
```

The protocol crate never points outward. The two processes share protocol
types, not policy or backend state. No crate depends on Nobox.
