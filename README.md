# Agent Seat Protocol

`agent-seat-proto` is the canonical Apache-2.0 project for a bounded protocol
between desktop providers and authority-free agent companions. The repository
is owned from its first commit by
[`ZaguanLabs`](https://github.com/ZaguanLabs).

E1 and the T0--T3 Tier 0 core are complete. Current source implements strict
Agent Seat wire revision 5, a generic MCP `2025-11-25` companion, and a standalone
provider with bounded EWMH observation, freshness-checked management, and
policy-controlled desktop-entry launch. The five deliverables are:

- `agent-seat-proto`: display-server-neutral wire types and framing only;
- `agent-seat-mcp`: a generic MCP translator with no policy authority;
- `agent-seat-x11`: a standalone Tier 0 provider for unmodified EWMH window
  managers such as Openbox;
- `agent-seat-settings`: a human-facing policy editor with display-independent
  validation, inspection, and recovery commands plus a GTK 4 interface; and
- `agent-seat-activity-broker`: optional research tooling for a possible future
  physical-activity assurance profile; it is not required by Tier 0.5 input.

The Tier 0 core provides bounded observation, supported EWMH management,
and controlled desktop-entry launch. Capture, input, and accessibility are
separate optional profiles and are not core-release promises.

Revision 5 adds an experimental Tier 0.5 X11 bridge: target-aware pointer move
and click plus focus-bound bounded text. It uses the existing `agent-seat-x11`
process, XTEST, the live keyboard map, fresh target evidence, and the volatile
runtime seat. It needs no root service, raw input-device permission, additional
group membership, or activity broker. It does not claim physical-user priority;
a person and an agent can overlap on ordinary X11.

Keyboard text follows the current effective XKB layout and group. The provider
uses XKB key types and levels, including bounded Shift, Level3, and Level5
modifiers, and refuses before sending when a scalar cannot be produced exactly.
It does not guess from compatibility-map columns, change the layout or group,
run compose/IME sequences, or use clipboard paste.

The repository retains the separately confined activity-broker experiment as
research for a possible future stronger profile. It is intentionally outside
the ordinary setup path. Its administrator workflow and unresolved physical-
replacement and trusted-lock gates remain documented under
[`docs/security`](docs/security/README.md).

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
# In another terminal, only while Agent Seat access is wanted:
agent-seat-x11 seat enable
```

For desktop input, grant `observe_structure` plus `input_pointer` and/or
`input_keyboard` in the policy (or on Settings → Access), restart the provider
after saving, then enable the current runtime seat. No device-group or root
setup is part of this path.

The provider runs in the foreground and every process starts with a volatile
disabled seat. Add only `agent-seat-x11 &` to Openbox autostart after
validating the policy; never auto-run `seat enable`. See the
[Tier 0.5 gate](docs/tier-0.5-seat-gate.md) and
[the provider guide](docs/provider.md)
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
distinguishes saved policy, best-effort active-policy evidence, and the current
provider's volatile Tier 0.5 seat. The separate runtime panel can Refresh,
explicitly Enable for this provider instance, or immediately Disable; opening
and saving never enable it. See
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
discovery and launch. Separately granted `pointer_move`, `pointer_click`, and
`keyboard_type` tools are available only while the volatile Tier 0.5 seat is
enabled; they are not part of the supported Tier 0 core.

The [documentation index](docs/README.md) separates user guides from technical
reference material. Portable pre-RFC semantics are in the
[`information model`](docs/protocol/information-model.md). The normative
implemented wire contract is
[`docs/protocol/specification.md`](docs/protocol/specification.md), with a
[machine-readable registry projection](docs/protocol/registry-v1.json)
governed by the repository [registry policy](docs/protocol/registries.md). The
companion contract is [`docs/mcp.md`](docs/mcp.md), and provider setup is
[`docs/provider.md`](docs/provider.md). Settings usage is
[`docs/settings.md`](docs/settings.md). Optional-profile stop decisions are in
[`docs/design/optional-profiles.md`](docs/design/optional-profiles.md). The
implementation-independent standards direction is the repository's non-external
[`R0 pre-RFC draft`](docs/protocol/r0-protocol-rfc.md), beginning with the
standalone
[`agent-seat.x11-ewmh-core.v1`](docs/protocol/profiles/x11-ewmh-core-v1.md)
backend profile, the optional
[`agent-seat.x11-tier0.5-input.v1`](docs/protocol/profiles/x11-tier0.5-input-v1.md)
profile, and portable
[`agent-seat.conformance-report/1`](docs/protocol/conformance-report.md) evidence
format.

## Project policy

Contributions are Apache-2.0 under DCO 1.1 sign-off. Read
[`CONTRIBUTING.md`](CONTRIBUTING.md) and [`PROVENANCE.md`](PROVENANCE.md) before
submitting work. Security reports use GitHub private vulnerability reporting as
described in [`SECURITY.md`](SECURITY.md).

This project is independently authored. Nobox is prior art and a future
black-box compatibility target, not a source dependency. No Nobox code,
history, fixtures, schemas, or prose is imported here.
