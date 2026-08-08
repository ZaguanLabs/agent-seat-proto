# Product roadmap

Status: E0, E1, T0--T3, the T4--T6 first-release decisions, and C0 are
complete in product release v0.1.0. E0 evidence is recorded in
[`e0-verification.md`](e0-verification.md).

## Goals

- Define one strict, bounded Agent Seat wire contract.
- Ship an authority-free generic MCP companion.
- Ship a standalone Tier 0 provider that works beside unmodified EWMH window
  managers, beginning with released Openbox.
- Make unsupported, refused, stale, sent-but-unobserved, timed-out, and failed
  outcomes distinct at public boundaries.
- Keep failures outside the window manager and keep source independently
  authored under Apache-2.0 with DCO sign-off.

## Non-goals

- Importing or relicensing Nobox implementation material.
- Treating same-user X11 as a secure isolation boundary.
- Giving the MCP companion policy authority.
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
