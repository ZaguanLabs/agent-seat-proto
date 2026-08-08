# Product roadmap

Status: E0, E1, and T0 complete; T1 is next. Milestones are sequential;
optional profiles do not delay the Tier 0 core release. E0 evidence is recorded in
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
Openbox/no-WM lifecycle coverage. Backend features remain empty until T1.

Implement explicit enablement, strict configuration, local peer credentials,
deny-by-default grants, bounded sessions, the private socket, and atomic
per-screen provider ownership.

End result: standalone lifecycle, duplicate refusal, stale recovery, peer
denial, slow-client eviction, and crash isolation pass beside Openbox.

## T1 — observation

Implement bounded EWMH snapshots, opaque session handles, provider-local
freshness, filtered titles/scopes, monotonic diffs, and resynchronization.

End result: independently observed Openbox state converges with snapshots and
diffs without direct hidden-client disclosure.

## T2 — management

Implement only advertised activation, polite close, workspace, state, and
geometry requests. Recheck scope and freshness before send and observe the
terminal state afterward.

End result: every supported operation and every stale, refused, unsupported,
ignored, disappeared, timed-out, and failed branch is externally tested.

## T3 — controlled launch

Implement bounded XDG application discovery and shell-free desktop `Exec`
parsing with deny, allow-listed, allow-installed, deny-list, and separate
user-entry policy.

End result: allowed launch, every refusal mode, hostile metadata, correlation
limits, and launch failure pass without affecting Openbox. This completes the
Tier 0 core.

## Optional profiles

- T4 may add capture only for modes that can reapply visibility and scope at
  capture time without returning unrelated pixels.
- T5 may add best-effort client-relative XTEST input only where human activity
  suppression and a local emergency stop can meet their stated contract.
- T6 may add bounded semantics only after fresh correlation and hidden-scope
  research succeeds.

Each profile keeps a typed unsupported result when its stop condition holds.

## C0 — compatibility and release

Test released companions and providers through public boundaries, publish the
exact version/revision/backend/WM matrix, assemble the source release and
checksums, and tag the first supported release.

End result: users can distinguish tested compatibility, partial support,
incompatibility, and untested combinations without relying on source sharing.
