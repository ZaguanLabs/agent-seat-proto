# Changelog

All notable changes to this project are documented here.

## Unreleased

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
