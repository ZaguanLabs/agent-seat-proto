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
profiles. Revision 3 exposes no calls for absent profiles; their feature atoms
remain unadvertised. A future revision that defines optional calls must return
typed `unsupported` results when its backend does not advertise them.

The crate also owns the non-runtime settings API used by the
`agent-seat-settings` application. It exposes a typed, bounded draft of a
validated policy, the same launchable XDG application catalog used at runtime,
and conflict-detecting atomic policy transactions. Rendering reuses the
provider's exact parser; replacement captures the expected inode and source,
refuses stale or concurrent edits, and retains the previous private policy.
Lock-held private runtime markers additionally provide best-effort evidence of
the exact policy loaded by current provider processes. These APIs do not
connect to X11, open the provider socket, or alter the policy active in a
running process.

## `agent-seat-settings`

This executable is a human-facing editor, not a provider. Its
display-independent model depends on `agent-seat-x11` for the exact policy
schema, XDG catalog, and atomic writes. Its GTK 4 shell may present and review
drafts but never connects to X11 directly, opens the provider socket, exposes
MCP tools, or changes a running grant. Terminal check, print, and recovery
commands execute without initializing GTK.

## Dependency direction

```text
agent-seat-mcp ──> agent-seat-proto <── agent-seat-x11 <── agent-seat-settings
                                           │
                                           ├── X11/EWMH realization
                                           ├── grants and scope
                                           └── XDG launch and saved policy
```

The protocol crate never points outward. The companion and provider share
protocol types, not policy or backend state. Settings deliberately depends on
the provider library's non-runtime policy API so validation cannot drift. No
crate depends on Nobox.

## Standards direction

The wire model is intended to remain implementation-independent. The initial
[R0 pre-RFC draft](r0-protocol-rfc.md) separates its normative identities,
grants, scope, freshness, outcomes, interruption, and assurance vocabulary from
backend conformance profiles and non-normative reference deployments.

An integrated window manager or compositor can satisfy a conformance profile
using state and ordering it owns directly. The standalone X11 reference may
need external observation and an optional Linux activity broker to make a
narrower claim. Neither systemd/evdev deployment details nor MCP translation
belong in the normative desktop protocol, and matching a tool surface does not
permit one backend to claim another backend's assurance level.
