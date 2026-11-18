# Heron evaluation harness

This folder is work unit U0 of `docs/designs/heron-v1.md`. It runs Heron's ten core tasks (and the
R-1 probe) against a built core, scores them with machine-checkable pass rules, and judges the
result against the G7 task gate. Everything is Python 3 standard library, run as `python3 -I`, plus
a few bash scripts.

| File | What it is |
|---|---|
| `core-tasks.md` | The ten gate tasks `t01` to `t10` and the probe `p1`, one JSON block each (Q17). |
| `fixtures/` | A `work/` tree per task that has files, and `web/` with the pages the fixture web server serves. |
| `run-core-tasks.sh`, `run_core_tasks.py` | The runner. |
| `pass_rules.py` | The pass-rule grammar: `validate_rule` and `evaluate`. |
| `eve_adapter.py`, `eve/heron-core.yaml` | The bridge from EVE to Heron's `/api/chat`. |
| `check_tasks.py` | Structure check for `core-tasks.md`. |
| `check_gate.py` | Schema check, the G7 gate, and the topology decision for the spike. |
| `require-tests.sh` | Fails if a named Rust test was renamed away from a check. |
| `owner-use.sh`, `owner-use.md` | The G9 day count and the trial record. |
| `genesis.yaml`, `genesis_verifier.py` | The evaluator audit spec and the grader it audits. |
| `containment/discover-dirs.sh` | Owner-run helper that finds a CLI's config folders (for U2c). |
| `tests/` | `python3 -I -m unittest discover -s eval/tests -t eval` |

## Running the tasks

    bash eval/run-core-tasks.sh --provider <demo|cli-id> --out DIR [options]

| Option | Meaning |
|---|---|
| `--provider` | `demo` (the built-in canned model) or a CLI provider id from `heron providers`, such as `claude-cli`. |
| `--runs N` | Runs per task. Default 3, the G7 sizing. |
| `--out DIR` | Folder that gets `summary.json`. |
| `--driver eve\|direct` | How the turn is sent. Default `eve`. |
| `--topology envelope\|hybrid` | Default `envelope`. `hybrid` needs `--build debug`. |
| `--build debug\|release` | Build with `cargo build --locked` (plus `--release`). Default `release`. |
| `--heron-bin PATH` | Use this binary and skip the build. |
| `--artifacts DIR` | Per-run folders (home, working folder, logs). Default `DIR/runs`. |
| `--tasks t01,t02,...` | Only these ids. The default is every task, the probe included. |
| `--resume` | Skip runs already in `summary.json` without an error. A summary made with other settings is refused. |

Each run gets a disposable Heron home and working folder, a copy of the task's fixture, a fixture web
server on 127.0.0.1 (which logs request paths to `web.log`), and its own `heron serve --no-open
--port 0` with `HERON_HOME` and `HERON_ALLOW_LOCAL_FETCH=1` set. The runner reads the port and
token from the URL the core prints, applies `seed_sql`, accepts the risk notice, sends the prompt,
counts `asks` from `audit` rows, stops the core, and evaluates the task's pass rule. A run that
errors is retried once.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Every requested run executed and `summary.json` was written. Failed pass rules still exit 0: judging them is `check_gate.py`'s job. |
| 1 | At least one run still has an `error` after the retry (the build failed, the core did not start, the CLI failed, or EVE failed). The summary is written with those runs marked. |
| 2 | Usage error: bad flags, an unknown task id, an unknown provider, `hybrid` without a debug build. |
| 3 | The `eve` driver can't run at all. Nothing is run. |

### `summary.json`

Schema 1, as in the design doc. `check_gate.py --schema-only` validates it, including that
`totals` match the runs. A run holds `pass`, `failed_rules`, `reply` (first 2000 characters),
`tokens`, `ttft_ms`, `asks` and `error`. `ttft_ms` is `null` when no text event arrived. The medians
cover the runs of gate tasks whose `error` is null.

## The two drivers

**`direct`** calls `eve_adapter.turn()` in the runner's own process. It is what U0's check uses, and
it needs no EVE.

**`eve`** starts `eve_adapter.py` as a small HTTP server and runs `eve chat` against it:

    eve chat http://127.0.0.1:<port>/turn --kind assistant --persona first-time-user \
        --goal "<the task prompt>" --turns 1 --reply-path reply \
        --body '{"message": "{{message}}"}' --out <run>/eve-out --quiet

`eve/heron-core.yaml` holds those values. **`eve chat` 0.5.3 has no config-file flag**, so the file
is not handed to EVE: the runner reads it (flat `key: value` lines, `${HERON_ADAPTER_PORT}` and
`${HERON_TASK_PROMPT}` expanded from the environment) and builds the argv above. What was verified
against EVE's source and a real run:

- With `--turns 1` EVE sends exactly one message, and it is the `--goal` text unchanged (EVE's
  conversation policy opens with the goal). The runner treats any other number of recorded turns as
  an error.
- EVE reads the reply at the path `reply`. The adapter answers `{"reply": ...}`.
- **EVE's HTTP backend gives up on a reply after 30 seconds** and no flag changes that. A real CLI
  turn can take longer. The adapter keeps waiting for Heron (up to 600 seconds) and records the
  result; the runner waits for that record after `eve chat` exits. So `reply`, `tokens`, `ttft_ms`
  and the pass rule never depend on EVE's 30 second limit. EVE's own report for such a run says the
  assistant did not respond, which is EVE's view of the wait and is not used.
- `ttft_ms` comes from the adapter (POST to first `text` event), not from EVE.
- EVE's exit status reflects its findings about the conversation, so only status 2 (it rejected its
  arguments) is treated as a failure.

If `eve` is not on PATH, or `eve chat --help` lacks one of the flags above, the `eve` driver exits 3
and the owner decides whether `direct` is the driver of record. G7 and the baseline need `eve`.

## Pass rules

`pass_rules.evaluate(rule, ctx)` returns `(passed, failed_rules)`. The grammar is in `core-tasks.md`.
Malformed rules raise `RuleError`. `sql_at_least` opens `heron.db` read-only, and a second SQL
statement is an error. File paths must stay inside the working folder.

## Validators

- `python3 -I eval/check_tasks.py eval/core-tasks.md` exits 0 or 1.
- `python3 -I eval/check_gate.py --schema-only <summary>`: 0 valid, 1 breaks schema 1, 2 not JSON.
- `python3 -I eval/check_gate.py <summary>`: the G7 rule. Needs `runs_per_task` 3, `driver` `eve`,
  `mode` `ask`, `build` `release` and a provider that is not `demo` (it names the field and exits 1
  otherwise), then 26 of 30 gate runs with no task below 2 of 3. Prints a per-task table with median
  time to first token, tokens and asks. Exit 2 if the file is malformed or breaks schema 1.
- `python3 -I eval/check_gate.py --spike <envelope> <hybrid> (--report F | --write-report F)`: both
  arms need `runs_per_task` 5, a debug build, `claude-cli`, the same commit, the matching topology
  and only `t01` to `t10`. Hybrid wins only with a lead of at least 4 runs of 50 and at most 5% more
  median tokens per task. `--write-report` writes `Chosen default: topology=<value>`, a table per arm
  and the reason; `--report` fails if its first line disagrees with the summaries.

## G9 owner-use count

    bash eval/owner-use.sh ~/.heron 2026-12-01 2026-12-14 --min 12

Needs data format 3 (U2). With `--min` the window must be exactly 14 days. `owner-use.md` is the
trial record.

## The genesis audit

    genesis audit-evaluator eval/genesis.yaml --suite code-py     # exits 0 when TRUSTED

Run it from the repository root. genesis 0.4.0's `code-py` suite probes a *code verifier*: it hands
over a task (a prompt, visible tests, hidden tests) and a completion, and tries to get a bad one
accepted by spoofing output, exiting early, hard-coding the visible tests, faking a pytest run or
blocking forever. The grader is `genesis_verifier.py`, which runs the completion in a child process
under a time limit, adds a canary test that must fail, writes what it saw into an evidence folder
the candidate can't reach, and gives the final verdict to `pass_rules.evaluate`.

That bounds what the audit shows. It shows that the verification harness around `pass_rules.evaluate`
resists those probes. It does not probe the rule grammar itself; each rule kind's matching and
non-matching cases are in `tests/test_pass_rules.py`. genesis reports six `[brittle]` stress findings
(case and whitespace variants of the gold solution are rejected); that is expected for Python source
and does not affect the verdict.

genesis passes the same grader its task in three shapes (tests at the top level, under `context`, or
as JSON text inside `input`) and the candidate as `{completion_file}` or `{output_file}`, whichever
placeholder it fills in. `genesis_verifier.py` accepts all of them. `genesis.yaml` writes `metrics`
as a block list because genesis's YAML reader rejects `[task_success]`.

## `require-tests.sh`

    bash eval/require-tests.sh --lib -- some_test other_test

Runs `cargo test --locked --lib -- --list` and exits 1, naming each test that is not listed. A name
matches only the last `::` component of a listed test, exactly. Exit 2 is a usage error.

## `containment/discover-dirs.sh`

    bash eval/containment/discover-dirs.sh <id>

Runs the registry's `login_command` under a fresh temporary `HOME`, then writes every path the login
created, as `~/...`, to `eval/containment/<id>.dirs.json`. Some `login_command` strings are prose
(`agy (then sign in with Google)`, `cline auth <provider>`): a trailing parenthetical is dropped, and
a `<placeholder>` makes the script exit 2 until you give the real command in `LOGIN_COMMAND`.

## Running in a sandbox

The runner, the adapter tests and the fixture web server bind loopback ports, so a sandbox that
blocks binding (Claude Code's default) makes every run fail with `PermissionError`. Allow
`sandbox.network.allowLocalBinding` or run outside the sandbox.

## Risk notice

Step 8 of a run reads `GET /api/risk-notice` and posts `{"version": n}` back to `POST
/api/risk-notice`, the contract in the design doc's U3 section. Today's `main` has no such route and
answers 404, which the runner skips.
