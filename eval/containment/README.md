# Containment check (work unit U2c)

Measures each installed candidate CLI against the containment check in `docs/designs/heron-v1.md`
(§2, item 31) and records one `<id>.json` per candidate. G4 is judged later, by U2w.

| File | Role |
|---|---|
| `discover-dirs.sh` | Owner-run, once per installed candidate before U2c: logs in under a fresh `HOME`, writes `<id>.dirs.json`. |
| `run.sh` | Writes `<id>.json` for claude-cli, opencode-cli, cline-cli, codex-cli, antigravity-cli, gemini-cli. |
| `results.py` | Finds binaries (as `resolve_binary` does, plus `~/.opencode/bin`) and builds the result records. |
| `check.py` | `--recorded` (U2c's check), default (G4), `--list-tier1`. |
| `test_containment.py` | `python3 -I -m unittest discover -s eval/containment -t eval` |

Run `bash eval/containment/run.sh` on the owner's Mac, outside any sandbox, with each candidate
logged in. It spends one turn of each subscription. The real canary needs Seatbelt, so it fails
inside another sandbox (and is recorded as `failed`, with the error).

## Result statuses

- `not-installed`: no binary found. `unavailable`: `<id>.dirs.json` has `unavailable_reason`; nothing runs.
- `passed` / `failed`: from `canary_real_cli` (`src/cli_llm.rs`). `passed` needs the canary's own
  verdict and all three of `containment.tools_off_canary`, `containment.child_env_is_exactly_the_allowlist`
  and `containment.write_outside_run_folder_fails` to be `true`. The raw report is kept under `canary`.
- `run.sh` exits 1, and writes no result for that CLI, only when the canary produced no report at all
  (cargo failed to build or run). A stale `<id>.json` is deleted before each run.

## Observations

`keychain_login` and `codex_nested_sandbox` are `ok` or `failed` only when the canary's one turn shows
it (a turn that ran inside Seatbelt, or an error naming the login or the sandbox); otherwise
`unknown`. `oauth_refresh` and `tcc_prompts` are always `unknown`: the canary can't see them, so a
person has to watch for TCC prompts during the run and note them in the release record.

## Config folders

`config_dirs` copies the paths from `<id>.dirs.json` (`"unconfirmed"` when it is missing).
`config_dirs_confirmed` follows the design rule: a Keychain record counts `paths` and `known_paths`
together as confirmed; a home-folder record needs `login_completed: true`; a file without those fields
counts as home-folder and completed. Shared parents (`~/.cache`, `~/.config`, ...) are copied as
listed. U2w drops them.
