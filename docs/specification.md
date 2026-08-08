# Specification structure

Status: E0 outline. No wire revision is assigned and no compatibility is
claimed yet.

The public specification will be completed in this order:

1. transport, local peer identity, frame length, and allocation bounds;
2. exact advertisement grammar and per-screen X11 provider ownership;
3. opening handshake, revision equality, assurance, backend features, and
   grants;
4. typed identities, requests, responses, errors, retry guidance, and strict
   unknown-field behavior;
5. bounded observation snapshots, cursors, diffs, and resynchronization;
6. EWMH management requests with distinct refusal, stale, unsupported, sent,
   observed, timeout, and failure outcomes;
7. controlled desktop-entry discovery, launch policy, spawn result, and
   qualified correlation; and
8. static MCP mapping and lazy provider discovery.

The first revision decision compares every public behavior against released
implementations using process-level evidence. An incompatible contract receives
a new revision. Optional capture, input, and accessibility profiles cannot add
implicit behavior to the core revision.
