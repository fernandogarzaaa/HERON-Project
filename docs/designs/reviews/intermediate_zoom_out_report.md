# Stage 8 Intermediate Zoom-Out Review: heron-v1.md

**Target:** `docs/designs/heron-v1.md` at 8778e91, full file.
**Reviewer:** `egm-zoom-out`, read only.
**Count:** 0 Blocker, 4 Major, 9 Nit.
**Verdict by area:**
- **Keep as written:** the read tiers and verdict table; the event bus and approval routes (no finding).
- **Simplify:** the shell layer, the CLI-folder lists, the spike, and the risk table.

The full wording of each finding is in this session's transcript. The table keeps one line per finding.

| # | Severity | Finding | Simplification |
|---|---|---|---|
| Z1 | Major | The shell classifier and the sandbox give contradictory verdicts. An approved `cat .env` fails in the sandbox, and the resolver duplicates what the sandbox already enforces. | With the sandbox on, any shell read that isn't Allowed is denied, with the `read_file` hint. Drop the glob expansion. With the sandbox off, shell is refused like CLI providers. Mark the Seatbelt regex assumption UNVERIFIED. |
| Z2 | Major | CLI config folders are listed three times, and `llm.cli_config_dir` is missed. | Add `CliSpec.config_dirs`, and derive Refused, the sandbox exception and the env extras from the registry plus `cli_config_dir`. |
| Z3 | Major | The untrusted-reads spike arm can't change the decision it measures, because the EVE script approves the extra asks. | Drop that arm and its `spike`-only code. Count approvals per task from the audit rows instead. |
| Z4 | Major | The Risks table adds mitigations no unit owns: a scheduled contract test, and a rerun that resumes per task. | Point to mitigations the plan builds. Make CLI-drift checking a manual pre-release step. Add `--resume` to the runner. |
| Z5 | Nit | Folder grants are stored per tool, and the sandbox honours only `read_file` grants. | Use one `read:<folder>/**` grant, honoured by all four tools and the sandbox. |
| Z6 | Nit | The sandbox lists are implementation detail, and the re-deny list duplicates the secret check. | State the principle and the test. Deny every secret-check match. The CLI child gets only the Allowed tier, its install folder and its config folders. |
| Z7 | Nit | `read_scope` sits beside the existing `classify_path`. | Extend `classify_path` instead. |
| Z8 | Nit | The risk notice duplicates the `sandbox` field in `/state`, and enforcement is spread across surfaces. | Remove the duplicate field, and add one acceptance check where turns start. |
| Z9 | Nit | The dropped-rules notice adds state for an almost empty migration. | Write one activity row instead. |
| Z10 | Nit | There are three ways to stop an orphaned core, and one could kill a core the owner started. | Keep stdin EOF only, and show "another core is running" instead of killing it. |
| Z11 | Nit | `subscription: bool` has the same value for every CLI. | Drop the field, and keep the per-provider override. |
| Z12 | Nit | The UI chain waits on the spike, though it doesn't use the spike's result. | U3 depends on U2, U2s runs alongside U3, and U2w comes after U3. |
| Z13 | Nit | The Risks table repeats itself and lists a non-goal as a risk. | Merge the two sandbox rows, drop the App Sandbox row, and point accepted risks to the register. |
