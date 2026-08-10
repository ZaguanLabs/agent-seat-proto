# Architecture

The product separates data from authority and backend realization.

## `agent-seat-proto`

This library owns bounded wire values, framing, revision identity, and strict
serialization only. It depends on no display server, transport, policy engine,
MCP library, or application catalog. Its eventual runtime dependency budget is
Serde plus the minimum serialization support justified by the wire format.

## `agent-seat-mcp`

This executable translates a static MCP tool surface into Agent Seat calls. It
supports both MCP `2026-07-28` discovery/per-request metadata and the
`2025-11-25` initialize lifecycle. It may discover and connect to a provider
lazily, but it owns no grant, consent, scope, or desktop policy. Discovery,
initialization, and tool listing do not require a desktop connection. The
provider distrusts and revalidates every call.

The modern MCP path represents state explicitly: `seat_status` opens a bounded
provider context, later tool arguments carry its identifier, and
`seat_release` closes it. The identifier is process-local bookkeeping, not an
authorization capability; the provider's verified peer and grant checks remain
authoritative. The legacy path preserves its existing implicit provider
connection for compatibility.

For the optional input reference deployment, a generated systemd command has
the user manager preconnect one exact provider socket and pass it by name to a
transient companion worker. Private network, device, temporary, runtime, home,
process, execution, and resource controls then remove ambient desktop and
broker authority without changing the MCP or Agent Seat protocols. The
ordinary cross-platform companion remains the default; this Linux/systemd
launcher profile is non-normative deployment plumbing.

## `agent-seat-x11`

This executable is a standalone Tier 0 provider beside an unmodified EWMH
window manager. It owns X11 discovery, one private local socket, verified peer
identity, strict configuration, grants, scopes, observation, EWMH requests,
desktop-entry policy, and bounded failure isolation. It never becomes an
Openbox plugin or window-manager dependency.

The experimental Tier 0.5 deployment adds a second private local socket for a
volatile provider-owned operator gate. Every provider process starts disabled;
the separate status/enable/disable command is not an Agent Seat wire call and
is not reachable through MCP. Each admitted session is bound to one enabled
generation. The control plane is authenticated to the desktop UID and is
therefore an operator boundary for the confined companion, not isolation from
arbitrary same-UID processes.

The core profile is observation, supported management, and controlled launch.
Capture, input, and accessibility remain separately reviewed optional
profiles. Revision 6 defines a separately granted obscured-client capture
profile and retains the explicitly weaker Tier 0.5 X11 input profile;
providers that do not implement either simply omit its grants and feature.

The experimental revision-5 input path uses the provider's existing X11
connection, XTEST, current target/focus evidence, the live XKB map, and the
volatile seat generation. Keyboard resolution follows the effective XKB group
and key types and fails closed when a requested symbol is not directly
reachable with bounded safe modifiers. It does not read raw input devices or
claim physical-user priority. The older separately confined activity broker and
private-device service remain optional research/hardening components, not
dependencies of the ordinary input path or the display-neutral wire contract.

The revision-6/7 capture path uses the same session observer but no input
authority. It automatically redirects only scoped clients selected under the
capture grant, names one target pixmap under a bounded server-grabbed read,
converts a verified TrueColor layout, and returns a bounded PNG. It never reads
the root or an output and cannot reconstruct pixels already obscured before
enrollment.

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

The separate typed Tier 0.5 control API validates the live selection-bound X11
advertisement, derives the provider-private control socket, and performs one
bounded status, Enable, or Disable request. It is not part of the policy API,
does not expose the fixed private framing, and is not an Agent Seat wire or MCP
operation.

## `agent-seat-settings`

This executable is a human-facing editor, not a provider. Its
display-independent model depends on `agent-seat-x11` for the exact policy
schema, XDG catalog, and atomic writes. Its GTK 4 shell may present and review
drafts. The shell alone may call the typed Tier 0.5 boundary to inspect or
change the current provider-owned volatile latch; it never owns either socket,
exposes MCP tools, changes a saved grant implicitly, or starts and stops the
provider. Terminal check, print, and recovery commands execute without
initializing GTK or contacting X11 or the provider.

## Dependency direction

```text
agent-seat-mcp ──> agent-seat-proto <── agent-seat-x11 <── agent-seat-settings
                                           │
                                           ├── X11/EWMH realization
                                           ├── grants and scope
                                           ├── XDG launch and saved policy
                                           └── typed volatile-seat control
```

The protocol crate never points outward. The companion and provider share
protocol types, not policy or backend state. Settings deliberately depends on
the provider library's non-runtime policy API so validation cannot drift. No
crate depends on Nobox.

## Standards direction

The wire model is intended to remain implementation-independent. The initial
[R0 pre-RFC draft](../protocol/r0-protocol-rfc.md) separates its normative identities,
grants, scope, freshness, outcomes, interruption, and assurance vocabulary from
backend conformance profiles and non-normative reference deployments.

The portable contract is factored further: the
[serialization-neutral information model](../protocol/information-model.md) defines
session, authority, identity, freshness, operation, and outcome semantics,
while [`specification.md`](../protocol/specification.md) remains the concrete local
Unix-stream/strict-JSON revision-8 binding.

An integrated window manager or compositor can satisfy a conformance profile
using state and ordering it owns directly. The standalone X11 reference may
make a deliberately narrower, non-physical-priority claim. Neither
systemd/evdev deployment details nor MCP translation
belong in the normative desktop protocol, and matching a tool surface does not
permit one backend to claim another backend's assurance level.
