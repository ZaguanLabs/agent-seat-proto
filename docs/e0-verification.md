# E0 bootstrap verification

Status: complete, 2026-08-08.

E0 established the independent product before any protocol or provider
implementation. No wire revision or compatibility is claimed.

## Source evidence

- The canonical upstream is the public
  [`ZaguanLabs/agent-seat-proto`](https://github.com/ZaguanLabs/agent-seat-proto)
  repository, created directly under the organization.
- GitHub recognizes the repository license as Apache-2.0. DCO 1.1,
  contribution, provenance, conduct, security, and release policies are
  present at the repository root.
- `agent-seat-proto`, `agent-seat-mcp`, and `agent-seat-x11` are separate Rust
  packages at version 0.1.0 with Apache-2.0 and the canonical repository in
  their package metadata. All remain `publish = false`.
- The protocol crate has no dependency and no outward policy/backend edge.
  The two executable crates are deliberate non-implementations until E1 and
  T0 respectively.
- The initial commits are maintainer-authored after repository creation and
  carry DCO sign-off. `PROVENANCE.md` accounts for the standard license/DCO
  texts and the fresh initial tree.

The source gate passed locally and in GitHub Actions:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo doc --workspace --no-deps
```

The verified CI run for the E0 head is
[`31254011157`](https://github.com/ZaguanLabs/agent-seat-proto/actions/runs/31254011157).

## Administration evidence

GitHub reported the following state at E0 exit:

- ZaguanLabs requires two-factor authentication;
- `kekePower` is the organization owner and initial code, security, release,
  recovery, and package-publishing owner;
- `main` rejects deletion and force-pushes;
- changes require the `source gate`, one approving review, stale-review
  dismissal, last-push approval, and resolved conversations;
- the organization owner retains the documented bootstrap/emergency bypass;
- web commits require sign-off and merged branches are deleted;
- workflow permissions default to read and cannot approve pull requests;
- private vulnerability reporting is enabled;
- secret scanning and push protection are enabled; and
- Dependabot security updates are enabled.

The repository has issues enabled and wiki/projects disabled. Its description,
homepage, topics, default branch, package metadata, policies, and documentation
all name `ZaguanLabs/agent-seat-proto` as canonical upstream.

## Provenance and separation audit

No Cargo dependency, submodule, generated schema, fixture, copied history, or
test import connects this repository to Nobox. Nobox is named only as prior art
and a future black-box compatibility target. The independent project has not
accepted outside source, published a crate, advertised a protocol revision, or
claimed compatibility.

## End result

The independent Apache-2.0 product is publicly owned and administered under
ZaguanLabs, builds from a clean source checkout, and has enforceable inbound,
security, review, and release controls. E1 may now independently specify and
implement the public wire and authority-free MCP boundary.
