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

## T0 provider foundation

The provider configuration, runtime socket, ownership, session, and lifecycle
tests are original work written for this repository. X11 selection behavior
uses the ICCCM reference above; later EWMH work follows the freedesktop.org
specification at <https://specifications.freedesktop.org/wm-spec/latest/>.
Openbox and Xvfb are executed only as external black-box test processes.

New direct runtime dependencies are `toml` 0.8 (MIT OR Apache-2.0) for strict
configuration, Rustix 1 (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT)
for safe kernel peer credentials and process identity, and `signal-hook` 0.3
(Apache-2.0 OR MIT) for safe shutdown flags. Their canonical upstreams are
<https://github.com/toml-rs/toml>, <https://github.com/bytecodealliance/rustix>,
and <https://github.com/vorner/signal-hook>.

## T1 observation

The bounded EWMH sampler, per-session identity model, diff queue, scope and
title policy, and Openbox process fixtures are original work written for this
repository. EWMH property names, types, meanings, and client-message behavior
use the freedesktop.org specification linked above. Openbox 3.6.1 and Xvfb are
used only through their public process and X11 behavior as external regression
oracles. No Nobox source, schema, fixture, or prose was used.
