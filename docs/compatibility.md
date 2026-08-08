# Compatibility matrix

E1 defines and verifies the independent wire and companion boundary. It does
not yet claim end-to-end desktop compatibility because T0 has not implemented
a provider.

| Protocol crate | Companion | Provider | Backend/WM | Status |
| --- | --- | --- | --- | --- |
| 0.1.1 / revision 3 | 0.1.1 | none (0.1.0 skeleton) | none | E1 boundary verified; no provider |

Future entries name exact released versions, advertised wire revision,
backend features, tested window manager and X server, and one of: compatible,
partially supported, incompatible, or untested. Tests use released binaries and
public process/socket/X11 behavior; they do not vendor another implementation's
test suite.
