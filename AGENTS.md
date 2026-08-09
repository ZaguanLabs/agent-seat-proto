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

## Documentation layout

`docs/README.md` is the documentation index and must stay current whenever a
document is added, moved, renamed, or removed. Classify a document by its
primary audience and purpose:

- Keep `docs/` itself mostly for user and operator guides. These explain setup,
  configuration, normal operation, compatibility, and troubleshooting without
  requiring implementation knowledge.
- Put internal architecture, roadmap, UI design, and feature-decision material
  in `docs/design/`.
- Put wire contracts, information models, registries, protocol profiles,
  conformance formats, and RFC preparation in `docs/protocol/`. Keep
  machine-readable protocol artifacts beside the document that governs them.
- Put trust models, threat analysis, privileged deployment contracts, and
  security stop decisions in `docs/security/`.
- Put completed evidence records, compatibility milestone records, hostile-test
  requirements, and full-system participation contracts in
  `docs/verification/`.

Prefer one clear home over duplicate user and technical documents. User guides
may link to technical detail but must state the supported behavior first. Keep
experimental, candidate, verified, and released claims visibly distinct.

Use relative Markdown links for repository documents. A documentation move is
not complete until all inbound links, command examples, provenance references,
release notes, and directory indexes are updated and checked. Verification
records must name the exact subject and environment, distinguish observation
from inference, and preserve limitations; logs alone are never evidence.
Historical release notes must retain paths that are accurate for their tagged
source archive; do not rewrite them to match a later tree layout.

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
