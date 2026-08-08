# Compatibility matrix

T2 verifies provider lifecycle, bounded revision-3 observation, and supported
EWMH management beside bare Xvfb and released Openbox. Launch remains absent,
so this is not yet complete Tier 0 core compatibility.

| Protocol crate | Companion | Provider | Backend/WM | Status |
| --- | --- | --- | --- | --- |
| 0.1.1 / revision 3 | 0.1.1 | 0.1.1 / `x11_ewmh`, `tier0` | Linux, Openbox 3.6.1 / Xvfb | T0 foundation verified; core features absent |
| 0.1.1 / revision 3 | 0.1.1 | 0.1.2 / `x11_ewmh`, `tier0`, `ewmh_observation` | Linux, Openbox 3.6.1 / Xvfb | T1 scoped snapshots, filtered diffs, title redaction, and resync verified |
| 0.1.1 / revision 3 | 0.1.1 | 0.1.3 / `x11_ewmh`, `tier0`, observation + management | Linux, Openbox 3.6.1 / Xvfb | T2 activation, polite close, workspace, state, geometry, and terminal outcomes verified |

Future entries name exact released versions, advertised wire revision,
backend features, tested window manager and X server, and one of: compatible,
partially supported, incompatible, or untested. Tests use released binaries and
public process/socket/X11 behavior; they do not vendor another implementation's
test suite.
