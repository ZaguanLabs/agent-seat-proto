# Compatibility matrix

T0 verifies the provider lifecycle and revision-3 status boundary beside bare
Xvfb and released Openbox. Observation, management, and launch remain absent,
so this is not yet Tier 0 core compatibility.

| Protocol crate | Companion | Provider | Backend/WM | Status |
| --- | --- | --- | --- | --- |
| 0.1.1 / revision 3 | 0.1.1 | 0.1.1 / `x11_ewmh`, `tier0` | Linux, Openbox 3.6.1 / Xvfb | T0 foundation verified; core features absent |

Future entries name exact released versions, advertised wire revision,
backend features, tested window manager and X server, and one of: compatible,
partially supported, incompatible, or untested. Tests use released binaries and
public process/socket/X11 behavior; they do not vendor another implementation's
test suite.
