# Second review: implementation readiness (Goldfish, 2026-11-23)

Reviewer: egm-implementation-readiness. It saw only the design doc, `eval/require-tests.sh`, and these source ranges: `src/agent.rs:613-710`, `src/app.rs:1763-1816` and `src/permissions.rs:168`.

Scope: the Pre-steps, the Work units, and the mechanisms the first resolution added:

- `RunFrame`
- `Call::refusal`
- baseline export and import
- the OQ9 gate
- two-pass recovery and leases
- the `run_parked` `resume:` path and its workspace check
- `guarded`

The Open Questions' answers were not judged.

**Counts:** P0 0, P1 6, P2 15, P3 5.

## P1 (a builder would have to make a design decision)

- **P1-1. anchor-harness sources aren't reachable by nodes.**
  - Location: files 2 to 4, and the W2, W3 and W4 rows.
  - Issue: those rows port code from `~/Documents/PERSONAL/anchor-harness` at `daed7ef`. No pre-step makes that repo readable inside a skein node's worktree.
  - Fix: copy the four source files into the repo in a pre-step, or state that the backend may read that path. List `default_deny_sockets` in §3.2.
- **P1-2. `guarded` doesn't say what happens if the intent insert or the lease fails.**
  - Location: §1.3, file 1.
  - Issue: the call could fail open or fail closed. Nothing states the error text or the audit row, and the timeout text doesn't match `src/agent.rs:687`.
  - Fix: state fail closed, the error text and the audit row, and align the timeout text.
- **P1-3. `RunFrame` and `in_run` have no struct or signature.**
  - Location: §1.2, file 7.
  - Issue: the doc doesn't specify:
    - how the frame is mutated;
    - the `run_id` type, or how the frame gets the sink and store;
    - what "ended normally" means;
    - what nested frames do;
    - the job audit id.
  - Fix: write the struct and signature, plus an end-of-run decision table: normal end, error, cancel and panic.
- **P1-4. The OQ9 gate isn't placed in the approval flow.**
  - Location: §1.4, OQ9, file 7.
  - Issue: the doc doesn't say whether the gate overrides session grants and `Always`, or what the ask text is. If the identical call is approved during the turn, the waiting approval stays `new`, and a later Run again runs the action twice.
  - Fix: place the gate after `decide`, bypassing grants and `Always`. Give the text, and say what happens to the waiting approval.
- **P1-5. `Call::refusal` can't carry the path.**
  - Location: §3.1, files 7 and 9.
  - Issue: an `Option<&'static str>` can't carry the required message. The doc doesn't list which tools override `protected`, or where the user home comes from. The check also runs twice, and `src/tools/fs.rs` isn't in the target-file list.
  - Fix: use an owned string or a typed refusal, list the tools, and add `fs.rs` to the files.
- **P1-6. The `effects` DDL isn't written, but a schema fingerprint pins it.**
  - Location: Pre-S1, §1.2.
  - Issue: a builder's own choice of DDL would become the fingerprint.
  - Fix: write the `CREATE TABLE` and `CREATE INDEX` text in the doc.

## P2

- **P2-1.** If `reopen_approval` returns false, the row stays `interrupted` forever. Pass 2 must handle that case.
- **P2-2.** Two identical interrupted calls share one approval, but the helpers assume one row per approval.
- **P2-3.** `run_parked` guards parked read walks too. It should guard only `mutates()` effects, per OD2.
- **P2-4.** The journal rows for a `resume:` rerun are undefined: their kind, name, args and label, and how they relate to the original action row.
- **P2-5.** The workspace check is underspecified:
  - whether it applies to `resume:`;
  - where it runs relative to the claim, the taint and `decide`;
  - canonical or literal path comparison;
  - what happens to `answered` when the call is blocked.
- **P2-6.** The snapshot timing is ambiguous. Does a denied, refused or waiting call count? Is the snapshot taken before `tool.call` or before the verdict? A full scan on the executor thread would block it.
- **P2-7.** The baseline JSON schema and the export and import rules aren't given: export with no baseline, import over an existing one, malformed input, and the `toolchain:changed` flag.
- **P2-8.** The tests in `egress::proxy::net` aren't listed.
- **P2-9.** W3's ordering test can't see `sandbox.rs`'s composition of the rules. Move it to W6b.
- **P2-10.** W5's dependency column is `—`, but its text says it waits for OQ1. Skein would start it at once.
- **P2-11.** The W6a row isn't conditional on OQ1. Give the alternative row.
- **P2-12.** W6b's check doesn't require W6a's tests, so a W6a regression passes W6b.
- **P2-13.** The checks prove the tests exist, not that they test anything, and the Seatbelt tests skip silently. Only `verify`'s testteeth step guards against that.
- **P2-14.** The supervisor's failure handling has no retry cap or stop rule. The label's meaning isn't stated. W4 may need `sandbox-exec`.
- **P2-15.** The `\|` table escape in the W7 and W8 checks may not be unescaped by skein. Put those checks in a script.

## P3

- **P3-1.** About 40 line numbers come from `39dbdf5` and will shift after Pre-S0. Cite function names.
- **P3-2.** `recover()` doesn't say what its count means.
- **P3-3.** `guarded`'s drop guard may find the store lock poisoned. Ignore that and log it.
- **P3-4.** The new `Profile::Shell` field types need `Debug` and `Clone`.
- **P3-5.** The `journal::` filter is a substring match. Anchor it.
