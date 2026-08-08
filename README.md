# Agent Seat Protocol

`agent-seat-proto` is the canonical Apache-2.0 project for a bounded protocol
between desktop providers and authority-free agent companions. The repository
is owned from its first commit by
[`ZaguanLabs`](https://github.com/ZaguanLabs).

E1, T0, T1, and T2 are complete. The project implements strict Agent Seat wire
revision 3, a generic MCP `2025-11-25` companion, and a standalone provider
with bounded EWMH observation and freshness-checked management. T3 begins
controlled desktop-entry launch. The three deliverables are:

- `agent-seat-proto`: display-server-neutral wire types and framing only;
- `agent-seat-mcp`: a generic MCP translator with no policy authority; and
- `agent-seat-x11`: a standalone Tier 0 provider for unmodified EWMH window
  managers such as Openbox.

The Tier 0 core will provide bounded observation, supported EWMH management,
and controlled desktop-entry launch. Capture, input, and accessibility are
separate optional profiles and are not core-release promises.

The current provider target is a local Linux X11 session. Other Unix peer
credential mechanisms and non-X11 backends are not yet supported.

## Build

Rust 1.85 or newer is required. The repository pins its minimum toolchain for
the ordinary source gate:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo doc --workspace --no-deps
```

`agent-seat-mcp` can initialize and list its static tools without a desktop.
Its first tool call resolves `--socket`, `AGENT_SEAT_SOCKET`, or the live
selection-bound X11 advertisement. The standalone provider now answers authenticated
`seat_status`, bounded desktop snapshots, filtered event subscriptions, and
supported EWMH management. T3 adds controlled launch.

The normative wire contract is [`docs/specification.md`](docs/specification.md),
the companion contract is [`docs/mcp.md`](docs/mcp.md), and provider setup is
[`docs/provider.md`](docs/provider.md).

## Project policy

Contributions are Apache-2.0 under DCO 1.1 sign-off. Read
[`CONTRIBUTING.md`](CONTRIBUTING.md) and [`PROVENANCE.md`](PROVENANCE.md) before
submitting work. Security reports use GitHub private vulnerability reporting as
described in [`SECURITY.md`](SECURITY.md).

This project is independently authored. Nobox is prior art and a future
black-box compatibility target, not a source dependency. No Nobox code,
history, fixtures, schemas, or prose is imported here.
