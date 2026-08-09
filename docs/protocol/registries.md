# Agent Seat registry custody

Status: repository pre-RFC policy, 2026-08-09. No external registry authority
or standards venue has been selected.

## Published registry view

[`registry-v1.json`](registry-v1.json) is the machine-readable publication view
of names allocated by the current repository specifications. It is deliberately
hand-reviewed data, not generated Rust source and not a runtime configuration
file.

The source hierarchy is:

1. an immutable released wire specification defines wire meaning;
2. an approved profile or extension specification defines profile meaning;
3. the registry projection records those allocations for tools and reviewers;
4. implementation enums and tests realize one selected wire revision.

If the projection and a released specification disagree, the specification
wins and the projection is corrected. An implementation cannot use the JSON
file to accept an atom absent from its selected revision. The repository does
not generate protocol code from the projection, so changing the file alone
cannot change runtime grammar.

## Projection format

The top-level `format` value is `agent-seat.registry-set/1`. A consumer that
does not recognize that exact value stops rather than guessing. The file
contains:

- protocol identity and historical wire-revision allocations;
- portable conformance-report format allocations;
- the repository-reserved core namespace;
- exact revision-5 message, capability, feature, backend, assurance, call,
  reply, event, state, action, result, error, and retry atoms; and
- separately allocated profile and extension records. The first profile is
  `agent-seat.x11-ewmh-core.v1`; no extensions are allocated.

Array order is significant only when a registry entry declares
`canonical_order: true`. Other arrays are published deterministically for
review but do not create a wire ordering requirement.

Every atom is lowercase ASCII using the punctuation fixed by its binding.
Names are compared byte-for-byte. A consumer cannot case-fold, normalize,
abbreviate, or infer aliases.

## Allocation and immutability

Until an external registry is approved, this repository allocates names by a
reviewed specification change. An allocation records its owning specification,
status, compatible revisions, semantics, bounds where applicable, authority or
privacy effect, and failure behavior.

Once a wire revision is released:

- its numeric identifier and every allocated spelling remain reserved;
- a spelling cannot be reused for different semantics;
- an incompatible grammar, bound interpretation, or semantic change allocates
  a new wire revision;
- withdrawing an extension reserves its names permanently; and
- implementation or crate versions do not alter registry identity.

Names beginning `agent-seat.` are reserved for core and repository-approved
profiles. Independent experimental extensions use a collision-resistant
reverse-domain namespace controlled by their author. Registration does not
grant authority, imply implementation, or establish profile conformance.

## Review transaction

A registry change is complete only when one review transaction updates:

1. the owning normative or provisional specification;
2. the machine-readable projection;
3. the selected wire binding when wire grammar changes;
4. public strict-decoding and hostile fixtures;
5. compatibility and security documentation; and
6. the revision or extension maturity ledger.

Generated protocol artifacts are not committed. A future publishing job may
render equivalent formats from the reviewed projection, but those outputs are
reproducible views and never allocation authority.

## External custody

Moving custody to an IETF, freedesktop.org, or other community registry needs
an explicit maintainer/community decision. That decision must define the
change controller, expert-review policy, dispute process, archival guarantees,
namespace migration, and how already released repository allocations remain
stable. This repository claims no such external endorsement today.
