# 04. Mutation analysis

## Tool

testteeth 0.2.0 (the owner's tool) driving cargo-mutants 27.1.0 (testteeth's Rust engine), 4
workers, default operators, full `cargo test` as the kill test. Run on clean `git archive`
exports, never on a working tree. Host macOS 27.2 arm64, rustc 1.99.1.

## Scope

| Module | Mutants listed | Run |
|---|---|---|
| `src/permissions.rs` (policy table, path and shell classification, SSRF guard) | 241 | **Yes, unsampled, twice** |
| `src/telegram.rs` | 64 | NOT_RUN (time) |
| `src/connectors.rs` | 82 | NOT_RUN (time) |
| `src/config.rs` (credential storage) | 77 | NOT_RUN (time) |
| `src/channel.rs` (PR #35) | 81 | NOT_RUN (time) |
| `src/oauth.rs` (PR #26) | 107 | NOT_RUN (time) |
| `src/tools/fs.rs`, `src/store.rs`, others | 147, 783, … | NOT_RUN |

`permissions.rs` went first because every tool call passes through it. Each full run took about
30 minutes on this machine.

## Results for `src/permissions.rs`

Denominator: 223 viable mutants (241 listed, 18 did not compile and are excluded).

| Revision | Killed (incl. timeout) | Survived | Score | Evidence |
|---|---|---|---|---|
| `main` + #39 (`18e798f`) | 154 | 69 | **69.1%** | `evidence/mutation/permissions-before-18e798f.{json,txt}` |
| PR #41 (`19283ba`, tests added by this run) | 196 | 27 | **87.9%** | `evidence/mutation/permissions-after-19283ba.{json,txt}` |

Compared by line, column and operator: 43 mutants changed from survived to killed. One went the
other way (484:54, below). Cause undetermined: besides the added tests, the runs differ in
base (#39 in or out), `TZ`, and whether `OPENROUTER_API_KEY` was set.

The proposed gate was 80% for designated critical modules. `permissions.rs` passes it **with PR #41
merged** and fails it on `main` today. The other critical modules were not measured, so the gate
as a whole is **NOT MET**.

## Surviving mutants, by risk

### Killed by PR #41 (high risk before)

| Function | Before | After | What was missing |
|---|---|---|---|
| `simple_command_is_readonly` | 44/52 | 52/52 | git options before the subcommand (`--config-env=core.pager=…`), `--ext-diff`/`--exec`/`--upload-pack`/`--textconv`, clock-setting `date` forms. Each could let a command that starts another program count as `LocalRead`, which is allowed without asking. |
| `classify_shell` | 10/17 | 17/17 | Boundary tests for the fail-closed limits (10 links, 1,500 characters). |
| `is_private_ip` | 29/60 | 56/60 | Any test of CGNAT, 0/8, 192.0.0/24, 198.18/15, 240/4 and several IPv6 ranges. |

### Still surviving: investigated

| Location | Mutant | Assessment |
|---|---|---|
| `permissions.rs:258` | delete `(Mode::ReadOnly, _)` arm in `decide_in` | Equivalent. Step 3 already denies every mutating effect in ReadOnly, so the arm is unreachable. Candidate for deletion as dead code. |
| `permissions.rs:227` | `==` → `!=` in the deny-rule message | Low: changes only which message text is shown. |
| `permissions.rs:618` | `is_unspecified() \|\| is_broadcast()` → `&&` | Equivalent: 0.0.0.0 and 255.255.255.255 are caught by later arms. |
| `permissions.rs:633`, `:648` (×2) | IPv4-compatible and Teredo conditions | Medium: IPv6 forms that embed an IPv4 address. Needs targeted addresses; not done. |
| `permissions.rs:484:54`, `:486` (×2) | `names_sensitive_path` splitting/skip conditions | 486 makes the check stricter (low). 484:54 stops splitting on `=`. A test with `sort --random-source=~/.ssh/id_rsa` still passed under the mutant, so another check flags it; no killing input found in a short attempt. Open. |
| `trusted_web_proxy_enabled`, `web_goes_through_proxy`, `local_web_fetch_allowed`, `check_url:727` | 0–67% | **Medium-high**: these read `HERON_TRUST_WEB_PROXY`, `HERON_ALLOW_LOCAL_FETCH` and `HTTPS_PROXY` and decide whether the SSRF guard is bypassed. Testing them needs process-wide environment changes, which race with parallel tests; they need a test seam (pass the environment in). Open. |
| `Effect::reads_only` | 0/2 | Medium: decides whether tool calls may run in parallel (`agent.rs:393`), not whether they are approved. |
| `Policy::deny_rules`, `tool_setting`, `set_tool_setting`, `Effect::label` | low scores | Low: settings display and labels. |

testteeth also lists 10 heuristic "failure-path gaps" (`.unwrap()` on lock poisoning and similar)
in the JSON. They are not mutants and are not counted.

## Not done

Mutation testing of the credential, messaging, connector and OAuth modules. Commands to run
next, each on a clean export with `TZ=UTC` and `OPENROUTER_API_KEY` unset until #39 merges:

```bash
testteeth run src/telegram.rs src/connectors.rs src/config.rs --lang rust --workers 4 --json telegram-connectors-config.json
```
```bash
testteeth run src/channel.rs --lang rust --workers 4 --json channel.json
```
```bash
testteeth run src/oauth.rs --lang rust --workers 4 --json oauth.json
```

The last two must run on the PR heads merged with #39.
