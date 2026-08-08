# Security model

Status: T0 foundation. Feature-specific analysis expands before each
implementation milestone.

E1 supplies strict bounded wire decoding and an authority-free companion. A
companion can request capabilities but cannot grant them, and it never treats
descriptive peer metadata as identity.

T0 supplies the policy boundary: a private pathname socket, kernel
`SO_PEERCRED` UID, strict owner-controlled configuration, explicit same-user
grant, bounded sessions, and atomic selection ownership. It deliberately
provides no cross-user isolation and no protection from another process that
already has the session owner's X11 authority.

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
- Buffers, frames, peers, queues, scans, strings, retries, and deadlines are
  finite before allocation or blocking work.
- Mutation distinguishes no-send decisions from sent but unobserved outcomes.
- Provider, peer, or socket failure never becomes a window-manager failure.
- Core operations do not fall back to a shell, XTEST, or global coordinates.

Same-user X11 clients may inspect or spoof desktop state and bypass the
provider entirely. Stronger isolation requires a different OS user/session or
a display architecture with an enforceable security boundary.
