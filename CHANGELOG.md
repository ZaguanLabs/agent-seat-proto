# Changelog

All notable changes to this project are documented here.

## Unreleased

### Added

- Planned an S0 `agent-seat-settings` application for safe, human-facing
  provider policy editing, shared validation, reviewable changes, and explicit
  active-versus-saved state without adding another runtime authority.

### Changed

- `agent-seat-x11` 0.1.6 now treats policy validity and provider activation as
  separate states: `--check-config` successfully validates complete disabled
  policies and reports their activation state, while an ordinary provider
  start continues to require explicit enablement. This is the first S0
  settings-application foundation.
- `agent-seat-x11` 0.1.5 now creates a private, disabled, extensively commented
  policy template on first use of the default configuration path, reports the
  exact next steps without touching X11, and provides complete first-run help
  and setup documentation. Explicit configuration paths remain non-creating.

## 0.1.0 - 2026-08-08

### Added

- Apache-2.0 project skeleton, contribution and provenance policy, security and
  release process, initial architecture, and CI source gate.
- Reserved protocol, MCP companion, and standalone X11 provider crate
  boundaries without claiming a wire revision.
- Completed E0 with ZaguanLabs ownership, public source, two-factor
  enforcement, branch protection, private security reporting, secret scanning,
  and a passing pinned-toolchain source gate.
- Defined independently authored Agent Seat wire revision 3 with bounded
  framing and values, strict messages, typed identities, exact capabilities,
  feature and assurance reporting, stable failures, and selection-bound X11
  advertisement parsing.
- Implemented the authority-free `agent-seat-mcp` 0.1.1 companion with MCP
  `2025-11-25` lifecycle handling, twelve closed-schema core tools, lazy
  provider connection, exact socket-source precedence, and actionable
  structured failures.
- Added malformed, oversized, revision-refusal, process-boundary, lazy-startup,
  and isolated-X11 discovery coverage for the E1 exit gate.
- Implemented the `agent-seat-x11` 0.1.1 T0 provider foundation with explicit
  strict configuration, same-user kernel credential grants, bounded sessions,
  private stale-socket recovery, atomic per-screen selection ownership, clean
  withdrawal, and typed status/refusal/unsupported results.
- Added bare-Xvfb and Openbox process tests for lifecycle, duplicate refusal,
  stale recovery, peer denial, slow-client eviction, selection loss, clean
  shutdown, and window-manager survival after provider failure.
- Implemented `agent-seat-x11` 0.1.2 T1 observation with strict deny-by-default
  client scope, independently gated titles, bounded EWMH snapshots,
  per-session opaque identities, generations, filtered monotonic diffs, and
  typed resynchronization.
- Added Openbox process coverage for client creation, title changes,
  minimize/remap, workspace movement, destruction, scope departure/return,
  title redaction, event filters, and stale cursors.
- Implemented `agent-seat-x11` 0.1.3 T2 management with a server-grabbed final
  freshness check, exact EWMH/target support checks, polite close, workspace
  switch/send, supported state changes, and decoration-aware frame geometry.
- Added bounded post-send `observed`, `timed_out`, and `target_gone` results and
  Openbox coverage for supported operations, refusal, stale and out-of-scope
  targets, invalid workspaces, unsupported state, ignored requests, responsive
  close, and target disappearance.
- Implemented `agent-seat-x11` 0.1.4 T3 launch with strict bounded XDG catalog
  discovery, deny/allow-list/allow-installed policy, a separate user-entry
  gate, shell-free desktop `Exec` parsing, bounded child supervision, and exact
  startup-ID correlation that remains optional in the reply.
- Added Openbox process coverage for an allowed system entry, an unlisted
  entry, a default-denied user entry, hostile metacharacters, exact and absent
  correlation, launch failure, and post-failure window-manager responsiveness.
- Completed C0 with the exact independent and cross-product compatibility
  matrix, explicit T4--T6 stop decisions, a verified source-archive contract,
  SHA-256 asset, annotated tag workflow, and ZaguanLabs GitHub release notes.
