# Agent Seat conformance report format v1

Status: repository pre-RFC format, 2026-08-09. A report records portable
black-box evidence; it does not certify an implementation, authenticate its
author, or create external standards status.

The machine schema is
[`conformance-report-v1.schema.json`](conformance-report-v1.schema.json). It
uses the official JSON Schema Draft 2020-12 dialect. The checked-in
[`example report`](conformance-report-v1.example.json) is deliberately
`incomplete` and is never conformance evidence.

## Purpose

A report lets another maintainer answer five questions without reading the
claiming implementation:

1. Which exact implementation, binding revision, and registered profile were
   tested?
2. Which operating environment and released desktop components were used?
3. Which stable profile fixtures passed, failed, or were not run?
4. What public evidence and enforcement establish each negative-authority
   claim?
5. What limitations remain, and does the result qualify as pass, fail, or
   incomplete?

The format records observations at process, wire, filesystem, socket, desktop,
and timing boundaries. Log text may be an artifact, but a log line alone never
makes a fixture pass.

## Required identity

`format` is exactly `agent-seat.conformance-report/1`. Unknown report formats
are not guessed. A report names:

- a collision-resistant report ID and UTC production time;
- the subject implementation name, version, source revision, and optional
  content digest;
- exact protocol name, positive wire revision, and transport binding;
- one registered profile identifier and the maturity status claimed by the
  report author;
- the capability and feature surface actually tested; and
- the test-harness identity plus relevant released environment components.

The report profile status is descriptive. It cannot promote a registry entry.
Profile maturity changes only through the registry review process.

## Fixture results

Every profile defines stable fixture IDs. A report includes each required ID
exactly once and may include registered optional fixtures. Results mean:

| Result | Meaning |
| --- | --- |
| `pass` | The externally observable assertion held and at least one content-digested evidence record is attached. |
| `fail` | The assertion was exercised and did not hold. |
| `skipped` | The fixture was deliberately not exercised. |
| `error` | Harness or environment failure prevented a result. |

Evidence records state the public boundary observed, a SHA-256 digest, and an
optional artifact URI. The digest covers the exact retained artifact bytes.
`observations` names the kinds of public boundary used; it does not replace the
artifact or assertion.

Fixture durations are optional monotonic elapsed milliseconds. Wall-clock
event timelines are omitted from the portable core because they add privacy
and clock-synchronization ambiguity.

## Negative authority

Every forbidden authority in the selected profile has one
`negative_authority` record naming:

- the actor;
- the authority it must not possess;
- the concrete enforcement mechanism;
- the fixture IDs that attempted the forbidden behavior; and
- `pass`, `fail`, or `incomplete`.

Source layout, an unused dependency, a current file mode, or a statement that a
component “does not normally” perform an action is not enforcement. The report
must name the sandbox, credential boundary, descriptor boundary, protocol
grammar, or other mechanism actually exercised.

Profiles such as standalone same-user X11 may explicitly declare that an OS
sandbox claim is outside scope. The report records that as a limitation; it
does not invent a passing negative-authority result.

## Conclusion algorithm

The report author computes `conclusion` as follows:

- `pass` only when every required profile fixture is present and `pass`, every
  required negative-authority record is `pass`, counts match the fixture array,
  and no profile-required limitation is omitted;
- `fail` when any required fixture or negative-authority record is `fail`; or
- `incomplete` when neither failure occurred but a required fixture is missing,
  skipped, errored, or required negative-authority evidence is incomplete.

The JSON Schema enforces structure, closed fields, value bounds, unique arrays,
and that a passing fixture has evidence. Cross-record profile membership,
fixture uniqueness by ID, count arithmetic, digest verification, artifact
retrieval, and the conclusion algorithm are semantic validation performed by a
conformance tool or reviewer.

## Privacy and artifact handling

The report itself must not contain window titles, screenshots, raw input
events, key codes, pointer coordinates, device identities, credentials, home
paths, socket paths, environment dumps, or unredacted wire payloads containing
private observations. It stores bounded labels, component versions, result
states, digests, and artifact references.

Artifacts are retained separately under the report producer's disclosure
policy. Before publication, the producer removes secrets and unrelated desktop
content while preserving enough exact public behavior to reproduce the
assertion. A digest authenticates bytes only relative to a trusted report; it
does not authenticate who produced either item.

## Validation

With the Python `jsonschema` command installed, validate a report using:

```sh
jsonschema \
  --instance docs/protocol/conformance-report-v1.example.json \
  docs/protocol/conformance-report-v1.schema.json
```

Schema validation is necessary but not sufficient. A conformance reviewer also
checks the selected profile, fixture IDs, conclusion algorithm, artifact
digests, negative-authority enforcement, environment compatibility, and
registry status.

## Versioning

Report-format versions are independent from wire, profile, product, crate, and
schema-dialect versions. Adding an optional field may retain format 1 only when
format-1 readers already permit that extension; this schema is closed, so a
field addition allocates a new report format. Clarifying prose or correcting a
schema bug without changing accepted documents does not.

Digital signatures, transparency logs, remote attestation, and a public report
repository are future governance questions. They are not silently inferred
from `sha256` evidence digests.

## Standard used

The schema declares the JSON Schema Draft 2020-12 meta-schema published by the
JSON Schema project: <https://json-schema.org/draft/2020-12/schema>.
