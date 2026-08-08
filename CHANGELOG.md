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
