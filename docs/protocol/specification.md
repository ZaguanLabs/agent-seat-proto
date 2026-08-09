# Agent Seat local JSON binding, wire revision 5

Status: experimental Tier 0.5 input extension over the released E1 contract.
Revision 5 is intentionally incompatible with Agent Seat revisions 3 and 4
and Nobox revision 2.

This document is the concrete pathname-Unix-stream and strict-JSON binding for
the portable semantics in the pre-RFC
[`information model`](information-model.md). Its JSON field names, framing,
socket discovery, and byte limits belong to revision 5, not to the abstract
model. This binding remains the repository's normative implemented wire
contract if the pre-RFC documents differ.

## Goal

Define one small, strict contract for a local authority-free companion and a
policy-owning desktop provider. Every allocation and collection has a finite
limit, mutation results state only what the provider observed, and display
server identities never cross the boundary.

## Non-goals

- Negotiating by accepting a message that merely resembles another revision.
- Giving the companion authority over grants, scope, launch policy, or backend
  features.
- Encoding X11 atoms, XIDs, Wayland objects, process IDs, or raw desktop
  properties.
- Treating Tier 0 observations as atomic window-manager state.
- Adding capture, input, or accessibility behavior to the core implicitly.

## Revision decision

The first independent wire was revision 3. Revision 2 was released by Nobox for
its integrated Tier 1 seat. Tier 0 requires an authenticated welcome that
states `x11_ewmh`, `tier0`, exact backend features, exact grants, and provider
limits. Its management response must distinguish an observed result from a
sent request that timed out or lost its target. Compatibility with all those
required shapes was not established for revision 2, so the independent
product does not reuse that number.

Revision 4 added one broker-gated `pointer.move` experiment and its qualified
input result. Revision 5 replaces that deployment requirement with the
provider-owned volatile seat gate and adds `pointer.click`, `keyboard.type`,
and a separate `input_keyboard` grant. It deliberately does not claim trusted
physical-activity detection. Strict older decoders cannot safely interpret
the additions, so revision 5 does not change an earlier revision in place.

An advertisement and opening message name one exact revision. There is no
range negotiation. A mismatched pair closes with `incompatible_revision` and
does not guess from JSON fields.

## Transport and framing

The provider accepts pathname `AF_UNIX` stream connections only. TCP, UDP,
abstract sockets, forwarded transports, and MCP stdio are not Agent Seat wire
transports.

Each message is one four-byte unsigned big-endian JSON byte length followed by
exactly that many UTF-8 JSON bytes:

```text
0                   31
+--------------------+--------------------------+
| JSON bytes (u32 BE) | strict JSON payload ... |
+--------------------+--------------------------+
```

- A zero length is malformed.
- Client-to-provider payloads are at most 65,536 bytes.
- Provider-to-client payloads are at most 1,048,576 bytes.
- The receiver rejects an over-bound prefix before allocating its payload.
- EOF before any prefix byte is a clean stream end. EOF inside a prefix or
  payload is truncated input.
- The JSON value is one complete object. Trailing bytes, duplicate known
  fields, unknown fields, unknown enum values, and wrong JSON types are
  malformed.
- Serde's finite JSON recursion limit applies; revision 5 defines no recursive
  message value.

Lengths constrain encoded bytes, not Unicode scalar counts. Bounded lists stop
deserializing when the first over-bound item is detected and retain no spare
capacity after successful construction.

## Opening and lifecycle

The first client message is `hello`. A provider answers with exactly one
`welcome` or terminal `goodbye`. Requests begin only after `welcome`.

```json
{
  "type": "hello",
  "body": {
    "protocol": "agent-seat",
    "revision": 5,
    "peer": {
      "name": "agent-seat-mcp",
      "version": "0.1.3",
      "purpose": "translate MCP desktop tools"
    },
    "requested": ["observe_structure", "observe_titles"]
  }
}
```

Peer metadata is descriptive and never authorizes. Names are at most 128
bytes, versions 64 bytes, and purpose 256 bytes. Requested capabilities are a
unique, canonical list of at most 32 atoms.

```json
{
  "type": "welcome",
  "body": {
    "protocol": "agent-seat",
    "revision": 5,
    "session": 1,
    "provider": {"name": "agent-seat-x11", "version": "0.1.0"},
    "backend": "x11_ewmh",
    "assurance": "tier0",
    "features": ["ewmh_observation"],
    "granted": ["observe_structure"],
    "limits": {
      "request_frame_bytes": 65536,
      "response_frame_bytes": 1048576,
      "events_per_poll": 1024,
      "poll_wait_ms": 30000
    }
  }
}
```

Provider name/version are nonempty and bounded like peer metadata. Features
and grants are unique canonical lists. The fixed backend is `x11_ewmh`; the
fixed assurance is `tier0`. A provider may grant fewer atoms than requested.
A feature never grants a capability and a capability never invents a feature.

The provider authenticates local credentials and evaluates grants before
`welcome`. A terminal `goodbye` carries an error code and an optional 512-byte
diagnostic. Closing the stream is otherwise the lifecycle shutdown signal.

## Capabilities and features

Canonical capability order is:

1. `observe_structure`
2. `observe_titles`
3. `observe_events`
4. `manage_activate`
5. `manage_close`
6. `manage_workspace`
7. `manage_state`
8. `manage_geometry`
9. `launch_list`
10. `launch_execute`
11. `input_pointer`
12. `input_keyboard`

Core features are `ewmh_observation`, `ewmh_management`, and
`desktop_launch`. Reserved optional feature names are
`client_visible_capture`, `obscured_capture`, `output_capture`,
`input_injection`, `human_activity`, and `accessibility`. Advertising a feature
is an implementation claim governed by its profile threat model; absent
features remain typed `unsupported` behavior.

## Requests and responses

A request has a nonzero peer-selected `id` and one adjacently tagged call:

```json
{
  "type": "request",
  "body": {
    "id": 7,
    "call": {
      "name": "client.activate",
      "arguments": {"client": 3, "generation": 9}
    }
  }
}
```

The response repeats the ID exactly and contains one outcome:

```json
{
  "type": "response",
  "body": {
    "id": 7,
    "outcome": {
      "status": "ok",
      "body": {
        "kind": "management",
        "value": {"observation": "observed", "sequence": 42}
      }
    }
  }
}
```

The core call names and arguments are:

| Call | Required capability | Arguments |
| --- | --- | --- |
| `seat.status` | `observe_structure` | closed empty object |
| `desktop.snapshot` | `observe_structure` | closed empty object |
| `events.subscribe` | `observe_events` | optional canonical `kinds`, at most 8 |
| `events.poll` | `observe_events` | `after`, `limit` 1..1024, `wait_ms` 0..30000 |
| `client.activate` | `manage_activate` | `client`, `generation` |
| `client.close` | `manage_close` | `client`, `generation` |
| `workspace.switch` | `manage_workspace` | `workspace`, snapshot `sequence` |
| `client.workspace` | `manage_workspace` | `client`, `generation`, `workspace` |
| `client.state` | `manage_state` | fresh client, typed `state`, add/remove/toggle |
| `client.geometry` | `manage_geometry` | fresh client and nonempty frame rectangle |
| `applications.list` | `launch_list` | cursor and page `limit` 1..256 |
| `application.launch` | `launch_execute` | canonical `.desktop` application ID |
| `pointer.move` | `input_pointer` | fresh client and client-relative unsigned `x`, `y` |
| `pointer.click` | `input_pointer` | fresh client, client-relative unsigned `x`, `y`, and primary/middle/secondary button |
| `keyboard.type` | `input_keyboard` | fresh client and bounded nonempty `text` |

Client IDs are nonzero provider-session handles. They are not XIDs. A
generation and sequence are provider-local unsigned 64-bit freshness values;
zero is valid before the first change. Workspace IDs are unsigned 16-bit EWMH
indexes. Rectangles use signed 32-bit positions and nonzero unsigned 32-bit
extents.

## Observation and events

A snapshot carries one sequence, the current workspace, at most 128 unique
workspace descriptors, at most 1,024 unique visible client descriptors, and an
optional active client that must occur in the visible client list. Missing
facts are absent, never fabricated zero values.

Titles are at most 1,024 bytes and exist only when granted. States and allowed
actions are unique canonical lists. Every optional rectangle has a nonzero
extent.

An event subscription returns an initial cursor. Poll responses contain at
most 1,024 strictly increasing event sequences no later than the returned
cursor. Events carry added/changed descriptors, removed handles, active-client
changes, workspace changes, or application-catalog invalidation. Queue
overflow is the `resync_required` error; the peer discards its model and takes
a new snapshot.

## Management result

Policy refusal, stale state, invisible target, and unsupported operations are
errors produced before an EWMH request is sent. A successful `management`
reply proves that a request was sent and carries exactly one observation:

- `observed`: the desired public state became true;
- `timed_out`: the fixed deadline expired without observing it; or
- `target_gone`: the target disappeared after send and the outcome is unknown.

None of these values claims that a foreign window manager or application
accepted an event internally.

## Input result

Input is an optional Tier 0.5 X11 profile, never a Tier 0 core promise. Every
input call requires a fresh scoped target, its separate input grant, and the
same enabled volatile-seat generation that admitted the session. For each
independently reportable action, the provider briefly grabs the X server,
refreshes target evidence, rechecks the seat generation, queues bounded XTEST
events, synchronizes, releases the server, and checks the seat again.

`pointer.move` accepts one client-relative destination. `pointer.click` moves
to that destination and sends one complete primary, middle, or secondary
press/release pair. Both require the current point to be inside the target and
the topmost visible X11 input ancestry to belong to that target; another window
covering the point causes `invalid_argument` without a click.

`keyboard.type` requires actual X11 input focus to be the fresh target or one
of its descendants. It never forces focus. Text is at most 1,024 UTF-8 bytes
and 256 Unicode scalar actions. Newline and tab are the only accepted control
characters. The provider resolves every character against the first two
levels of the live X11 map's first keyboard group before sending any key, so an
unavailable character is refused instead of guessed or written through a
shell. Each accepted character is one complete key press/release action with
a bounded Shift pair when required.

The input reply carries `completed`, `requested`, and `queued` or
`interrupted`. `queued` means only that every reported action was queued and
synchronized with X11. It does not prove that an application accepted,
rendered, or understood the input. `interrupted` may carry a partial count if
the operator disables the seat or required target, focus, or backend evidence
is lost between text actions.

This profile does not advertise `human_activity`. Ordinary same-user X11
cannot distinguish XTEST from physical input or guarantee that physical input
will always win a race. A person and the agent can therefore overlap. Stop the
provider or disable its volatile seat to revoke later Agent Seat requests.

## Launch result

Application pages contain at most 256 entries ordered uniquely by canonical
desktop ID. Each entry carries a bounded localized name and whether the winning
catalog entry comes from the user-specific XDG data root. Page cursor zero
starts a current scan; a nonzero cursor is meaningful only in the same
provider session after that scan. A launch re-resolves the current winning
entry and policy rather than trusting page contents.

A launch success carries a unique nonzero token and an optional visible client
handle only when provider-defined correlation evidence was sufficient. Missing
correlation is not an error and is never guessed. A Tier 0 X11 provider may use
an exact startup-notification ID on a newly visible, in-scope client; the match
is same-user X11 metadata and does not raise the assurance level or prove
causality.

## Error contract

An error outcome carries a stable `code`, stable `retry`, optional exact field,
optional bounded diagnostic, and optional current generation/sequence. English
never selects control flow.

| Code | Meaning before/after send |
| --- | --- |
| `unavailable` | no provider/source; no desktop request |
| `incompatible_revision` | exact revision mismatch; session closes |
| `refused` | grant/policy denied; nothing sent |
| `no_such_client` | missing, hidden, or out-of-scope; nothing sent |
| `stale` | freshness changed; nothing sent |
| `unsupported` | backend/target did not advertise support; nothing sent |
| `timed_out` | valid operation sent but not observed |
| `invalid_argument` | typed argument correction required; nothing sent |
| `malformed` | frame/schema violation |
| `too_large` | published bound exceeded |
| `internal` | provider could not complete the operation |
| `resync_required` | event backlog was discarded |
| `revoked` | live grant was removed or narrowed |
| `session_closed` | session cannot accept another call |

Retry is one of `never`, `reobserve`, or `reconnect`.

## X11 discovery

`_AGENT_SEAT` is `UTF8_STRING`, format 8, at most 256 bytes, with exactly
three NUL-separated UTF-8 fields and no trailing NUL:

```text
agent-seat NUL 5 NUL /absolute/pathname/socket
```

The revision uses canonical decimal. The socket is nonempty, absolute, has no
NUL, and fits the platform pathname socket bound (107 bytes on Linux).

The provider owns `_AGENT_SEAT_S<screen>` on a dedicated window. That window
and the selected root contain byte-identical advertisements. A consumer reads
the current owner, both bounded properties, and the owner again. No owner,
changed owner, missing property, or mismatch means no live discovery source.
Wrong type/format/size/UTF-8/grammar/revision is an error.

Companion resolution order is exactly explicit `--socket`,
`AGENT_SEAT_SOCKET`, then live selection-bound X11. A selected malformed or
unreachable higher-precedence source fails there. There is no conventional
filesystem or product-specific fallback.

## End result

Revision 5 retains the bounded T0--T3 contract and adds the explicitly
operator-gated Tier 0.5 pointer and keyboard surface. It gives the MCP
translator no authority and keeps every core outcome externally testable. A
later incompatible field, enum, or semantic change allocates another revision
instead of weakening strict decoding.
