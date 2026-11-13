# Engineering decisions

Decisions that deviate from, narrow, or interpret the mission, with reasons.

**D-1. One enum, not capability sets.** A call gets exactly one `Effect` (the most serious thing
it does), not a set of capabilities. Reason: one value makes the decision table total and
testable cell by cell; sets would need a lattice and a combination rule, and nothing today needs
two effects at once. Revisit when a tool genuinely does two (for example "read a secret and send
it").

**D-2. Clean conversations may search and fetch without asking.** `NetworkEgress` is allowed
when no outside content is present. Reason: with nothing but the user's own words in the
conversation there is no injected instruction to obey, and asking before every search would make
the product unusable. After outside content, every model-composed transmission asks, in every
mode. This is narrower than "approve all egress" and is recorded as a risk acceptance (see T2 in
the threat model, which weakens the "clean" assumption). Approved by the owner, 2026-11-13.

**D-3. The app's own buttons are not subject to the permission mode.** Adding a memory or a
reminder from the app is the user acting directly. The mode governs what the agent may do, and
what other programs may do through MCP. The cost is T9 in the threat model. Approved by the
owner, 2026-11-13.

**D-4. Addresses from tool output are never "the user's".** Stricter than the previous build,
which let any address that had appeared in the conversation be fetched. Consequence: after a
search, opening a result asks. "Allow searches for this conversation" keeps searching fluid;
opening results is asked about each time. This will be felt in research tasks and is the price
of the rule.

**D-5. Unknown MCP tools are `ExternalWrite`, trusted ones `NetworkEgress`.** A schema hash says
the description did not change, not what the code does.

**D-6. An interrupted approval becomes "unknown", not retried.** Heron cannot know whether a
half-finished action took effect; repeating it could do it twice.

**D-7. No formatting or lint gate added yet.** The baseline has 946 rustfmt diff hunks and 39
clippy warnings. Adding the gate first would mean either a red required check or a masked one.
The honest order is: format and fix in one mechanical change, then add the gate.

**D-8. Property testing by exhaustive enumeration.** The policy's input space is small
(4 × 10 × 2 × rule sets), so the invariant test tries every combination instead of sampling. A
fuzzing harness is still owed for the parsers (G3).

**D-9. Stacked pull requests.** The permission work is stacked on PR #2, which holds unmerged
work it depends on (the conversation-wide grant). Nothing is merged without the owner's say-so.

**D-10. Upgrades are one transaction, and only physical damage stops one.** Every owed step and the
new format number are committed together or not at all, so there is no half-upgraded state to
recover from. A database that fails SQLite's page and index check is not upgraded or copied. A row
that refers to a missing row is reported by `heron doctor` but does not block: the data is all
there, no step depends on it, and refusing would lock a user out of their own data over something
harmless. Steps 1 and 2 tolerate parts that already exist, because builds between 0.1.0 and the
first numbered format created some of them without a number; later steps must not need that.

**D-11. Going back is by restoring the copy, not by reverse steps.** The mission asks for
reversible migrations. Writing a tested "down" step for every "up" step doubles the surface for a
path almost nobody runs; a verified copy taken immediately before the upgrade is simpler and
restores exactly what was there. The cost: anything written after the upgrade is lost on going
back. Three upgrade copies are kept; copies made by hand or before a restore are never removed.

**D-12. Copies are not encrypted.** They are private files (0600 in a 0700 folder on macOS and
Linux; the user profile's own protection on Windows) holding the same data as the database beside
them, which is not encrypted either. Encrypted export for moving data between machines is still
owed (B5).
