# Security model

Status: T2 management. Feature-specific analysis expands before each later
implementation milestone.

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

The standalone provider is a policy boundary against accidental overreach,
malformed peers, and a compromised translator. It is not an isolation boundary
against another process that already has the same user's X11 authority.

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
