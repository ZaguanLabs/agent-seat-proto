# Changelog

All notable changes to this project are documented here.

## Unreleased

### Added

- Split the protocol pre-RFC's portable semantics into an explicit
  serialization-neutral information model and identified the existing
  pathname-Unix-stream, strict-JSON revision 4 contract as one concrete
  binding. This changes no released wire value.
- Added a hand-reviewed machine-readable projection of the revision and atom
  registries plus repository custody, immutability, namespace, and change-
  transaction rules. Released specifications remain authoritative, and the
  projection is neither generated runtime source nor external endorsement.
- Added the complete implementation-independent
  `agent-seat.x11-ewmh-core.v1` experimental profile: authority inventory,
  supported core surface, convergent observation, qualified EWMH management,
  controlled launch, lifecycle/resource requirements, public fixtures, known
  limitations, and prohibited claims. Input remains outside the profile.
- Added the experimental portable `agent-seat.conformance-report/1` format, a
  closed JSON Schema, stable core-profile fixture IDs, and an explicitly
  incomplete example. Reports identify exact subjects, bindings, profiles,
  environments, tested surfaces, digested public evidence, negative-authority
  enforcement, limitations, and pass/fail/incomplete conclusions without
  claiming certification.
- Added a participant-facing T5 evidence contract with stable external-harness,
  virtual-device replacement, and full-system lock fixture IDs. It specifies
  production-launcher escape checks, an immutable VM manifest, a greeter input
  barrier, one monotonic cross-boundary ordering, and the exact review bundle;
  it explicitly leaves all three gates incomplete until real participants run
  them.
- Added `agent-seat-mcp` 0.1.3 and `agent-seat-x11` 0.1.16 private companion
  deployment. The provider user unit uses one fixed mode-0600 socket. The
  emitted MCP registration has systemd preconnect only that socket with
  `OpenFile=`, pass one strictly validated named descriptor, and start the
  translator with private network, device, runtime, temporary, home, process,
  environment, execution, syscall, and resource boundaries. An emitted-profile
  hostile test and a delayed live installed `seat_status` call pass. The input
  profile gives one authenticated and granted session an interruptible idle
  slot while keeping initial handshakes, partial frames, and other sessions
  deadline-bound. Live deployment also replaced the broker-style permanent
  one-attempt provider start limit with a bounded three-attempt/30-second limit
  so a later explicit restart does not require clearing historical failures.
- Added `agent-seat-x11` 0.1.15 provider-side input confinement. An explicit
  `input.provider_private_devices` switch fails startup unless `/dev/input` and
  `/dev/uinput` are absent under the new non-enableable systemd user unit.
  Controlled launches use fixed, shell-free `systemd-run` delegation with
  unique random unit names and retain the existing 64-child bound, so admitted
  applications keep the user's ordinary device namespace without widening the
  provider. Static unit/configuration tests and an explicit live rootless
  hostile gate cover both sides of that boundary.
- Added the initial R0 pre-RFC draft. It separates the display-neutral core,
  evidence-based backend conformance profiles, assurance vocabulary, revision
  and extension governance, security considerations, and non-normative
  reference mapping without claiming external standards status or changing
  wire revision 4.

- Added the experimental revision-4 `pointer.move` vertical slice: an
  authority-free MCP tool, strict provider grant/config boundary, live
  target-relative hit testing, one-action XTEST realization, and qualified
  queued/interrupted results with isolated Openbox coverage.
- Added `agent-seat-activity-broker` 0.1.0 with fixed status and eligibility
  frames, verified Unix peers, exact inherited evdev descriptors, initial
  held-key/queued-event refusal, activity and `SYN_DROPPED` latching, and no
  X11, launch, MCP, policy, or injection authority.
- Added inert enrollment-rendered systemd service/socket sources. They require
  a persistent single broker, explicit arm cycle, read-only inherited
  eligibility/device descriptors, strict device and socket confinement, and
  pass local `systemd-analyze verify`; no enrollment or enablement is shipped.
- Added `agent-seat-activity-enroll inspect`, an unprivileged, read-only
  preflight that reports the exact current `seat0` evdev candidate from bounded
  sysfs and selected udev metadata. It never opens an input device, writes an
  enrollment, renders or installs units, starts a service, or changes policy.
- Added unprivileged `agent-seat-activity-enroll render`. It writes the exact
  inspected device set as inert, private systemd unit sources and a prominent
  review record in a new directory, refuses overwrite, and never installs,
  enables, or starts anything. Normal deployment needs no desktop-user or
  broker membership in the broad input-device group.
- Added unprivileged `agent-seat-activity-enroll verify`. It regenerates the
  current candidate and rejects a review bundle unless its bounded file set,
  direct-file metadata, private ownership/modes, UID, session, device set, and
  bytes still match exactly; it changes no file or service.
- Added the unprivileged `agent-seat-eligibility-guard`. It subscribes to
  logind and the kernel kobject-uevent group before checking the exact session,
  seat, UID, type, class, activity, remote, lock-hint, sleep, and shutdown
  state; emits only the fixed eligibility frame; and permanently fails closed
  on input-subsystem lifecycle changes, malformed or lost device evidence,
  state signals, login1 replacement, D-Bus loss, or a non-enrolled peer. It
  receives no input-event packets or device descriptors.
- Added a strict, human-readable initial input-class manifest covering every
  bounded `/sys/class/input/event*` mapping, not only the devices selected for
  broker descriptors. PID 1 passes the installed root-owned mode-0600 manifest
  read-only; after subscribing to kernel uevents, the guard reconciles the live
  mapping and refuses eligibility on mismatch or concurrent change.
- Added a separate bounded `enrolled-device-set.v1` review record for relevant
  devices. It binds canonical paths and classes to selected udev topology and
  hardware identity, records a short serial only when present, strictly
  round-trips its private format, and labels serial-less evidence as
  topology-only instead of claiming indistinguishable replacement detection.
- Added explicit root-only `install`, `arm`, `stop`, and `purge` enrollment
  transactions. Installation freshly verifies the current seat and the
  UID-owned review, requires packaged root-controlled executables, publishes
  only new private root-owned files with rollback, and never enables or starts
  a service. Arming freshly verifies the installed bytes before one bounded
  service start; stopping and purging name only the exact UID-bound units and
  files. Fixture tests cover refusal, unexpected files, and partial-publication
  rollback without modifying the host.
- Added an explicit rootless systemd confinement gate for both runtime
  authorities. A single-process hostile fixture proves exact inherited
  read-only evidence remains available while home/runtime/process metadata,
  input paths, host sockets, new network sockets, desktop environment, and
  direct execution are denied; the guard alone retains its exact system-bus
  socket. The test exposed and fixed a startup failure caused by `TasksMax=1`:
  the bounded value is now 2 so systemd can construct the namespace before the
  one-task process begins.
- Added an explicit production-identity variant of the hostile confinement
  gate. It uses only transient collected system-manager units, exercises the
  real dynamic broker and locked static guard identities, preserves their exact
  inherited channels, and denies X11, input-node, home, process, host-socket,
  network-socket, and direct-execution authority. Ordinary tests ignore it and
  its explicit run requires passwordless sudo.
- Added an explicit no-event uinput hotplug fixture. An installed systemd-258
  run proved real kernel lifecycle delivery, terminal eligibility and epoch
  transition, closure of every event descriptor, and a fresh-instance rearm
  without changing a physical device or emitting pointer movement.
- Added an exact-installed-unit confinement gate. It reads the root-owned
  rendered broker and guard service bytes, retains every confinement directive,
  substitutes only bounded hostile-test plumbing, runs both production identity
  models through uniquely named volatile system units, and removes those units
  without disturbing the ready installed broker.
- Added an explicit live rootless guard gate. Under the hardened transient
  profile it consumes a freshly rendered complete input-class manifest through
  fd 3, opens the real kernel uevent subscription, reaches the single
  re-exposed system-bus socket, authenticates its peer, and evaluates the
  current local X11 session. The eleven session/system predicates are also
  reduced to a bounded value object and tested one failure at a time.
- Installed-unit review found that systemd accepted but ignored a misplaced
  `CollectMode=` service directive. Version 0.1.11 moves it to `[Unit]` and
  strengthens the unit gate to reject any systemd diagnostic, not only a
  nonzero verification exit.
- The first production arm then proved that `DevicePolicy=strict` also applies
  while systemd opens `OpenFile=` descriptors for the service. Version 0.1.12
  renders one exact read-only `DeviceAllow=` per reviewed event node so the
  manager can open it. The broker still lacks input-group membership,
  host devices remain hidden by `PrivateDevices=`, and direct runtime opens
  therefore remain denied.
- The next installed arm exposed a second fail-closed integration error:
  reopening `/proc/self/fd/*` repeated DAC checks under the dynamic identity.
  Version 0.1.13 safely adopts systemd's named owned descriptors directly,
  validates the complete bounded name/order set, removes numeric descriptor
  arguments, and applies close-on-exec without reopening privileged files.
  Arming now samples unit activity both immediately and after a bounded 750 ms
  stabilization interval.
- The installed 0.1.13 attempt exposed two more production-profile facts.
  Multi-unit `systemctl is-active` succeeds when any named unit is active, so
  version 0.1.14 checks each service and socket independently. Also, the system
  bus refused a dynamic guard identity even though an ordinary stable identity
  passed the same hardened profile. Only the no-input guard now uses a locked,
  group-free `agent-seat-guard` user declared through `sysusers.d`; the evdev
  broker remains dynamic. The provider pins UID 0 because PID 1 owns the
  socket-activated listener used for its `SO_PEERCRED` check. The installed
  sandbox also made `/dev/urandom` unavailable as intended; instance IDs now
  use the already-allowed `getrandom(2)` syscall instead of opening a device.

- Completed `agent-seat-settings` 0.1.1 with native GTK 4 controls for explicit
  activation, every capability and dependency, observation scope and titles,
  the searchable provider-identical application catalog, bounded limits, an
  exact review diff, validated save/reload/discard/recovery flows, unsaved-work
  confirmations, and a persistent saved/draft/active state rail.
- Added a source XDG desktop entry and a complete graphical and terminal
  Settings guide.
- Added `agent-seat-x11` 0.1.10 best-effort active-policy evidence. A running
  provider holds a private locked marker containing its exact startup policy;
  Settings can report matching, restart-required, multiple, unavailable, and
  unreported states without X11 or a provider-socket connection.
- `agent-seat-x11` 0.1.14 now hit-tests the topmost mapped root child using
  bounded X Shape input regions, then proves that child is the scoped client's
  own Openbox reparenting frame. Deterministic tests distinguish a harmless
  lower full-screen override window from a covering top override. The live
  profile correctly refused input while XScreenSaver reported the seat locked,
  exposing `LockedHint=no` as incomplete evidence rather than weakening the
  coverage check.
- Added `agent-seat-settings` 0.1.0 with a display-independent policy session
  model, exact check and print commands, recoverable `.previous` exchange,
  strict CLI parsing, default-policy creation for the GTK entry path, and an
  initial native GTK 4 shell.
- Planned an S0 `agent-seat-settings` application for safe, human-facing
  provider policy editing, shared validation, reviewable changes, and explicit
  active-versus-saved state without adding another runtime authority.
- Selected GTK 4 for the standalone Settings executable and documented its
  security-centered interaction model, native accessibility requirements,
  policy-state rail, visual system, and dependency boundary.

### Changed

- Corrected the provider guide's current source version and expanded the
  compatibility matrix to separate installed runtime evidence, unconstrained
  external-harness status, and pre-RFC publication readiness.
- Patch-bumped `agent-seat-activity-broker` to 0.1.17 for exact bundle
  verification, the documented unprivileged pre-installation workflow,
  fail-closed runtime input-device lifecycle monitoring, and initial input
  class-set reconciliation. Manifest ownership is bound to the already
  authenticated service-manager peer: the rendered system profile fixes that
  peer to UID 0, while an ordinary user can exercise the same guard path in an
  explicitly non-production local test. Version 0.1.6 added the strict
  reviewed-device identity record and same-path changed-identity refusal;
  version 0.1.7 binds every relevant device to the complete kernel capability
  bitmap, rejects changed coverage evidence, and resamples relevant udev and
  capability evidence before inspection succeeds. Version 0.1.8 adds the
  explicit privileged file/service transactions and passes the root-owned
  device record to the runtime for descriptor-to-enrollment capability checks.
  Version 0.1.9 adds executable and `/run` isolation plus the hostile rootless
  systemd confinement proof. Version 0.1.10 adds live hardened guard startup
  against real logind, sysfs, and kernel lifecycle evidence. Version 0.1.14
  reaches `Ready` under the installed production identities and confines both
  processes with zero capabilities, no supplementary groups, seccomp, and
  private device views. Version 0.1.15 adds the explicit system-manager hostile
  test under the production identity models. Version 0.1.16 adds the installed
  synthetic-hotplug fixture and makes `arm` replace any prior terminal broker
  cycle after fresh verification, guaranteeing a new instance rather than a
  no-op start of an already active service. Version 0.1.17 adds the
  installed-unit-derived hostile confinement gate.
- Rewired the broker's inherited sockets without raw-descriptor conversion:
  PID 1 connects eligibility to standard input, places the socket-activated
  provider listener on standard output, passes the exact enrolled-device record
  at fd 3, and starts event descriptors at fd 4. The initial eligibility frame
  now has a two-second bounded wait, and timeout, partial data, hangup, or
  malformed data fails closed.
- Planned an R0 standards track that separates the display-neutral normative
  contract and assurance tiers from backend conformance profiles and
  non-normative reference implementations.

- `agent-seat-settings` 0.1.3 preserves the bounded allow-list when switching
  among application admission modes and across saves. The provider consults
  that list only in allow-list mode, so inactive selections grant nothing but
  return when the person switches back. `agent-seat-x11` is patch-bumped to
  0.1.12 for the retained-list policy semantics. Policy transactions now also
  unlock explicitly on drop so a concurrent process spawn cannot retain a
  transient inherited lock until `exec`.
- `agent-seat-settings` 0.1.2 now switches application admission modes
  reliably when an allow-list already exists. Version 0.1.3 supersedes its
  initial list-normalization behavior with lossless retained selections.
  `agent-seat-x11` was patch-bumped to 0.1.11 for that draft API.
- `agent-seat-x11` 0.1.9 preserves existing TOML comments and layout across
  typed Settings edits while still validating the exact rendered source. It
  also exposes bounded default-policy creation and recovery-path discovery so
  the standalone editor need not duplicate XDG or recovery conventions.
- `agent-seat-x11` 0.1.8 exposes the bounded S0 settings model and application
  catalog: validated grouped policy edits, exact-parser TOML rendering,
  published editor bounds, and the provider's own launchable XDG entries
  without opening X11 or consulting the active provider. Process tests cover
  typed edit-to-atomic-commit flow and isolated catalog discovery.
- `agent-seat-x11` 0.1.7 exposes the provider-owned S0 policy transaction core:
  validated snapshots, stale-edit and concurrent-writer refusal, atomic Linux
  exchange, private mode-0600 replacement and recovery files, rollback on
  pre-commit failure, directory synchronization, and hostile-target process
  coverage without adding a configuration schema or dependency.
- `agent-seat-x11` 0.1.6 now treats policy validity and provider activation as
  separate states: `--check-config` successfully validates complete disabled
  policies and reports their activation state, while an ordinary provider
  start continues to require explicit enablement. This is the first S0
  settings-application foundation.
- `agent-seat-x11` 0.1.5 now creates a private, disabled, extensively commented
  policy template on first use of the default configuration path, reports the
  exact next steps without touching X11, and provides complete first-run help
  and setup documentation. Explicit configuration paths remain non-creating.

## 0.1.0 - 2026-08-08

### Added

- Apache-2.0 project skeleton, contribution and provenance policy, security and
  release process, initial architecture, and CI source gate.
- Reserved protocol, MCP companion, and standalone X11 provider crate
  boundaries without claiming a wire revision.
- Completed E0 with ZaguanLabs ownership, public source, two-factor
  enforcement, branch protection, private security reporting, secret scanning,
  and a passing pinned-toolchain source gate.
- Defined independently authored Agent Seat wire revision 3 with bounded
  framing and values, strict messages, typed identities, exact capabilities,
  feature and assurance reporting, stable failures, and selection-bound X11
  advertisement parsing.
- Implemented the authority-free `agent-seat-mcp` 0.1.1 companion with MCP
  `2025-11-25` lifecycle handling, twelve closed-schema core tools, lazy
  provider connection, exact socket-source precedence, and actionable
  structured failures.
- Added malformed, oversized, revision-refusal, process-boundary, lazy-startup,
  and isolated-X11 discovery coverage for the E1 exit gate.
- Implemented the `agent-seat-x11` 0.1.1 T0 provider foundation with explicit
  strict configuration, same-user kernel credential grants, bounded sessions,
  private stale-socket recovery, atomic per-screen selection ownership, clean
  withdrawal, and typed status/refusal/unsupported results.
- Added bare-Xvfb and Openbox process tests for lifecycle, duplicate refusal,
  stale recovery, peer denial, slow-client eviction, selection loss, clean
  shutdown, and window-manager survival after provider failure.
- Implemented `agent-seat-x11` 0.1.2 T1 observation with strict deny-by-default
  client scope, independently gated titles, bounded EWMH snapshots,
  per-session opaque identities, generations, filtered monotonic diffs, and
  typed resynchronization.
- Added Openbox process coverage for client creation, title changes,
  minimize/remap, workspace movement, destruction, scope departure/return,
  title redaction, event filters, and stale cursors.
- Implemented `agent-seat-x11` 0.1.3 T2 management with a server-grabbed final
  freshness check, exact EWMH/target support checks, polite close, workspace
  switch/send, supported state changes, and decoration-aware frame geometry.
- Added bounded post-send `observed`, `timed_out`, and `target_gone` results and
  Openbox coverage for supported operations, refusal, stale and out-of-scope
  targets, invalid workspaces, unsupported state, ignored requests, responsive
  close, and target disappearance.
- Implemented `agent-seat-x11` 0.1.4 T3 launch with strict bounded XDG catalog
  discovery, deny/allow-list/allow-installed policy, a separate user-entry
  gate, shell-free desktop `Exec` parsing, bounded child supervision, and exact
  startup-ID correlation that remains optional in the reply.
- Added Openbox process coverage for an allowed system entry, an unlisted
  entry, a default-denied user entry, hostile metacharacters, exact and absent
  correlation, launch failure, and post-failure window-manager responsiveness.
- Completed C0 with the exact independent and cross-product compatibility
  matrix, explicit T4--T6 stop decisions, a verified source-archive contract,
  SHA-256 asset, annotated tag workflow, and ZaguanLabs GitHub release notes.
