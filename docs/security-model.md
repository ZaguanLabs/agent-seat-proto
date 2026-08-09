# Security model

Status: T3 Tier 0 core. Feature-specific analysis expands before each optional
profile milestone.

E1 supplies strict bounded wire decoding and an authority-free companion. A
companion can request capabilities but cannot grant them, and it never treats
descriptive peer metadata as identity.

T0 supplies the policy boundary: a private pathname socket, kernel
`SO_PEERCRED` UID, strict owner-controlled configuration, explicit same-user
grant, bounded sessions, and atomic selection ownership. It deliberately
provides no cross-user isolation and no protection from another process that
already has the session owner's X11 authority.

T1 adds independently sampled EWMH observation. Client enumeration is disabled
by default. Scope filtering happens before title allocation, titles require
both an explicit policy switch and a session capability, raw XIDs are replaced
with per-session opaque IDs, and all properties, scans, strings, queues, polls,
and waits have fixed bounds. Clients that leave a visibility scope lose their
identity and receive a new one if they return.

T2 adds mutation only for requests advertised by both the WM and target. Every
management capability depends on `observe_structure`; the provider refreshes
scope and opaque freshness under an X server grab immediately before sending.
Policy, missing target, stale generation/sequence, invalid workspace/geometry,
and unsupported operations are no-send failures. Close requires
`WM_DELETE_WINDOW`; there is no kill or input fallback.

T3 adds launch as two separately granted operations. Policy defaults to deny;
execution additionally depends on catalog visibility. Preference-ordered XDG
roots, paths, files, keys, entries, pages, arguments, children, and correlation
waits are bounded. User-root entries need a separate switch and remain user
entries even when allow-listed. The provider reparses current entry contents
before each spawn, constructs an argv vector directly, and never passes desktop
metadata or peer data through a shell. Invalid entries disappear rather than
being partially interpreted.

The standalone provider is a policy boundary against accidental overreach,
malformed peers, and a compromised translator. It is not an isolation boundary
against another process that already has the same user's X11 authority.

T5R's optional reference deployment introduces two additional boundaries. The
activity broker alone receives exact read-only evdev descriptors and reduces
them to readiness, instance, epoch, and terminal stop state. The X11 provider
receives no raw device descriptor or event field. When its private-device
policy switch is active, a systemd user service removes `/dev/input` and
`/dev/uinput`; the provider verifies both absences before claiming X11
ownership. Controlled applications are then launched in separate transient
user services so their ordinary device permissions do not require widening the
provider's authority. Failure to create either boundary is input-unavailable,
not a degraded mode.

The matching companion profile does not enter that provider's X11 namespace.
The user manager connects the fixed private provider socket first, passes the
one connected descriptor by an exact name, and then starts the companion with
private network and device namespaces plus an empty `/run`, private `/tmp`,
hidden home/process data, cleared desktop environment, no capabilities, no
arbitrary executable, and fixed resource bounds. This removes both filesystem
and abstract X11 sockets while preserving only Agent Seat provider IPC. The
external harness receives MCP stdio, never the inherited provider descriptor
or broker channel.

The connected descriptor carries normal Unix peer credentials, not proof of
which systemd properties surrounded the process after the connection was
opened. The provider therefore makes no such claim. It applies its configured
UID grant and, only in the private-device input profile, lets one authenticated
and granted session wait idle between complete frames. All initial handshakes,
partial frames, and additional sessions remain deadline-bound; shutdown
interrupts the idle wait. The emitted unit and hostile test, rather than peer
PID inference, establish companion confinement.

## Trusted

- the session owner who controls provider configuration and grants;
- the standalone provider's policy and bounded protocol implementation;
- local OS peer credentials and private runtime/configuration directories; and
- the X server for transport and observation, within X11's stated limits.

## Untrusted

- the MCP companion, harness, model, and all declared peer names;
- X11 client titles, classes, properties, process identifiers, and startup
  metadata;
- desktop entries and launched applications; and
- every frame, list length, string, timeout, and state transition received from
  outside the provider.

## Core rules

- The provider is disabled and deny-by-default.
- Only verified local peers reach grant evaluation; the provider rechecks
  capability, scope, feature, policy, and freshness on every call.
- One provider owns a screen through an X11 selection. Ownership prevents
  accidental conforming duplicates, not malicious selection theft.
- Raw X11 resource identifiers do not cross the provider boundary.
- Missing, hidden, and out-of-scope direct client lookups are indistinguishable.
- Optional malformed client metadata is omitted; malformed or absent required
  desktop structure fails the observation instead of inventing state.
- Event diffs are sampled and may require a snapshot resynchronization; they
  are never represented as atomic window-manager notifications.
- Buffers, frames, peers, queues, scans, strings, retries, and deadlines are
  finite before allocation or blocking work.
- Mutation distinguishes no-send decisions from sent but unobserved outcomes.
- A sent operation is sampled for a fixed second and reports observed,
  timed-out, or target-gone state without claiming WM acceptance.
- Provider, peer, or socket failure never becomes a window-manager failure.
- Core operations do not fall back to a shell, XTEST, or global coordinates.
- Launch correlation requires one exact startup-ID match on a newly visible,
  in-scope client. It never falls back to PID, title, class, timing, or window
  count, and lack of evidence is successful launch with no client handle.
- An input-profile provider must not see evdev or uinput paths even when its
  login UID normally can; an admitted application must not inherit the
  provider's private device namespace.
- An input-profile companion must receive exactly one already-connected,
  named provider descriptor; it must not retain X11 discovery, broker, raw
  input, network, home, or arbitrary execution authority.

Same-user X11 clients may inspect or spoof desktop state and bypass the
provider entirely. Stronger isolation requires a different OS user/session or
a display architecture with an enforceable security boundary.

Between property reads, a client or window manager can change or destroy a
window. The provider therefore guarantees bounded, convergent observation—not
an atomic view—and uses generations and resynchronization to make freshness
loss explicit. XID reuse between samples is an inherent X11 observation limit.

The pre-send server grab closes the provider's own observe/send scheduling
window but does not make a foreign WM trustworthy or make later realization
atomic. Pager-source activation and close carry `CurrentTime` because the
standalone provider has no trustworthy user-event timestamp; a WM may
legitimately ignore them, which remains a timed-out observation.

Desktop entries and executable paths can change, and allow-installed mode is a
deliberately broad trust decision. An allow list authorizes the current winning
entry with that ID, not a content digest or immutable binary. The provider
protects its parsing and policy boundary; it does not sandbox the launched
application. A same-user X11 process can spoof `_NET_STARTUP_ID`, so a
correlated handle is convenience evidence within the already-weak X11 trust
domain, never authentication or proof that the launched process owns the
window.
