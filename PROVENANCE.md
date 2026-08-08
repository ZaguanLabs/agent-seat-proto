# Clean-source provenance policy

This repository is independently authored under Apache-2.0. Public behavior
may be compatible with other Agent Seat implementations, but their source is
not an implementation input.

Every pull request must identify one provenance class:

1. original work written for this repository from its public specification;
2. a dependency or asset with its exact upstream URL, license, and purpose; or
3. a standards-derived fact with the exact public source named.

Copied or mechanically translated implementation code, comments, test
language, fixtures, schemas, or prose are rejected. Ideas from another product
must first be stated as observable requirements, then implemented afresh.

## Initial tree

The initial Rust sources, manifests, workflow, ownership rules, policies, and
project prose were written for this repository after it was created. The only
verbatim standard documents are:

- `LICENSE`: Apache License 2.0 from the Apache Software Foundation;
- `DCO.txt`: Developer Certificate of Origin 1.1 from The Linux Foundation.

GitHub's checkout action is referenced as a CI dependency and is not vendored.
The Rust toolchain is installed directly by `rustup`. No file in the initial
tree was copied from Nobox.

## E1 protocol and companion

The revision-3 wire types, codec, advertisement grammar, companion, and tests
are original work written for this repository from its public specification.
The implementation used these public standards as behavioral inputs:

- MCP `2025-11-25` lifecycle and tools specifications:
  <https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle>
  and <https://modelcontextprotocol.io/specification/2025-11-25/server/tools>;
- JSON-RPC 2.0: <https://www.jsonrpc.org/specification>; and
- ICCCM selection ownership conventions:
  <https://www.x.org/releases/current/doc/xorg-docs/icccm/icccm.html>.

Direct runtime dependencies are Serde 1 (MIT OR Apache-2.0) for typed data,
`serde_json` 1 (MIT OR Apache-2.0) for the specified JSON encoding, and
`x11rb` 0.14 (MIT OR Apache-2.0) for safe X11 discovery. Their canonical
upstreams are <https://github.com/serde-rs/serde>,
<https://github.com/serde-rs/json>, and <https://github.com/psychon/x11rb>.
Exact resolved versions and transitive dependencies are recorded in
`Cargo.lock`.
