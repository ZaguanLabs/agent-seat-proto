# Optional X11 profile decisions

Status: first Tier 0 core release decision, 2026-08-08. T4 is deferred to a
future wire revision in one narrow form; T5 and T6 meet their stop conditions
and remain unsupported. None of these profiles delays C0.

## Goals

- Decide from public X11 and AT-SPI contracts which optional guarantees can be
  stated honestly beside an unmodified window manager.
- Prevent a useful core release from silently acquiring weaker capture, input,
  or accessibility authority.
- Identify the smallest future protocol surface that could be tested without
  changing revision 3 in place.

## Non-goals

- Implementing optional calls before their wire schema and threat model are
  approved.
- Treating same-user X11 metadata, device IDs, process IDs, timing, titles, or
  geometry as authentication.
- Calling a screen image target-only, calling injected input accepted, or
  calling an accessibility object correlated when the evidence is heuristic.
- Adding persistent coordinates, screenshots, trees, workflows, or interaction
  history.

## Revision boundary

Revision 3 reserves feature names for capture, input, human activity, and
accessibility but defines no capabilities, calls, arguments, or replies for
them. Its peers use exact revision matching and strict enums. Adding optional
operations under revision 3 would therefore create two incompatible languages
with the same revision number.

Any shipped optional profile needs a new exact revision or a separately
specified extension-negotiation mechanism. Until then the provider does not
advertise the reserved features and the companion exposes no corresponding
tools. Unknown revision-3 calls remain malformed rather than being guessed.

## T4 capture decision

### Evidence

Core X11 `GetImage` does not provide a safe visible-client primitive. The X11
Security Extension specification records that obscured regions are undefined
without backing store and that some server implementations return pixels from
the obscuring windows. A direct client-window read can therefore include
another client. Reading the root or an output necessarily includes every
window contributing to those screen pixels, including clients outside Agent
Seat scope.

Composite `NameWindowPixmap` has a different meaning: it names the off-screen
storage for one redirected window. That is a plausible target-only pixel
source, but it exposes the target's obscured content rather than only what the
human can currently see. It also needs fresh map/scope/state validation, a
short server-grabbed name/read boundary, fixed dimensions and byte limits,
explicit pixel-format conversion, and destruction/error tests. It cannot be
presented as output capture or `client_visible_capture`.

### Decision

- `output_capture` fails the hidden/out-of-scope stop condition and is not
  shipped.
- Core `GetImage` client capture fails because obscured regions can contain
  unrelated pixels and is not shipped.
- A future `obscured_capture` profile may be specified for a freshly scoped,
  mapped target using only its Composite named pixmap. It requires a new wire
  revision and an explicit grant because it can reveal target content hidden
  behind other windows.
- Grounded coordinate grids remain an encoding layered on a qualifying future
  capture, never a way to make an unsafe source safe.

No capture feature is advertised in the first core release.

## T5 input decision

### Evidence

XTEST deliberately synthesizes device events at the X server's normal input
level, almost as if a cooperative user acted. The delivered core events do not
carry an authenticated “injected by this provider” marker. X RECORD can observe
XTEST requests and the resulting device events in order, but ordering and
timing do not authenticate a physical source. XInput device/source IDs describe
server topology; they do not prove that an event was human, and nested,
remote-desktop, virtual-device, hotplug, and other same-user injection paths can
collapse or spoof that distinction.

A global key grab could implement an emergency shortcut, but it would compete
with the window manager and applications, remain spoofable by another X11
client, and would not repair attribution for the suppression window. Reporting
only “queued to X11” would correctly limit delivery claims but would not meet
the roadmap's separate human-interruption requirement.

### Decision

The generic Tier 0 X11 environment cannot distinguish injected and human input
well enough to honor the promised suppression contract. T5 meets its stop
condition and remains unsupported. `input_injection` and `human_activity` are
not advertised, and there is no pointer, keyboard, text, or emergency-stop MCP
surface in revision 3.

Reconsideration requires a separately trusted input source outside ordinary
same-user X11—such as compositor/window-manager cooperation or an explicitly
privileged OS input path—with its own authority and deployment review. That
would be a different profile, not an inference from XInput IDs.

The later [`t5-input-reconsideration.md`](../security/t5-input-reconsideration.md) review
evaluates that privileged path. It identifies a narrowly bounded system
activity broker as a candidate, but leaves its new raw-input authority behind
explicit approval and deployment gates. Revision 3 and this stop decision are
unchanged.

## T6 semantics decision

### Evidence

AT-SPI exposes an application root on its accessibility D-Bus and permits a
consumer to resolve the D-Bus connection's Unix process ID. It does not define
a general authenticated mapping from an EWMH client window to exactly one
accessible root. `_NET_WM_PID` is client-supplied X11 metadata. Multi-process
browsers, helper processes, multiple top-level windows, popups, and toolkit
bridges make PID, title, class, geometry, and timing correlation incomplete or
ambiguous.

Walking the desktop accessibility registry before proving the target mapping
would expose out-of-scope structure to the helper. Filtering only the returned
tree would not restore hidden/out-of-scope equivalence because discovery,
errors, timing, and ambiguity already depend on unrelated applications.

### Decision

T6 meets its safe-correlation stop condition and remains unsupported. No
desktop-wide helper is started, no accessibility feature is advertised, and no
semantic handle or action is added. A future study needs an application- or
toolkit-provided binding to the already scoped client that is stronger than
PID/title/geometry heuristics. It must still use a disposable bounded helper
with no policy authority.

## End result

The first Tier 0 core release has an exact and intentionally small claim:
bounded observation, supported EWMH management, and policy-controlled launch.
It advertises none of the capture, input, human-activity, or accessibility
features. Output capture, generic input, and semantics are explicitly stopped;
only target-owned Composite capture remains a documented candidate for a new
revision. Persistent coordinate or workflow memory remains out of scope in
every case.

## Standards consulted

- X11 protocol `GetImage` behavior:
  <https://www.x.org/releases/X11R7.7/doc/xproto/x11protocol.html>
- X11 Security Extension image rules:
  <https://www.x.org/releases/X11R7.7/doc/xextproto/security.html>
- X Composite extension:
  <https://www.x.org/archive/X11R7.5/doc/man/man3/Xcomposite.3.html>
- XTEST extension:
  <https://www.x.org/releases/X11R7.7/doc/xextproto/xtest.html>
- X RECORD extension:
  <https://www.x.org/releases/X11R7.7/doc/recordproto/record.html>
- AT-SPI 2 interfaces:
  <https://gnome.pages.gitlab.gnome.org/at-spi2-core/libatspi/>
