# Agent Seat

Agent Seat is a local, policy-controlled bridge between AI agents and a Linux
desktop. It gives an agent a small set of structured desktop operations while
leaving authority, scope, and runtime control with a separate process in the
user's graphical session.

The current release targets X11 and is proven with Openbox. An agent can inspect
the windows it is allowed to see, use supported window-manager operations,
launch admitted desktop applications, and—when separately granted—capture a
target window or send bounded input. The design favors desktop metadata and
standard keyboard commands over repeated full-screen screenshots and blind
coordinate clicks.

Agent Seat is an independent open-source project maintained by
[ZaguanLabs](https://github.com/ZaguanLabs). It is licensed under the Apache
License 2.0; it is not an Apache Software Foundation project and is not
affiliated with the Foundation.

## What is included

The repository builds five components with deliberately separate roles:

| Component | Role |
| --- | --- |
| `agent-seat-x11` | Standalone X11 provider. Owns configuration, policy, grants, X11 authority, launch admission, and the volatile runtime seat. |
| `agent-seat-mcp` | Authority-free MCP companion. Translates MCP tools to one authenticated provider session and supports MCP `2026-07-28` and `2025-11-25`. |
| `agent-seat-settings` | GTK 4 and command-line editor for the provider policy, including explicit runtime seat controls. |
| `agent-seat-proto` | Display-server-neutral bounded wire types and framing. |
| `agent-seat-activity-broker` | Optional research component for a stronger physical-activity profile; it is not required for ordinary X11 input. |

Wire revision 9 provides the released local protocol binding. Crate versions
and wire revisions are separate: Agent Seat v0.2.0 ships all five crates at
version 0.2.0 without changing the wire contract.

## What works today

With Openbox on Linux X11, the v0.2.0 source has verified support for:

- scoped window and workspace observation, optional titles, and filtered
  events;
- freshness-checked EWMH activation, close, workspace, state, and geometry
  requests;
- shell-free launch from a bounded XDG desktop-entry catalog and explicit
  application admission policy;
- a disabled-by-default, per-provider runtime seat which a person must enable
  before an agent session is admitted;
- optional target-relative pointer actions, layout-aware typing, standard key
  commands such as Control+L and Page Down, and bounded long-form input;
- optional target-owned PNG capture, including bounded regions; and
- optional focused UTF-8 text transfer for exact accented and multiline text,
  with explicit clipboard side effects.

The core release promise is observation, advertised EWMH management, and
controlled desktop-entry launch. Input, capture, and exact text transfer are
experimental profiles with separate grants and narrower claims. In particular,
ordinary X11 cannot give Agent Seat physical-user priority, an accessibility
tree, proof that an application accepted an action, or permission to capture
the whole output. See the exact [compatibility matrix](docs/compatibility.md)
and [security model](docs/security/security-model.md).

## First run

Build the workspace with Rust 1.85 or newer. The Settings application also
requires GTK 4 development files (`libgtk-4-dev` on Debian-family systems).

```sh
cargo build --release --workspace
```

Install the binaries using your normal packaging or installation method, then
run the provider once from the X11 desktop session:

```sh
agent-seat-x11
```

On first run it creates a private, extensively commented policy at
`$XDG_CONFIG_HOME/agent-seat/config.toml`, or
`$HOME/.config/agent-seat/config.toml` when `XDG_CONFIG_HOME` is unset. The
template starts disabled and the provider exits without connecting to X11.
Review it, enable only the capabilities and applications you want, then run:

```sh
agent-seat-x11 --check-config
agent-seat-x11
```

In another terminal, explicitly enable access for that provider instance:

```sh
agent-seat-x11 seat enable
```

The seat starts disabled after every provider start and is disabled again when
the provider exits. Disable it immediately with:

```sh
agent-seat-x11 seat disable
```

You can edit and validate the same policy graphically:

```sh
agent-seat-settings
```

Register the MCP companion with an agent host using the equivalent of:

```json
{
  "mcpServers": {
    "agent-seat": {
      "command": "agent-seat-mcp"
    }
  }
}
```

`agent-seat-mcp --print-mcp-config` prints this minimal registration. Provider
discovery is lazy, so MCP initialization and tool listing work even when no
desktop provider is running. If the host removes desktop environment variables,
pass `DISPLAY`, `--socket`, or `AGENT_SEAT_SOCKET` as described in the
[companion guide](docs/mcp.md).

For the complete policy, Openbox autostart guidance, application admission,
optional profile grants, and troubleshooting, read the
[provider guide](docs/provider.md), [Settings guide](docs/settings.md), and
[documentation index](docs/README.md).

## How the boundary works

The agent harness does not receive X11 authority from Agent Seat. The generic
MCP companion owns no policy and discovers no provider until a desktop-backed
tool is called. `agent-seat-x11` authenticates local peers, applies the saved
grant and window scope, checks fresh target evidence, and performs only its
bounded operations.

Configuration activation and runtime access are independent gates. A valid
saved policy does not enable a running seat, and enabling one provider instance
does not survive restart, logout, or X11 loss. Requests fail closed when the
provider, target, focus, scope, or required evidence is missing or stale.

The protocol work is also being developed as a possible implementation-neutral
foundation for other desktop providers. The current
[information model](docs/protocol/information-model.md),
[pre-RFC draft](docs/protocol/r0-protocol-rfc.md), and
[conformance format](docs/protocol/conformance-report.md) separate portable
semantics from the X11 backend's actual guarantees.

## Development

The local quality gate is:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo doc --workspace --no-deps
```

Contributions must remain small, bounded, and explicit about authority and
assurance. Read [CONTRIBUTING.md](CONTRIBUTING.md) and
[PROVENANCE.md](PROVENANCE.md) before submitting work. Security reports use
GitHub private vulnerability reporting as described in
[SECURITY.md](SECURITY.md).

## License

Copyright holders license this project under the
[Apache License, Version 2.0](LICENSE). “Apache” describes the license only.
Agent Seat is independently authored and maintained and is not endorsed by,
sponsored by, or affiliated with the Apache Software Foundation.
