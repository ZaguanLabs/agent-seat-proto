# MCP 2026-07-28 compatibility verification

## Subject

- `agent-seat-mcp` 0.1.6 source
- MCP `2026-07-28` and legacy MCP `2025-11-25`
- Agent Seat wire revision 6 through `agent-seat-proto` 0.1.4

This record covers the newline-delimited JSON-RPC stdio implementation. It does
not cover Streamable HTTP, MCP authorization, extensions, subscriptions, or
multi-round-trip input.

## Evidence

Process-boundary tests start the real companion with no desktop and verify:

- legacy initialization, notification, 16-tool listing, unchanged schemas and
  absence of modern result/cache fields;
- modern `server/discover`, both advertised versions, required per-request
  metadata, `-32022` unsupported-version data, server identity, complete result
  typing, and public one-hour discovery/tool-list caching;
- deterministic 17-tool modern listing, with explicit context arguments and
  `seat_release` added only to that era;
- malformed JSON, invalid and null-ID requests, silent notifications, bounded
  lines, and lazy provider resolution.

A strict pathname-Unix provider fixture performs a real revision-6
`Hello`/`Welcome` exchange. It observes `seat_status` and
`applications_list` on the same explicitly named modern context, then observes
the companion release that context. The context is bounded to eight live
entries, never reused during one process, removed after provider failure, and
is not treated as provider authorization.

The repository gate is:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo doc --workspace --no-deps
```

## Conclusion and limits

The tested stdio companion supports both protocol eras without changing Agent
Seat wire revision 6 or moving grants, scope, peer authentication, or desktop
policy out of the provider. The result does not establish compatibility with a
particular released MCP host; host interoperability remains a separate
observation. The MCP context is process-local continuity bookkeeping, not a
cross-process session, credential, or authority token.
