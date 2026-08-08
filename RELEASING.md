# Release process

The initial release owner is `kekePower`. A release requires:

1. a clean checkout at the supported Rust toolchain;
2. formatting, clippy, test, documentation, and integration gates passing;
3. exact crate versions and wire revisions recorded separately;
4. an updated changelog and compatibility matrix;
5. a source archive containing license, policies, and build instructions, plus
   an adjacent checksum asset; and
6. an annotated or signed tag plus a GitHub release under
   `ZaguanLabs/agent-seat-proto`.

Package publication uses project-scoped credentials or registry trusted
publishing. Personal registry tokens are never committed, exposed to pull
requests, or stored as general repository secrets. Crates remain
`publish = false` until the relevant release checklist explicitly enables
them.

No release claims compatibility from structural similarity. Compatibility
status comes from black-box testing of released artifacts through public
process boundaries.

Pushing an annotated `vMAJOR.MINOR.PATCH` tag runs the complete source gate,
assembles and inspects a version-prefixed Git archive, writes its SHA-256 file,
and creates the GitHub release. A matching
`.github/release-notes/<tag>.md` supplies curated notes when present; otherwise
GitHub's configured generated notes are used.
