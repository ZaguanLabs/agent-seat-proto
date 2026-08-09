# Compatibility matrix

C0 verifies the complete revision-3 Tier 0 core beside bare Xvfb and released
Openbox and forces both directions of the separate Nobox revision-2 boundary.
Detailed release-candidate evidence is in
[`c0-verification.md`](c0-verification.md).

| Protocol crate | Companion | Provider | Backend/WM | Status |
| --- | --- | --- | --- | --- |
| 0.1.1 / revision 3 | 0.1.1 | 0.1.1 / `x11_ewmh`, `tier0` | Linux, Openbox 3.6.1 / Xvfb | T0 foundation verified; core features absent |
| 0.1.1 / revision 3 | 0.1.1 | 0.1.2 / `x11_ewmh`, `tier0`, `ewmh_observation` | Linux, Openbox 3.6.1 / Xvfb | T1 scoped snapshots, filtered diffs, title redaction, and resync verified |
| 0.1.1 / revision 3 | 0.1.1 | 0.1.3 / `x11_ewmh`, `tier0`, observation + management | Linux, Openbox 3.6.1 / Xvfb | T2 activation, polite close, workspace, state, geometry, and terminal outcomes verified |
| 0.1.1 / revision 3 | 0.1.1 | 0.1.4 / `x11_ewmh`, `tier0`, observation + management + launch | Linux, Openbox 3.6.1 / Xvfb | Compatible Tier 0 core; policy/refusal, shell-free launch, exact/absent correlation, failure isolation verified |

## Unreleased experimental source

| Protocol crate | Companion | Provider/profile | Test environment | Status |
| --- | --- | --- | --- | --- |
| 0.1.2 / revision 4 | 0.1.2 | `agent-seat-x11` 0.1.14 + activity broker 0.1.16 / pointer move | Linux, Openbox 3.6.1 / Xvfb; rendered units, unprivileged device inspection, hardened live current-manifest/logind/netlink guard handshake, 24-entry class-map plus 13-entry device-identity/capability render/verify, isolated root-layout transaction fixtures, rootless hostile sandbox probes, production-identity system-manager hostile probes, transient standard-stream/fd-placement probes, installed service startup, and one installed no-event synthetic hotplug on systemd 258 | Positive movement, lower-versus-covering override stacking, live locked-seat refusal, activity interruption, bounded initial eligibility, eligibility interruption, peer identity, fixed framing, loss classification, all session/system eligibility predicates, strict class-map/device-record parsing, double-sampled current-set rendering/verification, changed identity/capability refusal, exact device-filter rendering, named owned-descriptor adoption, diagnostic-free unit syntax, new-only install rollback, exact installed-byte verification, purge scope, rootless and production-identity negative authority, real kernel hotplug stop, descriptor closure, and fresh-instance rearm verified locally; physical replacement, trusted lock transition, and an exact installed-unit hostile fixture remain unsupported |

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
