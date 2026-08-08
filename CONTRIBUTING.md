# Contributing

Contributions are welcome when they keep the protocol small, bounded, and
honest about backend guarantees.

## Developer Certificate of Origin

Every commit must certify the Developer Certificate of Origin 1.1 in
[`DCO.txt`](DCO.txt). Sign off a commit with:

```sh
git commit --signoff
```

The sign-off states that you may submit the work under Apache-2.0. There is no
CLA or copyright assignment. Corporate contributors are responsible for any
employer authorization they require.

## Provenance

Describe the patch's origin in the pull request using one of the classes in
[`PROVENANCE.md`](PROVENANCE.md). Do not copy or mechanically translate Nobox
source, tests, fixtures, schemas, comments, or prose. Public specifications,
standards, official documentation, and independently observed behavior may be
used as requirements when named precisely.

## Quality gate

Before submitting:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo doc --workspace --no-deps
```

Keep dependencies and allocations deliberate. Unsafe Rust is forbidden.
Protocol inputs, collections, queues, strings, retries, and deadlines must have
finite bounds before expensive work begins.

Pull requests need a passing gate, resolved discussion, valid DCO sign-offs,
and an approving maintainer review. Incompatible wire behavior requires a new
advertised revision; resemblance to an older JSON shape is not compatibility.
