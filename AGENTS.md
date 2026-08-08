# Agent Seat project guidance

This repository is the independent Apache-2.0 Agent Seat product. Keep code
small, explicit, bounded, and honest about the assurance the backend can
provide. Every line and dependency must justify its runtime and maintenance
cost. Unsafe Rust is forbidden.

## Boundaries

- `agent-seat-proto` owns display-server-neutral wire types and framing only.
  It depends on Serde and minimal serialization support, never on MCP, X11,
  policy, transport listeners, XDG discovery, or another product.
- `agent-seat-mcp` is a generic translator with no authority. Provider
  discovery and connection are lazy; initialization and tool listing work with
  no desktop.
- `agent-seat-x11` is the standalone Tier 0 authority. It owns strict config,
  verified peer identity, grants, scopes, X11/EWMH realization, and XDG launch
  policy. It is a separate process and never patches or embeds in Openbox.
- Core Tier 0 is observation, advertised EWMH management, and controlled
  desktop-entry launch. Capture, input, and accessibility stay unsupported
  until their separate threat-model gates pass.

## Clean-source rule

Use public standards, official documentation, this repository's specification,
and independently observed process behavior. Do not copy, translate, or adapt
Nobox code, schemas, tests, fixtures, comments, or prose. Nobox is prior art
and a black-box compatibility target only. Record provenance as required by
`PROVENANCE.md`.

## Build and test

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo doc --workspace --no-deps
```

Integration work uses isolated Xvfb, Xephyr, or Xnest displays and released
Openbox without touching the person's desktop. Tests observe public process,
filesystem, socket, and X11 behavior; logs do not make a test pass.

## Versioning and releases

Crate versions and wire revisions are distinct. Patch-bump each crate changed
by a successful milestone; change minor or major versions only by explicit
maintainer decision. A breaking wire change always allocates a new advertised
revision. Keep crates `publish = false` until their release checklist passes.

After each verified milestone, commit and push `main`. A release additionally
needs the compatibility matrix, changelog, source archive, checksums, tag, and
GitHub release described in `RELEASING.md`.

Preserve unrelated work. Do not commit build output, credentials, live desktop
data, or generated protocol artifacts.
