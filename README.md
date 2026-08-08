# Agent Seat Protocol

`agent-seat-proto` is the canonical Apache-2.0 project for a bounded protocol
between desktop providers and authority-free agent companions. The repository
is owned from its first commit by
[`ZaguanLabs`](https://github.com/ZaguanLabs).

E1 and the T0--T3 Tier 0 core are complete. The project implements strict Agent
Seat wire revision 3, a generic MCP `2025-11-25` companion, and a standalone
provider with bounded EWMH observation, freshness-checked management, and
policy-controlled desktop-entry launch. The four deliverables are:

- `agent-seat-proto`: display-server-neutral wire types and framing only;
- `agent-seat-mcp`: a generic MCP translator with no policy authority;
- `agent-seat-x11`: a standalone Tier 0 provider for unmodified EWMH window
  managers such as Openbox; and
- `agent-seat-settings`: a human-facing policy editor with display-independent
  validation, inspection, and recovery commands plus a GTK 4 interface.

The Tier 0 core provides bounded observation, supported EWMH management,
and controlled desktop-entry launch. Capture, input, and accessibility are
separate optional profiles and are not core-release promises.

The current provider target is a local Linux X11 session. Other Unix peer
credential mechanisms and non-X11 backends are not yet supported.

## First run

Run the provider once from your X11 desktop session:

```sh
agent-seat-x11
```

If no configuration exists, the command creates a private, extensively
commented template at `$XDG_CONFIG_HOME/agent-seat/config.toml`, falling back
to `$HOME/.config/agent-seat/config.toml`, and exits without connecting to X11.
The template contains the current UID, explains every setting and capability,
and remains disabled until the user explicitly changes `enabled = false` to
`enabled = true`.

After reviewing the policy, validate and start it:

```sh
agent-seat-x11 --check-config
agent-seat-x11
```

The provider runs in the foreground. Add `agent-seat-x11 &` to Openbox
autostart after validating the policy. See [the provider guide](docs/provider.md)
for the complete configuration and security model. `agent-seat-x11 --help`
also describes the first-run flow and command-line options.

The Settings command can inspect and recover policy without a display:

```sh
agent-seat-settings --check
agent-seat-settings --print
agent-seat-settings --restore-previous
```

Run `agent-seat-settings` with no command to open its GTK interface. The
complete interface edits activation, capability grants, visible-window scope,
the bounded XDG launch catalog, and resource limits. It validates and shows an
exact policy diff before an atomic save, retains a private recovery policy, and
distinguishes saved policy from best-effort active-provider evidence. See
[the Settings guide](docs/settings.md) for the complete first-run and recovery
workflow.

The first supported source release is product tag `v0.1.0`. Its component
versions are `agent-seat-proto` 0.1.1, `agent-seat-mcp` 0.1.1, and
`agent-seat-x11` 0.1.4; crate versions and the wire revision are intentionally
separate identities.

## Build

Rust 1.85 or newer is required. The repository pins its minimum toolchain for
the ordinary source gate. Building the Settings interface also requires GTK 4
development files (`libgtk-4-dev` on Debian-family systems):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo doc --workspace --no-deps
```

`agent-seat-mcp` can initialize and list its static tools without a desktop.
Its first tool call resolves `--socket`, `AGENT_SEAT_SOCKET`, or the live
selection-bound X11 advertisement. The standalone provider answers
authenticated `seat_status`, bounded desktop snapshots, filtered event
subscriptions, supported EWMH management, and controlled XDG application
discovery and launch. Optional profiles remain absent.

The normative wire contract is [`docs/specification.md`](docs/specification.md),
the companion contract is [`docs/mcp.md`](docs/mcp.md), and provider setup is
[`docs/provider.md`](docs/provider.md). Settings usage is
[`docs/settings.md`](docs/settings.md). Optional-profile stop decisions are in
[`docs/optional-profiles.md`](docs/optional-profiles.md).

## Project policy

Contributions are Apache-2.0 under DCO 1.1 sign-off. Read
[`CONTRIBUTING.md`](CONTRIBUTING.md) and [`PROVENANCE.md`](PROVENANCE.md) before
submitting work. Security reports use GitHub private vulnerability reporting as
described in [`SECURITY.md`](SECURITY.md).

This project is independently authored. Nobox is prior art and a future
black-box compatibility target, not a source dependency. No Nobox code,
history, fixtures, schemas, or prose is imported here.
