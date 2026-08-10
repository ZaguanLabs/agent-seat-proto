# Compatibility matrix

C0 verifies the complete revision-3 Tier 0 core beside bare Xvfb and released
Openbox and forces both directions of the separate Nobox revision-2 boundary.
Detailed release-candidate evidence is in
[`c0-verification.md`](verification/c0-verification.md).

| Protocol crate | Companion | Provider | Backend/WM | Status |
| --- | --- | --- | --- | --- |
| 0.1.1 / revision 3 | 0.1.1 | 0.1.1 / `x11_ewmh`, `tier0` | Linux, Openbox 3.6.1 / Xvfb | T0 foundation verified; core features absent |
| 0.1.1 / revision 3 | 0.1.1 | 0.1.2 / `x11_ewmh`, `tier0`, `ewmh_observation` | Linux, Openbox 3.6.1 / Xvfb | T1 scoped snapshots, filtered diffs, title redaction, and resync verified |
| 0.1.1 / revision 3 | 0.1.1 | 0.1.3 / `x11_ewmh`, `tier0`, observation + management | Linux, Openbox 3.6.1 / Xvfb | T2 activation, polite close, workspace, state, geometry, and terminal outcomes verified |
| 0.1.1 / revision 3 | 0.1.1 | 0.1.4 / `x11_ewmh`, `tier0`, observation + management + launch | Linux, Openbox 3.6.1 / Xvfb | Compatible Tier 0 core; policy/refusal, shell-free launch, exact/absent correlation, failure isolation verified |

## Unreleased experimental source

| Protocol crate | Companion | Provider/profile | Test environment | Status |
| --- | --- | --- | --- | --- |
| 0.1.2 / revision 4 | 0.1.3 | `agent-seat-x11` 0.1.20 + activity broker 0.1.17 / pointer move; Settings 0.1.5 | Linux, Openbox 3.6.1 / Xvfb plus installed Mageia LightDM 1.32.0, GTK greeter 2.0.9, Xorg 21.1.23, systemd 258.7, and Linux 6.18.35; rendered units, unprivileged device inspection, hardened live current-manifest/logind/netlink guard handshake, 24-entry class-map plus 13-entry device-identity/capability render/verify, isolated root-layout transaction fixtures, rootless hostile sandbox probes, production-identity system-manager hostile probes, hostile probes derived from exact installed service bytes, provider/companion private-profile probes, transient standard-stream/fd-placement probes, installed service startup, Settings volatile-seat source boundary, one installed no-event synthetic hotplug on systemd 258, and one live LightDM/Openbox logout/relogin | Disabled-by-default volatile seat gate, selection-bound operator discovery across equivalent DISPLAY spellings, live explicit status/enable/disable and MCP deny/admit/deny round trip, typed Settings status/enable/disable plus distinct saved/active/runtime presentation, session-generation revocation and restart reset, and fail-closed LightDM/Openbox provider replacement; positive movement, lower-versus-covering override stacking, live locked-seat refusal, activity interruption, bounded initial eligibility, eligibility interruption, peer identity, fixed framing, loss classification, all session/system eligibility predicates, strict class-map/device-record parsing, double-sampled current-set rendering/verification, changed identity/capability refusal, exact device-filter rendering, named owned-descriptor adoption, diagnostic-free unit syntax, new-only install rollback, exact installed-byte verification, purge scope, rootless and production-identity negative authority, installed-unit-derived broker/guard confinement, provider device denial with delegated application access, companion-only named provider IPC under private networking, real kernel hotplug stop, descriptor closure, and fresh-instance rearm verified locally; physical replacement, other launchers, and trusted lock transition remain unsupported |
| 0.1.3 / revision 5 | 0.1.4 | `agent-seat-x11` 0.1.24 / Tier 0 core plus Tier 0.5 input; Settings 0.1.6 | Linux, released Openbox 3.6.1 / isolated Xvfb plus rootless private-device systemd and Bubblewrap probes | Target-relative pointer movement, all three complete logical button pairs, covering and over-bound hit-test refusal, focus-owner refusal, live-keymap lower/shifted/Return/Tab delivery, unmapped-scalar no-send, event-triggered exact partial interruption, separate pointer/keyboard grants, disabled-seat opening denial, generation revocation, absence of a `human_activity` claim, and actual-provider click/text delivery with evdev and uinput paths absent verified; ordinary input needs no root, broker, evdev, uinput, or input-group dependency |
| 0.1.3 / revision 5 | 0.1.4 | `agent-seat-x11` 0.1.25 / Tier 0 core plus Tier 0.5 XKB input; Settings 0.1.6 | Linux, released Openbox 3.6.1 / isolated Xvfb, xterm 407, setxkbmap 1.3.4, installed XKB data 2.40 | Norwegian `https://slashdot.org` application-visible regression passes; all 590 installed registry default/variant combinations classified, with 587 loadable cases producing exact xterm text (363) or pre-send refusal (224) and three definitions rejected by setxkbmap itself; current-group XKB types/levels and bounded Shift/Level3/Level5 resolution replace the unsafe core compatibility-column assumption; compose/IME, group switching, custom definitions, and general application acceptance remain unsupported |
| 0.1.4 / revision 6 | 0.1.5 | `agent-seat-x11` 0.1.26 / Tier 0 core plus experimental obscured-client capture; Settings 0.1.7 | Linux, released Openbox 3.6.1 / isolated Xvfb with Composite 0.4 | Separately granted target-owned PNG capture verified after enrollment while a differently colored override-redirect window covers the target; stale destroyed target refusal, fixed dimension/pixel/PNG/frame bounds, TrueColor conversion, MCP image projection, and provider-owned cleanup are covered. Output capture, cursor/decorations, already-obscured pre-enrollment reconstruction, and compositor portability beyond this environment remain unsupported |
| 0.1.4 / revision 6 | 0.1.6 | `agent-seat-x11` 0.1.26 / unchanged revision-6 profiles; Settings 0.1.7 | Linux / process-boundary stdio plus a strict fake revision-6 Unix provider | MCP `2026-07-28` discovery, per-request metadata, complete results, one-hour public static-tool caching, unsupported-version errors, and explicit bounded provider contexts pass alongside the unchanged MCP `2025-11-25` initialize lifecycle, 16-tool surface, schemas, and results. The modern surface has 17 tools because it adds `seat_release`; HTTP transport, MCP authorization, extensions, subscriptions, and multi-round-trip input are not implemented or claimed |
| 0.1.5 / revision 7 | 0.1.7 | `agent-seat-x11` 0.1.27 / experimental Tier 0.5 input v2; Settings 0.1.8 | Linux, released Openbox 3.6.1 / isolated Xvfb with XKB and XTEST | One finite `keyboard.key` action is wired through both MCP eras and the live XKB map. Page Down and Control+L produce balanced press/release events only for the already focused fresh target; closed schemas, canonical modifier validation, wire capability separation, and existing Norwegian text behavior pass locally. The legacy surface has 17 tools and the modern surface 18 including `seat_release`. Browser media back/forward, held keys, arbitrary sequences, forced focus, compose/IME, application acceptance, and physical-user priority remain unsupported |
| 0.1.6 / revision 8 | 0.1.8 | `agent-seat-x11` 0.1.28 / experimental Tier 0.5 input v3 and obscured capture v2; Settings 0.1.9 | Linux, released Openbox 3.6.1 / isolated Xvfb with XKB, XTEST, and Composite 0.4; strict fake provider for MCP process boundary | A 300-scalar multiline `keyboard.write` exceeds the retained short-text limit while producing balanced events under the existing focus/seat checks. A covered 64×32 target-relative region returns exact target-owned pixels and an out-of-target rectangle is refused. MCP saves, lists, and replays one session-local click through an ordinary provider call; context release/provider failure clears the bounded store. Legacy publishes 22 closed tools and modern 23 including `seat_release`. Clipboard/selection injection, arbitrary Unicode, element identity, persistent workflow memory, macros, application acceptance, and physical-user priority remain unsupported. |
| 0.1.6 / revision 8 | 0.1.9 | `agent-seat-x11` 0.1.29 / unchanged experimental profiles; Settings 0.1.9 | Linux, released Openbox 3.6.1 / isolated Xvfb with Norwegian XKB and XTEST; MCP process boundary | A 301-scalar long write ending in unavailable `í` is refused before its ASCII prefix and identifies character 301 as U+00ED. Both MCP eras publish the direct-XKB and no-layout-mutation guardrail. Wire operations, capabilities, profiles, and the lack of clipboard/arbitrary-Unicode support are unchanged; a separately granted text-transfer path remains an unimplemented candidate. |

The companion-profile evidence includes a delayed live `seat_status` after the
ordinary provider I/O deadline and a provider restart while that authenticated
session was idle. The call succeeds, while provider shutdown remains bounded.
The external harness itself is not confined by that result.
Stable candidate fixtures and the required isolated review bundle are defined
in the [T5 participant contract](verification/t5-participation-contract.md).
The current development command environment is explicitly nonqualifying: the
reference hostile probe reached uinput, Xauthority, provider, broker, user-
manager connection and unit submission, both filesystem and abstract X11, and
parent-process data. Only direct evdev open was denied, for a 9/10 ambient-
authority result. It inherited no input descriptor. This is baseline evidence,
not a harness-boundary pass.

## Pre-RFC publication matrix

These artifacts do not change runtime compatibility or claim external
standards status.

| Artifact | Identifier | Repository status | Remaining gate |
| --- | --- | --- | --- |
| Portable semantics | serialization-neutral information model | Complete repository draft; revision 5 binding remains authoritative | Independent implementation review |
| Registry | `agent-seat.registry-set/1` | Hand-reviewed machine projection plus custody/change policy | External custodian and governance decision |
| Backend profile | `agent-seat.x11-ewmh-core.v1` | Complete experimental standalone Tier 0 core profile | Independent implementation or independent black-box harness |
| Evidence report | `agent-seat.conformance-report/1` | Draft 2020-12 schema, stable fixture IDs, positive example validation, and three negative schema cases pass | Independent reports and publication venue |

## Cross-product matrix

| Companion | Provider | Forced explicit-socket result | Status |
| --- | --- | --- | --- |
| `agent-seat-mcp` 0.1.1 / revision 3 | `agent-seat-x11` 0.1.4 / revision 3 | Full source and process gates pass | Compatible |
| released `nobox-agent` 0.1.7 / revision 2 | `agent-seat-x11` 0.1.4 / revision 3 | Provider closes during incompatible opening; no request served | Incompatible |
| `agent-seat-mcp` 0.1.1 / revision 3 | released Nobox 0.1.3 Tier 1 / revision 2 | Provider closes during incompatible opening; companion returns `unavailable`/`reconnect` | Incompatible |

The first release does not advertise capture, input, human-activity, or
accessibility features. Those combinations are unsupported, not untested.

Future entries name exact released versions, advertised wire revision,
backend features, tested window manager and X server, and one of: compatible,
partially supported, incompatible, or untested. Tests use released binaries and
public process/socket/X11 behavior; they do not vendor another implementation's
test suite.
