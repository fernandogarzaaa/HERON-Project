# Heron Harness2: self-healing and self-evolution

*   **Author:** fernandogarzaaa (drafted with Claude)
*   **Status:** Draft, for the owner's approval
*   **Date:** 2026-11-22
*   **Builds on:** [heron-v1.md](heron-v1.md) (merged to `main` as `f732b4e`) and [heron-v1-features.md](heron-v1-features.md) (approved, being built)
*   **Read at:** `main` = `39dbdf5`

--------------------------------------------------------------------------------

## Overview

The owner asked for two switches in Settings, both off by default:

*   **Self-healing.** When the owner reports a bug, or Heron sees one of its own failures, Heron writes the fix and a regression test, checks it, rebuilds itself, and reinstalls itself in place at `/Applications/Heron.app`. The owner chose full autonomy for this loop, not proposal-only.
*   **Self-evolution.** A scout runs every 24 hours, 72 hours or week (the owner picks). It reads AI news and trending open-source repositories, and lists ideas that could improve Heron.

This doc designs both. Most of it is the set of rules that decide when a change may install itself and when it must wait for the owner. Those rules are proposals: the owner approves or changes them in review ([Owner questions](#owner-questions)).

Three facts shape the design, and the owner should weigh them before approving:

1.  **A sound fence leaves little to auto-install.** Security code is spread across most core files, not kept in a few. Of the last 30 `fix:` commits in this repository, **1** touched only files the proposed fence allows. With a looser, file-level fence, 23 of 30 would have qualified, but those 23 include Telegram credential, pairing-revocation and token-fencing fixes, which are security fixes. So in practice "full autonomy" mostly means "Heron opens a tested pull request". The installs it does on its own are mostly app-side UI fixes (HQ1).
2.  **The check that gates an install runs code a model wrote, outside any sandbox.** Heron's tests can't run under a nested Seatbelt sandbox. The memory notes for this repository record about 27 cargo test failures inside a sandboxed shell, and `swift build` failing there. This design accepts that risk only for runs whose every input is trusted, and names it (HQ2).
3.  **v1's full gate can't be the install gate.** `scripts/verify-v1.sh` needs 33 G7 runs on each Tier 1 CLI, the 14-day G9 trial, and a release build. It also throws away G7 results after any change under `src` (`scripts/verify-v1.sh:155`). The install gate in this doc is smaller. It loosens no v1 threshold, and [What an install does not prove](#what-an-install-does-not-prove) lists what it leaves unchecked.

## The Problem

Today a bug the owner hits in Heron waits until a coding session finds time for it. The fix then goes through a branch, a pull request, a build and a manual reinstall. The owner wants Heron to close that loop on the owner's own Mac.

Separately, the owner wants Heron to keep up with fast-moving agent tooling without reading the news by hand. Earlier research for this doc (summarised in [Additional Resources](#additional-resources)) found the useful parts are in the owner's own repositories, mainly `anchor-harness` and APE. Google's Harness² is not a drop-in. It tunes a harness for one task at a time, its executors bypass approvals and the sandbox, and Heron's 11-task G7 suite is too small to tune on without overfitting.

What makes this hard is that an agent that can change and reinstall itself is the strongest attack target Heron has. If outside content can steer a self-change, prompt injection becomes persistent code execution on the owner's Mac. It also becomes a public commit to a public repository: `fernandogarzaaa/HERON-Project` is public. Heron v1's standing decisions already say:

*   a tainted conversation's grants never reach the shell;
*   "never" rules are lifted only in the app;
*   memory never trusts outside content.

Harness2 must keep all three, including for code that Heron writes into itself.

## Goals

*   **G-H1.** On the owner's Mac, an owner-reported bug whose fix touches only allowlisted paths goes from report to a reinstalled, health-checked Heron with no further owner action. The fix comes with a regression test that fails on the old code, and a pull request into `main`.
*   **G-H2.** A fix that touches any fenced path, or that comes from any untrusted input, never installs itself. It ends as a pull request, or as a local diff the owner reviews first.
*   **G-H3.** A failed install reverts to the previous app by itself, and turns auto-install off until the owner turns it back on.
*   **G-H4.** Every installed self-change is a commit on its own branch, with a pull request into `main` and an entry in a local version history the owner can roll back from.
*   **G-H5.** The scout runs on the chosen schedule, uses fixed fetchers on an allowlist of hosts, and shows candidate cards. It never changes code, memory or settings.
*   **G-H6.** Heron's repair agent fixes at least 6 of the 10 historical bugs in the replay suite ([Verification](#verification-plan)) before the self-healing switch can be turned on (HQ10).
*   **G-H7.** Preview users get a "Report a problem" path that sends nothing on its own. Their reports reach the owner's Heron only through the owner.

## Non-Goals

*   **A Harness²-style optimizer.** Proposing and scoring prompt or skill edits across many rollouts is deferred until a task suite of 50 or more tasks exists. On 11 tasks, any tuning overfits.
*   **Self-healing on preview users' Macs.** They have no Rust or Xcode toolchain and no write access to the repository, and local changes would fork the code (see [Preview Macs](#preview-macs)).
*   **An auto-updater for preview users.** No updater exists today. Shipping one needs a signed, notarized update feed, which is its own design.
*   **Reading GitHub issues, PR comments or CI logs as triggers.** The repository is public, so anyone can write them. They are outside content, and Harness2 never reads them on its own.
*   **Changing any v1 threshold:** G7's 26/30, the 2/3 per-task minimum, testteeth's 70, or the containment check.
*   **Self-changes to Harness2 itself, to the fence, or to the gate.** These are always fenced.
*   **New Cargo or SwiftPM dependencies.**

## Definitions

| Term | Meaning here |
|---|---|
| Builder Mac | A Mac where Harness2 can build Heron: `[harness2] repo` points at a checkout of `fernandogarzaaa/HERON-Project`, and `git`, `gh` (logged in), `cargo`, `swift` and a code-signing identity are found. In practice, the owner's Mac. |
| Trigger | What starts a self-heal run: an owner report, a failure record, an imported preview report, or a scout candidate. |
| Failure record | A row Heron writes when its own code fails: a panic, a core exit, a failed migration, a failed health check. It holds only fields Heron's own code produces (see [Trust](#1-trust-which-inputs-may-drive-an-install)). |
| Trusted run | A run whose every model-visible input is trusted. Only a trusted run may execute model-written code on the Mac, and only a trusted run may install. |
| Fence | The compiled-in list of paths an auto-installed change may touch. Everything else is fenced, so the fence denies by default. |
| Install gate | `scripts/verify-selfheal.sh`: the checks an auto-install needs. |
| Repair run | One attempt by Heron's agent to fix one trigger, in a disposable home and a fresh git worktree. |
| Swapper | `heron-cli selfheal swap`: a copy of the old `heron-cli` that replaces the app bundle, relaunches it, health-checks it and reverts it on failure. |
| Version history | `~/.heron/selfheal/selfheal.db`: every installed build, with its commit, PR and gate report, plus the effect journal. |

## Owner questions

Every one has a proposed default, and the build uses the default unless the owner changes it. HQ1 to HQ5 are the decisions that shape the design. HQ6 to HQ11 are tuning.

*   **HQ1, how wide the fence is.** **Proposed: the tight allowlist in [Fence](#2-fence-what-an-auto-install-may-touch).** Under it, 1 of the last 30 fixes would have auto-installed (`41dd4bd`, a Mac app store fix), and the rest become PRs. The alternative is a file-level fence that fences only `sandbox.rs`, `permissions.rs`, `agent.rs`, `cli_llm.rs`, `memory.rs`, `mcp.rs`, `config.rs`, `store.rs`, `tools/` and `eval/`. Under it, 23 of 30 would have qualified, including `telegram.rs` credential and pairing fixes and `skills.rs` inventory fixes. The proposal fences those too: Telegram pairing is an approval surface, and skills are trusted prompt content. *Note: the 30 commits are a run of Telegram and skills hardening work, so the true share for everyday bugs is probably higher than 1 in 30. The build measures it on the replay suite.*
*   **HQ2, running model-written code unsandboxed.** For trusted runs, the install gate runs `cargo test`, `swift test` and the new regression test outside any sandbox, because those tests fail under nested Seatbelt. **Proposed: accept this for trusted runs only, and write it up as a section of `docs/engineering/SECURITY_THREAT_MODEL.md`, not as a new `## R-` entry.** Reason: `docs/ACCEPTED_RISKS.md` is the risk notice every preview user accepts (`src/risk_notice.rs:9`). Adding to it would make every preview user accept again, for a risk only the builder Mac carries. It is also capped at 3 entries (`scripts/verify-v1.sh:248-249`). Code from untrusted runs never runs on the Mac before the owner has read it ([Trust](#1-trust-which-inputs-may-drive-an-install)).
*   **HQ3, unattended runs and the Q6 hold.** Heron's v1 rule (Q6) is that unattended work never calls a subscription model unless `allow_unattended_subscription` is set (`src/app.rs:1510-1513`). A failure record and a scout tick are unattended. **Proposed: Harness2 obeys the hold.** A held failure record becomes an inbox note, "Heron found a bug in itself. Fix it?", and the owner's click starts the run. A held scout tick waits until the app is opened. An owner report is attended and runs at once. The alternative is a separate `harness2.allow_unattended` switch.
*   **HQ4, failures in tainted sessions.** A failure record from a tainted session holds only Heron-owned fields, but an attacker could have caused it on purpose. **Proposed: the run is still trusted, because its inputs are Heron-owned, but its result can't install itself. It ends as a PR.**
*   **HQ5, signing.** `build-app.sh --unsigned` signs ad hoc. An ad-hoc signature is a hash of the binary, so every rebuild is a new app to macOS privacy permissions (TCC) and keychain access lists. Grants such as Screen Recording (F9), Automation (F5) and Notifications could reset after every self-fix. The upgrade test doesn't cover the keychain (`packaging/macos/upgrade-test.sh`, header). **Proposed: auto-install requires `[harness2] codesign_identity`, a stable identity such as a free "Apple Development" certificate from Xcode's personal team. Without one, runs still end as PRs, and Settings says why.** Prototype P2 confirms whether a stable identity keeps the grants.
*   **HQ6, which model repairs.** **Proposed: the active Tier 1 CLI (claude-cli today), through Heron's normal CLI path, with that CLI's default model. codex-cli is the fallback when the first is limited.**
*   **HQ7, caps.** **Proposed:**
    *   at most 3 attempts per trigger;
    *   at most 40 steps and 500,000 tokens per attempt;
    *   at most 3 repair runs and 2 installs in any 24 hours;
    *   at most 2 auto-installed PRs still unmerged; past that, new fixes become PR-only;
    *   one scout run per tick, at most 60 fetched items, one model call of at most 50,000 tokens.

    Every run also counts against the existing usage budget (`src/usage.rs`).
*   **HQ8, scout sources and default schedule.** **Proposed sources:**
    *   GitHub's search API: repositories created or pushed in the last 7 days with the topics `llm-agent`, `ai-agent`, `agent-framework` or `mcp`, sorted by stars. GitHub has no trending API.
    *   Hacker News, through the Algolia API: stories with 100 or more points matching "agent", "LLM" or "MCP".
    *   arXiv: the `cs.AI`, `cs.CL` and `cs.SE` listings, filtered for "agent" in the title.
    *   The news and engineering feeds of Anthropic, OpenAI and Google DeepMind.

    **Proposed default schedule: weekly.**
*   **HQ9, what goes into a public PR.** The repository is public. **Proposed:**
    *   a PR body holds the gate report, the changed files, and a description written from the diff alone;
    *   the owner's report text goes in only when the owner ticks "OK to publish this report" (off by default);
    *   a redaction scan runs over the diff and the PR text before any push, and a hit stops the push and asks the owner.
*   **HQ10, the replay go/no-go threshold.** **Proposed: 6 of 10.** Below that, the self-healing switch stays disabled, with the reason shown.
*   **HQ11, how long the rollback archive lives.** **Proposed: 24 hours of the new build running, then it is deleted.** Older builds come back through "Undo last self-fix", which rebuilds the previous commit from git.

## Critical User Journeys (CUJs)

### CUJ H-1: The owner reports a UI bug, and Heron fixes and reinstalls itself

*   **Given:** a builder Mac with self-healing on, a signing identity set, no run active and no approval waiting.
*   **When:** the owner chooses Help › Report a problem, types "The Usage screen shows 0 tokens after a long chat", and sends.
*   **Then:**
    1.  Heron makes a worktree at the installed commit and runs a repair.
    2.  The agent changes `macos/HeronMac/Sources/HeronUI/Stores/ContentStores.swift` and adds a test under `macos/HeronMac/Tests/`.
    3.  The fence passes. The new test fails on the old code and passes on the fix. The install gate passes.
    4.  Heron pushes a branch and opens a PR labelled `self-heal:auto`.
    5.  The app shows "Installing fix #7. Heron will restart", quits, and comes back. The swapper's health check passes.
    6.  The inbox shows "Fixed: Usage screen total (PR #NN). Undo last self-fix".

    This takes about 1 to 2 hours, most of it the G7 smoke stage. Prototype P4 measures the real time.

### CUJ H-2: The fix touches fenced code

*   **Given:** the same Mac.
*   **When:** the owner reports "Telegram pairing asks twice".
*   **Then:** the fix touches `src/telegram.rs`. The fence marks the run PR-only. The gate still runs, because the run is trusted. Heron opens a PR labelled `self-heal:needs-owner` with the gate report, and the inbox says "Fix ready for review: PR #NN. Not installed: touches src/telegram.rs." Nothing is installed.

### CUJ H-3: The new build fails its health check

*   **Given:** an auto-install has started.
*   **When:** the new app starts, but its core never writes its port file within 90 seconds.
*   **Then:**
    1.  The swapper stops the new app, puts back the archived previous app, and relaunches it.
    2.  Auto-install turns off.
    3.  The PR gets a comment: "Auto-install failed its health check (core did not start); reverted."
    4.  The inbox says the same, with a "Turn auto-install back on" button.

### CUJ H-4: The scout finds a candidate, and the owner wants it built

*   **Given:** self-evolution is on, with the weekly schedule.
*   **When:** the scout runs and lists "RepeatGuard-style A-B loop detection (from a trending repository)".
*   **Then:**
    1.  The candidate card shows its source link, the part of Heron's harness it touches, the effort, and a "from the web" marker.
    2.  The owner clicks "Build this". Heron runs a repair with read-only tools, because the card is outside content. The agent returns a diff, and Heron applies it to a local branch.
    3.  The app shows the diff. Nothing runs and nothing is pushed.
    4.  Only after reading the diff does the owner click "Run checks" (the install gate, PR-only) or "Open draft PR". It never installs itself.

### CUJ H-5: A preview user reports a problem

*   **Given:** a preview Mac with no toolchain.
*   **When:** the user chooses Help › Report a problem.
*   **Then:**
    1.  Heron writes `Heron-report-<date>.heronreport` to the Desktop. It holds the user's text, the failure records from the last 7 days, the version and commit, and conversation text only if the user ticked "Include this conversation".
    2.  Heron opens the macOS share sheet. Nothing is sent automatically.
    3.  The user sends the file to the owner however they usually talk.
    4.  The owner drops it on their Heron (File › Import report). It is treated as outside content, so the run goes no further than a local diff the owner reviews (as in CUJ H-4).
    5.  Once the owner merges a fix, it reaches preview users in the next DMG the owner sends.

## Technical Plan (Architecture)

### 0. Shape

```text
 owner's Heron.app (builder Mac)
 ┌─────────────────────────────────────────────────────────────────────────────────┐
 │ SwiftUI app: Settings › Self-improvement · Report a problem · cards · diff view │
 └───────────────▲──────────────────────────┬──────────────────────────────────────┘
                 │ /api/harness2/*          │
 ┌───────────────┴──────────────────────────▼──────────────────────────────────────┐
 │ Rust core: src/harness2/                                                        │
 │  triggers ──▶ trust ──▶ repair runner ──▶ fence ──▶ gate ──▶ publish ──▶ install│
 │  (report,    (decision   (heron-cli       (compiled  (verify-   (git push, (swapper│
 │   failure,    table)      selfheal repair  allowlist) selfheal   gh pr)     copy of│
 │   import,                 in a disposable                .sh)               old    │
 │   scout)                  home + worktree)                                  heron-cli)
 │  scout: fixed fetchers ──▶ tool-less ranking call ──▶ candidate cards           │
 │  selfheal.db: version history + effect journal (port of anchor-harness)         │
 └─────────────────────────────────────────────────────────────────────────────────┘
```

Harness2 is a new core module, `src/harness2/`, plus one subcommand group, `heron-cli selfheal`. The repair agent is Heron's own agent loop, not a second coding agent (see [Alternative A](#alternative-a-a-headless-coding-cli-with-its-own-tools-does-the-fix)). The gate is a fixed script that Harness2 runs, never the agent. The swapper is a copy of the old binary, so the old code judges the new one.

### 1. Trust: which inputs may drive an install

The trust of a run comes from the class of every input the model sees in it. It does not come from who clicked. An owner click on outside content doesn't make that content trusted. This follows v1's rule that taint never clears.

| Trigger | What the model sees | Run class |
|---|---|---|
| Owner report, typed in the app or with `heron-cli report` | The owner's text; the version and commit; the failure records of the last 24 hours | Trusted |
| Owner report with "attach this conversation" ticked | As above, plus that conversation | Trusted if the conversation is untainted; otherwise **untrusted** |
| Failure record | Only these fields: `kind` (from a fixed enum: `panic`, `core_exit`, `migration_failed`, `health_failed`, `internal_error`), `location` (the `file:line` from Rust's panic info, or Heron's own error site), `message_template` (the Heron format string, with every argument replaced by `<arg>`), `version`, `commit`, `count`, and `origin_tainted`. Never tool output, model text, MCP or HTTP error bodies, file names or paths from tools, or Telegram text. | Trusted. If `origin_tainted`, PR-only (HQ4) |
| Imported preview report | Another person's text and records | **Untrusted** |
| Scout candidate | Text derived from web pages | **Untrusted** |

**The decision table.** This table is the rule. The test `decision_table_selfheal` checks every row.

| # | Run class | Paths touched | Other state | Install gate | Outcome |
|---|---|---|---|---|---|
| 1 | Trusted | All allowlisted, content rules met | Under every cap, signing identity set, not `origin_tainted` | Passes | **Auto-install**, PR labelled `self-heal:auto` |
| 2 | Trusted | Any fenced path, or a content rule broken | any | Passes | PR labelled `self-heal:needs-owner`; not installed |
| 3 | Trusted | All allowlisted | A cap is reached, no signing identity, `origin_tainted`, or auto-install turned off after a failed install | Passes | As row 2, and the PR says which condition applied |
| 4 | Trusted | any | any | Fails after 3 attempts | No PR. An inbox note with the last gate failure. The branch is kept locally for 7 days |
| 5 | Untrusted | any | any | **Not run** | Local branch plus a diff in the app. "Run checks" (gate, then row 2 at best) or "Open draft PR". Never installs |
| 6 | any | any | A run is active, an approval is waiting, or Heron is paused | — | The trigger waits in a queue |
| 7 | any | any | Held by Q6 (HQ3) | — | Inbox note; the owner's click starts it |

Row 5 is where the trust split does its work. An untrusted run's repair session is tainted from its first message. Under v1's rule 6, once outside content is present nothing is changed without the user's approval, in every mode (`src/permissions.rs:391-395`). So an untrusted run gets only the four read tools, and ends its turn with a unified diff as text. Harness2's own code then applies that diff to the scratch worktree with `git apply --check` followed by `git apply`. That worktree is a draft nothing runs, and the owner's review of the diff is the approval rule 6 asks for. Harness2 doesn't run the gate on that code, because the gate runs unsandboxed. It doesn't push it either, because pushing to a public repository starts CI with that code. The owner reads the diff first. "Run checks" after reading is an owner decision about code they have seen.

### 2. Fence: what an auto-install may touch

The fence is a `const` list in `src/harness2/fence.rs`, so it is compiled into the installed binary. The **running** Heron checks it against `git diff --name-status <base> <fix>` for the exact commit it then gates, builds and installs. The worktree's copy of `fence.rs` is never read. A fix can't widen its own fence, because `src/harness2/**` is fenced.

**Allowed.** Every changed path must match one of these, and no deny entry may match it:

*   `macos/HeronMac/Sources/HeronUI/**`
*   `macos/HeronMac/Sources/HeronCore/**`
*   `macos/HeronMac/Tests/**`
*   `src/ui/**`, `src/schedule.rs`, `src/events.rs`, `src/embed.rs`, `src/eval.rs`
*   `tests/**`, except `tests/fixtures/**`
*   `docs/**`

**Denied, even inside an allowed directory.** A deny entry wins over an allow entry.

*   `macos/HeronMac/Sources/HeronUI/Views/ApprovalSheetView.swift`, `.../NeverViews.swift` and `.../FirstRunViews.swift`. These are the approval sheet, the "never" rules and the risk notice.
*   `macos/HeronMac/Sources/HeronUI/Stores/ChatAndApprovalsStores.swift`
*   `macos/HeronMac/Sources/HeronCore/CoreProcess.swift` and `.../ManagedCoreClient.swift`. These start the core and hold its token.
*   `macos/HeronMac/Sources/HeronUI/SelfImprovement/**`, which is Harness2's own app code.
*   `docs/ACCEPTED_RISKS.md`, the risk notice text.
*   `docs/engineering/SECURITY_THREAT_MODEL.md` and `docs/designs/**`.

**Fenced by default.** Everything else is fenced. That includes:

*   every other file in `src/`, including `agent.rs`, `permissions.rs`, `sandbox.rs`, `cli_llm.rs`, `memory.rs`, `mcp.rs`, `config.rs`, `store.rs` (migrations), `telegram.rs`, `server.rs`, `skills.rs`, `util.rs` (secret redaction), `usage.rs` (budgets), `tools/**` and `harness2/**`;
*   `eval/**`, `scripts/**`, `packaging/**` and `.github/**`;
*   `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, any `build.rs`, `Package.swift`, `*.plist` and `*.entitlements`;
*   `macos/HeronMac/Sources/Heron/**`, the app's entry point.

**Content rules.** They are checked by the same code, and any broken rule makes the run PR-only:

*   **R1, add-only tests.** Under `tests/**` and `macos/HeronMac/Tests/**`, the diff has no removed lines. Every test name listed on `base` (from `cargo test --locked -- --list` and `swift test list`) is still listed on `fix`.
*   **R2, a regression test that bites.** At least one new test file exists under `tests/` or `macos/HeronMac/Tests/`. The gate copies only the new and changed test files onto a checkout of `base`, which must compile, and the new tests must **fail** there. On `fix` they must pass. A fix whose test needs a new API can't meet R2, so it becomes a PR.
*   **R3, no deletions, renames or binary files.**
*   **R4, a size cap.** At most 400 changed lines.
*   **R5, a clean redaction scan.** The diff passes the existing secret redaction check (`src/util.rs`) with no hit.

**The cost of this fence.** Most core bug fixes become PRs. That cost is chosen on purpose and stated in HQ1. It can't be made finer-grained: the security parts of `agent.rs` (prompt safety, taint and the delegation limits) live in the same file as ordinary loop code. A fence on parts of a file would be a prompt instruction, and v1 holds that prompts are not a defence (`src/permissions.rs:391-393`).

### 3. The repair run

A repair run is one Heron agent turn, run by `heron-cli selfheal repair --run <id>` as a separate process. It is set up as follows:

*   **Home:** a disposable `HERON_HOME` at `~/.heron/selfheal/runs/<id>/home`, created fresh, like the eval runner's disposable home (`eval/run_core_tasks.py`). It holds no memories, persona, connectors, Telegram or MCP.
*   **Workspace:** a new git worktree at `~/.heron/selfheal/runs/<id>/wt`, on the branch `heron/selfheal/<yyyymmdd>-<id>`, cut from the **installed commit**. That commit is the `commit` field this doc adds to `GET /api/state`. Fixing what the owner runs avoids mixing in unreleased work from `main`.
*   **Tools:** for a trusted run, only `read_file`, `write_file`, `edit_file`, `list_dir`, `search_files`, `find_files` and `shell`. For an untrusted run, only `read_file`, `list_dir`, `search_files` and `find_files`, and the turn ends with a unified diff (§1, row 5). The registry is filtered when `App::assemble` builds it, so the model never sees any other tool.
*   **Policy:** a fixed `PermissionConfig` compiled into `src/harness2/repair.rs`, with `mode = "auto"` and these allow rules:
    *   `shell:cargo test --locked*`
    *   `shell:cargo check --locked*`
    *   `shell:cargo clippy --locked*`
    *   `shell:git diff*`
    *   `shell:git status*`

    All other shell calls are denied. Compound commands are never covered (`src/agent.rs:134-149`). The shell runs under Heron's normal Seatbelt profile, with writes only inside the workspace. For an untrusted run, the session starts tainted, so no shell grant applies at all.
*   **Prompt:** a fixed repair prompt in `src/harness2/repair.rs` gives the trigger's fields, says "Fix the cause, add a test under `tests/` or `macos/HeronMac/Tests/` that fails before your fix, and do not edit test files that already exist", and lists the allowed paths, so the agent can aim for a fix that installs itself. That list is advice. The fence still decides.
*   **Model:** the HQ6 provider through `cli_llm`, so all five CLI containment invariants hold (`src/cli_llm.rs:30-50`).
*   **Retries:** up to 3 attempts. Attempts 2 and 3 get the previous gate failure, cut to the failing step's last 60 lines, as Harness²'s "actionable retry feedback". That text comes from Heron's own gate output on trusted code, so it is trusted.

The agent's own `cargo test` runs are advisory. Under the nested sandbox, about 27 known tests fail. The repair prompt names the filter to use (`cargo test --locked --test <name>`), and the gate is what counts.

### 4. The install gate

`scripts/verify-selfheal.sh <base> <fix> <worktree>` runs **only for trusted runs**. Harness2 runs it as a child process outside the sandbox (HQ2), in this order. It stops at the first failure.

1.  **Fence and content rules.** Run by the installed `heron-cli selfheal check-fence <base> <fix>`, not by the worktree's code. The script records the verdict (`auto` or `pr-only`), and every later step still runs, so a PR-only fix also gets a full report.
2.  **Build and lint:** `cargo build --locked --release`, `cargo clippy --locked --all-targets -- -D warnings`, and `swift build -c release --package-path macos/HeronMac`.
3.  **Tests:** `cargo test --locked`, `cargo test --locked --features desktop --bin heron-desktop`, `swift test --package-path macos/HeronMac`, and R2's base-fails and fix-passes run.
4.  **Mutation grade of the diff:** `testteeth run --base <base> --fail-under 70 -q`. This is the same threshold as v1 step 4.
5.  **Containment:** `python3 -I eval/containment/check.py` (v1's G4, unchanged).
6.  **G7 smoke, on the active Tier 1 CLI:** one run of each of the 10 gate tasks and p1, through `eval/run-core-tasks.sh --runs 1`.
    *   Every task that passed in that CLI's last stored full G7 result must pass. A task that fails is run once more, and a second failure fails the gate.
    *   t09, t10 and p1 must pass, whatever the stored result says.
    *   This is a new non-regression rule in a fenced script. It doesn't use or change `check_gate.py`'s 26/30 rule.
7.  **App build and upgrade test:** `packaging/macos/build-app.sh --identity "$HARNESS2_CODESIGN_IDENTITY"` (a new flag, below), then `packaging/macos/upgrade-test.sh`.

The gate writes `gate-report.json` to the run folder. It records, for each step, the exit code, the duration and the last 60 lines. Its SHA-256 goes into the version history.

#### What an install does not prove

An auto-installed build has **not** passed:

*   G7 at 3 runs per task on every Tier 1 CLI;
*   G9, the owner-use trial;
*   the `verify-v1.sh` G3 record;
*   the genesis audits.

The PR into `main` carries those checks, because `main`'s branch protection and the owner's merge still apply. Until the PR merges, the owner runs a build that has passed a smaller gate, and the inbox and Settings say so ("Running 1 self-fix not yet in main").

### 5. Publishing

All publishing happens after the gate, for rows 1 to 3 only, and every step is journaled (§7):

*   The worktree is committed as `fix(<area>): <summary>`. The message is written from the diff by a tool-less model call, then put through the R5 redaction scan.
*   The branch is pushed to `origin`.
*   Heron runs `gh pr create --base main --head heron/selfheal/<…> --label self-heal:auto|self-heal:needs-owner --body-file <file>`.
    *   When an earlier self-heal PR is still open and the new branch was cut on top of it, the new PR's `--base` is that earlier branch, so each PR shows only its own diff.
    *   The body holds the gate report summary, the changed files, the decision table row, and the owner's report text only with the HQ9 tick.
*   On every tick of the scheduler, Heron checks open self-heal PRs with `gh pr view --json state`:
    *   **merged:** the version history marks it in `main`;
    *   **closed without merging:** the inbox offers "Return to main". That rebuilds and installs `origin/main` through the same gate and swapper. Nothing reverts without the owner's click, because the owner may have closed it only to reword it.

### 6. Install and rollback

The **swapper** is `heron-cli selfheal swap --run <id>`. Before anything else, the core copies its own `heron-cli` to `~/.heron/selfheal/swapper/heron-cli` and starts it from there, detached (`setsid`, so it survives both the app and the core exiting). The old code then judges the new build. The swapper steps are:

1.  **Preconditions:** no active run, no waiting approval, not paused. Otherwise it waits up to 30 minutes, then gives up and leaves an inbox note.
2.  **Journal** `swap_intent` with the expected commit (§7).
3.  **Ask the app to quit.** `POST /api/harness2/quit-for-install` publishes an `install_pending` event. The app shows "Installing fix #N. Heron will restart" and terminates, and the core exits with it. The swapper waits for both PIDs, up to 60 seconds, then sends `SIGTERM`.
4.  **Archive** `/Applications/Heron.app` with `ditto -c -k --keepParent` to `~/.heron/selfheal/previous/Heron-<commit>.zip`. It is a zip, not a bundle, so there is still exactly one launchable Heron.app.
5.  **Back up the data** with `Store::backup` (`src/store.rs:665`) to `~/.heron/backups/heron-before-selfheal-<id>.db`. Record `PRAGMA user_version`.
6.  **Swap.**
    *   `ditto` the new bundle to `/Applications/.Heron-new.app`.
    *   `rename(2)` the old bundle to `/Applications/.Heron-old.app`.
    *   `rename(2)` the new bundle to `/Applications/Heron.app`.
    *   Remove `.Heron-old.app`. The archive from step 4 is the rollback copy.
7.  **Relaunch** with `/usr/bin/open -a /Applications/Heron.app`.
8.  **Health check**, within 90 seconds:
    *   the core's port file appears;
    *   `GET /api/state` answers, with `commit` equal to the expected commit and `risk_notice_accepted` unchanged;
    *   `heron-cli audit` prints "Activity log intact";
    *   `PRAGMA user_version` is unchanged;
    *   the app process is still alive 30 seconds after the core answered.
9.  **Pass:** journal `swap_done`, write the version history row, and schedule the archive's deletion after 24 hours of uptime (HQ11).
10. **Fail:**
    *   Stop the new app. Unzip the archive back into place. Restore the data backup only if the audit or `user_version` check failed. Relaunch.
    *   Journal `swap_reverted`, set `auto_install = false`, comment on the PR, and leave an inbox note (CUJ H-3).

**"Undo last self-fix"** in Settings rebuilds the previous version-history commit through the gate and the swapper. It doesn't depend on the 24-hour archive.

**`/Applications` permissions.** The swapper needs write access to `/Applications/Heron.app` and its parent. Prototype P3 checks this on the owner's Mac. If the folder isn't writable, auto-install is unavailable, and Settings says so.

### 7. Version history and effect journal

`~/.heron/selfheal/selfheal.db` is a separate SQLite file from `heron.db`. A data restore never rolls it back, and the swapper reads it without opening the owner's data. It has these tables:

*   `versions(commit, branch, pr_url, gate_report_sha256, installed_at, previous_commit, state)`, where `state` is one of `installed | reverted | in_main | closed`. This is the idea of anchor-harness's content-addressed version store with rollback to an exact hash (`crates/harness-core/src/versions.rs:31`, `:122`), with the git commit as the hash.
*   `effects(key, run_id, step, state, detail, at)` is a port of anchor-harness's effect journal (`crates/harness-core/src/effects.rs`: `begin_effect` at `:113`, `commit_effect` at `:174`, `mark_effect_unknown` at `:179`, `reconcile_effect` at `:184`, `unresolved_effects` at `:208`).
    *   The steps are: `worktree`, `commit`, `push`, `pr`, `archive`, `backup`, `swap_intent`, `swap_done`, `swap_reverted`.
    *   `state` is one of `begun | committed | unknown`.
*   `triggers(id, kind, class, fields_json, state, attempts, created_at)`
*   `candidates(id, source, url, title, harness_part, why, effort, created_at, state)`
*   `seen(url_sha256, first_seen)`

**Crash recovery.** On every core start, `harness2::recover()` reads the unresolved effects:

*   **`swap_intent` with no `swap_done`:** if the installed app's commit differs from the expected one and the archive exists, it restores the archive. This is the "crashed mid-swap" case.
*   **`push` or `pr` left `unknown`:** it reconciles them by asking git and `gh` (`git ls-remote`, `gh pr list --head`) before trying again, so a PR is never opened twice.

The journal and the version history use only `rusqlite`, which is already a dependency.

### 8. The scout (self-evolution)

*   **Schedule.** `schedule.rs` can't express 72 hours: `every 72h` fails with "does not divide evenly" (`src/schedule.rs:250-256`). So the scout keeps its own timer instead of a row in `schedules`. `[harness2] scout_every_hours` is 24, 72 or 168, and `scout_last_run` is stored in `selfheal.db`. The app's existing scheduler tick (`App::run_due_schedules`, `src/app.rs:1483`) calls `harness2::scout_due()`. The scout obeys pause and the Q6 hold (HQ3). The scout is not a user schedule, so automations and the agent can't edit it.
*   **Fetchers.** Fixed code, one per source (HQ8), using `reqwest`. Hosts are allowlisted in `src/harness2/scout.rs`: `api.github.com`, `hn.algolia.com`, `export.arxiv.org`, `www.anthropic.com`, `openai.com` and `deepmind.google`.
    *   GitHub and Hacker News return JSON.
    *   arXiv and the lab feeds are Atom or RSS. A tolerant reader using the existing `regex` crate takes only `title`, `link` and `updated`/`pubDate` from each `<entry>` or `<item>`, and skips any entry it can't parse. No new dependency.
    *   Each response is capped at 1 MB. URLs already in `seen` are dropped.
*   **Ranking.** One model call with **no tools**, in a session marked tainted from the start:
    *   **Input:** a fixed description of Heron's harness parts (steps and limits, tools, delegation, long conversations, safety, CLI models, memory, eval) and the fetched items, each with an `item_id`, wrapped as untrusted content in the usual way (`src/agent.rs:692-702`).
    *   **Output:** JSON of the form `[{item_id, harness_part, why, effort: S|M|L, change}]`, at most 10 entries.
    *   **Validation:** each `item_id` must be one that was fetched. The card's link and title come from the fetched data, never from the model, so the model can't put links on cards. Invalid entries are dropped.
*   **Output.** Rows in `candidates`, shown in the app as cards marked "from the web". The scout writes no memory, skills, settings, schedules or code. Cards expire after 30 days.
*   **"Build this"** starts an untrusted repair run (decision table row 5) with the card's `change` text as the trigger. Building on research already done, the cards for the anchor-harness ports (RepeatGuard, the completion check, protected paths) are seeded as built-in candidates, so the first scout run has something real to show.

### 9. Settings and the app

A new pane, **Settings › Self-improvement**, in `macos/HeronMac/Sources/HeronUI/SelfImprovement/`:

*   **Self-healing** (switch, off by default). Below it is a checklist built from `GET /api/harness2`: source folder, Rust, Xcode tools, GitHub login, signing identity, write access to `/Applications`, and the replay result (G-H6). The switch is disabled until every item passes, and each failing item says how to fix it. Below the switch:
    *   **Install fixes automatically** (on, once self-healing is on). Turning it off makes every run PR-only. A failed install turns it off (G-H3).
    *   A status line, "Running N self-fixes not yet in main", plus **Undo last self-fix** and **Return to main**.
*   **Self-evolution** (switch, off by default), and **Check every** with the choices 24 hours, 72 hours and Week (HQ8 default: Week). Below them, a **Last run** line and the candidate cards.
*   **Help › Report a problem** is on every Mac. On a builder Mac with self-healing on, it starts a trigger. Otherwise it writes the `.heronreport` file and opens the share sheet (CUJ H-5).
*   **File › Import report** appears on builder Macs only.
*   **Diff view** for untrusted runs (row 5): a read-only, file-by-file diff, with "Run checks", "Open draft PR" and "Discard".

All Harness2 actions are app-only. Telegram and the HTTP chat can't start, approve or install a self-change. This matches the v1 rule that "never" rules are lifted only in the app, and Harness2 is a stronger power than lifting a "never" rule.

### 10. Preview Macs

*   **Self-healing.** On a Mac that isn't a builder Mac, the switch is replaced by one line: "Self-healing runs on the Mac that builds Heron. Use Help › Report a problem to send a report." Fixes reach preview users as a new DMG from the owner, the way updates reach them today. An updater is a Non-Goal.
*   **Self-evolution.** Hidden. The candidates are about Heron's code, which a preview user can't change.
*   **Report a problem.** As in CUJ H-5. The report file is the only thing that leaves the preview Mac, and the user chooses where it goes.

### 11. Changes to v1 code, and the definition of done

Harness2 changes some fenced files, and this doc is the owner approval those changes need:

*   `GET /api/state` gains `commit`. It comes from `option_env!("HERON_COMMIT")`, which `build-app.sh` sets. Without it, the value is `"unknown"`.
*   `build-app.sh` gains `--identity <name>`, which signs like `--unsigned` but with a stable identity (HQ5).
*   `App::assemble` gains a tool filter for the repair profile.
*   `run_due_schedules` calls `scout_due()` and `harness2::tick()`, which polls PRs and starts queued triggers.

Panic and failure records are written by a panic hook and by the existing core-exit and migration error paths.

The v1 features doc's eight checks ([heron-v1-features.md, "The extended definition of done"](heron-v1-features.md#the-extended-definition-of-done)) apply here as follows:

| Check | Harness2 |
|---|---|
| 1. New tools | None for the model. The repair profile reuses six fs tools and `shell` |
| 2. Decision-table rows | `decision_table_selfheal` (the [trust table](#1-trust-which-inputs-may-drive-an-install), all 7 rows) and `decision_table_repair_profile` (each allow rule, a compound command, and a tainted session) |
| 3. Prompt blocks | The repair prompt is a new Trusted `PromptSource::Repair`. The scout's items are Outside content |
| 4. EVE | One EVE task: report a problem on a preview build and check that a file is written and nothing is sent |
| 5. Content entries | Owner report (trusted); failure record (trusted fields); imported report (tainting); scout fetch (tainting) |
| 6. Approval surfaces | The Self-improvement pane, Report a problem, the diff view, and Import report. App only; no `Effect` is answerable from Telegram |
| 7. Processes outside the `shell` tool | `git` (worktree, commit, push, ls-remote, diff), `gh` (pr create, view, list, comment), `cargo`, `swift`, `testteeth`, `python3 -I`, `bash scripts/verify-selfheal.sh`, `codesign` (through `build-app.sh`), `ditto`, `/usr/bin/open -a /Applications/Heron.app`, and the swapper. Each has a fixed path, fixed argument shapes, an environment of `PATH`, `HOME` and `TMPDIR` plus `GH_*` for `gh` only, and runs unsandboxed only for trusted runs (HQ2) |
| 8. TCC | None new. HQ5 is about keeping the existing grants |

## Alternatives Considered

### Alternative A: A headless coding CLI with its own tools does the fix

Run `claude -p --permission-mode acceptEdits --allowedTools …` in the worktree, the way skein's `claude_build` backend built v1.

*   **Pros:** a stronger coding agent today, and it is proven on this repository.
*   **Cons:**
    *   It breaks CLI containment invariant 3, "The CLI is a text model for Heron, not a second agent" (`src/cli_llm.rs:40-43`).
    *   It adds a second permission system that Heron's audit log, taint and approvals don't see.
    *   The owner asked for Heron itself to do the fix.
*   **Reason for rejection:** it gives up the containment v1 was built on, for a capability gap that the replay suite (G-H6) measures. If Heron's own loop scores below the HQ10 threshold, this alternative comes back as an owner question, not as a quiet switch.

### Alternative B: Run the install gate inside Heron's Seatbelt profile

*   **Pros:** no unsandboxed execution of model-written code.
*   **Cons:**
    *   About 27 Heron tests fail under a nested sandbox, and `swift build` dies on its own `sandbox-exec` (repository memory notes, 2026-11-19).
    *   The gate would have to skip those tests, which is the "skips pass green" hole v1 closed with `HERON_REQUIRE_SEATBELT=1` (`scripts/verify-v1.sh:40-44`).
*   **Reason for rejection:** a gate that skips tests proves less than it claims. Untrusted code instead never reaches the gate (row 5). Trusted code's risk is named (HQ2).

### Alternative C: Proposal-only for everything

Every self-change becomes a PR, and nothing installs itself.

*   **Pros:** the simplest and safest design.
*   **Cons:** the owner explicitly chose full autonomy for trusted bugs.
*   **Reason for rejection:** an owner decision. With the HQ1 fence, the design is already close to this for core code. Autonomy remains where a mistake is cheapest: app UI, tests and docs.

### Alternative D: The full v1 gate (`verify-v1.sh`) before every install

*   **Pros:** an install would mean the same as a release.
*   **Cons:**
    *   66 or more G7 runs across two CLIs per fix.
    *   It needs a G9 trial that takes 14 days.
    *   It discards G7 results on any `src` change (`scripts/verify-v1.sh:155`).
*   **Reason for rejection:** it can't finish in a day. The PR into `main` still gets the full gate.

### Alternative E: Run Google's Harness², or a port of APE's propose-probe-compose loop, as self-evolution

*   **Pros:** it has published gains, and APE is the owner's own working implementation.
*   **Cons:**
    *   It tunes per task.
    *   Its executors bypass approvals and the sandbox.
    *   About 4,000 rollouts per task.
    *   Heron's editable prompt surface is compiled Rust.
    *   With 11 G7 tasks, any adopted edit overfits.
*   **Reason for rejection:** it needs a suite of 50 or more tasks that doesn't exist yet. What this doc takes from it: actionable retry feedback (§3), the rule that lessons from outside content are untrusted (§1, from APE's `lessons.js`), and validation of the whole change before adoption (§2). See [Future Improvements](#future-improvements).

### Alternative F: Accept bug reports as GitHub issues

*   **Pros:** standard tooling, and visible to others.
*   **Cons:**
    *   The repository is public, so a preview user's report would publish their text.
    *   Anyone can open an issue, so issues are outside content anyway.
*   **Reason for rejection:** privacy for preview users, and no gain in trust. The `.heronreport` file plus the owner's import does the same job privately.

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Prompt injection becomes persistent code | Untrusted runs never execute, push or install code (row 5). The scout's links come from fetched data, not the model. Taint never clears on an owner click |
| A fix weakens a security check | Default-deny fence compiled into the installed binary, checked against the exact gated commit. `src/harness2/**` and the gate scripts are fenced. R1 (add-only tests) and R2 (the new test must fail on base) |
| Model-written tests execute unsandboxed (trusted runs) | Only trusted inputs reach the model in those runs. Named in the threat model (HQ2). Caps (HQ7) |
| A broken build replaces the app | The old binary health-checks the new one, then reverts automatically and turns auto-install off. The effect journal recovers a mid-swap crash |
| Privacy grants or keychain access reset on rebuild | A stable signing identity is required for auto-install (HQ5); prototype P2 checks it |
| Private text published in a public PR | Report text stays out by default. R5 redaction scan before every push. The commit message is written from the diff |
| The installed build drifts from `main` | At most 2 unmerged auto-PRs (HQ7). Settings shows the count. "Return to main" |
| Cost and subscription limits | Caps (HQ7), the usage budget, the Q6 hold (HQ3), and the G7 smoke at one run per task |
| The repair agent is too weak to help | The replay suite gates the switch (G-H6). Below the threshold, Alternative A returns as an owner question |
| The gate is wrong in a way that passes bad fixes | genesis audits the fence checker and the replay grader ([Verification](#verification-plan)). testteeth grades the Harness2 code itself |

## Non-Functional Requirements

*   **Security and privacy:** covered above. The threat model gains a section "Harness2: code Heron writes into itself".
*   **Time:** a trusted run is expected to take 1 to 2 hours end to end. P4 measures it. The swap itself, from quit to healthy, should take under 2 minutes.
*   **Storage:**
    *   A worktree with `target/` is about 3 to 5 GB. Worktrees are removed when the run ends: after 7 days for row 4, and once the PR is opened for the other rows.
    *   One archived app, about 60 MB, lives for 24 hours.
*   **Rollout:**
    *   Both switches ship off.
    *   The self-healing switch can't be turned on until the replay suite passes on that Mac's build (G-H6).
    *   Turning a switch off stops new triggers. A run already started finishes its current step and stops before the install.

### Additional Resources

*   [heron-v1.md](heron-v1.md): Q6 (unattended subscription use), Q10 (preview audience), taint, permission modes.
*   [heron-v1-features.md](heron-v1-features.md): the extended definition of done, and the format of the work units table.
*   `anchor-harness` (owner's repository, `~/Documents/PERSONAL/anchor-harness`):
    *   `crates/harness-core/src/effects.rs`, the effect journal;
    *   `versions.rs`, the version store and rollback;
    *   `gate.rs`, the promotion rule (deferred);
    *   `docs/HARNESS2.md`, a comparison with Google's Harness².
*   APE (`~/workspace/ape-business/src/agent/`): `lessons.js` (lessons treated as untrusted), `probe.js` (locked surfaces), `drift.js`.
*   Research synthesis, 2026-11-22: three read-only reports comparing Harness² with Heron and ranking harness candidates across the owner's repositories. Their findings are restated where used in this doc. The reports are session scratch files and are not linked.

### Future Improvements

*   **An offline harness optimizer** for the "How to work" text, tool guidance and skills. It needs:
    *   a suite of 50 or more tasks with a train and test split (G7 plus the EVE usability tasks as the seed);
    *   a feature-gated prompt overlay;
    *   anchor-harness's three-arm promotion rule (`gate.rs:74`, Wilson intervals at `:42`).

    Promotion would always be an owner-reviewed PR.
*   **Runtime ports from the scout's seeded candidates:** RepeatGuard, the completion check, protected paths, and recoverable tool output. Each is its own design delta.
*   **An updater for preview users**, once builds are signed and notarized.

--------------------------------------------------------------------------------

## Detailed Implementation Plan

### Preconditions

*   **P-1. The features base must merge first.** The v1 features base graph (B1 to B6 in heron-v1-features.md) merges into `main` before this graph starts. Both edit `src/config.rs`, `src/server.rs`, `src/app.rs` and `SettingsView.swift`. Harness2 doesn't depend on any feature. This is only to avoid merge conflicts.
*   **P-2. The build session runs four prototypes by hand, outside any sandbox, on the owner's Mac.** It records each result in `docs/designs/research/harness2-prototypes.md` as one line, `P<n>: pass|fail: <evidence>`. These can't be skein nodes, because they need `/Applications`, TCC and unsandboxed builds.
    *   **P1.** Does `cargo test --locked --test <one integration test>` run inside Heron's Seatbelt shell, with the target directory inside the workspace?
    *   **P2.** Do TCC grants (Notifications, plus Screen Recording once F9 ships) and keychain items survive a rebuild signed with a stable "Apple Development" identity? Do they survive an ad-hoc rebuild?
    *   **P3.** Can a detached process started by the core outlive both the app and the core, and rename `/Applications/Heron.app` with no admin prompt?
    *   **P4.** How long does `scripts/verify-selfheal.sh` take on a one-line UI fix?

    A `fail` on P2 or P3 doesn't stop the build. It sets the corresponding Settings checklist item to "unavailable", and the doc's HQ5 or §6 text is updated in the same change.
*   **P-3.** The owner has answered HQ1 to HQ5, or accepted the defaults.

### Target Files to Create/Modify

#### Unit H1: core module `src/harness2/`

##### 1. `src/harness2/mod.rs` (Create)

*   **Scope:** the module root. It holds `pub struct Harness2 { db: SelfhealDb, cfg: Harness2Config }`, plus:
    *   `pub fn tick(&self, app: &App)`, which polls PRs and starts queued triggers when no run is active;
    *   `pub fn scout_due(&self, now) -> bool`;
    *   `pub fn recover(&self)`, the crash recovery in §7.

##### 2. `src/harness2/trust.rs` (Create)

*   `enum TriggerKind { OwnerReport, FailureRecord, ImportedReport, ScoutCandidate }`
*   `struct FailureRecord { kind: FailureKind, location: String, message_template: String, version, commit, count: u32, origin_tainted: bool }`, where `FailureKind` is the five-value enum in §1.
*   `fn run_class(trigger, attached_session_tainted) -> RunClass { Trusted, Untrusted }`
*   `fn outcome(class, fence_verdict, state: &CapsState, gate: GateResult) -> Outcome { AutoInstall, PrOnly(reason), NoPr, LocalDiff, Wait, Held }`. This is exactly the 7 rows of the decision table.
*   `fn template_of(fmt_args)`. The panic hook uses it to replace every argument with `<arg>`.

##### 3. `src/harness2/fence.rs` (Create)

*   `const ALLOW: &[&str]` and `const DENY: &[&str]` as in §2, using `globset`, which is already a dependency.
*   `fn check(repo, base, fix) -> FenceVerdict { Auto, PrOnly(Vec<Reason>) }` applies the path rules and content rules R1, R3, R4 and R5.
*   R1 reads test names by running `cargo test --locked -- --list` and `swift test list` on both commits, through `selfheal check-fence`.
*   R2 is checked by the gate, which reports the result back through `gate-report.json`.

##### 4. `src/harness2/repair.rs` (Create)

*   `const REPAIR_POLICY: PermissionConfig`, `const REPAIR_TOOLS: [&str; 7]` and `const READ_ONLY_TOOLS: [&str; 4]`, as in §3.
*   `fn apply_untrusted_diff(run, text) -> Result<()>` takes the last fenced `diff` block of the final reply, runs `git apply --check`, then `git apply`, then a local `git commit`. A diff that doesn't apply ends the attempt.
*   `const REPAIR_PROMPT: &str`.
*   `fn prepare(run_id, installed_commit) -> RunDirs` runs `git worktree add`, writes the disposable home's `config.toml`, and copies the provider settings, but no keys, because CLI logins stay with the CLI.
*   `async fn attempt(run, feedback: Option<String>) -> AttemptResult` builds the App with the tool filter and runs one turn.

##### 5. `src/harness2/gate.rs` (Create)

*   `fn run_gate(run) -> GateResult` spawns `bash scripts/verify-selfheal.sh <base> <fix> <wt>` with the §11 environment, a 4-hour timeout, and output to `runs/<id>/gate.log`. It parses `gate-report.json` and refuses an untrusted run with a typed error, so row 5 can't be bypassed by a caller.

##### 6. `src/harness2/publish.rs` (Create)

*   `fn commit_push_pr(run, verdict) -> PrRef` runs the redaction scan, then `git commit`, `git push`, and `gh pr create` with the base chosen as in §5. Each step goes through `journal::step()`.
*   `fn poll_prs()` sets `versions.state` from `gh pr view --json state,mergedAt`.

##### 7. `src/harness2/install.rs` (Create)

*   `fn start_swap(run)` copies `heron-cli` to `swapper/` and spawns `swapper/heron-cli selfheal swap --run <id>` with `setsid` and stdio pointing at `runs/<id>/swap.log`.
*   `fn swap(run) -> SwapResult` runs steps 1 to 10 of §6, with the exact paths, timeouts and checks given there.
*   `fn undo_last()` and `fn return_to_main()` queue a trusted rebuild of the chosen commit. That rebuild skips the repair step and runs the gate and the swap.

##### 8. `src/harness2/journal.rs` (Create)

*   A port of anchor-harness's `effects.rs` semantics onto `selfheal.db`:
    *   `begin(run, step, detail) -> Key`, `commit(key)`, `unknown(key)`, `reconcile(key, happened: bool)`, `unresolved(run)`;
    *   `idempotency_key = sha256(run_id, step)`, so each run has at most one record per step;
    *   the schema in §7 as `CREATE TABLE IF NOT EXISTS`, with `PRAGMA user_version = 1` for this file.

##### 9. `src/harness2/scout.rs` (Create)

*   `const HOSTS`, one `async fn fetch_*` per source (HQ8), and `fn parse_feed(text) -> Vec<Item>`, the regex reader.
*   `async fn rank(items) -> Vec<Candidate>`, the tool-less call and validation in §8.
*   `const SEEDED: &[Candidate]` holds the four anchor-harness ports.

##### 10. `src/harness2/report.rs` (Create)

*   `fn write_report(desc, include_conversation: Option<SessionId>) -> PathBuf` writes the `.heronreport` file: a zip of `report.json` with a schema version, the text, the records, the version and commit, and an optional transcript.
*   `fn import_report(path) -> TriggerId` always gives `class = Untrusted`.

##### 11. Tests (inline in `src/harness2/*.rs`)

*   `trust.rs`:
    *   `decision_table_selfheal`, with one case per row;
    *   `untrusted_click_stays_untrusted`;
    *   `tainted_attachment_makes_run_untrusted`;
    *   `failure_record_has_no_free_text`, which feeds a panic message holding a URL and a file name and asserts the template holds neither;
    *   `origin_tainted_failure_is_pr_only`.
*   `fence.rs`:
    *   `fence_allows_heronui_store_change`;
    *   `fence_denies_approval_sheet`;
    *   `fence_denies_harness2_and_gate_script`;
    *   `fence_denies_unlisted_src_file`;
    *   `fence_denies_cargo_lock_and_build_rs`;
    *   `deleted_test_line_is_pr_only`;
    *   `renamed_file_is_pr_only`;
    *   `diff_over_400_lines_is_pr_only`;
    *   `redaction_hit_is_pr_only`.
*   `repair.rs`:
    *   `decision_table_repair_profile`;
    *   `repair_registry_has_only_seven_tools`;
    *   `compound_shell_is_denied_in_repair`;
    *   `untrusted_repair_session_starts_tainted`;
    *   `untrusted_repair_has_only_read_tools`;
    *   `untrusted_diff_that_does_not_apply_ends_attempt`.
*   `gate.rs`: `gate_refuses_untrusted_run`.
*   `journal.rs`:
    *   `journal_step_is_at_most_once`;
    *   `unknown_push_reconciles_before_retry`;
    *   `recover_restores_archive_after_mid_swap_crash`.
*   `scout.rs`:
    *   `scout_drops_unknown_item_ids`;
    *   `scout_card_link_comes_from_fetch_not_model`;
    *   `scout_rejects_unlisted_host`;
    *   `feed_reader_skips_malformed_entries`;
    *   `scout_writes_no_memory`.
*   `install.rs`:
    *   `swap_waits_for_active_run`;
    *   `health_fail_reverts_and_disables_auto_install`;
    *   `archive_is_zip_not_bundle`.

    These run against a temporary directory standing in for `/Applications`, through `HARNESS2_APPS_DIR`, which only `#[cfg(test)]` code reads.
*   `report.rs`:
    *   `report_excludes_conversation_unless_ticked`;
    *   `imported_report_is_untrusted`.

#### Unit H2: wiring in existing core files

##### 12. `src/lib.rs` (Modify)

*   Adds `pub mod harness2;`.

##### 13. `src/config.rs` (Modify)

*   Adds a `[harness2]` section with these keys and defaults:
    *   `self_healing = false`
    *   `auto_install = true`
    *   `self_evolution = false`
    *   `scout_every_hours = 168`, validated to one of 24, 72 or 168
    *   `repo = ""`
    *   `codesign_identity = ""`
    *   the HQ7 caps
    *   `publish_report_text = false`

##### 14. `src/server.rs` (Modify)

*   `GET /api/state` gains `commit`.
*   New routes, each answering only with the app's bearer token, like every other route:
    *   `GET /api/harness2`: switches, the checklist, the status, open PRs, candidates and queued triggers;
    *   `POST /api/harness2`: the switches and the schedule;
    *   `POST /api/harness2/report`
    *   `POST /api/harness2/import`
    *   `POST /api/harness2/runs/{id}/check`
    *   `POST /api/harness2/runs/{id}/open-pr`
    *   `POST /api/harness2/runs/{id}/discard`
    *   `GET /api/harness2/runs/{id}/diff`
    *   `POST /api/harness2/candidates/{id}/build`
    *   `POST /api/harness2/undo-last`
    *   `POST /api/harness2/return-to-main`
    *   `POST /api/harness2/quit-for-install`

##### 15. `src/main.rs` (Modify)

*   Adds the `selfheal` subcommand group: `repair --run`, `check-fence <base> <fix>`, `swap --run`, and `report`.

##### 16. `src/app.rs` (Modify)

*   `App::assemble` takes an optional tool filter.
*   `run_due_schedules` calls `harness2.tick()` and the scout, under `paused()` and `model_held(false)`.
*   Installs the panic hook that writes failure records.
*   Calls `harness2.recover()` at start.

##### 17. `src/agent.rs` (Modify)

*   Adds `PromptSource::Repair`, classed Trusted. The test `every_prompt_block_is_trusted_or_taints` keeps holding.

##### 18. Tests for H2

*   Inline tests:
    *   `state_reports_commit`;
    *   `harness2_routes_need_token`;
    *   `scout_tick_respects_pause_and_hold`;
    *   `panic_hook_writes_failure_record`.
*   An integration test in `tests/integration.rs`: `api_harness2_lists_switches_and_checklist`, with the fixture `tests/fixtures/api/harness2.json`.

#### Unit H3: replay suite `eval/selfheal/`

##### 19. `eval/selfheal/cases.md`, `eval/selfheal/run_replay.py`, `eval/selfheal/genesis.yaml`, `eval/tests/test_selfheal_replay.py` (Create)

*   **`cases.md`** lists 10 historical fix commits, mixed between allowlisted and fenced files. Each entry has:
    *   `base` (the fix's parent);
    *   a bug report written in the owner's words, without the fix;
    *   `hidden_tests`, the fix commit's test names.
*   **`run_replay.py`** runs, for each case:
    1.  check out `base` into a temporary worktree;
    2.  run `heron-cli selfheal repair` with the report;
    3.  copy the fix commit's tests in;
    4.  pass the case if the hidden tests and `cargo test --locked` pass.

    It prints `replay: N/10` and exits 0 when N is at least the HQ10 threshold.
*   **`genesis.yaml`** declares the replay grader as an evaluator, so `genesis audit-evaluator` can check that it doesn't pass an empty patch or a patch that deletes the hidden test.
*   **`test_selfheal_replay.py`** unit-tests the grader on canned outcomes: an empty patch fails, a deleted test fails, a real fix passes.

#### Unit H4: gate script

##### 20. `scripts/verify-selfheal.sh` (Create)

*   The seven steps of §4, in the style of `verify-v1.sh`: `begin`, `fail`, `HERON_REQUIRE_SEATBELT=1` on Darwin, and a `gate-report.json` writer.
*   Its arguments are validated as 40-hex commits and an existing directory.
*   Step 6 reads the last stored full G7 summary from `${HERON_G7_ROOT:-$TMPDIR/heron-verify-eval}/<cli>/summary.json`. With none, it fails, because there is no baseline to hold to.

#### Unit H5: packaging

##### 21. `packaging/macos/build-app.sh` (Modify)

*   Adds `--identity <name>`: sign `heron-cli`, then the app, with `codesign --force --options runtime --sign "<name>"`, with no notarization.
*   Exports `HERON_COMMIT=$(git rev-parse HEAD)` into both cargo builds.

#### Unit H6: Mac app

##### 22. `macos/HeronMac/Sources/HeronUI/SelfImprovement/` (Create)

*   `SelfImprovementSettingsView.swift`, `CandidateCardView.swift`, `RunDiffView.swift` and `ReportProblemSheet.swift`.
*   `SelfImprovementStore.swift`, which polls `GET /api/harness2` and listens for the `install_pending` event, then shows the restart notice and terminates.

##### 23. `macos/HeronMac/Sources/HeronCore/Harness2Client.swift` (Create), `Models.swift` (Modify)

*   Codable models for the routes in item 14.

##### 24. `macos/HeronMac/Sources/HeronUI/SettingsView.swift` (Modify), `macos/HeronMac/Sources/Heron/HeronApp.swift` (Modify)

*   Adds the pane, and the Help › Report a problem and File › Import report menu items.

##### 25. `macos/HeronMac/Tests/HeronUITests/SelfImprovementTests.swift` (Create)

*   `test_selfHealingSwitchDisabledUntilChecklistPasses`
*   `test_previewMacShowsReportLineInsteadOfSwitch`
*   `test_scoutScheduleOffersThreeChoices`
*   `test_untrustedRunShowsDiffWithoutRunningChecks`
*   `test_installPendingTerminatesApp`
*   `test_reportSheetDefaultsToNoConversation`

#### Unit H7: docs

##### 26. `docs/engineering/SECURITY_THREAT_MODEL.md` (Modify), `docs/features/self_improvement.md` (Create)

*   **Threat model:** a section `## Harness2: code Heron writes into itself`. It covers the trust table, the fence, the unsandboxed gate for trusted runs (HQ2), and the process list from the eighth definition-of-done check.
*   **The user guide** covers the switches, the checklist, Report a problem, Undo and Return to main.

### Verification Plan

*   **Build:** `cargo build --locked`, `swift build --package-path macos/HeronMac`, and `bash packaging/macos/build-app.sh --unsigned`.
*   **Unit and integration tests:** `cargo test --locked`, `cargo test --locked --test integration`, and `swift test --package-path macos/HeronMac`. The named tests in items 11, 18 and 25 must exist and pass (`eval/require-tests.sh`).
*   **Lint:** `cargo clippy --locked --all-targets -- -D warnings`.
*   **Test quality, with testteeth:** `testteeth run --base main --fail-under 70 -q` over the branch. Surviving mutants in `fence.rs`, `trust.rs` and `journal.rs` are killed, not justified, because those are the checks the design rests on.
*   **Evaluator audits, with genesis:**
    *   `genesis audit-evaluator eval/selfheal/genesis.yaml` for the replay grader.
    *   A genesis claim that the fence checker refuses every path in the deny list and the default-deny set. Its cases are the fence tests in item 11, run as a table.
    *   Both must be SOUND (exit 0) before the replay score or the fence is trusted.
*   **The replay go/no-go:** `python3 -I eval/selfheal/run_replay.py` reaches the HQ10 threshold on the owner's Mac (G-H6).
*   **End to end, by hand, once:** CUJ H-1 with a planted one-line UI bug, then CUJ H-3 with a planted crash on start. The swap log and the PR are attached to the build PR.

## Work units

The graph runs on a feature branch cut from `main` after P-1. P-2's prototype file is committed to that branch before `skein run` starts. Scopes don't overlap. H1 and H2 both touch `src/`, so H2 depends on H1 and never runs at the same time. In the check column, `\|` is the table's escape for a shell pipe `|`.

| id | what to build | scope | depends on | check | model role |
|---|---|---|---|---|---|
| H1 | `src/harness2/` module: trust table, fence, repair profile, gate runner, publish, swapper, journal, scout, reports, inline tests (items 1-11) | `src/harness2/**` | none | `cargo test --locked --lib harness2 && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked --lib -- --list \| grep -c -E 'harness2::.*(decision_table_selfheal\|untrusted_click_stays_untrusted\|failure_record_has_no_free_text\|fence_denies_approval_sheet\|fence_denies_harness2_and_gate_script\|fence_denies_unlisted_src_file\|deleted_test_line_is_pr_only\|decision_table_repair_profile\|gate_refuses_untrusted_run\|recover_restores_archive_after_mid_swap_crash\|scout_card_link_comes_from_fetch_not_model\|health_fail_reverts_and_disables_auto_install): test' \| grep -qx 12` | hardest tasks: opus high |
| H2 | Wiring: `lib.rs`, `[harness2]` config, routes, `selfheal` subcommands, tick, panic hook, `PromptSource::Repair`, `commit` in state, integration test and fixture (items 12-18) | `src/**` | H1 | `cargo test --locked --lib --bins && cargo test --locked --test integration && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib -- state_reports_commit harness2_routes_need_token scout_tick_respects_pause_and_hold panic_hook_writes_failure_record every_prompt_block_is_trusted_or_taints && test -s tests/fixtures/api/harness2.json` | hardest tasks: opus high |
| H3 | Replay suite, grader, genesis evaluator, grader tests (item 19) | `eval/**` | none | `python3 -I -m unittest discover -s eval/tests -t eval -p test_selfheal_replay.py && test "$(grep -c '^## case' eval/selfheal/cases.md)" -eq 10 && genesis audit-evaluator eval/selfheal/genesis.yaml` | feature, refactoring: sonnet high |
| H4 | `scripts/verify-selfheal.sh` (item 20) | `scripts/**` | none | `bash -n scripts/verify-selfheal.sh && grep -q 'check-fence' scripts/verify-selfheal.sh && grep -q 'HERON_REQUIRE_SEATBELT=1' scripts/verify-selfheal.sh && grep -q 'eval/containment/check.py' scripts/verify-selfheal.sh && grep -q 'fail-under 70' scripts/verify-selfheal.sh` | feature, refactoring: sonnet high |
| H5 | `build-app.sh --identity` and `HERON_COMMIT` (item 21) | `packaging/macos/**` | none | `bash -n packaging/macos/build-app.sh && grep -q -- '--identity' packaging/macos/build-app.sh && grep -q 'HERON_COMMIT' packaging/macos/build-app.sh` | feature, refactoring: sonnet high |
| H6 | Self-improvement pane, report sheet, diff view, client, models, menu items, UI tests (items 22-25) | `macos/**` | H2 | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && swift test list --package-path macos/HeronMac \| grep -c 'SelfImprovementTests' \| grep -qx 6` | feature, refactoring: sonnet high |
| H7 | Threat-model section and user guide (item 26) | `docs/**` | H2 | `grep -qx '## Harness2: code Heron writes into itself' docs/engineering/SECURITY_THREAT_MODEL.md && test -s docs/features/self_improvement.md` | judgment and prose: opus high |
| verify | Full suite, test-quality gate, evaluator audits | `scripts/gates/harness2` (check only) | H1, H2, H3, H4, H5, H6, H7 | `cargo test --locked && cargo clippy --locked --all-targets -- -D warnings && swift test --package-path macos/HeronMac && python3 -I -m unittest discover -s eval/tests -t eval && genesis audit-evaluator eval/selfheal/genesis.yaml && testteeth run --base main --fail-under 70 -q` | feature, refactoring: sonnet high |

**Run notes for the build session.**

*   H1, H2 and `verify` need unsandboxed `cargo test`, and H6 needs unsandboxed `swift build`. The repository memory notes record both failing inside node sandboxes. The skein supervisor runs those checks, as it did for v1.
*   The replay go/no-go (`run_replay.py`) and the two end-to-end CUJs need the owner's CLI login and `/Applications`. They run by hand after `verify`, and their output goes in the build PR.
*   `verify` gets `--verify-timeout 3600`, because the mutation run over a new module is long.

--------------------------------------------------------------------------------

*Drafted with EGM*
