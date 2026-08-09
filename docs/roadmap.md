# Product roadmap

Status: E0, E1, T0--T3, the T4--T6 first-release decisions, C0, and S0 are
complete. R0 has an initial repository pre-RFC draft. The Tier 0 core and C0
shipped in product release v0.1.0; S0 is complete on `main` in
`agent-seat-settings` 0.1.3 and `agent-seat-x11` 0.1.12. T5R now has an approved
experimental pointer-move slice; deployment remains gated. E0 evidence is
recorded in
[`e0-verification.md`](e0-verification.md).

## T0.5 — volatile provider seat gate

Status: implemented experimentally in `agent-seat-x11` 0.1.17, 2026-08-09.

The provider now starts with a disabled runtime seat independently of saved
policy. A separate local operator command can inspect, enable, or disable the
current provider instance; the command is not an MCP operation and state is
never persisted. Sessions are generation-bound, so disable revokes existing
sessions and re-enable requires a new handshake. Pointer movement rechecks the
same generation under its X11 server grab.

This is the launcher-neutral [Tier 0.5 seat-gate contract](tier-0.5-seat-gate.md):
provider or X11 death denies everything and every new provider begins disabled.
It is a useful operator consent/kill switch under the confined-companion
profile, not same-UID isolation and not a solution to the remaining LightDM
credential-surface ordering gate.

The next UI milestone may add explicit runtime status, Enable, and Disable
controls to Settings. It must not conflate saved policy activation with the
volatile latch or enable automatically. LightDM is the first logout/relogin
compatibility case, while other launchers remain independent participants in
the same lifecycle contract.

The [Tier 0.5 verification record](t0.5-verification.md) now contains an
installed systemd-service deny/enable/deny round trip, including MCP refusal
while disabled, a successful fresh session while enabled, generation advance
on disable, and selection-bound discovery across equivalent `DISPLAY`
spellings. Its pre-logout LightDM evidence is recorded; the deliberate
logout/relogin observation remains pending.

## Goals

- Define one strict, bounded Agent Seat wire contract.
- Ship an authority-free generic MCP companion.
- Ship a standalone Tier 0 provider that works beside unmodified EWMH window
  managers, beginning with released Openbox.
- Provide a human-facing settings application for safely reviewing, editing,
  and validating provider policy without moving runtime authority out of the
  provider.
- Make unsupported, refused, stale, sent-but-unobserved, timed-out, and failed
  outcomes distinct at public boundaries.
- Keep failures outside the window manager and keep source independently
  authored under Apache-2.0 with DCO sign-off.
- Mature the proven display-neutral contract into an implementation-independent
  RFC that other desktop maintainers can implement and extend deliberately.

## Non-goals

- Importing or relicensing Nobox implementation material.
- Treating same-user X11 as a secure isolation boundary.
- Giving the MCP companion policy authority.
- Giving a settings application an MCP surface, X11 control, provider socket
  ownership, or independent runtime grant authority.
- Emulating unsupported EWMH operations with shell commands or synthetic
  pointer/keyboard input.
- Shipping capture, input, accessibility, or persistent coordinate memory as
  part of the core profile.
- Making Linux evdev, systemd, X11, Openbox, MCP, or this reference
  implementation mandatory parts of the eventual protocol standard.

## R0 — protocol RFC preparation

Status: repository draft in progress, 2026-08-09. The
[pre-RFC draft](r0-protocol-rfc.md) separates the portable core, backend
conformance profiles, and non-normative reference mapping. The portable core
is now factored into a [serialization-neutral information model](information-model.md)
and the existing concrete local Unix-stream/strict-JSON binding. Existing
revisions remain the repository's normative wire contract. A hand-reviewed
[machine-readable registry projection](registry-v1.json) and repository
[custody policy](registries.md) now record current allocations without making
generated data or the projection itself runtime authority. No external
standards status is currently claimed.

The first complete implementation-independent backend profile is
[`agent-seat.x11-ewmh-core.v1`](profiles/x11-ewmh-core-v1.md). It defines the
supported revision/capability subset, authority inventory, convergent
observation, management send boundary and outcomes, controlled launch,
lifecycle and resource behavior, required public fixtures, and prohibited
claims. It remains experimental pending genuinely independent evidence.

The experimental
[`agent-seat.conformance-report/1`](conformance-report.md) format now provides
a closed JSON Schema for portable subject/binding/profile identity, released
environment, stable fixture results, digested public evidence,
negative-authority enforcement, limitations, and pass/fail/incomplete
conclusions. Its checked-in example is explicitly incomplete and is not
evidence.

Prepare an implementation-independent RFC from behavior that has survived the
reference implementation and hostile tests. The RFC must have three clearly
separated layers:

- a display-server-neutral normative core covering identities, grants, scope,
  freshness, bounded framing, outcomes, interruption, and assurance claims;
- backend conformance profiles describing the evidence an integrated display
  authority, standalone X11 provider, or future backend must supply; and
- non-normative reference implementations, deployment recipes, and hostile
  conformance fixtures.

Integrated window managers and compositors may satisfy ordering and
human-priority requirements inside their authoritative event loop. They must
not be required to reproduce the standalone X11 broker architecture. Likewise,
a Tier 0 backend cannot claim the assurance of an integrated backend merely by
matching its tool surface. Each profile advertises only guarantees it can
prove, with typed unsupported or qualified outcomes for the rest.

R0 also defines revision allocation, extension ownership, capability
negotiation, security considerations, conformance terminology, and the process
for independent implementations to report compatible subsets. Publication as
an external RFC or standard remains a later maintainer/community decision. The
draft records the remaining work for external implementation and desktop-
maintainer review, external registry governance, and publication.

## E0 — bootstrap

Status: complete, 2026-08-08.

Create the canonical ZaguanLabs repository, project policies, crate boundaries,
specification outline, source gate, and administration controls. No wire
revision is claimed.

End result: the skeleton builds from source, provenance and inbound licensing
are auditable, and all metadata names the canonical upstream.

## E1 — protocol and companion

Status: complete, 2026-08-08. `agent-seat-proto` 0.1.1 implements wire
revision 3 and `agent-seat-mcp` 0.1.1 implements the static MCP boundary and
lazy discovery. The provider remains deliberately absent until T0.

Specify and implement bounded framing, strict messages, identities,
capabilities, errors, feature reporting, advertisement parsing, and revision
handling. Implement static MCP initialization/tool listing and lazy discovery
with explicit socket, environment, then selection-bound X11 precedence.

End result: malformed/oversized process tests, strict round trips, revision
refusal, desktop-free MCP startup, and isolated-X11 discovery all pass.

## T0 — provider foundation

Status: complete, 2026-08-08. `agent-seat-x11` 0.1.1 provides strict explicit
configuration, same-user kernel credential grants, bounded sessions, private
socket recovery, atomic selection ownership, clean withdrawal, and isolated
Openbox/no-WM lifecycle coverage.

Implement explicit enablement, strict configuration, local peer credentials,
deny-by-default grants, bounded sessions, the private socket, and atomic
per-screen provider ownership.

End result: standalone lifecycle, duplicate refusal, stale recovery, peer
denial, slow-client eviction, and crash isolation pass beside Openbox.

## T1 — observation

Status: complete, 2026-08-08. `agent-seat-x11` 0.1.2 advertises
`ewmh_observation` and implements bounded scoped snapshots, separately gated
titles, opaque session handles, descriptor generations, filtered monotonic
diffs, and typed resynchronization beside Openbox.

Implement bounded EWMH snapshots, opaque session handles, provider-local
freshness, filtered titles/scopes, monotonic diffs, and resynchronization.

End result: independently observed Openbox state converges with snapshots and
diffs without direct hidden-client disclosure.

## T2 — management

Status: complete, 2026-08-08. `agent-seat-x11` 0.1.3 advertises
`ewmh_management` and implements freshness-checked activation, polite close,
workspace switch/send, supported state changes, and decoration-aware frame
geometry with bounded post-send observation.

Implement only advertised activation, polite close, workspace, state, and
geometry requests. Recheck scope and freshness before send and observe the
terminal state afterward.

End result: every supported operation and every stale, refused, unsupported,
ignored, disappeared, timed-out, and failed branch is externally tested.

## T3 — controlled launch

Status: complete, 2026-08-08. `agent-seat-x11` 0.1.4 advertises
`desktop_launch` and implements bounded preference-ordered XDG discovery,
deny/allow-list/allow-installed policy, separately gated user entries,
shell-free `Exec` realization, bounded child supervision, launch tokens, and
exact startup-ID correlation without guessing.

Implement bounded XDG application discovery and shell-free desktop `Exec`
parsing with deny, allow-listed, allow-installed, deny-list, and separate
user-entry policy.

End result: allowed launch, every refusal mode, hostile metadata, correlation
limits, and launch failure pass without affecting Openbox. This completes the
Tier 0 core.

## Optional profiles

Status: first-release decisions complete, 2026-08-08. The evidence and stop
conditions are recorded in
[`optional-profiles.md`](optional-profiles.md). T4 output/core-window capture,
T5 input, and T6 semantics remain unsupported. A narrowly target-owned
Composite `obscured_capture` is deferred to a new wire revision and does not
delay C0.

- T4 may add capture only for modes that can reapply visibility and scope at
  capture time without returning unrelated pixels.
- T5 may add best-effort client-relative XTEST input only where human activity
  suppression and a local emergency stop can meet their stated contract.
- T6 may add bounded semantics only after fresh correlation and hidden-scope
  research succeeds.

Each profile keeps a typed unsupported result when its stop condition holds.

## C0 — compatibility and release

Status: complete in v0.1.0, 2026-08-08. The exact matrix, forced
revision-2 boundary probes, source gate, archive contract, and release evidence
are recorded in [`c0-verification.md`](c0-verification.md). The annotated
`v0.1.0` tag drives its verified ZaguanLabs GitHub source release.

Test released companions and providers through public boundaries, publish the
exact version/revision/backend/WM matrix, assemble the source release and
checksums, and tag the first supported release.

End result: users can distinguish tested compatibility, partial support,
incompatibility, and untested combinations without relying on source sharing.

## S0 — settings application

Status: complete in `agent-seat-settings` 0.1.3 and `agent-seat-x11` 0.1.12,
2026-08-08. The provider library owns exact validation, a typed bounded draft,
comment-preserving rendering, provider-identical XDG catalog discovery,
race-aware atomic replacement, private recovery, and lock-held active-policy
evidence. The GTK 4 application implements every bounded control and the
saved/draft/active state rail described in
[settings-design.md](settings-design.md); its display-independent commands
retain validation and recovery when no graphical session is available. The
source desktop entry and complete user workflow are documented in
[settings.md](settings.md).

Add a standalone `agent-seat-settings` application that makes the strict
provider policy approachable without weakening it. The application should
discover the same effective configuration path as `agent-seat-x11`, explain
the security effect and dependencies of every capability, and support the
following bounded tasks:

- enable or disable the provider policy explicitly;
- configure observation scope, title access, and resource limits;
- grant or revoke observation, management, and launch capabilities while
  showing required capability dependencies;
- browse valid XDG desktop entries and manage launch allow/deny lists by their
  canonical desktop IDs;
- show a reviewable before/after policy diff and validation errors before any
  write; and
- distinguish saved configuration from the policy active in a running
  provider, with an explicit restart instruction when required.

The settings application is a policy editor, not a second authority. It must
not expose MCP tools, connect to X11, listen on the provider socket, grant a
live session, silently enable capabilities, or start/stop the provider without
a separate user-confirmed design. It must reuse the provider's exact parser
and validation rules rather than maintaining a schema that can drift.

Writes must be recoverable and race-aware: reject symlinks and non-regular
targets, preserve ownership, replace only after successful validation, retain
mode 0600, and never turn a failed edit into a partially written policy. The
initial implementation should remain usable without a running X server so
configuration recovery is possible from a terminal session.

End result: a first-time user can safely configure observation and an
application launch allow-list, understand what each permission enables,
validate the exact saved policy, and see whether a provider restart is needed.
Filesystem and process tests must prove refusal of unsafe targets, invalid
policy, concurrent replacement, and write failure before the milestone is
complete.

Exit evidence: transaction process tests cover unsafe targets, invalid
candidates, stale and concurrent writers, and pre-commit infrastructure
failure. Model tests cover typed edit, exact rendering, save, restore, and
display-independent commands. Isolated Xvfb/Openbox tests prove the GTK first
run maps without touching the person's desktop, creates only a disabled mode
0600 policy, and does not fabricate provider state. Provider lifecycle tests
distinguish matching, changed, and crash-stale active-policy evidence.

## T5R — input reconsideration

Status: broker authority and a narrow experimental implementation were approved
on 2026-08-08. Revision 4, one pointer-move tool, the broker protocol/runtime,
inert confinement templates, and unprivileged exact-set inspection, inert
rendering, and byte-exact current-set verification are implemented. An
unprivileged eligibility guard now subscribes to logind and bounded kernel
input-device lifecycle notifications before its initial decision. It also
requires a strict peer-owned manifest of every input-event class mapping and
reconciles the live mapping after subscription; the production profile pins
that peer to root. Local render/verify exercised 24 complete mappings and 13
relevant broker candidates. The same host produced and byte-verified the new
13-entry relevant-device identity and complete-capability record: one entry was
serial-backed and 12 were topology-only for hardware identity but coverage-
bound by their kernel capability bitmaps. An ordinary-user live guard test
completed the current manifest handshake. Transient user-manager probes
confirmed eligibility on standard input, the provider listener on standard
output, the installed device record at fd 3, and events at fd 4 onward.
Explicit new-only install, freshly verified arm, exact stop, scoped purge, and
partial-publication rollback are fixture-tested. Rootless hostile systemd
probes now deny unintended authority for both runtime profiles while preserving
their exact inherited channels; they also caught and fixed an over-tight task
bound that prevented namespace setup. The hardened rootless guard also passes
a live current-manifest, kernel-uevent, and logind handshake, while every
session/system eligibility predicate has a direct negative case. Installed
production-identity startup now reaches broker `Ready` with bounded authority
and a live locked-seat pointer refusal. A same-host process inspection confirms
the intended identities, one-task bound, zero capabilities, no supplementary
groups, `NoNewPrivileges`, seccomp, and private device views. A repeatable
hostile test also passes under the system manager with the production identity
models. An installed no-event synthetic-device transition proves that a real
kernel input hotplug latches eligibility off, advances the broker epoch, closes
every event descriptor, and requires a newly identified broker cycle. Hostile
probes derived from the exact root-owned installed service bytes pass under the
production broker and guard identities without changing the live services. The
provider now also has a non-enableable private-device user unit and an enforced
configuration switch. A live rootless hostile gate proves that the provider
loses `/dev/input` and `/dev/uinput` even when its UID can open uinput, while a
controlled launch delegated by that confined provider retains the UID's normal
device namespace. The emitted companion profile now also uses a single
service-manager-connected provider descriptor inside private network, device,
filesystem, process, and execution boundaries. Its hostile test passes for a
UID that can otherwise open uinput, and an installed worker reached live
`seat_status`. The physical replacement matrix and LightDM lock-transition
contract are not yet proven. The external harness boundary, click, and keyboard
input remain gated and unsupported.

The remaining evidence now has a participant-facing integration contract with
stable harness, physical-replacement, and full-system lock fixture IDs. It
defines the production-launcher escape checks, immutable VM manifest,
hypervisor-controlled device topology, greeter input barrier, monotonic
cross-boundary ordering, and conformance-report bundle required for review.
No local VM or participating harness result exists yet, so the gates remain
open.

Re-evaluate T5 using a separately trusted Linux input source rather than
weakening the ordinary-X11 stop decision. The review in
[`t5-input-reconsideration.md`](t5-input-reconsideration.md) rejects XTEST-only
activity inference, broad provider access to evdev, forced logind controller
takeover, and unnecessary uinput injection. It records one candidate: a
separately installed system broker that reduces raw physical-seat input to a
fail-closed activity epoch while the provider retains bounded XTEST
realization.

The candidate creates a new privileged authority with keylogging-grade access
inside its process. It therefore requires explicit maintainer approval of the
authority, deployment, one-action race bound, target-validation contract, and
adversarial exit tests before revision 4 or implementation work begins. A
formal process-authority inventory now records what the harness, companion,
provider, and candidate broker may and may not do. Negative-authority tests
must prove deployed confinement, rather than inferring it from source layout
or current device permissions.

If those designs are approved, the first code milestone is deterministic
hostile and negative-authority fixtures. Input realization follows only after
the fixtures establish loss, interruption, target-change, partial-action
cleanup, and one-action race contracts. An untestable race stops T5 instead of
weakening its tests or public promise.

The candidate broker deployment contract in
[`t5-broker-deployment.md`](t5-broker-deployment.md) now specifies explicit
administrator enrollment, exact service-manager-passed read-only evdev
descriptors, an unprivileged confined broker, fail-closed lifecycle, minimal
IPC, and upgrade/removal behavior. Its overall gate remains failed: generic
Openbox supplies no independently trusted lock-state source, and logind's
desktop-provided lock hint is insufficient. The approved experimental source
work does not authorize installation or a supported deployment.

The lock follow-up in
[`t5-lock-state-study.md`](t5-lock-state-study.md) rejects in-session lock
hints, saver state, windows, and same-user services. It records one candidate
for isolated full-system testing: a supported display manager must move the
seat to a distinct greeter or lock-screen session and make the enrolled user
session inactive before credentials can be entered. LightDM is the first test
candidate, not a supported profile. Unlock never rearms the broker, and the
lock gate remains failed pending deterministic evidence and explicit approval.

End result for this review: T5 now has concrete pass/fail gates and a smallest
candidate architecture. Until those gates are approved and passed, the public
behavior remains the same typed absence described by the first-release T5
decision.
