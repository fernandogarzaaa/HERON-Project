# Heron Harness2: self-healing, self-evolution, connectors and observation

*   **Author:** fernandogarzaaa (drafted with Claude)
*   **Status:** Draft, for the owner's approval. Goldfish review: [docs/designs/reviews/heron-harness2/](reviews/heron-harness2/)
*   **Date:** 2026-11-22
*   **Builds on:** [heron-v1.md](heron-v1.md) (merged to `main` as `f732b4e`) and [heron-v1-features.md](heron-v1-features.md) (approved, being built)
*   **Read at:** `main` = `39dbdf5`

--------------------------------------------------------------------------------

## Overview

The owner asked for these, each a Settings switch that is off by default:

*   **Self-healing.** When the owner reports a bug, or Heron sees one of its own failures, Heron writes the fix and a regression test, checks it, rebuilds itself and reinstalls itself in place at `/Applications/Heron.app`. The owner chose full autonomy for this loop, not proposal-only (OD1).
*   **Self-evolution.** A scout runs every 24 hours, 72 hours or week (the owner picks). It reads AI news and trending open-source repositories and lists ideas that could improve Heron.
*   **Everything is in scope, including new connectors and integrations (OD2).** Heron may write a new connector itself. Structural rules keep a self-written connector inside Heron's safety model ([§7](#7-self-written-connectors-and-integrations)).
*   **Observation and workflow learning.** Separate switches for:
    *   the screen;
    *   app use;
    *   browser history;
    *   keystrokes (OD3);
    *   files outside the working folder;
    *   learning the owner's workflow.

    Raw observations stay inside the owner's **sealbox**, and only gated, derived summaries reach Heron ([§8](#8-observation-and-workflow-learning)).

Most of this doc is the set of rules that decide when a self-change may install itself and when it must wait for the owner. Those rules are proposals. The owner approves or changes them in review ([Owner questions](#owner-questions)).

Four facts shape the design, and the owner should weigh them before approving:

1.  **A sound fence leaves almost nothing to auto-install today.**
    *   Security code is spread across most core files.
    *   The Mac app's stores and its core client can call `setMode`, `removeNever`, `acceptRiskNotice` and `approve` (`macos/HeronMac/Sources/HeronCore/CoreClientProtocol.swift`), so they are security code too.
    *   Under the proposed fence, **0** of the last 30 `fix:` commits would have been eligible to install themselves. A file-level fence would have allowed 23 of 30, but those include Telegram credential and pairing-revocation fixes.

    So, for now, "full autonomy" means Heron writes, tests and opens a pull request on its own. HQ22 therefore builds auto-install in a second phase, once the replay suite and the first month of self-heal PRs show how many fixes would qualify.
2.  **The gate runs code a model wrote, outside any sandbox.** Heron's tests can't run under a nested Seatbelt sandbox. The repository's memory notes record about 27 cargo test failures inside a sandboxed shell, and `swift build` failing there. This design accepts that only for runs whose every input is trusted (HQ2).
3.  **v1's full gate can't gate a self-fix.** `scripts/verify-v1.sh` needs 33 G7 runs on each Tier 1 CLI and the 14-day G9 trial. It also throws away G7 results after any change under `src` (`scripts/verify-v1.sh:155`). The self-heal gate is smaller. It loosens no v1 threshold, and [What the gate does not prove](#what-the-gate-does-not-prove) lists what it leaves out. The PR into `main` still gets the full gate.
4.  **sealbox can keep raw observations sealed, but it can't summarise them in words.** Its design lets text out only as counts and filtered aggregates. It has no model inside the vault, and sending raw text to a model provider would break the containment the owner chose. Heron will know that the owner spends 20 minutes on invoice emails every weekday morning. It will not know what the emails said ([§8](#8-observation-and-workflow-learning)).

## The Problem

A bug the owner hits in Heron waits until a coding session finds time for it. The fix then goes through a branch, a pull request, a build and a manual reinstall. The owner wants Heron to close that loop on the owner's Mac. The owner also wants Heron to keep up with agent tooling without anyone reading the news by hand, to add integrations it needs, and to work from full context about how the owner works.

What makes this hard: an agent that can change and reinstall itself, and that sees everything typed and shown on screen, is the strongest attack target Heron has.

*   If outside content can steer a self-change, prompt injection becomes persistent code execution on the owner's Mac.
*   It also becomes a public commit, because `fernandogarzaaa/HERON-Project` is a **public** repository.
*   If observation leaks, it leaks passwords, private messages and banking sessions.

Heron v1's standing decisions already say:

*   a tainted conversation's grants never reach the shell;
*   "never" rules are lifted only in the app;
*   memory never trusts outside content.

Harness2 keeps all three, including for code Heron writes into itself and for everything it observes.

Earlier research, summarised in [Additional Resources](#additional-resources), found that the reusable parts are in the owner's own repositories: anchor-harness's version store and promotion rule, and APE's handling of lessons as untrusted. Google's Harness² isn't a drop-in:

*   it tunes a harness for one task at a time;
*   its executors bypass approvals and the sandbox;
*   Heron's 11-task G7 suite is too small to tune on.

## Goals

*   **G-H1.** An owner-reported bug becomes a pull request into `main` with no further owner action. The PR holds a fix, a regression test that fails on the old code and passes on the new, and a gate report.
*   **G-H2.** A fix that touches a fenced path, or that comes from any untrusted input, never installs itself.
*   **G-H3 (Phase 2).** A trusted, auto-eligible fix installs itself. A failed health check reverts it and turns auto-install off until the owner turns it back on.
*   **G-H4.** Every self-change is a commit on its own branch with a PR into `main`. Installed builds are recorded with their commits.
*   **G-H5.** The scout runs on the chosen schedule. It uses Heron's existing safe fetcher on an allowlist of hosts and writes inbox notes. It never changes code, memory or settings.
*   **G-H6.** Heron's repair agent fixes at least 6 of the 10 historical bugs in the replay suite before the self-healing switch can be turned on (HQ10).
*   **G-H7.** A preview user's "Report a problem" sends nothing by itself.
*   **G-H8.** A self-written connector can't escape these properties, whatever its code does:
    *   its content taints;
    *   its memories start unconfirmed;
    *   every call goes through the decision function;
    *   it never sees a secret's value;
    *   it reaches only the hosts its manifest lists.
*   **G-H9.** Observation:
    *   raw observations never leave the sealbox vault;
    *   only gated, derived results reach Heron, as outside content;
    *   password fields and Secure Input are never captured;
    *   an indicator and a one-click pause are always visible;
    *   "Forget everything observed" deletes the vault, its key and everything derived from it.

## Non-Goals

*   **A Harness²-style optimizer.** It is deferred until a suite of at least 50 tasks exists ([Future Improvements](#future-improvements)).
*   **Self-healing on preview users' Macs.** They have no toolchain, and their changes would fork the code ([§6](#6-preview-macs)).
*   **An auto-updater for preview users.**
*   **Reading GitHub issues, PR comments or CI logs as triggers.** The repository is public, so they are outside content.
*   **Changing any v1 threshold:** G7's 26/30, the 2/3 per-task minimum, testteeth's 70, or the containment check.
*   **Self-changes to Harness2, the fence, the gate, observation code or any trust table.** These are always fenced.
*   **New dependencies.** The two new workspace crates use only crates already in `Cargo.lock`. sealbox is an external tool the owner installs.
*   **Observation leaving the user's Mac**, apart from derived results inside prompts to that user's own model provider. There is no remote observation of another person's Mac.
*   **Changing sealbox's protections.** Heron needs only additions to sealbox (precondition P-4).

## Definitions

| Term | Meaning here |
|---|---|
| Builder Mac | A Mac where `[harness2] repo` points at a checkout of `fernandogarzaaa/HERON-Project`, and `git`, `gh` (logged in), `cargo` and `swift` are found. In practice, the owner's Mac. |
| Trigger | What starts a repair: an owner report, a failure record, or an owner's yes to a connector offer. Scout ideas, preview reports and workflow suggestions are untrusted and never start one ([§1](#1-trust-which-inputs-may-start-a-repair)). |
| Failure record | A row Heron's own code writes when it fails. It holds only `kind`, `location`, `version`, `commit`, `count` and `origin_tainted` (§1). |
| Trusted run | A repair whose every model-visible input is trusted. Only trusted runs exist in this design. |
| Fence | The compiled-in rules that decide whether a diff is **auto-eligible**. It denies by default. |
| Self-heal gate | `scripts/verify-selfheal.sh`: the checks a self-fix needs before its PR, and in Phase 2 before it installs. |
| Phase 1 / Phase 2 | Phase 1 builds everything except installing; every trusted run ends as a PR. Phase 2 adds auto-install, once its entry criteria hold (HQ22). |
| Self-written connector | A connector in the new crate `crates/heron-ext`, built against `crates/heron-ext-api` and registered by the core's fenced adapter (§7). |
| Observer | App-side code that captures one kind of observation (§8). |
| Observation vault | A sealbox vault at `~/.heron/observe/vault`, encrypted with a key held in the macOS keychain. |

## Owner decisions recorded in this doc

*   **OD1 (2026-11-22).** Full autonomy for trusted self-heal runs, not proposal-only. HQ22 proposes building it in two phases. The decision is unchanged; only the order changes.
*   **OD2 (2026-11-22).** Self-healing and self-evolution apply to everything, including adding new connectors and integrations. A connector from a trusted trigger may install itself under the same loop. Scout-found integrations stay proposals. Changes to trust tables stay fenced and need owner review.
*   **OD3 (2026-11-22).** Keystroke capture stays in, as its own switch, contained by sealbox.
    *   **What it overruled.** The earlier recommendation was to log no raw keystrokes, because a keystroke log holds passwords, 2FA codes and private messages, and to derive workflow from app switches, window titles and file events instead.
    *   **The owner's reason.** v1.x is a research preview testing how a personal agent works when it has full context, so accuracy from rich context is the goal.
    *   **What stays.** Raw keystrokes stay inside sealbox. Secure Input and password fields are never captured. Only gated, derived results come out, and they count as outside content.

## Owner questions

Every question has a proposed default, which the build uses unless the owner changes it.

*   **Shape decisions:** HQ1 to HQ5 and HQ22.
*   **Tuning:** HQ6 to HQ11.
*   **Connectors:** HQ12 to HQ15.
*   **Observation:** HQ16 to HQ21.

*   **HQ1, how wide the fence is.** **Proposed: the allowlist in [§3](#3-the-fence-what-may-install-itself).** It allows:
    *   the design tokens and the avatar in the Mac app, which can't reach the core client;
    *   three core files with no security role;
    *   new tests;
    *   self-written connectors under rule R6.

    Under it, 0 of the last 30 fixes would have been auto-eligible.
    *   Mac app views are left out, because any view can call a store method that changes the permission mode or removes a "never" rule.
    *   A file-level fence that fences only the known security files would have allowed 23 of 30, including `telegram.rs` pairing and credential fixes.
    *   Widening safely means splitting the app's views from the stores first ([Future Improvements](#future-improvements)).
    *   *These 30 commits are mostly Telegram and skills hardening, so the share for everyday bugs may differ. Phase 1 measures it.*
*   **HQ2, running model-written code unsandboxed.** The self-heal gate runs `cargo test`, `swift test` and the new regression test outside any sandbox, because those tests fail under nested Seatbelt. **Proposed: accept this for trusted runs only, and write it up in `docs/engineering/SECURITY_THREAT_MODEL.md`, not as a new `## R-` entry.** `docs/ACCEPTED_RISKS.md` is the risk notice every preview user accepts (`src/risk_notice.rs:9`). A new entry would make every preview user accept again, for a risk only the builder Mac carries. It is also capped at 3 entries (`scripts/verify-v1.sh:248-249`).
*   **HQ3, unattended runs and the Q6 hold.** v1's Q6 rule is that unattended work never calls a subscription model unless `allow_unattended_subscription` is set (`src/app.rs:1510-1513`). Failure records and scout runs are unattended. **Proposed: Harness2 obeys the hold.** A held failure record becomes the inbox note "Heron found a bug in itself. Fix it?", and the owner's click starts the run. A held scout run waits until the app is next open. An owner report is attended and runs at once.
*   **HQ4, failures in tainted sessions.** A failure record holds only fields Heron's own code produces, but an attacker could have caused it on purpose. **Proposed: the run is trusted, and its result is never auto-eligible. It always ends as a PR.**
*   **HQ5, the signing identity (Phase 2).** An ad-hoc signature is a hash of the binary, so after each ad-hoc rebuild macOS may treat Heron as a new app, and privacy permissions (TCC) and keychain access could reset. **Proposed: Phase 2 signs self-fix builds with the same Developer ID the release DMGs use, without notarization.** It uses `build-app.sh`'s existing default mode, with `CODESIGN_IDENTITY` set (`packaging/macos/build-app.sh:10`, `:97`). Prototype P2 checks that the grants survive a switch between a release DMG and a self-fix build, in both directions.
*   **HQ6, which model repairs.** **Proposed: the active Tier 1 CLI (claude-cli today), with its default model, through Heron's normal CLI path. codex-cli is the fallback when the first is at a limit.**
*   **HQ7, caps.** **Proposed:**
    *   3 attempts per trigger;
    *   40 steps and 500,000 tokens per attempt;
    *   3 repair runs per 24 hours;
    *   Phase 2: 2 installs per 24 hours, and no auto-install while an installed self-fix is still unmerged;
    *   the scout: 60 fetched items and one model call of at most 50,000 tokens per run.

    Every run also counts against the usage budget (`src/usage.rs`).
*   **HQ8, scout sources and schedule.** **Proposed sources:**
    *   **GitHub's search API:** repositories with the topic `llm-agent`, `ai-agent`, `agent-framework` or `mcp`, pushed in the last 7 days, sorted by stars. GitHub has no API for trending repositories.
    *   **Hacker News (Algolia API):** stories with at least 100 points that match "agent", "LLM" or "MCP".
    *   **arXiv:** `cs.AI`, `cs.CL` and `cs.SE`, with "agent" in the title.
    *   **Lab feeds:** the news feeds of Anthropic, OpenAI and Google DeepMind.

    **Proposed default schedule: weekly.**
*   **HQ9, what goes into a public PR.** **Proposed:**
    *   the PR body holds the gate report, the changed files and a description written from the diff;
    *   the owner's report text goes in only when the owner ticks "OK to publish this report" (off by default);
    *   a redaction scan runs over the diff and the PR text before any push, and a hit stops the push.
*   **HQ10, the replay threshold.** **Proposed: 6 of 10.** Below it, the self-healing switch stays disabled, and Settings says why.
*   **HQ11, the rollback copy (Phase 2).** **Proposed: keep exactly one zip of the previous app, replaced at the next install.** "Undo last self-fix" unzips it. Undo is unavailable if the data format changed in between.
*   **HQ12, connectors written from web documentation.** Documentation fetched from the web is outside content. A run that reads it is untrusted, so it can't start or change a repair (§1). **Proposed: a self-written connector is built only from what the model already knows, in an untainted run.** When it needs documentation, Heron writes an inbox note: "I'd need Todoist's API docs to write this. Paste the parts you trust into a report." Text the owner pastes is treated as the owner's own words. That is how v1 already treats anything the owner types, and the note says so.
*   **HQ13, a new connector's first uses.** **Proposed: each operation of a self-written connector asks once per conversation until the owner has approved it 3 times.** After that, the normal decision function applies: `NetworkEgress` for reads and `ExternalWrite` for writes.
*   **HQ14, OAuth.** OAuth providers are a fixed list in the fenced `src/oauth.rs`. **Proposed: an API-key connector can be auto-eligible. A connector that needs a new OAuth provider is PR-only.**
*   **HQ15, Heron offering connectors.** **Proposed: in an untainted chat, Heron may offer "I can write a connector for X", and the owner's yes is a trusted trigger.** In a tainted chat (say, after reading an email that mentions X), Heron only writes an inbox note.
*   **HQ16, sealbox's sandbox on macOS.** For model-run Python, sealbox uses Linux namespaces. On macOS it falls back to an audit hook (sealbox `README.md`; `src/sealbox/sandbox.py:62-66`). **Proposed: Heron runs sealbox under its own Seatbelt profile, with no network and file access only to `~/.heron/observe/`, and turns sealbox's `run_python` tool off.**
*   **HQ17, sealbox on preview Macs.** sealbox needs Python 3.10 or later with `duckdb`, `cryptography` and `mcp`. **Proposed: observation is offered only where `uv` is installed.** The Settings row reads "Needs uv" and links to the one-line install. The alternative is bundling Python, which adds about 60 to 80 MB.
*   **HQ18, keystrokes (follows OD3).** **Proposed:**
    *   capture only while Secure Input is off, the focused element isn't an `AXSecureTextField`, and the frontmost app isn't excluded;
    *   keep text in chunks of at most 1 minute per app;
    *   run sealbox's detectors before each chunk is written, dropping tokens shaped like card numbers, one-time codes and API keys.

    The owner should know that macOS doesn't guarantee Secure Input in every app. Some web password fields and terminals don't turn it on, and the `AXSecureTextField` check covers native fields only.
*   **HQ19, retention.** **Proposed: raw observations are kept for 14 days and derived memories for 90 days. "Forget everything observed" works at any time.**
*   **HQ20, default exclusions.** **Proposed:**
    *   **Apps:** 1Password, Bitwarden, Keychain Access, Passwords, System Settings, and any app whose name or bundle ID contains "bank".
    *   **Private browsing windows:** best effort only. Safari and Chrome don't reliably mark private windows to Accessibility, and the Settings row says so.
    *   **Sites:** banking, health and password-manager domains, from a shipped list the owner can edit.
    *   **Folders:** `~/Library`, `~/.ssh`, `~/.gnupg` and Heron's home.
*   **HQ21, what preview users' providers see.** Derived results go to that user's own model provider inside prompts, like any memory. **Proposed: the consent screen says this in plain words.** Raw observations go to no provider.
*   **HQ22, phasing auto-install.** **Proposed:**
    *   **Phase 1** (the work units below) builds the whole loop up to the PR. That covers reports, failure records, the repair agent, the fence, the gate, publishing, the scout, connectors and observation. Every self-fix ends as a PR labelled `self-heal:auto-eligible` or `self-heal:needs-owner`.
    *   **Phase 2** adds the installer, health check, rollback and the signing identity (HQ5). It starts when three things hold:
        *   the replay suite passes HQ10;
        *   prototypes P2 and P3 pass;
        *   over 30 days, at least 3 merged self-heal PRs were labelled `auto-eligible`.

    The third criterion keeps Heron from building the riskiest machinery for a fence that admits nothing.

## Critical User Journeys (CUJs)

### CUJ H-1: The owner reports a bug, and Heron opens a tested PR

*   **Given:** a builder Mac with self-healing on, no run active, and no approval waiting.
*   **When:** the owner chooses Help › Report a problem, types "Next run time shows UTC instead of local time for 'every monday at 8'", and sends.
*   **Then:**
    1.  Heron makes a worktree at the installed commit and starts a repair.
    2.  The agent fixes `src/schedule.rs` and adds `tests/schedule_local_time.rs`.
    3.  The fence marks the diff auto-eligible. The new test fails on the old code and passes on the fix. The gate passes.
    4.  Heron pushes the branch and opens a PR labelled `self-heal:auto-eligible`.
    5.  The inbox says "Fix ready: next-run time zone (PR #NN). Gate passed."

    Expected time: 30 to 90 minutes, mostly the G7 smoke step, which runs because Rust changed. Prototype P4 measures it.

### CUJ H-2: The fix touches fenced code

*   **When:** the owner reports "Telegram pairing asks twice".
*   **Then:** the fix touches `src/telegram.rs`. The gate runs anyway, because the run is trusted. The PR is labelled `self-heal:needs-owner`, and the inbox says "Fix ready for review: PR #NN (touches src/telegram.rs)".

### CUJ H-3 (Phase 2): A self-fix installs itself, then fails its health check

*   **Given:** Phase 2 is on, the signing identity is set, and the CUJ H-1 fix passed the gate.
*   **Then:**
    1.  Heron shows "Installing fix #7. Heron will restart" and quits. The swapper replaces the app and relaunches it.
    2.  The new core never writes its port file, so after 90 seconds the swapper puts back the zipped previous app, relaunches it, and turns auto-install off.
    3.  It comments on the PR, and the inbox note offers "Turn auto-install back on".

### CUJ H-4: The scout finds an idea

*   **Given:** self-evolution is on, with the weekly schedule.
*   **Then:**
    1.  The inbox gains "Idea from the web: A-B loop detection in <repository>". It includes the link, the part of Heron it touches, the effort, and a "from the web, not checked" line.
    2.  Nothing runs. If the owner wants it, the owner writes a report in their own words ("Add detection for two tool calls repeating in turn"), and that is a trusted trigger.

### CUJ H-5: A preview user reports a problem

*   **Given:** a preview Mac.
*   **Then:**
    1.  Help › Report a problem writes `Heron-report-<date>.heronreport`, a JSON file, to the Desktop. It holds the user's text, the last 7 days of failure records, and the version and commit. It holds conversation text only if the user ticked "Include this conversation", and it never holds observations. Then the share sheet opens.
    2.  The user sends the file to the owner however they like. On the owner's Mac, File › Import report shows it in the inbox as outside content.
    3.  The owner restates it in a report of their own if they want a fix. A merged fix reaches the user in the next DMG.

### CUJ H-6: The owner asks for a connector

*   **Given:** a builder Mac and an untainted chat.
*   **When:** the owner writes "Connect my Todoist so the morning brief has my tasks". Heron offers "I can write a Todoist connector (API key). Build it?", and the owner says yes.
*   **Then:**
    1.  A trusted repair writes three files:
        *   `crates/heron-ext/src/todoist.rs`;
        *   `crates/heron-ext/connectors/todoist.toml`, which lists the host `api.todoist.com`, the operations `list_tasks` (read) and `add_task` (write), and `api_key` auth;
        *   a test with recorded responses.
    2.  R6 passes and the gate passes. Heron opens a PR labelled `self-heal:auto-eligible`; in Phase 2 it installs.
    3.  Once the PR's build is installed, Settings › Connectors shows "Todoist (written by Heron)" with an empty key field. The owner pastes the key. Heron never asks for it in chat and never types it.
    4.  The first `list_tasks` call asks (HQ13). Its output taints the session. Anything learned from it is saved unconfirmed.

### CUJ H-7: The owner turns on keystroke capture

*   **Given:** `uv` and sealbox are installed, and every observer is off.
*   **When:** the owner turns on Settings › Observation › Keystrokes.
*   **Then:**
    1.  A consent screen says what is captured, what never is, where it is kept and for how long, what leaves the vault, and how to stop. The owner accepts.
    2.  macOS asks for Input Monitoring. Heron uses no other route to that permission.
    3.  The menu bar icon gains an observing dot. Its menu starts with "Pause observing".
    4.  The next day, the workflow job finds "Most weekdays 9:05 to 9:20: Mail, then Numbers 'Invoices', then Preview". That becomes an unconfirmed memory and an "Automate this?" suggestion. Accepting the suggestion opens `save_automation`'s normal approval, in a tainted session.

## Technical Plan (Architecture)

### 0. Shape

```text
 Heron.app (builder Mac)
 ┌───────────────────────────────────────────────────────────────────────────────────────┐
 │ SwiftUI: Settings › Self-improvement · Settings › Observation · Report a problem       │
 │          observers (screen, apps, browser, keys, files) ─────────────┐                 │
 └──────────────▲────────────────────────────────────────────────────────┼────────────────┘
                │ /api/harness2/*, /api/observe/*                         │ /api/observe/ingest
 ┌──────────────┴────────────────────────────────────────────────────────▼────────────────┐
 │ Rust core                                                                              │
 │  src/harness2: trigger ─▶ trust ─▶ repair (heron-cli selfheal repair) ─▶ fence ─▶ gate │
 │                ─▶ publish (git push, gh pr) ─▶ [Phase 2: swapper ─▶ health ─▶ revert]  │
 │                scout ─▶ web::get_limited ─▶ tool-less ranking ─▶ inbox notes           │
 │  src/ext.rs: heron-ext connectors ─▶ tools (untrusted output) + sources (ext:<name>)    │
 │  src/observe: sealbox append / serve (Seatbelt, no network) ─▶ MCP (trust = outside)   │
 └────────────────────────────────────────────────────────────────────────────────────────┘
```

*   **The repair agent is Heron's own agent loop**, not a second coding agent ([Alternative A](#alternative-a-a-headless-coding-cli-with-its-own-tools-does-the-fix)).
*   **The gate is a fixed script run by Harness2**, never by the agent.
*   **The fence runs in the installed binary**, never from the worktree.

### 1. Trust: which inputs may start a repair

A run's trust comes from the class of every input the model sees in it. It does not come from who clicked: v1's taint never clears, and an owner's click on outside content doesn't make that content trusted. Only the owner's own words do, which is v1's existing rule for anything the owner types.

| Input | What the model sees | Class | What it can do |
|---|---|---|---|
| Owner report, typed in the app or with `heron-cli report` | The owner's text, the version and commit, and the failure records from the last 24 hours | Trusted | Start a repair |
| Owner report with "attach this conversation" ticked | As above, plus the conversation | Trusted if the conversation is untainted | Start a repair. If the conversation is tainted, the attachment is refused and the inbox note says why |
| Failure record | `kind` (one of `panic`, `core_exit`, `migration_failed`, `health_failed`, `internal_error`), `location` (`file:line` from the panic location or Heron's error site), `version`, `commit`, `count`, `origin_tainted`. No message text, tool output, model text, HTTP or MCP bodies, paths or Telegram text | Trusted | Start a repair. When `origin_tainted`, it is never auto-eligible (HQ4) |
| Connector offer accepted in an untainted chat (HQ15) | The owner's yes, the service name, and the operations the owner named | Trusted | Start a repair |
| Imported preview report | Another person's text | Untrusted | An inbox note only |
| Scout idea | Text derived from web pages | Untrusted | An inbox note only |
| Workflow suggestion | Derived results from the observation vault | Untrusted | A suggestion. Accepting it opens a normal, tainted `save_automation` approval. It never starts a repair |

**The decision table.** This is the rule. The test `decision_table_selfheal` has one case per row.

| # | Run | Fence verdict | Other state | Gate | Outcome |
|---|---|---|---|---|---|
| 1 | Trusted | Auto-eligible | Phase 2 on, auto-install on, under the caps, signing identity set, not `origin_tainted`, installed commit is in `origin/main` | Passes | **Install**, plus a PR labelled `self-heal:auto` |
| 2 | Trusted | Auto-eligible | Any condition in row 1 doesn't hold (in Phase 1 this is always the case) | Passes | PR labelled `self-heal:auto-eligible`. Its body says which condition stopped the install |
| 3 | Trusted | Needs owner | any | Passes | PR labelled `self-heal:needs-owner` |
| 4 | Trusted | any | any | Fails 3 times | No PR. An inbox note with the last failure. The branch is kept locally for 7 days |
| 5 | — | — | A run is active, an approval is waiting, or Heron is paused | — | The trigger waits in the queue |
| 6 | — | — | Held by Q6 (HQ3) | — | Inbox note; the owner's click starts it |

Untrusted inputs never start a run, so the table has no untrusted rows. Model-written code from outside content therefore never runs on the Mac and is never pushed.

### 2. The repair run

`heron-cli selfheal repair --run <id>` runs as a separate process:

*   **Home:** a disposable `HERON_HOME` at `~/.heron/selfheal/runs/<id>/home`, like the eval runner's (`eval/run_core_tasks.py`). It has no memories, persona, connectors, Telegram or MCP.
*   **Workspace:** a git worktree at `~/.heron/selfheal/runs/<id>/wt`, on the branch `heron/selfheal/<yyyymmdd>-<id>`, cut from the **installed commit**. That is the new `commit` field of `GET /api/state`.
*   **Tools:** only `read_file`, `write_file`, `edit_file`, `list_dir`, `search_files`, `find_files` and `shell`. `App::assemble` filters the registry, so the model sees nothing else.
*   **Policy:** fixed in `src/harness2/repair.rs`, with `mode = "auto"` and these allow rules:
    *   `shell:cargo test --locked*`
    *   `shell:cargo check --locked*`
    *   `shell:cargo clippy --locked*`
    *   `shell:git diff*`
    *   `shell:git status*`

    Every other shell call is denied. Compound commands are never covered (`src/agent.rs:134-136`). The shell runs under Heron's normal Seatbelt profile.
*   **Prompt:** a fixed repair prompt (new Trusted `PromptSource::Repair`). It gives the trigger's fields and says "Fix the cause; add a test under `tests/`, `macos/HeronMac/Tests/` or `crates/heron-ext/tests/` that fails before your fix; don't edit existing tests". It lists the auto-eligible paths as advice; the fence decides.
*   **Model:** the HQ6 provider through `cli_llm`, so all five CLI containment invariants hold (`src/cli_llm.rs:30-50`).
*   **Retries:** up to 3 attempts. Attempts 2 and 3 get the previous gate failure: the failing step's last 60 lines. That is Harness²'s actionable retry feedback, and the text is trusted because it is Heron's own gate output on trusted code.

The agent's own test runs are only a guide, because about 27 tests fail under the nested sandbox. The gate decides.

### 3. The fence: what may install itself

The fence lives in `src/harness2/fence.rs` as constants, compiled into the installed binary. The running Heron checks it, through `heron-cli selfheal check-fence <base> <fix>`, against the exact commit that is then gated (and in Phase 2, built and installed). The worktree's copy of `fence.rs` is never used. `src/harness2/**` is fenced, so a fix can't widen the fence.

**Auto-eligible paths.** Every changed path must match one of these:

*   `macos/HeronMac/Sources/HeronUI/Tokens/**` and `macos/HeronMac/Sources/HeronUI/Avatar/**`. These are display-only and reference no store, client or `URLSession` (checked by R7).
*   `macos/HeronMac/Tests/**`
*   `src/schedule.rs`, `src/events.rs` and `src/embed.rs`. Lines inside their `#[cfg(test)]` modules may only be added (R1).
*   `tests/**`, except `tests/fixtures/**`.
*   Self-written connectors, under R6:
    *   `crates/heron-ext/src/<name>.rs`, one connector per run;
    *   `crates/heron-ext/connectors/<name>.toml`;
    *   `crates/heron-ext/src/registry.rs`;
    *   `crates/heron-ext/tests/**`.

**Everything else needs the owner.** For example:

*   **Core:** every other `src/` file. That includes `agent.rs`, `permissions.rs`, `sandbox.rs`, `cli_llm.rs`, `memory.rs` (with its trust lists `OUTSIDE_SOURCES`, `TRUSTED_SOURCES` and `VOUCHING_SOURCES`), `mcp.rs`, `connectors.rs`, `sources.rs`, `oauth.rs`, `config.rs`, `store.rs`, `telegram.rs`, `server.rs`, `skills.rs`, `util.rs`, `usage.rs`, `eval.rs` (its retrieval thresholds), `ui/` (Slint, not shipped on macOS), `tools/`, `harness2/`, `observe/` and `ext.rs`.
*   **The Mac app:** all of `HeronCore/**` and `HeronUI/Stores/**`, and `HeronUI/Views/**`, because any view can call a store method that changes the mode or lifts a "never" rule. Also `macos/HeronMac/Sources/Heron/**`, the observers, and Harness2's own panes.
*   **The connector crates' fixed parts:** `crates/heron-ext/src/lib.rs`, `crates/heron-ext/Cargo.toml` and all of `crates/heron-ext-api/**`.
*   **Evaluation, build and project files:** `eval/**`, `scripts/**`, `packaging/**`, `.github/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, any `build.rs`, `Package.swift`, `*.plist` and `*.entitlements`.
*   **Docs:** all of `docs/**`. A docs-only fix changes nothing that runs, so it is a PR with no install.

**Content rules.** A broken rule makes the verdict "needs owner".

*   **R1, tests are add-only.** In `tests/**`, `macos/HeronMac/Tests/**`, `crates/heron-ext/tests/**` and the `#[cfg(test)]` modules of the three core files, the diff removes no lines.
*   **R2, the regression test bites.** At least one new test file exists. The gate copies only the new test files onto a checkout of `base`, which must still compile, and runs them: they must **fail** there and **pass** on `fix`.
    *   A fix whose test needs a new API can't meet R2, so its verdict is "needs owner".
    *   A run that only adds a **new** connector meets R6's test rule instead, because its tests can't compile on `base`.
*   **R3, no deletions, renames or binary files.**
*   **R4, a size cap.** At most 400 changed lines, or 800 for a new connector.
*   **R5, a clean redaction scan.** The existing secret redaction (`src/util.rs`) finds nothing in the diff.
*   **R6, connector rules:**
    *   **The registry.** `registry.rs` matches a whole-file regex: only `mod <name>;` lines and one `connectors![…]` list.
    *   **Banned tokens.** Once comments and string literals are stripped, a connector file contains none of these:
        *   `unsafe`, `extern`, `#[link`, `include_str!`, `include_bytes!`, `env!`, `option_env!`, `asm!`;
        *   `std::process`, `std::fs`, `std::net`, `std::os`, `std::env`;
        *   `tokio::`;
        *   `::heron`;
        *   `crate::` other than `crate::registry`;
        *   `super::super`.
    *   **The manifest.** It validates against `heron-ext-api`'s schema:
        *   **Hosts:** HTTPS DNS names only, with no IP literals, `localhost` or `.local`, checked with the same rules as `connectors::validate`.
        *   **Operations:** each is `read` or `write`.
        *   **Auth:** `none` or `api_key`. An `oauth` connector is never auto-eligible (HQ14).
    *   **Tests.** At least one test replays recorded responses through `heron-ext-api`'s test transport, and it passes.
*   **R7, display-only Swift.** In `Tokens/**` and `Avatar/**`, no added line mentions `CoreClient`, `CoreClientProtocol`, `AppModel`, any identifier ending in `Store`, `URLSession`, or any method declared in `HeronCore/CoreClientProtocol.swift` at `base`.
*   **R8, no conflict with an open self-heal PR.** If an open self-heal PR touches the same files, the verdict is "needs owner".

### 4. The self-heal gate

`scripts/verify-selfheal.sh <base> <fix> <worktree>` runs as a child process of Harness2, outside any sandbox (HQ2), and only for trusted runs. It stops at the first failure. Which steps run depends on what the diff touches:

| Step | Runs when | Command |
|---|---|---|
| 1. Fence | always | `heron-cli selfheal check-fence <base> <fix>`. It uses the installed binary and records the verdict. Later steps run either way |
| 2. R2 | always | Copy the new test files onto `base`. They must fail there and pass on `fix` |
| 3. Swift | anything under `macos/` changed | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac` |
| 4. Rust | anything under `src/`, `tests/` or `crates/` changed | `cargo test --locked`, `cargo test --locked --features desktop --bin heron-desktop`, `cargo clippy --locked --all-targets -- -D warnings` |
| 5. Mutation | Rust changed | `testteeth run --base <base> --fail-under 70 -q` (v1's threshold) |
| 6. Containment | Rust changed | `python3 -I eval/containment/check.py` (v1's G4) |
| 7. G7 smoke | Rust changed | One run of each gate task and p1 on the active Tier 1 CLI, on **both** `base` and `fix`, through `eval/run-core-tasks.sh --runs 1` |
| 8. App build (Phase 2) | the fix will install | `build-app.sh --no-dmg --host-arch` in the default signing mode, with `CODESIGN_IDENTITY` set, then `packaging/macos/upgrade-test.sh` if Rust changed |

Step 7's comparison works like this:

*   Every task that passes on `base` must pass on `fix`. A failure there is run once more, and a second failure fails the gate.
*   t09, t10 and p1 must pass on `fix`.
*   The `base` results are cached by commit in `~/.heron/selfheal/g7-base/<cli>/<commit>.json`, so they run once per installed build.

This compares a candidate with its own base, the way anchor-harness's promotion gate does. It is a new rule in a fenced script, and it doesn't touch `check_gate.py`.

The gate writes `gate-report.json` to the run folder. For each step it records the exit code, the duration and the last 60 lines.

#### What the gate does not prove

A self-fix that passed this gate has **not** passed:

*   G7 at 3 runs per task on every Tier 1 CLI;
*   G9;
*   the G3 record;
*   the genesis audits.

The PR into `main` carries those, under `main`'s branch protection. In Phase 2, an installed self-fix runs before those checks, and Settings shows "Running 1 self-fix not yet in main".

### 5. Publishing, scout and storage

**Publishing (rows 1 to 3).** Each step checks the world first, so it is safe to rerun after a crash:

1.  Commit `fix(<area>): <summary>`. The message is written from the diff by a tool-less model call, then scanned (R5).
2.  Push the branch, unless `git ls-remote` already shows it.
3.  Open the PR into `main` with `gh pr create`, unless `gh pr list --head <branch>` already shows one. The body follows HQ9.

Each tick, Heron checks open self-heal PRs with `gh pr view --json state` and records the result. All self-heal PRs target `main`. R8 keeps two open ones from touching the same files.

**Scout.** The scout keeps its own timer. `schedule.rs` can't express 72 hours: `every 72h` fails with "does not divide evenly" (`src/schedule.rs:250-256`).

*   **Config:** `[harness2] scout_every_hours` is 24, 72 or 168.
*   **Trigger:** `App::run_due_schedules` (`src/app.rs:1483`) calls `harness2::scout_due()`, under pause and the Q6 hold.
*   **Fetching:** through the existing `tools::web::get_limited` (`src/tools/web.rs:76`), which already guards against server-side request forgery and caps body size, plus a host allowlist in `src/harness2/scout.rs`: `api.github.com`, `hn.algolia.com`, `export.arxiv.org`, `www.anthropic.com`, `openai.com` and `deepmind.google`.
*   **Parsing:**
    *   GitHub and Hacker News return JSON.
    *   arXiv and the lab feeds are read with a tolerant `regex` reader that takes `title`, `link` and `updated` or `pubDate` and skips any entry it can't parse.
*   **Ranking:** one model call with no tools, in a session tainted from the start.
    *   **Input:** each item, with an `item_id`, wrapped as untrusted content (`src/agent.rs:692-702`).
    *   **Output:** JSON of the form `[{item_id, harness_part, why, effort}]`, at most 10 entries.
    *   **Validation:** each `item_id` must be one that was fetched. Links and titles on notes come from the fetched data, never from the model.
*   **Output:** each candidate becomes an inbox note through the existing `Store::add_inbox` (`src/store.rs:1283`), titled "Idea from the web: …". Seen URLs are recorded in `selfheal.db`. The scout writes no memory, skill, setting, schedule or code.

**Storage.** `~/.heron/selfheal/selfheal.db` is separate from `heron.db`. It has these tables:

*   `triggers(id, kind, fields_json, state, attempts, created_at)`
*   `runs(id, trigger_id, base, branch, verdict, gate_report_sha256, pr_url, pr_state)`
*   `scout_seen(url_sha256, first_seen)`
*   Phase 2: `installs(commit, previous_commit, installed_at, state)`. This follows anchor-harness's version store, which keeps a history with rollback to an exact version (`crates/harness-core/src/versions.rs:31`, `:122`). Here the git commit is the version.

Only `rusqlite` is used.

**Failure records.** They come from the existing panic hook in `src/main.rs:714-722`, which is extended rather than duplicated, and from the existing core-exit and migration error paths. They take the hook's location (`file:line`) and no message text.

**Report files.** `.heronreport` is plain JSON with a `schema` field. No zip, so no new dependency.

### 6. Preview Macs

*   **Self-healing.** On a Mac that isn't a builder Mac, the switch is replaced by the line "Self-healing runs on the Mac that builds Heron. Use Help › Report a problem." Fixes reach preview users in a new DMG from the owner. An updater is a Non-Goal.
*   **Self-evolution.** Hidden, because the ideas are about Heron's code.
*   **Observation.** Available, under the user's own control (§8).

### 7. Self-written connectors and integrations

OD2 asks for five properties that a self-written connector can't escape:

*   its content taints;
*   its memories start unconfirmed;
*   the decision function covers every call;
*   it never stores or sees secrets;
*   it reaches only the hosts it declares.

A crate boundary and R6 enforce them; the prompt doesn't.

**`crates/heron-ext-api`** (fenced) defines what a connector can do:

*   **The trait:** `trait Connector { fn manifest() -> Manifest; async fn call(op: &str, args: Value, ctx: &Ctx) -> Result<Value> }`.
*   **`Manifest`:** `{name, title, hosts, auth: none | api_key {header, scheme} | oauth {…}, ops: [{name, kind: read | write, description, params}]}`, where `params` is a JSON Schema.
*   **`Ctx::http()`** returns a client with these limits:
    *   it reaches only the manifest's hosts, over HTTPS;
    *   it allows only `GET` during a `read` operation;
    *   it caps responses at 1 MB and times out after 20 seconds.
*   **`Ctx::auth()`** returns an opaque handle. The client turns it into the manifest's header. No method returns the secret.
*   **A test transport** replays recorded responses.
*   **Dependencies:** `reqwest`, `serde`, `serde_json` and `async-trait`, all already in `Cargo.lock`.

**`crates/heron-ext`** holds the connectors:

*   Its `lib.rs` (fenced) is `#![forbid(unsafe_code)] pub mod registry;`.
*   Its `Cargo.toml` (fenced) depends only on `heron-ext-api`, `serde`, `serde_json` and `async-trait`.
*   With no `reqwest` or `tokio`, and with R6's bans, a connector's only way to the network is `Ctx::http()`.

**The adapter, `src/ext.rs`** (new, fenced), handles each entry in `heron_ext::registry` at startup:

1.  It validates the manifest again in the core. A connector that fails isn't registered, and Settings says why.
2.  It registers one tool per operation, `ext_<name>_<op>`. The effect is set by the adapter, never by the connector: `NetworkEgress` for `read` (the arguments are model-composed) and `ExternalWrite` for `write`. Every tool has `untrusted_output = true`, so its output is wrapped as outside content and taints (`src/agent.rs:692-702`).
3.  It returns `Ask` for an operation until the owner has approved it 3 times (HQ13). The count is kept in kv as `ext_approvals:<name>:<op>`.
4.  It registers a context source, `ext:<name>`, with `src/sources.rs`, so "learn" works.
5.  It adds a card, category "Written by Heron", to `connectors::list`. The key goes through the existing keychain path (`connectors::put_api_key`, `POST /api/connectors/custom/{id}/api-key`), and the owner types it in Settings. Heron never asks for it in chat, never types it and never invents it.
6.  It runs each call in a spawned task with a 30-second timeout.

**Trust-table changes, made once by this doc.**

*   `src/memory.rs` treats any source starting with `ext:` as outside, and adds the source `observation` to `OUTSIDE_SOURCES` (§8).
*   After that, the trust lists are fenced like every other trust table. A self-change can't add a source to them.

**What a self-written connector can still do:**

*   send the agent's arguments to its own hosts, under the decision function;
*   return hostile data, which taints;
*   change state with a `GET` on a badly designed API.

The PR and the connector card list its hosts and operations.

### 8. Observation and workflow learning

**Switches.** Settings › Observation has six switches, each off by default, on any Mac that meets HQ17:

1.  Screen
2.  App use
3.  Browser history
4.  Keystrokes
5.  Files outside the working folder
6.  Workflow learning, which needs at least one of switches 1 to 5

The first time a switch is turned on, a plain consent screen appears, and `observe_consent:<switch>:<version>` is recorded. A new consent text asks again.

**Observers.** They live in `macos/HeronMac/Sources/HeronCore/Observers/` (fenced), because macOS grants privacy permissions to the app. Each observer asks for its permission only through the system prompt. If the owner refuses, it stops and says so in Settings.

| Observer | How it works | Permission | What is kept |
|---|---|---|---|
| Screen | ScreenCaptureKit captures the frontmost window every 30 s. Vision (`VNRecognizeTextRequest`) reads its text on the Mac | Screen Recording | OCR text, app, title, time. The image is discarded |
| App use | `NSWorkspace` activation notifications, plus the window title through Accessibility | Accessibility | App, bundle ID, title, start and end time |
| Browser history | A read-only copy of Safari's `History.db` and Chrome's `History` every 15 minutes | Full Disk Access (Safari) | Host, page title, visit time. Never full URLs |
| Keystrokes | `CGEventTap`, listen-only, on key-down | Input Monitoring | Text per app per minute, after detectors (HQ18). Nothing while Secure Input is on, while an `AXSecureTextField` has focus, or while an excluded app is frontmost |
| Files outside the working folder | FSEvents on `~/Documents`, `~/Desktop` and `~/Downloads` | possibly the Files & Folders prompt | Path, event kind, time. Never file contents |

Exclusions (HQ20) are applied in the observer, before anything is written. Observers also stop:

*   while Heron is paused (`POST /api/pause`, or `/pause` from the phone);
*   while "Pause observing" is on;
*   while the screen is locked.

The phone can pause observers but can't turn them on.

**sealbox.** sealbox is the owner's compute-to-data MCP server (`fernandogarzaaa/sealbox` v0.1, read at `c863992`). Heron uses these parts of it:

*   its encrypted vault (AES-256-GCM, a data key per dataset, wrapped by a master key);
*   `run_sql`, a restricted SQL subset;
*   its egress gate (k-anonymity, PII redaction, injection stripping, content fingerprints and per-session leak budgets);
*   its detectors;
*   its hash-chained audit log.

The integration works like this:

1.  **Install.** Settings › Observation › Install sealbox runs `uv tool install git+https://github.com/fernandogarzaaa/sealbox@<commit>`, with the commit pinned in fenced `src/observe/sealbox.rs`. The owner clicks to start it.
2.  **Key.** The core generates the master key, stores it in the macOS keychain, and passes it only to sealbox child processes as `SEALBOX_MASTER_KEY`, which sealbox already reads. No keyfile is ever written.
3.  **Ingest.** Observers post batches to `POST /api/observe/ingest` (loopback, bearer token). The core keeps nothing: it streams each batch to a `sealbox append` child, which writes encrypted hourly segments to `~/.heron/observe/vault`. Raw observations never touch `heron.db`, its backups, the event bus or the logs.
4.  **Sandbox.** Every sealbox child runs under a Heron Seatbelt profile. It has no network, can read and write only `~/.heron/observe/`, and can read the `uv` tool environment (HQ16).
5.  **Agent access.** `sealbox serve --tools list_datasets,describe_dataset,run_sql,get_result,budget_status` is registered as an MCP server with `trust = "outside"`, so its output taints through the existing MCP path (`src/mcp.rs`). `run_python` is off.
6.  **Policy.** It comes from the `observation` template:
    *   OCR and keystroke text columns are `aggregate_only`. They can be filtered (`WHERE text LIKE '%invoice%'`) and counted, but never read.
    *   App, title and host columns are `public`.
    *   Differential privacy is off, because the only person in the data is the user asking. The raw-value and fingerprint checks stay on.
7.  **Fixed jobs.** `src/observe/jobs.rs` (fenced) runs once a day:
    *   time per app;
    *   app sequences seen on 3 or more days, with `k_min = 3` and `day` as the entity, so one moment never comes out as a workflow;
    *   documents touched;
    *   site visits per host.

    The results are saved as memories with the source `observation`, which is outside, so they start unconfirmed. This uses the existing reflect path (`src/app.rs:1840-1875`).
8.  **Workflow to automation.** A repeated sequence becomes a suggestion through the existing `Store::add_suggestion` (`src/store.rs:1600`). Its `session_id` is a new tainted session holding the derived result, and its prompt is fixed text: "Create an automation for the routine in this conversation."
    *   Accepting it continues that tainted session (`answer_suggestion` returns the `session_id`, `src/server.rs:1606-1615`).
    *   `save_automation` then asks for approval as usual.
    *   This is how workflow learning feeds self-evolution: as untrusted proposals only.
9.  **Retention.** Each day, `sealbox prune --older-than 14d` runs, and `observation` memories older than 90 days are deleted.
10. **Forget everything observed.** It stops every observer, deletes `~/.heron/observe/`, the keychain key, every `observation` memory, and every suggestion that came from observation. It then reports the counts.
11. **Indicator.** While any observer is on, the menu bar icon shows a dot. Its menu starts with "Pause observing" (for 1 hour, or until resumed) and "Observation settings…".

**Where sealbox falls short** (its README and source at `c863992`):

*   **It captures nothing.** All five observers are new Heron code.
*   **It only ingests whole datasets** (`Vault.ingest`, `src/sealbox/vault.py:268`). There is no append or prune. Heron needs three additions in sealbox's repository first (P-4): `sealbox append`, `sealbox prune` and `sealbox serve --tools`.
*   **Its sandbox is Linux-only.** On macOS it falls back to an audit hook. Heron's Seatbelt profile and turning `run_python` off cover this.
*   **It has no summariser inside the vault.** Text comes out only as counts and filtered aggregates (Overview, fact 4).
*   **Its privacy tools assume many people.** k-anonymity and DP are repurposed here, with days as entities and DP off.
*   **Its budgets reset when the process restarts.**
*   **It is alpha.** Its red-team results are "evidence, not proof", in its own words.
*   **It needs Python** (HQ17).
*   **It doesn't protect against a compromised Mac** (its README, threat model).

**Preview users.** Each user controls their own switches, behind the same consent screens. Nothing observed leaves their Mac except derived results inside prompts to their own provider (HQ21). The owner's Heron never receives any of it. Reports never include observations.

### 9. Phase 2: install and rollback

These invariants hold, and the steps are in implementation item 21:

*   **The old code judges the new.** The swapper is a copy of the old `heron-cli`, taken before the swap and started detached (`setsid`), so it outlives both the app and its core.
*   **Exactly one launchable Heron.app.** The previous app is kept only as one zip in `~/.heron/selfheal/previous/`.
*   **No install while work is in progress:** no install during an active run, a waiting approval, or a pause.
*   **The health check decides.** The new core must:
    *   answer `GET /api/state` with the expected `commit`;
    *   report `risk_notice_accepted` unchanged;
    *   pass `heron-cli audit`;
    *   keep its `PRAGMA user_version` unchanged;
    *   still be running 30 seconds later.

    If it fails, the swapper restores the zip, relaunches, and turns auto-install off.
*   **A crash in the middle is recovered.** A one-row `swap_intent` marker in `selfheal.db` is written before the swap and cleared after the health check. At startup, a marker whose commit doesn't match the installed one restores the zip.
*   **Data:** `store.rs` is fenced, so a self-fix can't migrate data. The existing pre-migration snapshot (`heron-before-upgrade-from-vN`) covers the "Return to main" path.

**Undo and Return to main** are owner clicks outside the decision table:

*   **Undo last self-fix** unzips the previous app and runs the health check. It is unavailable when `user_version` differs.
*   **Return to main** builds `origin/main`, code the owner has already merged, with step 8 of the gate only, then runs the swap and the health check.

### 10. Changes to v1 code, and the definition of done

This doc is the owner approval these changes to fenced files need:

*   **`GET /api/state`** gains `commit`, from `option_env!("HERON_COMMIT")`, which `build-app.sh` sets. Without it the value is `"unknown"`.
*   **`build-app.sh`** gains `--no-dmg` and `--host-arch`. Its default mode drops `--deep` and adds `--options runtime`.
*   **`App::assemble`** gains a tool filter.
*   **`run_due_schedules`** calls `harness2::tick()` and the scout.
*   **The panic hook** in `main.rs` writes failure records.
*   **`ContentEntry`** gains `OwnerReport`, `FailureRecord`, `ConnectorOutput` and `ObservationResult`. That enum comes from the features base B2 (precondition P-1).
*   **`memory.rs`** gains the `ext:` prefix rule and the `observation` source (§7).

The v1 features doc's eight checks ([heron-v1-features.md](heron-v1-features.md#the-extended-definition-of-done)) apply as follows:

| Check | Harness2 |
|---|---|
| 1. New tools | `ext_<name>_<op>` per connector operation (`NetworkEgress` or `ExternalWrite`, untrusted output), and sealbox's MCP tools (trust `outside`). The repair profile reuses existing tools |
| 2. Decision-table rows | `decision_table_selfheal`, `decision_table_repair_profile` and `decision_table_ext_tools` |
| 3. Prompt blocks | `PromptSource::Repair` (Trusted). Scout items, connector output and sealbox output are Outside |
| 4. EVE | Report a problem on a preview build: a file is written and nothing is sent. Turn on App use: consent, indicator and Pause work |
| 5. Content entries | `OwnerReport` and `FailureRecord` (trusted); `ConnectorOutput` and `ObservationResult` (tainting). Imported reports and scout ideas are inbox notes only and never reach a model |
| 6. Approval surfaces | The Self-improvement and Observation panes, consent screens, Report a problem, Import report, and the connector key field. App only. The phone can pause observers and nothing more |
| 7. Processes outside the `shell` tool | Each has a fixed path, fixed argument shapes, and an environment of `PATH`, `HOME` and `TMPDIR`. They are: `git` (worktree, commit, push, ls-remote); `gh` (pr create, view, list, comment), with `GH_*` added; `cargo`; `swift`; `testteeth`; `python3 -I`; `bash scripts/verify-selfheal.sh`; and, in Phase 2, `build-app.sh` with `codesign`, `ditto`, `/usr/bin/open -a /Applications/Heron.app` and the swapper. Observation adds `uv tool install git+…@<pinned commit>`, and `sealbox append`, `serve` and `prune`, which run under Seatbelt with no network and get `SEALBOX_MASTER_KEY`. Model-written code runs unsandboxed only in the gate, for trusted runs (HQ2) |
| 8. TCC | Observation adds Screen Recording, Accessibility, Input Monitoring and Full Disk Access, each requested only when its switch is turned on. Phase 2 needs HQ5 so that existing grants survive |

## Alternatives Considered

### Alternative A: A headless coding CLI with its own tools does the fix

Run `claude -p --permission-mode acceptEdits` in the worktree, the way skein's `claude_build` backend built v1.

*   **Pros:** a stronger coding agent, proven on this repository.
*   **Cons:**
    *   It breaks CLI containment invariant 3, "The CLI is a text model for Heron, not a second agent" (`src/cli_llm.rs:40-43`).
    *   It adds a second permission system that Heron's audit log and taint don't see.
*   **Reason for rejection:** it gives up v1's containment for a capability gap that the replay suite measures (G-H6). If Heron's loop scores below HQ10, this alternative comes back to the owner as a question.

### Alternative B: Run the gate inside Seatbelt

*   **Pros:** no unsandboxed model-written code.
*   **Cons:** about 27 tests fail and `swift build` dies under a nested sandbox. Skipping them is the "skips pass green" hole that v1 closed with `HERON_REQUIRE_SEATBELT=1` (`scripts/verify-v1.sh:40-44`).
*   **Reason for rejection:** a gate that skips tests proves less than it claims. Untrusted code never reaches the gate at all.

### Alternative C: Proposal-only, for good

*   **Pros:** the simplest design.
*   **Cons:** the owner chose autonomy (OD1).
*   **Reason for rejection:** an owner decision. Phase 1 is proposal-only in practice, and Phase 2 adds autonomy when the measured share justifies it (HQ22).

### Alternative D: `verify-v1.sh` before every PR

*   **Cons:** 66 or more G7 runs, a 14-day trial, and G7 results discarded on any `src` change.
*   **Reason for rejection:** it can't finish in a day. The PR into `main` still gets it.

### Alternative E: Run Harness², or APE's propose-probe-compose loop, as self-evolution

*   **Cons:** it tunes per task, its executors bypass safety, it needs about 4,000 rollouts per task, and 11 tasks overfit.
*   **Reason for rejection:** it needs a suite of 50 or more tasks. Three ideas from it are kept here:
    *   actionable retry feedback (§2);
    *   outside lessons treated as untrusted (§1, from APE's `lessons.js`);
    *   validating the whole change (§3).

### Alternative F: A separate pipeline that turns untrusted triggers into local diffs

The model would return a diff from a read-only session, and the owner would review it.

*   **Cons:**
    *   It needs a whole second path: read-only tools, applying diffs, a diff view and four routes.
    *   The triggers it serves are rare (a weekly scout, at most 25 preview users).
    *   Models often produce diffs that don't apply.
*   **Reason for rejection:** an owner who restates the idea in their own words gets a trusted run with none of that machinery.

### Alternative G: A full effect journal (a port of anchor-harness's `effects.rs`)

*   **Cons:**
    *   Local steps can be checked directly, and push and PR are reconciled from git and GitHub.
    *   Recovery needs only a swap marker.
*   **Reason for rejection:** the journal's state machine would be unused. The world is the source of truth, and §5's publishing checks it first.

### Alternative H: A self-written connector as a local MCP server process

*   **Cons:** it would see the API key in its environment, could do anything the user can, and nothing would limit its hosts.
*   **Reason for rejection:** it fails OD2's secret and host rules, which the crate boundary enforces at compile time.

### Alternative I: Keep raw observations in `heron.db`

*   **Cons:** raw keystrokes would sit in Heron's store, its backups and its upgrade snapshots, and every memory and prompt path could reach them.
*   **Reason for rejection:** OD3 puts raw observations in sealbox.

### Alternative J: Summarise raw observation text with the user's model provider

*   **Pros:** summaries in words.
*   **Cons:** raw keystrokes and screen text would go to the provider.
*   **Reason for rejection:** it breaks the containment the owner chose. A local model inside the vault is a later candidate.

### Alternative K: No keystroke capture

*   **Reason for rejection:** an owner decision (OD3).

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Prompt injection becomes persistent code | Untrusted inputs never start a run. Scout links come from fetched data. Taint never clears on a click |
| A self-fix weakens a security check | A default-deny fence in the installed binary, checked against the exact commit, with R1, R2 and R7. Harness2, the gate, observation and the trust tables are fenced |
| Model-written tests run unsandboxed | Trusted inputs only. Recorded in the threat model (HQ2). Caps (HQ7) |
| A broken build replaces the app (Phase 2) | The old binary health-checks the new one. Automatic revert, auto-install switched off, and a crash marker |
| Grants or keychain access reset | A stable identity, checked by P2 (HQ5) |
| Private text in a public PR | Report text is out by default, R5 scans, and commit messages are written from the diff |
| A connector exfiltrates or reads secrets | Manifest hosts only, R6 bans, an opaque auth handle, the decision function, and asks for its first 3 uses |
| The vault is read | Encrypted, its key in the keychain, Seatbelt-only access. A compromised Mac can still read it, which is sealbox's own stated limit |
| Passwords or codes captured | Secure Input and `AXSecureTextField` skips, detectors, exclusions, 14 days. macOS doesn't guarantee Secure Input everywhere (HQ18) |
| Other people's content captured (messages on screen, mail being read) | The consent screen says so. 14 days, and only aggregates come out |
| Observation without the user knowing | Off by default, consent per switch, macOS's own prompts and indicators, Heron's menu bar dot, one-click pause |
| The repair agent is too weak | The replay suite gates the switch. Alternative A returns as a question |
| The fence checker or replay grader is wrong | genesis audits both, and testteeth grades Harness2's code |

## Non-Functional Requirements

*   **Time:** a trusted run takes about 30 to 90 minutes when Rust changes, and about 10 to 20 minutes when it is Swift-only. P4 measures this.
*   **Storage:**
    *   A worktree with `target/` takes 3 to 5 GB. It is removed once the PR opens, or after 7 days for row 4.
    *   Phase 2 keeps one app zip of about 60 MB.
    *   The observation vault grows by an estimated 50 to 200 MB a week with every observer on, and is pruned to 14 days.
*   **Rollout:** every switch ships off.
    *   Self-healing stays disabled until the replay suite passes (G-H6).
    *   Phase 2 has its own entry criteria (HQ22).
    *   Turning a switch off stops new triggers. A running repair finishes its current step and stops.

### Additional Resources

*   [heron-v1.md](heron-v1.md): Q6, Q10, taint and permission modes.
*   [heron-v1-features.md](heron-v1-features.md): `ContentEntry`, the eight-check definition of done, and the work units format.
*   anchor-harness (`~/Documents/PERSONAL/anchor-harness`):
    *   `crates/harness-core/src/versions.rs`, the version store and rollback;
    *   `gate.rs`, the promotion rule (deferred);
    *   `effects.rs` (see Alternative G);
    *   `docs/HARNESS2.md`.
*   APE (`~/workspace/ape-business/src/agent/`): `lessons.js`, `probe.js`.
*   sealbox (`github.com/fernandogarzaaa/sealbox` at `c863992`): `README.md` (threat model), `src/sealbox/vault.py`, `sandbox.py`, `gate.py`.
*   Research synthesis, 2026-11-22: three read-only reports comparing Harness² with Heron and ranking harness candidates across the owner's repositories. Their findings are restated where they are used. They are session scratch files and aren't linked.
*   Goldfish review reports: [docs/designs/reviews/heron-harness2/](reviews/heron-harness2/).

### Future Improvements

*   **Split the app's views from its stores.** Views would receive read-only view models and action closures that the stores supply. Then most UI fixes could become auto-eligible.
*   **An offline harness optimizer.** It needs a suite of at least 50 tasks with a train/test split (G7 plus EVE usability tasks as the seed), and anchor-harness's three-arm promotion rule (`gate.rs:74`, with Wilson intervals at `:42`). Promotion is always an owner-reviewed PR.
*   **Runtime ports:** RepeatGuard, the completion check, protected paths and recoverable tool output, each its own delta.
*   **A local model inside the observation vault**, for summaries in words.
*   **An updater for preview users.**

--------------------------------------------------------------------------------

## Detailed Implementation Plan

### Preconditions

*   **P-1. The v1 features base (B1 to B6) is merged into `main`.** Harness2 uses its `ContentEntry` enum, and both edit `config.rs`, `server.rs`, `app.rs` and `SettingsView.swift`.
*   **P-2. The build session runs four prototypes by hand, outside any sandbox, on the owner's Mac.** It records each in `docs/designs/research/harness2-prototypes.md` as `P<n>: pass|fail: <evidence>`.
    *   **P1:** Does `cargo test --locked --test <name>` run in Heron's Seatbelt shell, with the target folder inside the workspace?
    *   **P2 (needed for Phase 2):** Do TCC grants and keychain items survive a switch between a release DMG and a Developer ID self-fix build, in both directions? What happens with an ad-hoc build?
    *   **P3 (needed for Phase 2):** Can a detached process started by the core outlive the app and the core, and rename `/Applications/Heron.app` without an admin prompt?
    *   **P4:** How long does `verify-selfheal.sh` take on a one-line Rust fix and on a one-line Swift fix?
*   **P-3.** The owner has answered, or accepted the defaults for, HQ1 to HQ5, HQ12, HQ16 to HQ18 and HQ22.
*   **P-4. sealbox gains three additions in its own repository, as its own PR.** Its commit is then pinned. The additions are:
    *   `sealbox append <dataset> --kind events` (JSON lines on stdin, written as encrypted hourly segments);
    *   `sealbox prune --older-than <days>`;
    *   `sealbox serve --tools <list>`;
    *   an `observation` policy template.

    Its check is `python -m pytest -q && python eval/redteam.py`, with 0 leaks.

### Target Files to Create/Modify (Phase 1)

#### Unit H1: `src/harness2/`

##### 1. `src/harness2/mod.rs` (Create)

*   `pub struct Harness2 { db: SelfhealDb, cfg: Harness2Config }`
*   `pub fn tick(&self, app: &App)`: polls PRs, starts queued triggers when idle, and checks whether the scout is due.
*   `pub fn queue(&self, t: Trigger) -> TriggerId`

##### 2. `src/harness2/trust.rs` (Create)

*   `enum TriggerKind { OwnerReport, FailureRecord, ConnectorRequest }`
*   `struct FailureRecord { kind: FailureKind, location: String, version: String, commit: String, count: u32, origin_tainted: bool }`, with `FailureKind` as listed in §1.
*   `fn accept_attachment(session_tainted: bool) -> Result<()>`
*   `fn outcome(verdict: FenceVerdict, state: &RunState, gate: GateResult) -> Outcome { Install, Pr(Label, Option<Reason>), NoPr, Wait, Held }`, covering exactly the 6 rows of §1.

##### 3. `src/harness2/fence.rs` (Create)

*   `const AUTO: &[&str]`, the globs from §3, using the existing `globset` crate.
*   `fn check(repo: &Path, base: &str, fix: &str) -> FenceVerdict { AutoEligible, NeedsOwner(Vec<Reason>) }` applies the path rules and R1 and R3 to R8.
*   **R7's banned identifiers** are read from `HeronCore/CoreClientProtocol.swift` at `base`, with the regex `func (\w+)`, plus the fixed names in §3.
*   **R6's banned tokens** are checked after comments and string literals are stripped, with a small lexer for `//`, `/* */`, `"…"` and `r#"…"#`.
*   **R2** is run by the gate, which reports the result back in `gate-report.json`.

##### 4. `src/harness2/repair.rs` (Create)

*   `const REPAIR_POLICY`, `const REPAIR_TOOLS: [&str; 7]`, and `const REPAIR_PROMPT`.
*   `fn prepare(run, installed_commit) -> RunDirs`: runs `git worktree add`, then writes the disposable home's `config.toml`. It copies the provider settings, but no keys, because CLI logins stay with the CLI.
*   `async fn attempt(run, feedback: Option<String>) -> AttemptResult`

##### 5. `src/harness2/gate.rs` (Create)

*   `fn run_gate(run) -> GateResult`: spawns `bash scripts/verify-selfheal.sh <base> <fix> <wt>` with the §10 environment and a 3-hour timeout, logs to `runs/<id>/gate.log`, and parses `gate-report.json`.

##### 6. `src/harness2/publish.rs` (Create)

*   `fn publish(run, verdict) -> PrRef`: runs the R5 scan, then commits, pushes and opens the PR, each step only if it hasn't already happened (§5). The PR body follows HQ9.
*   `fn poll_prs()`

##### 7. `src/harness2/scout.rs` (Create)

*   `const HOSTS`, one `async fn fetch_*` per HQ8 source (each through `tools::web::get_limited`), and `fn parse_feed(text) -> Vec<Item>`.
*   `async fn rank(items) -> Vec<Idea>`, then `fn post(ideas)`, which calls `Store::add_inbox`.

##### 8. `src/harness2/report.rs` (Create)

*   `fn write_report(desc, include: Option<SessionId>) -> PathBuf`: writes JSON, refuses a tainted session, and never includes observations.
*   `fn import_report(path)`: adds an inbox note marked as outside.

##### 9. `src/harness2/db.rs` (Create)

*   `selfheal.db` with the tables `triggers`, `runs` and `scout_seen`, created with `CREATE TABLE IF NOT EXISTS`, and `PRAGMA user_version = 1`.

##### 10. Tests (inline in `src/harness2/*.rs`)

*   `trust.rs`:
    *   `decision_table_selfheal`
    *   `tainted_attachment_is_refused`
    *   `failure_record_has_no_free_text`
    *   `origin_tainted_failure_is_never_auto`
*   `fence.rs`:
    *   `fence_allows_tokens_and_schedule`
    *   `fence_denies_views_and_stores`
    *   `fence_denies_harness2_and_gate_script`
    *   `fence_denies_unlisted_src_file`
    *   `fence_denies_cargo_lock_and_build_rs`
    *   `docs_only_diff_needs_owner`
    *   `removed_test_line_needs_owner`
    *   `renamed_file_needs_owner`
    *   `diff_over_cap_needs_owner`
    *   `redaction_hit_needs_owner`
    *   `connector_banned_token_needs_owner`
    *   `connector_token_in_string_is_ignored`
    *   `registry_with_code_needs_owner`
    *   `manifest_with_ip_host_needs_owner`
    *   `oauth_connector_needs_owner`
    *   `tokens_file_calling_client_needs_owner`
    *   `fence_bans_every_client_protocol_method`
    *   `overlapping_open_pr_needs_owner`
*   `repair.rs`:
    *   `decision_table_repair_profile`
    *   `repair_registry_has_only_seven_tools`
    *   `compound_shell_is_denied_in_repair`
*   `publish.rs`:
    *   `publish_skips_existing_branch_and_pr`
    *   `pr_body_omits_report_text_by_default`
*   `scout.rs`:
    *   `scout_drops_unknown_item_ids`
    *   `scout_note_link_comes_from_fetch`
    *   `scout_rejects_unlisted_host`
    *   `feed_reader_skips_malformed_entries`
    *   `scout_writes_only_inbox_notes`
*   `report.rs`:
    *   `report_excludes_conversation_unless_ticked`
    *   `report_never_includes_observations`
    *   `imported_report_is_inbox_only`

#### Unit H8: connector crates

##### 11. `crates/heron-ext-api/` (Create)

*   `Cargo.toml`, depending on `reqwest`, `serde`, `serde_json` and `async-trait`, all from `Cargo.lock`.
*   `src/lib.rs`, holding `Connector`, `Manifest`, `Ctx`, `Http`, `AuthHandle`, `TestTransport` and `fn validate_manifest(&Manifest) -> Result<()>`.
*   Tests:
    *   `http_refuses_unlisted_host`
    *   `read_op_refuses_post`
    *   `response_over_1mb_is_refused`
    *   `auth_handle_has_no_getter` (a compile-fail doctest)
    *   `manifest_rejects_ip_and_localhost`

##### 12. `crates/heron-ext/` (Create)

*   `Cargo.toml`, depending on `heron-ext-api`, `serde`, `serde_json` and `async-trait`.
*   `src/lib.rs`, which is `#![forbid(unsafe_code)] pub mod registry;`.
*   `src/registry.rs`, holding an empty `connectors![]`.
*   `tests/fixture_connector.rs`, a test-only `echo` connector under `tests/`, which checks that a registered connector's calls go through `Ctx`.

#### Unit H8r: workspace

##### 13. `Cargo.toml` and `Cargo.lock` (Modify)

*   Add `[workspace] members = [".", "crates/heron-ext-api", "crates/heron-ext"]` and the path dependency `heron-ext`.
*   `Cargo.lock` gains only the two path crates.

#### Unit H2: wiring in existing core files

##### 14. `src/lib.rs`, `src/config.rs`, `src/main.rs` (Modify)

*   **`lib.rs`:** add `pub mod harness2; pub mod ext; pub mod observe;`. In this unit, `observe` is an empty module with only its config types.
*   **`config.rs`:** add a `[harness2]` table with these keys and defaults:
    *   `self_healing = false`
    *   `self_evolution = false`
    *   `scout_every_hours = 168`, validated to 24, 72 or 168
    *   `repo = ""`
    *   `publish_report_text = false`
    *   the HQ7 caps
*   **`main.rs`:**
    *   Add the subcommands `selfheal repair --run`, `selfheal check-fence <base> <fix>` and `report`.
    *   Extend the existing panic hook (`:714-722`) to write a `FailureRecord` with `kind = panic` and the panic `location()`, before it calls the previous hook.

##### 15. `src/server.rs` (Modify)

*   `GET /api/state` gains `commit`.
*   New routes, all bearer-token protected:
    *   `GET /api/harness2`, which returns the switches, the checklist (source folder, Rust, Xcode tools, GitHub login, replay result) and queued and recent runs with PR links;
    *   `POST /api/harness2`;
    *   `POST /api/harness2/report`;
    *   `POST /api/harness2/import`;
    *   `POST /api/harness2/connector-request`, which accepts only a session id whose session is untainted.

##### 16. `src/app.rs`, `src/agent.rs` (Modify)

*   **`app.rs`:**
    *   `App::assemble` takes an optional tool filter.
    *   `run_due_schedules` calls `harness2.tick()` under `paused()` and `model_held(false)`.
    *   The core-exit and migration error paths write failure records.
*   **`agent.rs`:**
    *   Add `PromptSource::Repair` (Trusted).
    *   Add the four `ContentEntry` variants from §10.
    *   Add the connector-offer path: in an untainted session, Heron's reply may carry an offer card, and the owner's yes calls `connector-request`.

##### 17. `src/ext.rs` (Create), `src/memory.rs`, `src/sources.rs`, `src/connectors.rs` (Modify)

*   **`ext.rs`:** the adapter in §7.
*   **`memory.rs`:** `fn is_outside(source)` returns true for every source in `OUTSIDE_SOURCES`, every source with the prefix `ext:`, and `observation`. The existing checks call it.
*   **`sources.rs`:** register `ext:<name>` sources.
*   **`connectors.rs`:** list cards with the category "Written by Heron".

##### 18. Tests for H2

*   Inline tests:
    *   `state_reports_commit`
    *   `harness2_routes_need_token`
    *   `scout_tick_respects_pause_and_hold`
    *   `panic_hook_writes_failure_record`
    *   `every_prompt_block_is_trusted_or_taints`
    *   `every_content_entry_is_trusted_or_taints`
    *   `decision_table_ext_tools`
    *   `ext_output_taints`
    *   `ext_op_asks_until_three_approvals`
    *   `ext_source_memories_start_unconfirmed`
    *   `observation_source_is_outside`
    *   `connector_request_refuses_tainted_session`
*   Integration test, in `tests/integration.rs`: `api_harness2_lists_switches_and_checklist`, with the fixture `tests/fixtures/api/harness2.json`.

#### Unit H10: observation core `src/observe/`

##### 19. `src/observe/mod.rs`, `sealbox.rs`, `jobs.rs`, `forget.rs` (Create), `src/server.rs`, `src/sandbox.rs` (Modify)

*   **`sealbox.rs`:**
    *   `const SEALBOX_COMMIT`
    *   `fn install()`, which runs `uv tool install …@SEALBOX_COMMIT`
    *   `fn master_key()`, which uses the keychain and creates the key on first use
    *   `fn spawn_append()` and `fn spawn_serve()`, under the new Seatbelt profile `observe` in `sandbox.rs`: deny network, allow read and write in `~/.heron/observe/`, and allow reads of the `uv` tool directory
    *   registration of `serve` as an MCP server with `trust = "outside"`
*   **`jobs.rs`:** the four fixed SQL jobs, the daily prune, and saves through the reflect path with the source `observation`.
*   **`forget.rs`:** `fn forget_everything() -> ForgetReport`.
*   **Routes:**
    *   `GET /api/observe`, which returns the switches, consent versions, the sealbox status and the counts;
    *   `POST /api/observe` (switches and exclusions);
    *   `POST /api/observe/ingest`, which streams to `append` and keeps nothing;
    *   `POST /api/observe/pause`;
    *   `POST /api/observe/forget`.
*   **Tests:**
    *   `ingest_keeps_nothing_outside_vault`
    *   `sealbox_profile_denies_network`
    *   `sealbox_key_never_written_to_disk`
    *   `serve_registered_with_outside_trust`
    *   `workflow_job_uses_k_min_three_days`
    *   `forget_removes_vault_key_memories_and_suggestions`
    *   `observers_stop_when_paused`
    *   `workflow_suggestion_session_is_tainted`

#### Unit H3: replay suite

##### 20. `eval/selfheal/cases.md`, `run_replay.py`, `genesis.yaml`, `eval/tests/test_selfheal_replay.py` (Create)

*   **`cases.md`** lists 10 historical fix commits, mixed between auto-eligible and fenced files. Each case has its `base`, a bug report in the owner's words, and `hidden_tests`.
*   **`run_replay.py`:**
    1.  Check out `base`.
    2.  Run `heron-cli selfheal repair`.
    3.  Copy in the fix commit's tests.
    4.  The case passes if those tests and `cargo test --locked` pass.

    It prints `replay: N/10` and `auto-eligible: M`, and exits 0 when N meets HQ10.
*   **`genesis.yaml`** declares the grader, so `genesis audit-evaluator` can confirm that an empty patch and a patch that deletes the hidden test both fail.
*   **The unit tests** check the grader on canned outcomes.

#### Unit H4: gate script

##### 21. `scripts/verify-selfheal.sh` (Create)

*   The steps in §4, chosen from `git diff --name-only <base> <fix>`, in the style of `verify-v1.sh` (`begin`, `fail`, and `HERON_REQUIRE_SEATBELT=1` on Darwin).
*   It validates its arguments: two commits of 40 hex digits and an existing directory.
*   Step 7 writes the `base` result cache in `~/.heron/selfheal/g7-base/`.
*   Step 8 is behind `--install`, which only Phase 2 passes.

#### Unit H5: packaging

##### 22. `packaging/macos/build-app.sh` (Modify)

*   Export `HERON_COMMIT=$(git rev-parse HEAD)` into both cargo builds.
*   Add `--no-dmg`, which skips step 6, and `--host-arch`, which builds only `$(uname -m)`.
*   Default mode: `codesign --force --options runtime --sign "${CODESIGN_IDENTITY:--}"`, signing `heron-cli` and then the app, with no `--deep`.

#### Unit H6: Mac app, self-improvement

##### 23. `macos/HeronMac/Sources/HeronUI/SelfImprovement/` (Create), `HeronCore/Harness2Client.swift` (Create), `Models.swift`, `SettingsView.swift`, `Heron/HeronApp.swift` (Modify)

*   **`SelfImprovementSettingsView`** shows:
    *   the self-healing switch, disabled until the checklist passes;
    *   the checklist;
    *   recent runs with PR links;
    *   the self-evolution switch, and "Check every" with the choices 24 hours, 72 hours and Week;
    *   on preview Macs, the replacement line from §6.
*   **`ReportProblemSheet`**, with the "Include this conversation" and "OK to publish" ticks.
*   **Menu items:** Help › Report a problem, and File › Import report (builder Macs only).
*   **The connector offer card** in chat.
*   **Tests**, in `SelfImprovementTests.swift`:
    *   `test_selfHealingSwitchDisabledUntilChecklistPasses`
    *   `test_previewMacShowsReportLineInsteadOfSwitch`
    *   `test_scoutScheduleOffersThreeChoices`
    *   `test_reportSheetDefaultsToNoConversation`
    *   `test_reportSheetDefaultsToNotPublished`

#### Unit H11: Mac app, observation

##### 24. `macos/HeronMac/Sources/HeronCore/Observers/` and `macos/HeronMac/Sources/HeronUI/Observation/` (Create), `MenuBarViews.swift`, `packaging/macos/Info.plist` (Modify)

*   **Observers:** `ScreenObserver`, `AppUseObserver`, `BrowserHistoryObserver`, `KeystrokeObserver`, `FileEventsObserver`, `Exclusions` and `ObservationUploader`, each as in §8.
*   **Panes:** `ObservationSettingsView`, `ConsentView`, `ExclusionsView`, and the menu bar dot with "Pause observing".
*   **`Info.plist`:** the usage description strings for Screen Recording and Input Monitoring. This change belongs to H5's scope, so H5 makes it (see the Work units).
*   **Tests**, in `ObservationTests.swift`:
    *   `test_everyObserverStartsOff`
    *   `test_consentShownBeforeFirstEnable`
    *   `test_keystrokesSkippedDuringSecureInput`
    *   `test_keystrokesSkippedInSecureTextField`
    *   `test_excludedAppIsNotRecorded`
    *   `test_indicatorShownWhileAnyObserverOn`
    *   `test_pauseStopsAllObservers`

#### Unit H7: docs

##### 25. `docs/engineering/SECURITY_THREAT_MODEL.md` (Modify), `docs/features/self_improvement.md`, `docs/features/observation.md` (Create)

*   **The threat model** gains two sections:
    *   `## Harness2: code Heron writes into itself`, covering the trust table, the fence, the unsandboxed gate for trusted runs (HQ2), connectors and the process list;
    *   `## Observation`, covering the observers, sealbox, what leaves the vault, and its shortfalls.
*   **The two guides** are written for the owner and preview users.

### Phase 2 (a separate graph, started once HQ22's criteria hold)

Phase 2 gets its own short design delta. It holds the swapper (§9), with `start_swap` and `swap` in `src/harness2/install.rs`, the `installs` table, the `swap_intent` marker, Undo and Return to main, and these routes:

*   `POST /api/harness2/quit-for-install`
*   `POST /api/harness2/undo-last`
*   `POST /api/harness2/return-to-main`

It also holds the app's install notice and buttons, and step 8 of the gate. Its units follow the same pattern: `src/harness2/**`, then `src/**`, then `macos/**`, then `scripts/**`, then verify.

### Verification Plan

*   **Build:** `cargo build --locked`, `swift build --package-path macos/HeronMac`, and `bash packaging/macos/build-app.sh --unsigned`.
*   **Tests:** `cargo test --locked` (the workspace, so both crates too), `cargo test --locked --test integration`, and `swift test --package-path macos/HeronMac`. The named tests in items 10, 11, 18, 19, 23 and 24 must exist and pass (`eval/require-tests.sh`, `swift test list`).
*   **Lint:** `cargo clippy --locked --all-targets -- -D warnings`.
*   **Test quality, with testteeth:** `testteeth run --base main --fail-under 70 -q`. Surviving mutants in `fence.rs`, `trust.rs`, `ext.rs`, `heron-ext-api` and `observe/` are killed, not justified, because the design rests on them.
*   **Evaluator audits, with genesis.** All must be SOUND (exit 0) before the replay score or the fence is trusted:
    *   `genesis audit-evaluator eval/selfheal/genesis.yaml`, for the replay grader;
    *   a genesis claim for the fence checker, refusing every path in the fenced classes and every R6 and R7 case, with item 10's fence tests as its cases;
    *   sealbox's own `eval/redteam.py` at the pinned commit, with 0 leaks.
*   **Replay:** `python3 -I eval/selfheal/run_replay.py` meets HQ10 on the owner's Mac.
*   **End to end, by hand, once:**
    *   CUJ H-1 with a planted bug in `src/schedule.rs`;
    *   CUJ H-6 with a fake service on the test transport;
    *   CUJ H-7 with App use only, then "Forget everything observed".

    The logs go in the build PR.

## Work units

Phase 1 only. The graph runs on a feature branch cut from `main` after P-1. P-2's prototype file and P-4's pinned sealbox commit are committed before `skein run` starts.

*   **Ordering.** Scopes that share a prefix (`src/**` and `src/harness2/**`; `macos/**` twice) are ordered by depends-on, so no two run at once.
*   **Escaping.** In the check column, `\|` stands for a shell `|`.

| id | what to build | scope | depends on | check | model role |
|---|---|---|---|---|---|
| H1 | `src/harness2/`: trust, fence (R1-R8), repair profile, gate runner, publish, scout, reports, db, inline tests (items 1-10) | `src/harness2/**` | none | `cargo test --locked --lib harness2 && cargo clippy --locked --all-targets -- -D warnings && test "$(cargo test --locked --lib -- --list \| grep -c -E 'harness2::.*::(decision_table_selfheal\|failure_record_has_no_free_text\|origin_tainted_failure_is_never_auto\|fence_denies_views_and_stores\|fence_denies_harness2_and_gate_script\|fence_denies_unlisted_src_file\|removed_test_line_needs_owner\|connector_banned_token_needs_owner\|tokens_file_calling_client_needs_owner\|fence_bans_every_client_protocol_method\|decision_table_repair_profile\|scout_note_link_comes_from_fetch\|report_never_includes_observations): test')" -eq 13` | hardest tasks: opus high |
| H8 | `crates/heron-ext-api` and `crates/heron-ext` (items 11-12) | `crates/**` | none | `cargo test --locked --manifest-path crates/heron-ext-api/Cargo.toml && cargo test --locked --manifest-path crates/heron-ext/Cargo.toml && cargo test --locked --manifest-path crates/heron-ext-api/Cargo.toml -- --list \| grep -q 'http_refuses_unlisted_host: test'` | hardest tasks: opus high |
| H8r | Workspace members and path dependency (item 13) | `Cargo.*` | H8 | `cargo metadata --locked --format-version 1 --no-deps \| grep -q '"name":"heron-ext-api"' && cargo build --locked` | feature, refactoring: sonnet high |
| H2 | Wiring, adapter, trust-list change, routes, subcommands, panic hook, `ContentEntry` variants, integration test (items 14-18) | `src/**` | H1, H8r | `cargo test --locked --lib --bins && cargo test --locked --test integration && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib -- state_reports_commit harness2_routes_need_token scout_tick_respects_pause_and_hold panic_hook_writes_failure_record every_prompt_block_is_trusted_or_taints every_content_entry_is_trusted_or_taints decision_table_ext_tools ext_output_taints ext_op_asks_until_three_approvals ext_source_memories_start_unconfirmed observation_source_is_outside connector_request_refuses_tainted_session && test -s tests/fixtures/api/harness2.json` | hardest tasks: opus high |
| H10 | `src/observe/`, the sealbox Seatbelt profile, observe routes (item 19) | `src/**` | H2 | `cargo test --locked --lib --bins && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib -- ingest_keeps_nothing_outside_vault sealbox_profile_denies_network sealbox_key_never_written_to_disk serve_registered_with_outside_trust workflow_job_uses_k_min_three_days forget_removes_vault_key_memories_and_suggestions observers_stop_when_paused workflow_suggestion_session_is_tainted` | hardest tasks: opus high |
| H3 | Replay suite, grader, genesis evaluator, grader tests (item 20) | `eval/**` | none | `python3 -I -m unittest discover -s eval/tests -t eval -p test_selfheal_replay.py && test "$(grep -c '^## case' eval/selfheal/cases.md)" -eq 10 && genesis audit-evaluator eval/selfheal/genesis.yaml` | feature, refactoring: sonnet high |
| H4 | `scripts/verify-selfheal.sh` (item 21) | `scripts/**` | none | `bash -n scripts/verify-selfheal.sh && grep -q 'check-fence' scripts/verify-selfheal.sh && grep -q 'HERON_REQUIRE_SEATBELT=1' scripts/verify-selfheal.sh && grep -q 'eval/containment/check.py' scripts/verify-selfheal.sh && grep -q 'fail-under 70' scripts/verify-selfheal.sh && grep -q 'g7-base' scripts/verify-selfheal.sh` | feature, refactoring: sonnet high |
| H5 | `build-app.sh` options, `HERON_COMMIT`, default signing mode, and `Info.plist` usage strings (items 22, 24) | `packaging/macos/**` | none | `bash -n packaging/macos/build-app.sh && grep -q -- '--no-dmg' packaging/macos/build-app.sh && grep -q -- '--host-arch' packaging/macos/build-app.sh && grep -q 'HERON_COMMIT' packaging/macos/build-app.sh && ! grep -q -- '--deep' packaging/macos/build-app.sh && /usr/libexec/PlistBuddy -c 'Print :NSScreenCaptureUsageDescription' packaging/macos/Info.plist >/dev/null` | feature, refactoring: sonnet high |
| H6 | Self-improvement pane, report sheet, import, connector offer card, client and models (item 23) | `macos/**` | H2 | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && test "$(swift test list --package-path macos/HeronMac \| grep -c 'SelfImprovementTests')" -eq 5` | feature, refactoring: sonnet high |
| H11 | Observers, Observation pane, consent, exclusions, menu bar indicator (item 24) | `macos/**` | H6, H10, H5 | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && test "$(swift test list --package-path macos/HeronMac \| grep -c 'ObservationTests')" -eq 7` | hardest tasks: opus high |
| H7 | Threat-model sections and the two guides (item 25) | `docs/**` | H2, H10 | `grep -qx '## Harness2: code Heron writes into itself' docs/engineering/SECURITY_THREAT_MODEL.md && grep -qx '## Observation' docs/engineering/SECURITY_THREAT_MODEL.md && test -s docs/features/self_improvement.md && test -s docs/features/observation.md` | judgment and prose: opus high |
| verify | Full suite, test-quality gate, evaluator audits | `scripts/gates/harness2` (check only) | H1, H2, H3, H4, H5, H6, H7, H8, H8r, H10, H11 | `cargo test --locked --workspace && cargo clippy --locked --all-targets -- -D warnings && swift test --package-path macos/HeronMac && python3 -I -m unittest discover -s eval/tests -t eval && genesis audit-evaluator eval/selfheal/genesis.yaml && testteeth run --base main --fail-under 70 -q` | feature, refactoring: sonnet high |

**Run notes for the build session:**

*   H1, H2, H8, H10 and `verify` need `cargo test` outside the sandbox, and H6 and H11 need `swift build` outside it. The repository's memory notes record both failing inside node sandboxes. The skein supervisor runs those checks, as it did for v1.
*   These run by hand after `verify`, and their output goes in the build PR:
    *   the replay go/no-go;
    *   the genesis fence claim;
    *   sealbox's red-team run;
    *   the end-to-end CUJs.

    They need the owner's CLI login, the owner's TCC grants, or the sealbox checkout.
*   Give `verify` `--verify-timeout 3600`.

--------------------------------------------------------------------------------

*Drafted with EGM*
