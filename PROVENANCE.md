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

## T5 input reconsideration

### Maintainer-requested prior-art comparison

On 2026-08-08, after the revision-4 pointer path, broker, guard, device
inspection, and inert bundle renderer had been independently implemented and
tested, the maintainer explicitly requested a read of
`../nobox/docs/agent-seat-tier-complexity.md`. The document was used only as a
high-level comparison checklist for the already-recorded distinction between
an integrated display authority and an external Tier 0 observer. No Nobox
implementation, schema, test, fixture, comment, service definition, or prose
was copied, translated, or adapted; no Agent Seat wire or runtime behavior was
derived from it. Subsequent work continues to use this repository's approved
contract, public standards, and independently observed process behavior.

The T5 threat-model review in `docs/t5-input-reconsideration.md` is original
work derived from this repository's public roadmap and security model. Its
technical inputs are the public XTEST specification already recorded above,
the Linux kernel evdev and uinput documentation at
<https://docs.kernel.org/input/event-codes.html> and
<https://docs.kernel.org/input/uinput.html>, and the systemd-logind D-Bus API
at
<https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.login1.html>.
No Nobox source, schema, test, fixture, comments, or prose was consulted during
its authorship or adapted. The review adds no code or dependency.

The later process-authority inventory, deployment questions,
negative-authority requirements, hostile-test ordering, and governing Tier 0
rule incorporate direct maintainer review of that original document. They add
no external implementation source, schema, fixture, dependency, or product
prose.

The candidate broker deployment contract is original design work from those
requirements. Its standards-derived inputs are the Linux evdev documentation,
udev and libinput device-property documentation, and the systemd service,
resource-control, execution, and logind interfaces linked from
`docs/t5-broker-deployment.md`. The local systemd 258 and udev database were
inspected only as public process and filesystem behavior to check that the
document does not assume unavailable fields. No broker code, service unit,
schema, fixture, dependency, or third-party product prose was imported.

The lock-state integration study is original analysis of the public
systemd-logind contract, the X11 Screen Saver Extension specification, and the
upstream LightDM project and `dm-tool` documentation linked from
`docs/t5-lock-state-study.md`. LightDM is treated only as a future black-box
compatibility candidate; no LightDM or Nobox implementation, schema, test,
fixture, comments, or prose was copied or adapted. The study adds no code,
service definition, schema, fixture, or dependency.

The revision-4 pointer-move gate, broker framing/runtime, inert systemd unit
sources, and hostile tests are original implementation work from the approved
repository threat contract. Standards-derived inputs are Linux evdev event and
`SYN_DROPPED` semantics, XTEST fake-input semantics, Unix peer credentials,
and systemd 253+ `OpenFile=`, socket activation, and execution/resource-control
directives linked from `docs/t5-broker-deployment.md`. The new direct runtime
dependency is `evdev` 0.13.2 (Apache-2.0 OR MIT) from
<https://github.com/emberian/evdev>. No Nobox code, schema, test, fixture,
comments, service definition, or prose was consulted during implementation or
adapted.

The later pointer destination fix is original work from live Openbox process
behavior and the public X Shape and QueryTree contracts. It uses the existing
`x11rb` dependency's Shape feature to bound effective input-region inspection,
then binds Openbox's reparenting frame to the already scoped client. The lower
and covering override-window fixtures were written specifically for this
repository; no external hit-test implementation or fixture was consulted.

The read-only enrollment inspection command is original work based on the
public Linux sysfs device view and the documented `udevadm info` query surface.
It requests only the input-class and seat properties listed in the deployment
contract, validates a fixed absolute `/usr/bin/udevadm` result, and adds no
dependency. No Nobox enumeration code, output, schema, fixture, or prose was
consulted during implementation or adapted.

The inert review-bundle renderer is original work over the repository's own
systemd unit sources and inspection result. Its path, identifier, output-mode,
no-overwrite, exact-descriptor, and unresolved-marker checks add no external
dependency. No third-party enrollment implementation, schema, fixture, unit,
or prose was consulted or adapted.

The root-only install, arm, stop, and purge transactions are original work from
the repository's administrator-action and fail-closed deployment contract.
They use Rustix's safe filesystem interfaces for no-follow exclusive creation,
no-replace rename, exact unlink, and synchronization, plus fixed
`/usr/bin/systemctl` process behavior. Their ownership, mode, file-set,
rollback, timeout, and non-enablement tests use only private temporary fixture
directories. No third-party installer, service controller, schema, fixture, or
prose was consulted or adapted.

The rootless confinement probe is an original single-process hostile fixture
for the repository's own systemd unit contract. It exercises only public
filesystem, process, socket, environment, inherited-descriptor, and transient
user-manager behavior. It adds no product dependency and does not use another
project's fixture, service, test language, or implementation.

The production-identity confinement variant is original work reusing that
repository-owned hostile executable under transient collected system-manager
units. It exercises the public systemd execution, dynamic-user, static-user,
descriptor-passing, namespace, device, syscall, and resource-control behavior
already recorded for the deployment contract. It adds no dependency,
persistent unit, enrollment format, runtime authority, or third-party fixture.

The no-event hotplug fixture and fresh-cycle arm change are original work from
the repository's approved fail-closed lifecycle contract. The fixture uses the
already recorded evdev dependency's safe uinput API to create one bounded
relative-capability device without emitting events. Its installed observation
used only public systemd state, the fixed broker status frame, `/proc` descriptor
links, and udev settling behavior. It adds no dependency, input realization,
physical-device access, persistent unit, or third-party fixture.

The installed-unit-derived confinement gate is original work over the
repository renderer's installed output and its existing hostile executable. It
uses public systemd unit loading, volatile `/run/systemd/system` units, process
result properties, and the already recorded confinement directives. The test
retains those directives and changes only explicitly enumerated fixture
plumbing. It adds no dependency, persistent unit, runtime feature, external
implementation input, or third-party fixture.

The live rootless guard gate is original integration work over the repository's
own renderer, guard, unit contract, and fixed wire frame. It observes only the
current public sysfs, kernel uevent, systemd user-manager, Unix peer-credential,
and logind interfaces. It does not change the session, read an event node, or
use another project's test or implementation.

The exact device-cgroup allow rendering was derived from the repository's first
installed systemd-258 arm attempt and the public `OpenFile=`, `DevicePolicy=`,
`DeviceAllow=`, and `PrivateDevices=` contracts. The observed manager setup
failed closed at `status=202/FDS` until the reviewed nodes were admitted
read-only by the service device filter. No third-party service definition,
installer, test, or implementation was used.

Safe ownership of systemd-passed descriptors uses `sd-listen-fds` 0.2.0
(Apache-2.0 OR MIT) from <https://github.com/kpcyrd/sd-listen-fds>. Its public
API returns owned descriptors without requiring unsafe code in Agent Seat. The
repository's local transient systemd-258 probes independently established the
name-list behavior when a socket-activated descriptor has already been moved
onto a standard stream; the implementation then validates the complete raw
name list and exact remaining order. No third-party broker implementation,
service definition, schema, or test was used.

The fixed eligibility-guard identity and sysusers declaration follow a failed
installed systemd-258 handshake, a successful same-host stable-user control,
the public `sysusers.d` contract, and systemd upstream's documented warning
against dynamic identities for D-Bus services:
<https://github.com/systemd/systemd/issues/9503>. The identity has no input
group, device descriptor, home, shell, capability, or owned runtime state. No
third-party unit, account declaration, broker code, or test was adapted.

The broker instance identifier now uses Rustix's safe `getrandom(2)` wrapper
after the installed private-device profile correctly denied `/dev/urandom`.
This uses an existing dependency and the already-allowed syscall; it grants no
device or filesystem authority.

The exact bundle verifier is original work over that renderer and the fresh
inspection result. Its direct-file, ownership, mode, link-count, size,
identity-stability, exact-name, and byte-comparison checks use Rust standard
filesystem metadata only and add no dependency. It does not incorporate any
Nobox format, behavior, implementation, test, fixture, or prose.

The eligibility guard is original work against the documented login1 D-Bus
interfaces and D-Bus signal matching. It uses `dbus` 0.9 (Apache-2.0 OR MIT)
and the system `libdbus`; this replaces a rejected 50-package pure-Rust D-Bus
candidate with two locked packages. No LightDM, Nobox, locker, or session-guard
implementation, schema, fixture, comments, or prose was consulted during
implementation or adapted.

Its later device-lifecycle monitor is original work against Linux netlink
kobject-uevent documentation and the upstream kernel broadcast implementation.
It uses the existing safe Rustix netlink wrappers, accepts only kernel-sender
messages, bounds bytes and fields, and reduces all input-subsystem lifecycle
metadata to terminal ineligibility. It does not read evdev packets or expose
device metadata. No external broker or hotplug implementation, schema, test,
fixture, comments, or prose was consulted or adapted.

The strict initial input-class manifest and reconciliation are original work
over the already inspected public `/sys/class/input` and canonical sysfs view.
The small tab-delimited format is private deployment metadata, not Agent Seat
wire protocol; it is bounded, revisioned, and implemented with the Rust
standard library and existing Rustix dependency only. No external manifest,
enrollment format, parser, fixture, service definition, comments, or prose was
consulted or adapted.

The broker standard-stream descriptor layout is original integration work
against the public systemd `StandardInput=file:`, `StandardOutput=socket`,
socket-activation, and `OpenFile=` contracts. Local transient user-manager
services were used only to observe documented descriptor placement and Unix
socket connection behavior. The broker duplicates inherited standard streams
through safe Rustix APIs. Named `OpenFile=` descriptors are adopted through the
separately recorded safe dependency and never reopened through procfs; no
external broker implementation or raw-descriptor conversion pattern was copied
or adapted.

The reviewed relevant-device identity record is original work over selected
properties from the already bounded `udevadm info` query. Its distinction
between topology-only and serial-backed evidence follows the public Linux input
model, where `phys` is a physical hierarchy path and `uniq` exists only when a
device provides a unique identifier, plus udev's documented property database
and path-based device identification. The later coverage-equivalence record
uses the Linux input documentation's public `capabilities/` and `properties`
sysfs bitmaps and adds no device access or dependency. No third-party
enrollment record, parser, schema, fixture, or prose was copied or adapted.

## R0 pre-RFC preparation

The R0 pre-RFC draft is original work distilled from this repository's public
wire specification, architecture, security model, hostile-test requirements,
and independently observed reference behavior. Its normative-keyword
convention uses RFC 2119 and RFC 8174 from the RFC Editor. No Nobox or other
Agent Seat implementation code, schema, test, fixture, comment, or prose was
consulted, copied, translated, or adapted. The draft adds no runtime behavior,
dependency, wire revision, or external standards claim.

The serialization-neutral information model, registry projection and custody
policy, standalone X11/EWMH core profile, stable fixture identifiers,
conformance report semantics, schema, and incomplete example are original work
derived from the same repository contracts and public behavior. The report
schema uses the JSON Schema Draft 2020-12 meta-schema published by the JSON
Schema project at <https://json-schema.org/draft/2020-12/schema>; no schema,
example, implementation, or prose was copied from another conformance system.
These documents add no runtime dependency, wire change, certification, or
external standards claim.

The T5 participant contract is original work decomposing this repository's
already approved negative-authority, physical-replacement, and lock-transition
gates into stable fixture identifiers and public evidence requirements. Its VM
and harness interfaces are candidates, not copied launcher, hypervisor,
display-manager, greeter, or authentication implementation material. No Nobox
source, schema, test, fixture, comment, or prose was consulted or adapted.
The dependency-free safe-Rust harness probe is original work implementing
those public-boundary attempts with the Rust standard library and fixed
`systemd-run`/`true` paths. It adds no crate, dependency, installed binary,
credential access, raw input read, or external protocol allocation.

## Provider private-device deployment

The optional provider user unit, startup absence check, user-manager launch
delegation, and hostile two-namespace fixture are original work from this
repository's approved process-authority inventory. They use public systemd
user-service, transient-unit, execution, device, namespace, and resource-
control behavior already cited in `docs/t5-broker-deployment.md`. The local
systemd-258 probe independently established that a confined process can submit
a transient user unit which receives the user's ordinary device namespace. No
Nobox or other project code, unit, schema, test, fixture, comment, or prose was
consulted, copied, translated, or adapted. The change adds no dependency,
evdev descriptor, raw activity field, input operation, root requirement, or
external protocol claim.

The private companion registration and hostile fixture are original work over
the same public systemd contracts. systemd's documented `OpenFile=` behavior
connects a filesystem `AF_UNIX` socket in the service manager and passes the
owned descriptor using the `sd_listen_fds(3)` activation environment. The
companion adopts it through the already locked `sd-listen-fds` 0.2.0 safe API,
which is now a direct dependency of `agent-seat-mcp`; no new package was added
to `Cargo.lock`. Local systemd-258 tests used only transient collected user
units, dummy private sockets, public process/filesystem evidence, and the live
Agent Seat protocol. No Nobox or other product implementation, launcher, unit,
schema, test, fixture, comment, or prose was consulted or adapted.

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

## Tier 0.5 volatile seat gate

The disabled-by-default provider latch, private same-UID operator control
plane, generation-bound session revocation, pointer pre-send recheck, process
tests, and launcher-neutral lifecycle contract are original work from the
maintainer's stated requirement that provider presence and an explicit runtime
seat switch should be necessary for Agent Seat operation. The implementation
uses this repository's existing Unix peer-credential, private runtime-socket,
session, and X11 lifecycle mechanisms and adds no dependency. LightDM remains
an external future black-box lifecycle candidate under the public interfaces
already recorded above. No Nobox or other product source, schema, fixture,
test, comment, service definition, or prose was consulted or adapted.
