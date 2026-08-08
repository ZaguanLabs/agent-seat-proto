# C0 compatibility and release verification

Status: complete for product release v0.1.0, 2026-08-08.

## Goal

Verify the first Tier 0 core solely through public process, socket, MCP, and
X11 boundaries; record exact compatible and incompatible combinations; and
make the source release reproducible without importing another product's test
implementation.

## Release identity

| Item | Value |
| --- | --- |
| Product tag | `v0.1.0` |
| Wire | Agent Seat revision 3 |
| Protocol crate | `agent-seat-proto` 0.1.1 |
| Companion | `agent-seat-mcp` 0.1.1; MCP `2025-11-25` |
| Provider | `agent-seat-x11` 0.1.4; `x11_ewmh`; `tier0` |
| Core features | `ewmh_observation`, `ewmh_management`, `desktop_launch` |
| Rust gate | 1.85.0 |
| Test WM/server | Openbox 3.6.1 / Xvfb 21.1.23 |
| Host | Linux 6.18.35 x86-64 |

## Independent-product result

The complete source gate passed from a clean working tree:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
```

The process tests cover framing and strict decoding, MCP lazy discovery,
selection-bound advertisement, provider ownership/lifecycle, same-UID grants,
bounded observation/events, EWMH management terminal states, XDG launch policy,
shell-free execution, exact/absent startup correlation, and provider/WM failure
isolation. The ten-test Openbox provider suite also passed ten consecutive runs
after T3.

Result: `agent-seat-mcp` 0.1.1 and `agent-seat-x11` 0.1.4 are compatible for
the complete revision-3 Tier 0 core.

## Released Nobox boundary probes

The public GitHub source archive for Nobox v0.1.3 was downloaded from
`kekePower/nobox`, SHA-256
`b732fcb7eb2fb773b0514b6ffead4b6078ff50f58b9a4e6fdf85b57b0c24aee9`, and
built at released commit `be4e2157079c080f091a1081ede2b99df846e3a7` with
Rust 1.94.0. Only its resulting executables and published documentation were
used for the probes; no source, schema, fixture, or implementation was copied.

Each direction used an isolated Xvfb display and explicit private socket so
discovery precedence could not hide the version boundary:

1. Released `nobox-agent` 0.1.7 was initialized over MCP `2025-11-25` and sent
   `seat_status` to the independent revision-3 provider. The provider refused
   the revision-2 opening and closed without serving a request; the Nobox
   companion reported that no compatible manager connection existed.
2. `agent-seat-mcp` 0.1.1 was initialized over MCP `2025-11-25` and sent
   `seat_status` to the released Nobox v0.1.3 Tier 1 seat. Nobox refused the
   revision-3 opening and closed without serving a request; the independent
   companion returned typed `unavailable`/`reconnect` with “provider closed
   during session opening.”

Result: the products are intentionally incompatible across Nobox wire revision
2 and independent wire revision 3. Forced explicit-socket use fails closed in
both directions. Similar names and concepts are not treated as compatibility.

## Optional features

The first release advertises no capture, obscured capture, output capture,
input injection, human activity, or accessibility feature. The T4--T6 evidence
and stop decisions are in [`optional-profiles.md`](optional-profiles.md). Their
absence is supported behavior, not an untested compatibility claim.

## Source-release result

The tag workflow reruns the complete gate, requires an annotated tag, creates a
`git archive` with a versioned top-level directory, verifies the archive
contains the Apache license, lockfile, toolchain, build instructions, security,
contribution, provenance, and release policies, checks the extracted workspace
with its locked dependency graph, and publishes an adjacent SHA-256 file.
GitHub's generated archives remain convenience links; the custom archive and
checksum are the supported source assets.

## End result

Users can distinguish the supported independent revision-3 pairing, the
explicitly incompatible Nobox revision-2 pairings, and absent optional
profiles. No claim depends on shared source or structural similarity.
