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

## T2 management

The EWMH sender, frame/client geometry conversion, freshness checks,
post-request sampler, and management process tests are original work written
for this repository. Message fields, source indication, StaticGravity, state
actions, and advertised-action rules use EWMH 1.5 from the freedesktop.org
specification linked above. Openbox remains an external black-box oracle.

## T3 controlled launch

The bounded XDG catalog, strict desktop-entry parser, launch policy, process
supervisor, startup correlation, and Openbox launch fixtures are original work
written for this repository. Search order, desktop IDs, entry syntax, key
semantics, `Exec` quoting/field codes, and startup-ID behavior use:

- XDG Base Directory Specification 0.8:
  <https://specifications.freedesktop.org/basedir/latest/>;
- Desktop Entry Specification 1.5:
  <https://specifications.freedesktop.org/desktop-entry/latest/>; and
- Startup Notification Protocol 0.2:
  <https://specifications.freedesktop.org/startup-notification/0.2/>.

No new runtime dependency was added. Openbox and Xvfb remain external
black-box test processes; standard command-line programs are process fixtures,
not linked or distributed dependencies.

## Optional profile decision study

The T4--T6 analysis in `docs/optional-profiles.md` is original work written for
this repository. It uses the public X11 core, Security, Composite, XTEST, and
RECORD extension specifications and the public AT-SPI 2 API documentation
linked from that document. No implementation source or fixture from Nobox or
another Agent Seat product was consulted or imported.

## First-run configuration workflow

The generated configuration template, creation behavior, CLI guidance,
documentation, and process tests are original work written for this
repository from its existing strict provider policy. No external product code,
schema, fixture, comments, or prose was used.

The later separation of read-only policy validity from runtime activation is
also original work derived from the same policy. It introduces no external
schema or implementation input.

The settings transaction API and its filesystem process tests are original
work. They use the safe Rustix wrappers for Linux `flock`, `renameat2` exchange,
and no-replace behavior; Rustix provenance and licensing are recorded above.

The typed settings draft and public installed-application catalog are original
work derived from the provider's existing strict policy and T3 discovery code.
The catalog continues to use only the XDG and Desktop Entry standards recorded
above. Comment-preserving edits use `toml_edit` 0.22 (MIT OR Apache-2.0), which
was already a locked transitive component of `toml` and is now a direct
dependency. Its canonical upstream is <https://github.com/toml-rs/toml>. No
external schema or other product implementation was used.

## Settings application roadmap

The S0 settings application requirements and authority boundaries are original
planning work derived from this repository's existing strict configuration and
provider architecture. No external product code, schema, fixture, comments, or
prose was used.

The Settings interaction and visual design is original work. GTK 4 toolkit
selection uses the official gtk-rs documentation at
<https://crates.io/crates/gtk4/0.10.3> and GTK documentation at
<https://docs.gtk.org/gtk4/> as public technical inputs. No code or asset was
copied from that documentation, and no Nobox source or design artifact was
consulted.

The Settings model, command parser, process tests, and initial GTK shell are
original work written for this repository. The direct GUI dependency is
`gtk4` 0.10.3 (MIT) from <https://github.com/gtk-rs/gtk4-rs>; it declares Rust
1.83 and is pinned for the workspace's Rust 1.85 baseline. The dynamically
linked GTK 4 system toolkit is LGPL-2.1-or-later and documented at
<https://docs.gtk.org/gtk4/>. Exact Rust transitive versions are locked in
`Cargo.lock`. No external implementation, schema, fixture, prose, or visual
asset was copied.

The completed Settings controls, state rail implementation, exact diff,
desktop entry, user guide, and isolated GUI process test are original work
derived from this repository's S0 roadmap and interaction design. The
active-policy marker format and lifecycle are original work derived from the
provider's saved-versus-active requirement and use the already recorded safe
Rustix `flock` wrappers. No Nobox source, schema, test, fixture, prose, or
design artifact was consulted.
