# Stage 9 fix ledger

Source: `implementation_readiness_report.md` (48 findings). Each row: the fix chosen and where it lands in `heron-v1.md`. "Plan" means the Detailed Implementation Plan; "Spec" means sections above it.

Code facts checked in this session (2026-11-18, worktree at 6d7cd2b):
- `cargo clippy --locked --all-targets -- -D warnings` fails on `main` with 57 errors, all in `src/` (most are `field_reassign_with_default`).
- `cargo fmt --check` fails on `main` (for example `examples/mock_llm.rs`).
- No `v0.3.1` tag exists, locally or on origin. CHANGELOG has `0.3.1 — 2026-11-14`, prepared in commit `9253959`, and `v0.3.0` has the same `STEPS = [to_v1, to_v2]`.
- The cargo binary is `heron`. `heron-cli` exists only inside the bundle (`build-app.sh:22`).
- `tests/integration.rs` uses `server::router(app, token)`, `Session::create(store, title)` and one `ToolCtx { .. }` literal (line 350), so those signatures stay stable.
- `mcp_client_talks_to_a_real_server_and_detects_changed_tools` (`tests/integration.rs:334`) and `server_speaks_mcp` (`mcp.rs:299`) expect `remember` to succeed over MCP.
- `/approve` decisions: `once`, `always`, `session`, anything else denies (`server.rs:526-536`).
- The demo model issues real tool calls (`llm.rs:595`).
- `genesis 0.4.0`: `genesis audit-evaluator <spec.yaml> --suite <behavioral|code|code-py|json|math>`, exit 0 = TRUSTED.
- `testteeth run --base BASE_REF [--fail-under N] [-q]`.
- `DISABLE_AUTOUPDATER` and `DISABLE_FEEDBACK_COMMAND` are set for claude-cli only (`cli_llm.rs:1240-1244`).
- `fs.rs` `read_effect` treats `cfg.home` as SensitiveRead; `search_files` skips SensitiveRead, `find_files` filters nothing.
- PRs #26 and #35 are not on `origin/main` (ad2ef31).

| ID | Fix chosen | Lands in |
|---|---|---|
| B1 | `config_dirs` and the registry table move into U1. `sessions.origin` stays in U2's `to_v3`, and U1 no longer sets it. The M-4 test splits: `m4_globs_quotes_and_unlisted_secrets_ask` (U1) and `m4_sandbox_denies_globs_quotes_and_unlisted_secrets` (U2). | Plan U1, U2; findings table |
| B2 | New unit U3a (`docs/**`) writes `docs/ACCEPTED_RISKS.md` (`Version: 1`) and the threat-model links before U3. `include_str!("../docs/ACCEPTED_RISKS.md")` from `src/risk_notice.rs`. | Plan U3a, U3; Spec §6 |
| B3 | The sandbox-off refusal applies on macOS only (`sandbox::required()`). The shell tests stop setting `sandbox = "off"`, the off-write half is split by platform, and the fake-CLI tests run under real Seatbelt. Owner question Q11. | Spec §1; Plan U2; Q11 |
| B4 | Only `read:<folder>/**` rules grant `OutsideRead`, matched by path prefix against the canonical target, never by the `{tool}:{summary}` glob. The Settings "allow" switch is refused for the four read tools. The promises test gains the broad rule sets. | Spec §1; Plan U1 |
| B5 | All 10 tasks with JSON blocks and a rule grammar; the `summary.json` schema; runner exit codes 0, 1, 2. | Plan U0 |
| B6 | A stdlib EVE adapter (`eval/eve_adapter.py`) answers approvals over `/api/approve`. The token is parsed from the serve URL line, `--provider` maps to exact config, and every route has `/api`. | Plan U0 |
| B7 | U5 leaves this plan and becomes its own design delta after Q8. verify lists its dependencies by id. | Plan; Q16 |
| B8 | New tests stay inline in `src/`; integration-test updates go to a new `tests/**` unit Ut. The `semver` crate is replaced by a small in-crate comparator, and the `spike` feature by `#[cfg(debug_assertions)]` plus `HERON_CLI_TOPOLOGY`, so there is no `Cargo.toml` edit. U0's runner has `--topology`. verify scope is `scripts/**`. U6b's `README.md` is a declared single-file exception (Q19). | Plan; Q19 |
| M1 | `Effect::Refused` (13 effects). `classify_path(path, workspace, cfg, write)` with `ReadRoots::of(cfg)` memoized; five callers listed. `read_effect` removed. Writes to Refused paths are denied (Q13). | Spec §1; Plan U1 |
| M2 | Enum diff, properties and match arms written out. | Plan U1 |
| M3 | Grant target per tool; home normalization; broad grants make `can_always` false; `add_allow -> Result`; no load-time drops; `ignored_rules` on `GET /api/settings`. | Spec §1; Plan U1; Q2 |
| M4 | Both walkers skip Refused and secret matches; test `walkers_skip_refused_and_secret_paths`. | Plan U1 |
| M5 | MCP refuses state writes (taint + Ask = refusal). The `save_automation` case is dropped; test changes listed. Q12. | Plan U1, Ut; Q12 |
| M6 | `recall` returns confirmed memories only; new `recall_unconfirmed` (untrusted). `recent_activity` defined in `personal.rs`. Q14. | Spec §1; Plan U1 |
| M7 | Call-site → origin table; `Session::create_with_origin`; `create_session(title, origin)`. | Plan U2 |
| M8 | v2 hash = v1 length-prefixed fields + `provider`, `cli_version`, each length-prefixed. `run_id` dropped from audit. `Store::audit(&AuditRow)`. Fingerprint test update. | Spec §2; Plan U2 |
| M9 | Reuse the `before-upgrade-from-v{n}` snapshot; the upgrade test checks `backups/heron-before-upgrade-from-v2-*.db`. | Spec NFR; Plan U2, U7 |
| M10 | `config.rs` item with exact fields, types and defaults (untagged enum for `cli_allow_api_key`). No `Cargo.toml` change. | Plan U2 |
| M11 | The hold is in `app.rs` and `workflow.rs`: held runs record status `held-subscription` plus an inbox item; "Run now" is the existing `POST /api/schedules/{id}/run`. `POST /api/providers/{id}/accept-version`. Tests named. | Spec §2; Plan U2, U3 |
| M12 | Registry table (config_dirs, API-key variables) with UNVERIFIED marks; Cline stays experimental; version-manager order mise, fnm, nvm. | Plan U1, U2 |
| M13 | `sandbox::wrap(cfg, workspace, &Profile)`; the spawn sites; the SBPL skeleton (last match wins, metadata reads allowed); Bubblewrap unchanged; a working folder inside a Refused path is refused; the exact hint. | Spec §1; Plan U2 |
| M14 | `McpServe { refuse_all, schemas }`, an early return before `Config::load`, `mcp::serve_refuse_all`, a schema file per turn, server name `heron`. | Spec §2; Plan U2 |
| M15 | The bus moves to `App`; background and Telegram runs publish; `run_id` is a UUID string; `/cancel` returns `{ok, cancelled}`; park origin; `/api` prefix. | Spec §3; Plan U3 |
| M16 | Gate on macOS only; tests seed with `risk_notice::accept_current(&store)`; the runner posts `/api/risk-notice`; terminal y/n in `main.rs`; the web UI shows the error. Q15. | Spec §6; Plan U3, Ut, U0 |
| M17 | `start` returns `Started { url, port, token, background_owner, task }`; loopback from the real bind; no banner; stdin-EOF branch; `/api/providers` and `/api/grants` routes. | Spec §3; Plan U3 |
| M18 | Exact SQL and decision list; `eval/owner-use.md`; requires schema v3. | Spec §7; Plan U0 |
| M19 | The testteeth base is the baseline commit from `eval/baseline/summary.json`; the per-PR claim is dropped (Q18); U1 fixes the 57 clippy errors first; trial dates come from env vars; the exact genesis command. `cargo fmt` isn't a gate. | Plan U1, verify; Verification Plan; Definitions |
| M20 | A SwiftPM package (no `.xcodeproj`); an in-repo snapshot helper using `ImageRenderer`; product `Heron`; macOS 14; a universal `swift build --arch`. | Spec §4; Plan U4 |
| M21 | `build-app.sh` steps in order; `--unsigned` keeps the ad-hoc signature; CI keychain import secrets; notarize and staple both the app and the DMG; no entitlements file. | Spec §6; Plan U7, U7b |
| M22 | Fixture script from commit `9253959`; exact upgrade and restore commands with pass conditions; release title "Research preview"; the version bump is an owner release step. | Spec §6; Plan U7, U7b |
| M23 | Threat-model files go in U3a; register format and signature line; the mode-table change in U6b; headings match the table; U8 → verify. | Plan |
| M24 | Checks run unfiltered test sets plus a `--list` presence check of named tests. | Plan table |
| M25 | `--topology` in U0's runner; "records the chosen topology and why"; link the rule. | Plan U0, U2s |
| M26 | The exact added regex alternatives. | Spec §1; Plan U1 |
| M27 | A list of existing tests to rewrite; every regression test named; names match the findings table. | Plan U1, U2, U3, Ut |
| M28 | Parse `forward_origin` (or legacy `forward_from*`) into `forwarded: bool`; wrap with `wrap_untrusted`; set taint and kv; `act_on_suggestion` taints. Re-check after #35. | Plan U1 |
| N1 | One form per command (`--locked`, `-q`, `--include-ignored`); fmt dropped. | Plan |
| N2 | G7 rule defined once (G7), referenced elsewhere; genesis stated once. | Plan; Verification Plan |
| N3 | Reworded to "Refused paths and secret-pattern matches are not indexed or observed." | Plan U1 |
| N4 | The diagram sentence and "pay off" removed. | Plan U4 |
| N5 | The entitlement line removed: no entitlements are needed outside the App Sandbox (UNVERIFIED until U7 notarizes). | Plan U7 |
| N6 | verify dependencies listed by id. | Plan table |
| N7 | Scheduling text rewritten. | Plan |
| N8 | `<run>` = a UUID per turn; the `cli_llm.rs:2363` test update listed. | Spec; Plan U2 |
| N9 | EVE YAML and baseline parameters given; EVE flag names UNVERIFIED until U0. | Plan U0 |
| N10 | Hybrid argv given, marked UNVERIFIED until U2s. | Plan U2 |
| N11 | `${TMPDIR:-/tmp}`; the U2s check validates run counts per arm; regex `feature *= *"spike"` is moot now (env var grep). | Plan table |
| N12 | Precondition: re-check cited line numbers after #26 and #35 merge. | Plan Preconditions |
