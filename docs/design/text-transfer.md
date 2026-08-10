# Candidate Unicode text transfer

Status: candidate design, unimplemented and unadvertised.

`keyboard.type` and `keyboard.write` describe keyboard actions. They resolve
each scalar through direct symbols in the current effective XKB layout and
group. That is the honest contract for XTEST keystrokes, but it cannot provide
layout-independent Unicode: a Norwegian layout can expose a dead acute key
without exposing `í` as one directly resolvable symbol, for example.

Agent Seat must not work around that limit by changing the XKB layout or
mapping, guessing compose or input-method sequences, invoking another program,
or granting an agent general clipboard access. Those choices either mutate
shared desktop state or depend on application-specific acceptance.

## Candidate boundary

A future protocol revision may add a separately named text-transfer operation.
It would transfer bounded UTF-8 content to one freshly scoped, already focused
target. It would not be another spelling of `keyboard.write`, and the existing
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

The candidate may use ICCCM selection ownership internally on X11. That is an
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

## Approval gates

Implementation remains stopped until all of the following are settled:

1. Allocate a new wire revision, capability, operation, result states, bounds,
   and conformance-profile revision.
2. Approve the operator-facing clipboard-displacement and retention warning.
3. Define requestor ancestry checks for reparented and helper windows without
   accepting arbitrary same-display requestors.
4. Define bounded handling for `TARGETS`, UTF-8 text targets, property writes,
   `SelectionNotify`, selection loss, and an unresponsive target.
5. Add hostile tests for a competing selection owner, clipboard manager,
   requestor substitution, focus/seat loss at every transition, oversized
   requests, provider death, and a target that never requests the selection.
6. Prove negative authority: the MCP companion cannot read selections and the
   provider exposes no clipboard-read operation.

Until those gates pass, accented or otherwise unavailable scalars remain a
precise pre-send refusal from the keyboard operations.
