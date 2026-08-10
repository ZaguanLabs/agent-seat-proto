# Unicode text-transfer decision

Status: approved and implemented experimentally in wire revision 9. The
normative behavior is in the [wire specification](../protocol/specification.md)
and [X11 profile](../protocol/profiles/x11-text-transfer-v1.md).

`keyboard.type` and `keyboard.write` describe keyboard actions. They resolve
each scalar through direct symbols in the current effective XKB layout and
group. That is the honest contract for XTEST keystrokes, but it cannot provide
layout-independent Unicode: a Norwegian layout can expose a dead acute key
without exposing `í` as one directly resolvable symbol, for example.

Agent Seat must not work around that limit by changing the XKB layout or
mapping, guessing compose or input-method sequences, invoking another program,
or granting an agent general clipboard access. Those choices either mutate
shared desktop state or depend on application-specific acceptance.

## Approved boundary

Wire revision 9 adds a separately named text-transfer operation. It transfers
bounded UTF-8 content to one freshly scoped, already focused target. It is not
another spelling of `keyboard.write`, and the existing
`input_keyboard` capability would not authorize it.

The corresponding provider authority would be write-only and request-local:

- no operation reads or returns the existing clipboard or primary selection;
- the provider owns only the selection needed for the approved transfer;
- only the freshly scoped target may request the offered text;
- the offered MIME targets and UTF-8 byte bound are finite and public;
- seat disable, target/focus loss, timeout, selection loss, provider shutdown,
  or incomplete ownership evidence ends the transfer fail-closed; and
- the result distinguishes text offered, selection delivered, interrupted,
  and failed. It never claims that an application inserted or retained text.

The X11 profile uses ICCCM selection ownership internally. That is an
implementation mechanism, not a portable protocol promise. Other display
servers or participating applications may realize the same information-model
operation through a native, target-scoped text-transfer facility.

## Authority and visible side effects

Even write-only selection ownership is new desktop authority. Acquiring the
X11 clipboard selection displaces its previous owner. X11 provides no honest
way to restore that owner after taking the selection, and a clipboard manager
may retain transferred content. Those effects must be shown to the operator
and covered by a distinct saved grant. An implementation must not read the old
selection merely to simulate restoration.

The provider may need to send one conventional paste command after it owns the
selection. That command remains subject to the volatile seat, fresh target,
focus, and balanced-key rules. Selection delivery must additionally verify the
requestor belongs to the scoped client. A paste command alone is not evidence
that the selection was requested or inserted.

## Approval resolution

The maintainer approved the new authority on 2026-08-10. Revision 9 allocates
the separate `text_transfer` grant and feature, `text.insert`, 32 KiB/16,384
scalar bounds, and delivered/offered/interrupted results. The Settings and
first-run surfaces state the clipboard displacement and possible retention
effect before the grant is saved.

The X11 implementation accepts only a requestor with the same X-Resource 1.2
client identity as the scoped target. This admits toolkit-owned child/helper
windows on the target's X connection but refuses a helper using a different
connection; incomplete identity evidence stops the transfer. It handles only
`TARGETS` and three finite UTF-8 text atoms, writes complete properties, and
sends `SelectionNotify`. After the first text response, it remains available
for a bounded 250-millisecond quiet period so an owner-change prefetch does not
race cleanup; the existing two-second maximum still applies. A later selection
owner is never cleared during cleanup.

Isolated hostile evidence covers a displaced prior owner, an out-of-scope
selection requestor, a normal `TARGETS` negotiation, exact accented multiline
bytes, selection loss, later-owner preservation, an unresponsive target,
wire bounds, and request-local cleanup. The wire exposes no read call, the MCP
companion has no selection API or X11 text authority, and the provider never
requests prior selection contents. The exact evidence and remaining limits are
recorded in [revision 9 verification](../verification/revision9-text-transfer-verification.md).

Keyboard operations remain unchanged. They still refuse an accented or other
scalar absent from the live layout before sending; agents use `text_insert`
only when its distinct operator grant is present.
