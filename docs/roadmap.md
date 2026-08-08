# Product roadmap

Status: E0, E1, T0--T3, the T4--T6 first-release decisions, and C0 are
complete in product release v0.1.0. E0 evidence is recorded in
[`e0-verification.md`](e0-verification.md).

## Goals

- Define one strict, bounded Agent Seat wire contract.
- Ship an authority-free generic MCP companion.
- Ship a standalone Tier 0 provider that works beside unmodified EWMH window
  managers, beginning with released Openbox.
- Provide a human-facing settings application for safely reviewing, editing,
  and validating provider policy without moving runtime authority out of the
  provider.
- Make unsupported, refused, stale, sent-but-unobserved, timed-out, and failed
  outcomes distinct at public boundaries.
- Keep failures outside the window manager and keep source independently
  authored under Apache-2.0 with DCO sign-off.

## Non-goals

- Importing or relicensing Nobox implementation material.
- Treating same-user X11 as a secure isolation boundary.
- Giving the MCP companion policy authority.
- Giving a settings application an MCP surface, X11 control, provider socket
  ownership, or independent runtime grant authority.
- Emulating unsupported EWMH operations with shell commands or synthetic
  pointer/keyboard input.
- Shipping capture, input, accessibility, or persistent coordinate memory as
  part of the core profile.

## E0 — bootstrap

Status: complete, 2026-08-08.

Create the canonical ZaguanLabs repository, project policies, crate boundaries,
specification outline, source gate, and administration controls. No wire
revision is claimed.

End result: the skeleton builds from source, provenance and inbound licensing
are auditable, and all metadata names the canonical upstream.

## E1 — protocol and companion

Status: complete, 2026-08-08. `agent-seat-proto` 0.1.1 implements wire
revision 3 and `agent-seat-mcp` 0.1.1 implements the static MCP boundary and
lazy discovery. The provider remains deliberately absent until T0.

Specify and implement bounded framing, strict messages, identities,
capabilities, errors, feature reporting, advertisement parsing, and revision
handling. Implement static MCP initialization/tool listing and lazy discovery
with explicit socket, environment, then selection-bound X11 precedence.

End result: malformed/oversized process tests, strict round trips, revision
refusal, desktop-free MCP startup, and isolated-X11 discovery all pass.

## T0 — provider foundation

Status: complete, 2026-08-08. `agent-seat-x11` 0.1.1 provides strict explicit
configuration, same-user kernel credential grants, bounded sessions, private
socket recovery, atomic selection ownership, clean withdrawal, and isolated
Openbox/no-WM lifecycle coverage.

Implement explicit enablement, strict configuration, local peer credentials,
deny-by-default grants, bounded sessions, the private socket, and atomic
per-screen provider ownership.

End result: standalone lifecycle, duplicate refusal, stale recovery, peer
denial, slow-client eviction, and crash isolation pass beside Openbox.

## T1 — observation

Status: complete, 2026-08-08. `agent-seat-x11` 0.1.2 advertises
`ewmh_observation` and implements bounded scoped snapshots, separately gated
titles, opaque session handles, descriptor generations, filtered monotonic
diffs, and typed resynchronization beside Openbox.

Implement bounded EWMH snapshots, opaque session handles, provider-local
freshness, filtered titles/scopes, monotonic diffs, and resynchronization.

End result: independently observed Openbox state converges with snapshots and
diffs without direct hidden-client disclosure.

## T2 — management

Status: complete, 2026-08-08. `agent-seat-x11` 0.1.3 advertises
`ewmh_management` and implements freshness-checked activation, polite close,
workspace switch/send, supported state changes, and decoration-aware frame
geometry with bounded post-send observation.

Implement only advertised activation, polite close, workspace, state, and
geometry requests. Recheck scope and freshness before send and observe the
terminal state afterward.

End result: every supported operation and every stale, refused, unsupported,
ignored, disappeared, timed-out, and failed branch is externally tested.

## T3 — controlled launch

Status: complete, 2026-08-08. `agent-seat-x11` 0.1.4 advertises
`desktop_launch` and implements bounded preference-ordered XDG discovery,
deny/allow-list/allow-installed policy, separately gated user entries,
shell-free `Exec` realization, bounded child supervision, launch tokens, and
exact startup-ID correlation without guessing.

Implement bounded XDG application discovery and shell-free desktop `Exec`
parsing with deny, allow-listed, allow-installed, deny-list, and separate
user-entry policy.

End result: allowed launch, every refusal mode, hostile metadata, correlation
limits, and launch failure pass without affecting Openbox. This completes the
Tier 0 core.

## Optional profiles

Status: first-release decisions complete, 2026-08-08. The evidence and stop
conditions are recorded in
[`optional-profiles.md`](optional-profiles.md). T4 output/core-window capture,
T5 input, and T6 semantics remain unsupported. A narrowly target-owned
Composite `obscured_capture` is deferred to a new wire revision and does not
delay C0.

- T4 may add capture only for modes that can reapply visibility and scope at
  capture time without returning unrelated pixels.
- T5 may add best-effort client-relative XTEST input only where human activity
  suppression and a local emergency stop can meet their stated contract.
- T6 may add bounded semantics only after fresh correlation and hidden-scope
  research succeeds.

Each profile keeps a typed unsupported result when its stop condition holds.

## C0 — compatibility and release

Status: complete in v0.1.0, 2026-08-08. The exact matrix, forced
revision-2 boundary probes, source gate, archive contract, and release evidence
are recorded in [`c0-verification.md`](c0-verification.md). The annotated
`v0.1.0` tag drives its verified ZaguanLabs GitHub source release.

Test released companions and providers through public boundaries, publish the
exact version/revision/backend/WM matrix, assemble the source release and
checksums, and tag the first supported release.

End result: users can distinguish tested compatibility, partial support,
incompatibility, and untested combinations without relying on source sharing.

## S0 — settings application

Status: foundation in progress. `agent-seat-x11` 0.1.6 separates complete
read-only policy validation from provider activation. Version 0.1.7 adds the
provider-owned validated snapshot and atomic policy transaction API with
stale-edit refusal and a private recovery file. No graphical toolkit has been
selected.

Add a standalone `agent-seat-settings` application that makes the strict
provider policy approachable without weakening it. The application should
discover the same effective configuration path as `agent-seat-x11`, explain
the security effect and dependencies of every capability, and support the
following bounded tasks:

- enable or disable the provider policy explicitly;
- configure observation scope, title access, and resource limits;
- grant or revoke observation, management, and launch capabilities while
  showing required capability dependencies;
- browse valid XDG desktop entries and manage launch allow/deny lists by their
  canonical desktop IDs;
- show a reviewable before/after policy diff and validation errors before any
  write; and
- distinguish saved configuration from the policy active in a running
  provider, with an explicit restart instruction when required.

The settings application is a policy editor, not a second authority. It must
not expose MCP tools, connect to X11, listen on the provider socket, grant a
live session, silently enable capabilities, or start/stop the provider without
a separate user-confirmed design. It must reuse the provider's exact parser
and validation rules rather than maintaining a schema that can drift.

Writes must be recoverable and race-aware: reject symlinks and non-regular
targets, preserve ownership, replace only after successful validation, retain
mode 0600, and never turn a failed edit into a partially written policy. The
initial implementation should remain usable without a running X server so
configuration recovery is possible from a terminal session.

End result: a first-time user can safely configure observation and an
application launch allow-list, understand what each permission enables,
validate the exact saved policy, and see whether a provider restart is needed.
Filesystem and process tests must prove refusal of unsafe targets, invalid
policy, concurrent replacement, and write failure before the milestone is
complete.
