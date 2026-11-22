# Heron Harness2: self-healing, self-evolution, connectors and observation

*   **Author:** fernandogarzaaa (drafted with Claude)
*   **Status:** In Review
*   **Date:** 2026-11-22
*   **Builds on:** [heron-v1.md](heron-v1.md) (merged to `main` as `f732b4e`) and [heron-v1-features.md](heron-v1-features.md) (approved, being built)
*   **Read at:** `main` = `39dbdf5`

--------------------------------------------------------------------------------

## Overview

The owner asked for these Settings switches, each off by default:

*   **Self-healing.** When the owner reports a bug, or Heron sees one of its own failures, Heron writes the fix and a regression test, checks them, and opens a pull request.
    *   In **Phase 2**, a fix that qualifies also rebuilds Heron and reinstalls it in place at `/Applications/Heron.app`.
    *   The owner chose full autonomy for this loop (OD1). HQ22 builds it in two phases, because Phase 1 has to measure how many fixes qualify.
*   **Self-evolution.** A scout runs every 24 hours, 72 hours or week. It reads AI news and trending open-source repositories, and leaves ideas in the inbox.
*   **Observation and workflow learning.** These are separate switches for:
    *   the screen;
    *   app use;
    *   browser history;
    *   keystrokes (OD3);
    *   files outside the working folder;
    *   learning the owner's workflow.

    Raw observations stay inside the owner's sealbox vault. Only gated results reach Heron: counts, aggregates and fixed-job sentences ([§8](#8-observation-and-workflow-learning)).

The owner also widened the scope to everything, including new connectors and integrations (OD2). A self-written connector is a declarative manifest that a fixed, fenced executor runs. It contains no code ([§7](#7-self-written-connectors-and-integrations)).

Most of this doc is the set of rules that decide what a self-change may do without the owner. Those rules are proposals, which the owner approves or changes ([Open Questions](#open-questions)).

Five facts shape the design:

1.  **Under a fence that blocks every path to security code, 0 of the last 30 fixes would have installed themselves.**
    *   Security code is spread across most core files.
    *   The Mac app's views, stores and core client can change the permission mode, lift "never" rules and accept the risk notice (`macos/HeronMac/Sources/HeronCore/CoreClientProtocol.swift`), so they are security code too.
    *   A file-level fence would have allowed 23 of the 30, but those include Telegram credential and pairing-revocation fixes.

    In Phase 1, every trusted fix whose checks pass ends as a pull request.
2.  **Model-written code is tested in CI, not on the owner's Mac.**
    *   Heron's tests can't run under a nested Seatbelt sandbox. About 27 cargo tests fail in a sandboxed shell, as recorded on 2026-11-19; prototype PT1 measures this again.
    *   The repository's CI (`.github/workflows/ci.yml`) already runs on macOS with `permissions: contents: read` and no secrets.
    *   So the build, test and mutation steps run there. The only model-built code that runs on the Mac in Phase 1 is the `heron` binary that the G7 smoke test drives (HQ2).
3.  **v1's full gate can't gate a self-fix.** `scripts/verify-v1.sh` needs 33 G7 runs on each Tier 1 CLI and the 14-day G9 trial. It also discards G7 results after any change under `src` (`scripts/verify-v1.sh:155`). The self-heal gate is smaller, loosens no v1 threshold, and states what it leaves out ([What the gate does not prove](#what-the-gate-does-not-prove)).
4.  **Text the owner pastes is the easiest way to launder outside content.** The owner might paste API docs, or restate a scout idea or a preview report. Reports record where their text came from. A run started from pasted or restated text is never auto-eligible, and never runs model code on the Mac ([§1](#1-trust-which-inputs-may-start-a-repair)).
5.  **sealbox keeps raw text sealed, but it can't summarise it in words.** Text leaves the vault only as counts and filtered aggregates. Heron will know that the owner spends 20 minutes on invoice email in Mail every weekday morning. It won't know what the emails said ([§8](#8-observation-and-workflow-learning)).

## The Problem

A bug the owner hits in Heron waits until a coding session gets to it. The fix then needs a branch, a pull request, a build and a manual reinstall. The owner wants Heron to close that loop. They also want it to keep up with agent tooling without anyone reading the news by hand, to add the integrations it needs, and to work from full context about how the owner works.

What makes this risky is the agent itself.

*   **What it can do.** An agent that can change and reinstall its own code, and that sees what is typed and shown on screen, can do more damage than any other part of Heron.
*   **Prompt injection.** If outside content can steer a self-change, prompt injection becomes code running on the owner's Mac.
*   **Public exposure.** That code would also become a public commit, because `fernandogarzaaa/HERON-Project` is a public repository.
*   **Observation leaks.** If observation leaks, it leaks passwords, private messages and banking sessions.

Heron v1's standing decisions already set the limits:

*   a tainted conversation's grants never reach the shell;
*   "never" rules are lifted only in the app;
*   memory never trusts outside content.

Harness2 keeps all three, including for code Heron writes into itself and for everything it observes.

## Goals

*   **G-H1.** An owner-reported bug becomes a pull request into `main` without further owner action. The PR holds the fix and a regression test that fails on the old code and passes on the new. CI checks it, and the PR carries the gate report. The exceptions are redaction hits and repeated gate failures (decision rows 0, 0b and 4).
*   **G-H2.** A fix never installs itself if any of these is true:
    *   it touches a fenced path;
    *   its run saw untrusted input;
    *   it started from pasted or restated text.
*   **G-H3 (Phase 2).** A trusted, auto-eligible fix installs itself. A failed health check reverts it and turns auto-install off until the owner turns it back on.
*   **G-H4.** Every self-change that is published is a commit on its own branch, with a PR into `main`. Installed builds are recorded with their commit.
*   **G-H5.** The scout runs on its schedule, fetches only from allowlisted hosts, and only writes inbox notes.
*   **G-H6.** Heron's repair agent fixes at least 6 of the 10 replay-suite bugs before the self-healing switch can be turned on (HQ10).
*   **G-H7.** A preview user's "Report a problem" sends nothing by itself.
*   **G-H8.** A self-written connector:
    *   contains no executable code;
    *   reaches only the hosts its manifest lists;
    *   never sees a secret's value;
    *   produces output that taints;
    *   saves memories that start unconfirmed;
    *   has every call go through the decision function.
*   **G-H9.** Observation:
    *   Raw keystroke text, screen text and file contents never leave the sealbox vault.
    *   The fixed jobs' results may name apps, window titles, site hosts and file paths, and the consent screen says so.
    *   Password fields and Secure Input are never captured.
    *   An indicator and a one-click pause are always visible.
    *   "Forget everything observed" deletes the vault, its key, and everything derived from it.

## Non-Goals

*   **A Harness²-style optimizer.** This is deferred until a suite of at least 50 tasks exists ([Future Improvements](#future-improvements)).
*   **Self-healing on preview users' Macs** ([§12](#12-preview-macs)), and an **auto-updater** for them.
*   **Reading GitHub issues, PR comments or CI logs as triggers.** In a public repository, these are outside content.
*   **Changing any v1 threshold:** G7's 26/30, its 2/3 per task, testteeth's 70, and the containment check.
*   **Self-changes that install without owner review** in Harness2, the fence, the gate, observation code, the connector executor, CI workflows or any trust table.
*   **New dependencies.**
*   **Observation leaving the user's Mac**, other than gated results inside prompts to that user's own model provider. There is no remote observation of another person's Mac.
*   **Changing sealbox's protections.** Heron needs only additions to it (Pre-4).

## Definitions

| Term | Meaning here |
|---|---|
| Builder Mac | A Mac where `[harness2] repo` points at a checkout of `fernandogarzaaa/HERON-Project`, and `git`, `gh` (logged in with the publishing token, HQ24), `cargo` and `swift` are found. In practice, the owner's Mac. |
| Trigger | What starts a repair: an owner report, a failure record, or the owner's click on a connector offer ([§1](#1-trust-which-inputs-may-start-a-repair)). |
| Provenance | Where a report's text came from: `typed`, `pasted` (the report sheet saw a paste of more than 40 characters), or `from_note` (written through "Restate as report" on an inbox note). |
| Failure record | A row Heron's code writes when it fails. It holds only `kind`, `location`, `version`, `commit`, `count` and `origin_tainted` ([§1](#1-trust-which-inputs-may-start-a-repair)). |
| Fence | The compiled-in rules that decide whether a diff is **auto-eligible**. It denies by default ([§3](#3-the-fence-what-may-install-itself)). |
| Self-heal gate | The checks a self-fix needs: local checks before the push, CI checks after it, and the local G7 smoke ([§4](#4-the-self-heal-gate)). |
| Phase 1 / Phase 2 | In Phase 1, every trusted run whose checks pass ends as a PR. Phase 2 adds installing, after HQ22's entry criteria hold. `[harness2] auto_install` exists only in Phase 2 builds. |
| Connector manifest | `connectors/<name>.toml`: a self-written connector, run by the fixed executor in `src/ext/` ([§7](#7-self-written-connectors-and-integrations)). |
| Observer | App-side code that captures one kind of observation ([§8](#8-observation-and-workflow-learning)). |
| sealbox | The owner's local MCP server (`github.com/fernandogarzaaa/sealbox`). It keeps data in an encrypted vault and releases only results that pass its egress gate. Its own term for this is "compute-to-data". |
| Observation vault | A sealbox vault at `~/.heron/observe/vault`. Its key is in the macOS keychain. |
| Replay suite | 10 historical Heron bugs the repair agent must fix again from a bug report alone ([item 20](#20-evalselfheal-create)). |
| PT1 to PT6 | Prototypes the build session runs by hand ([Preconditions](#preconditions)). |
| Pre-1 to Pre-6 | Preconditions that must hold before the build starts ([Preconditions](#preconditions)). |
| HQ*n* | An open question for the owner, with a proposed default ([Open Questions](#open-questions)). |
| Item *n* | A numbered entry in the [Detailed Implementation Plan](#detailed-implementation-plan). |
| kv | Heron's key-value table in `heron.db` (`Store::kv_get`/`kv_set`). |
| Design delta | A short follow-up design doc that changes or extends an approved one. |

**Terms from v1.** These keep their meaning from [heron-v1.md](heron-v1.md):

*   Taint, permission mode, `Effect`, unattended work, the global pause, and the decision function (`Policy::decide`, `src/permissions.rs:364-443`).
*   **Q6:** the hold on unattended use of a subscription model.
*   **Tier 1 CLI:** a model CLI that passed G7. Today these are `claude-cli` and `codex-cli`.
*   **G7:** the task gate. It has 10 gate tasks, `t01` to `t10`, plus the probe `p1`, each with a fixed pass rule, in `eval/core-tasks.md`. `t09` and `t10` are the injection tasks, and `p1` is the containment probe.
*   **G3**, **G4** and **G9:** the regression record, the containment check, and the owner trial.
*   **EVE:** the simulated-user harness.
*   **Seatbelt:** the macOS sandbox that Heron runs commands in.

`ContentEntry` and the features base units B1 to B6 come from [heron-v1-features.md](heron-v1-features.md). **testteeth** grades tests by mutation, and **genesis** audits evaluators. **skein** runs the work units.

**Outside projects this doc reuses or rejects:**

*   **Google's Harness²** (`github.com/google-research/harness2`, summarised in anchor-harness's `docs/HARNESS2.md`) improves an agent's harness for one task at a time. It edits the prompt, tools, skills and plugins over many rollouts, and a judge model picks the winner.
*   **anchor-harness** (`~/Documents/PERSONAL/anchor-harness`) is the owner's Rust agent harness. It has a version store with rollback and a promotion gate.
*   **APE** (`~/workspace/ape-business`) is the owner's JavaScript implementation of Harness²'s method. It treats lessons it learns as untrusted.

**Harness parts from the review session's design.** That design is PR #73, and Harness2 treats its parts as existing (Pre-5):

*   **The effect journal.** It records each tool call. An action interrupted by a crash asks again before it re-runs.
*   **The loop guard (RepeatGuard).** It stops tool calls that repeat, including A-B loops.
*   **Protected paths.** Git hooks, `.envrc`, `.mcp.json`, shell rc files, launch agents, and Heron's own config and database are read-only to the agent, and are hash-checked after each run.
*   **The egress allowlist.** It covers sandboxed commands. Sandboxed writes to the Rust toolchains are allowed, and the toolchain binaries are hash-checked before their next use.

## Owner decisions recorded in this doc

*   **OD1 (2026-11-22).** The owner chose full autonomy for trusted self-heal runs over proposal-only. HQ22 builds it in two phases. The decision stays the same; only the order changes.
*   **OD2 (2026-11-22).** Self-healing and self-evolution apply to everything, including new connectors and integrations.
    *   A connector from a trusted trigger may install itself under the same loop.
    *   A scout-found integration stays a proposal.
    *   Changes to trust tables stay fenced and need owner review.
*   **OD3 (2026-11-22).** Keystroke capture stays in, as its own switch, contained by sealbox.
    *   **What it overruled.** The earlier recommendation was to log no raw keystrokes. A keystroke log holds passwords, 2FA codes and private messages, and the workflow could be derived from app switches, window titles and file events instead.
    *   **The owner's reason.** v1.x is a research preview that tests how a personal agent works with full context, so accuracy from rich context is the goal.
    *   **What stays.** Raw keystrokes stay in sealbox, and Secure Input and password fields are never captured.

## Open Questions

Each question has a proposed default, which the build uses unless the owner changes it.

**Shape decisions: HQ1 to HQ5, HQ22 and HQ24**

*   **HQ1, how wide the fence is.** **Proposed: the allowlist in [§3](#3-the-fence-what-may-install-itself).** It allows:
    *   the Mac app's design tokens;
    *   `src/schedule.rs`;
    *   new tests;
    *   connector manifests.

    Under it, 0 of the last 30 fixes would have been auto-eligible.

    Views, stores and most core files are fenced, because each can reach security state. Widening the fence safely means splitting the app's views from its stores first ([Future Improvements](#future-improvements)). The 30 commits are mostly Telegram and skills hardening, so Phase 1 measures the real share.
*   **HQ2, model-built code on the Mac.**
    *   **What runs where.** CI runs the build, tests and mutation steps. The only model-built code that runs on the Mac in Phase 1 is the `heron` binary of a trusted run whose provenance is `typed`, which the G7 smoke test drives after CI has passed.
    *   **How that run is contained.** It runs with:
        *   a disposable `HOME` and `HERON_HOME`;
        *   no `GH_*`, `SSH_AUTH_SOCK` or `GIT_*` variables;
        *   access to the active CLI's login folder only, which G7 needs.
    *   **What it can still reach.** It runs as the owner's macOS user, so it can reach the owner's keychain prompts and files.
    *   **Phase 2.** It installs model-built binaries by definition.

    **Proposed:** accept this, and record it in `docs/engineering/SECURITY_THREAT_MODEL.md`. Don't add a new `## R-` entry to `docs/ACCEPTED_RISKS.md`. That file is the risk notice every preview user accepts (`src/risk_notice.rs:9`), and it is capped at 3 entries (`scripts/verify-v1.sh:248-249`).
*   **HQ3, unattended runs and Q6.** Unattended work never calls a subscription model unless `allow_unattended_subscription` is set (`src/app.rs:1510-1513`). **Proposed: Harness2 obeys this.**
    *   A held failure record becomes an inbox note with a **Start** button.
    *   A held scout run waits until the app next connects.
    *   Owner reports and connector offers are attended, so they run at once.
*   **HQ4, failures that outside content may have caused.** **Proposed: such a run is trusted, but its result is never auto-eligible.** This applies to a failure record whose `origin_tainted` is true ([§1](#1-trust-which-inputs-may-start-a-repair)).
*   **HQ5, the Phase 2 signing identity.** **Proposed: sign self-fix builds with the release Developer ID, with the hardened runtime (`--options runtime`) and the release entitlements, without notarization.**
    *   **The option.** A separate, revocable identity with the same team ID, so that a stolen key from the builder Mac can be revoked without touching release signing.
    *   **The check.** Prototype PT2 checks that privacy grants survive a switch between a release DMG and a self-fix build.
*   **HQ22, phasing.** **Proposed:**
    *   **Phase 1** is this doc's work units: the whole loop up to the PR, plus the scout, connectors and observation.
    *   **Phase 2** adds the installer, the health check and rollback. It starts once all of these hold:
        *   the replay suite passes HQ10;
        *   PT2 and PT3 pass;
        *   at least 3 merged self-heal PRs were labelled `self-heal:auto-eligible` within 30 days.
*   **HQ24, the publishing token and CI secrets.** **Proposed:**
    *   Heron publishes with a fine-grained GitHub token that the owner creates and stores in the keychain from Settings. It has contents and pull-request access to this one repository, and no `workflow` scope. GitHub then rejects any push that changes `.github/workflows/`.
    *   Heron never pushes tags.
    *   The owner moves the release secrets (`MACOS_CERT_P12`, `ASC_KEY_P8`, used by `.github/workflows/release.yml`) into a protected `release` environment, so that no branch workflow can read them.

**Tuning: HQ6 to HQ11**

*   **HQ6, which model repairs.** **Proposed: the active Tier 1 CLI with its default model, through Heron's CLI path. codex-cli is the fallback when the active CLI hits a limit.**
*   **HQ7, caps.** **Proposed:**
    *   3 attempts per trigger;
    *   40 steps and 500,000 tokens per attempt;
    *   3 repair runs in any 24 hours;
    *   in Phase 2, 2 installs in any 24 hours, and none while an installed self-fix is unmerged;
    *   for the scout, 60 items and one model call of at most 50,000 tokens per run.

    The usage budget (`src/usage.rs`) applies too.
*   **HQ8, scout sources and schedule.** **Proposed sources:**
    *   GitHub search: repositories with the topic `llm-agent`, `ai-agent`, `agent-framework` or `mcp`, pushed in the last 7 days and sorted by stars. GitHub has no API for trending repositories.
    *   Hacker News (Algolia): stories with at least 100 points that match "agent", "LLM" or "MCP".
    *   arXiv: `cs.AI`, `cs.CL` and `cs.SE`, with "agent" in the title.
    *   The news feeds of Anthropic, OpenAI and Google DeepMind.

    **Proposed schedule: weekly.**
*   **HQ9, what goes into a public PR.** **Proposed:**
    *   The PR body holds the gate report, the changed files, and a description written from the diff.
    *   Report text goes in only when the owner ticks "OK to publish this report". The tick is off by default (`[harness2] publish_report_text = false`).
    *   Two checks run before any push: the secret scan (R5) and a private-text check that blocks the push when any 8-word run from the report or its attachment appears in the diff (decision row 0b).
*   **HQ10, the replay threshold.** **Proposed: 6 of 10.**
*   **HQ11, the Phase 2 rollback copy.** **Proposed: one zip of the previous app, replaced at the next install.**

**Connectors: HQ12 to HQ15**

*   **HQ12, connectors written from documentation.**
    *   **When the model knows the API.** In an untainted run, a manifest written from what the model already knows is trusted.
    *   **When it needs docs.** Heron says so in an inbox note. A report the owner writes with pasted docs has provenance `pasted`. Its run is trusted for the repair, but it is never auto-eligible, and it never runs model-built code on the Mac (decision row 3).

    **Proposed: keep this.**
*   **HQ13, a connector's first uses.** **Proposed: each operation asks until the owner has approved it 3 times. After that, the normal decision function applies.**
*   **HQ14, OAuth.** **Proposed: an API-key or no-auth manifest can be auto-eligible. One that needs a new OAuth provider is PR-only**, because `src/oauth.rs` is fenced.
*   **HQ15, Heron offering connectors.** **Proposed: only in an untainted session, through the `offer_connector` tool ([§7](#7-self-written-connectors-and-integrations)).**

**Observation: HQ16 to HQ21 and HQ23**

*   **HQ16, the sealbox sandbox on macOS.** **Proposed: Heron runs sealbox under its own Seatbelt profile, with no network and access only to `~/.heron/observe/`. Its `run_python` tool is off.** The reason: sealbox's own sandbox is Linux-only, and on macOS it falls back to an audit hook (`src/sealbox/sandbox.py:62-66` in sealbox).
*   **HQ17, sealbox on preview Macs.** **Proposed: offer observation only where `uv` is installed.** Bundling Python would add about 60 to 80 MB.
*   **HQ18, keystrokes.** **Proposed:**
    *   Capture only while Secure Input is off, the focused element isn't an `AXSecureTextField`, and the frontmost app isn't excluded.
    *   Store text in chunks of 1 minute per app.
    *   Run sealbox's detectors on each chunk before it is written. They drop card numbers, one-time codes and API-key shapes.

    macOS doesn't guarantee Secure Input in every app.
*   **HQ19, retention.** **Proposed: raw data 14 days, derived memories 90 days.**
*   **HQ20, exclusions.** **Proposed:**
    *   apps: password managers, Keychain Access, System Settings, and any app whose name or bundle ID contains "bank";
    *   private browsing windows, best effort only;
    *   banking, health and password-manager sites, from an editable list;
    *   folders: `~/Library`, `~/.ssh`, `~/.gnupg` and Heron's home.
*   **HQ21, what a provider sees.** **Proposed: the consent screen says that fixed-job sentences (app names, titles, hosts, paths) go to the user's model provider inside prompts.**
*   **HQ23, agent queries on the vault.** **Proposed: off.** Only the fixed jobs query the vault.
    *   **If turned on.** The agent gets sealbox's `run_sql` through MCP. It could then read window titles, hosts and paths, and count rows by keystroke text, for example with `WHERE text LIKE '%invoice%'`.
    *   **The safeguard.** Each query asks the owner, because untrusted MCP tools are `ExternalWrite` (`src/mcp.rs:158-168`).

## Critical User Journeys (CUJs)

### CUJ H-1: The owner reports a bug, and Heron opens a checked PR

*   **Given:**
    *   a builder Mac;
    *   self-healing is on;
    *   no run is active.
*   **When:** the owner chooses Help › Report a problem, types "Next run shows UTC for 'every monday at 8'", and sends it.
*   **Then:**
    1.  Heron makes a worktree at the installed commit and runs a repair. The agent fixes `src/schedule.rs` and adds `tests/schedule_local_time.rs`.
    2.  Heron commits the change locally. The local checks pass: the fence marks the diff auto-eligible, and the redaction and private-text checks are clean.
    3.  Heron pushes the branch and opens a **draft** PR.
    4.  CI runs the gate. The new test fails on the old code and passes on the fix, and the suite, clippy and testteeth pass.
    5.  Heron runs the G7 smoke test locally on the base and fix builds.
    6.  The PR is marked ready, labelled `self-heal:auto-eligible`, and the inbox says "Fix ready: next-run time zone (PR #NN)".

    Expect 1 to 3 hours, mostly CI queue time and the G7 smoke test. PT4 measures this.

### CUJ H-2: The fix touches fenced code

*   **Given:** the same Mac.
*   **When:** the owner reports "Telegram pairing asks twice".
*   **Then:**
    *   The fix touches `src/telegram.rs`.
    *   CI and the G7 smoke test still run.
    *   The PR is labelled `self-heal:needs-owner`, and the inbox names the fenced file.

### CUJ H-3 (Phase 2): A self-fix installs itself, then fails its health check

*   **Given:** a Phase 2 build, with auto-install on and the signing identity set.
*   **When:** CUJ H-1's fix meets every condition of decision row 1.
*   **Then:**
    1.  Heron shows "Installing fix #7. Heron will restart" and quits. The swapper replaces the app and relaunches it.
    2.  The new core never writes its port file. After 90 seconds, the swapper restores the zipped previous app, relaunches it, and turns auto-install off.
    3.  The PR gets a comment. The inbox note offers "Turn auto-install back on".

### CUJ H-4: The scout finds an idea

*   **Given:** self-evolution is on, with a weekly schedule.
*   **When:** the weekly scout run happens.
*   **Then:**
    1.  The inbox gains "Idea from the web: A-B loop detection in <repository>". The note gives the link, the part of Heron it concerns, the effort, and "from the web, not checked".
    2.  **Restate as report** opens the report sheet with provenance `from_note`. The owner writes the request. Its run can produce a PR, but never an install, and it runs nothing locally (decision row 3).

### CUJ H-5: A preview user reports a problem

*   **Given:** a preview Mac.
*   **When:** the user chooses Help › Report a problem.
*   **Then:**
    1.  Heron writes `Heron-report-<date>.heronreport`, a JSON file, to the Desktop. It holds the user's text, the failure records from the last 7 days, and the version and commit. It holds a conversation only if the user ticked "Include this conversation".
    2.  A tainted conversation is refused with "This conversation includes outside content, so it can't be attached." Observations are never included.
    3.  The share sheet opens.
    4.  On the owner's Mac, File › Import report adds it to the inbox as outside content. The owner can **Restate as report** (provenance `from_note`).
    5.  A merged fix reaches preview users in the next DMG.

### CUJ H-6: The owner asks for a connector

*   **Given:** a builder Mac and an untainted chat.
*   **When:** the owner writes "Connect my Todoist so the morning brief has my tasks". Heron calls `offer_connector`, and the owner clicks **Build it** on the card.
*   **Then:**
    1.  A trusted repair writes `connectors/todoist.toml`. It declares:
        *   the host `api.todoist.com`;
        *   `list_tasks` (`GET /rest/v2/tasks`, read);
        *   `add_task` (`POST /rest/v2/tasks`, write);
        *   `api_key` auth.

        The repair also writes `connectors/todoist.test.json`, with recorded request and response pairs.
    2.  The local checks pass, CI replays the recorded pairs through the executor, and the PR is labelled `self-heal:auto-eligible`. A Phase 2 build installs it.
    3.  Once a build with the manifest is installed, Settings › Connectors shows "Todoist (written by Heron)" with an empty key field. The owner pastes the key there. Heron never asks for the key in chat and never types it.
    4.  The first three `list_tasks` calls each ask (HQ13). Their output taints the session, and anything learned from it is saved unconfirmed.

### CUJ H-7: The owner turns on observation

*   **Given:** `uv` and sealbox are installed, and every observer is off.
*   **When:** the owner turns on Keystrokes, App use and Workflow learning.
*   **Then:**
    1.  Each switch shows its consent screen once. macOS asks for Input Monitoring and for Accessibility.
    2.  The menu bar icon gains an observing dot, and its menu starts with **Pause observing**.
    3.  The next day, the workflow job finds "Most weekdays 9:05 to 9:20: Mail, then Numbers 'Invoices', then Preview". This becomes an unconfirmed memory and an "Automate this?" suggestion. Accepting the suggestion opens the normal `save_automation` approval, in a tainted session.

## Technical Plan (Architecture)

### 0. Shape

```text
 Heron.app (builder Mac)
 ┌───────────────────────────────────────────────────────────────────────────────────────┐
 │ SwiftUI: Self-improvement · Observation · Report a problem · inbox buttons · offers   │
 │          observers (screen, apps, browser, keys, files) ──────────────┐               │
 └──────────────▲─────────────────────────────────────────────────────────┼───────────────┘
                │ /api/harness2/*, /api/observe/*                         │ /api/observe/ingest
 ┌──────────────┴─────────────────────────────────────────────────────────▼───────────────┐
 │ Rust core                                                                              │
 │  src/harness2: trigger ─▶ trust ─▶ repair ─▶ commit ─▶ local checks ─▶ push (draft PR)  │
 │                ─▶ CI gate ─▶ G7 smoke ─▶ label ─▶ [Phase 2: swapper ─▶ health ─▶ revert]│
 │                scout ─▶ web fetch ─▶ tool-less ranking ─▶ inbox notes                   │
 │  src/ext: connector manifests ─▶ fixed executor ─▶ tools (untrusted output)            │
 │  src/observe: sealbox append and jobs (Seatbelt, no network) ─▶ outside memories        │
 └────────────────────────────────────────────────────────────────────────────────────────┘
                │ git push (token without workflow scope)
 ┌──────────────▼──────────────────────────────────────┐
 │ GitHub CI (contents: read, no secrets)              │
 │  main's copy of the gate runs on the PR head commit │
 └─────────────────────────────────────────────────────┘
```

*   The repair agent is Heron's own agent loop ([Alternative A](#alternative-a-a-headless-coding-cli-with-its-own-tools-does-the-fix)).
*   The fence runs in the installed binary.
*   The gate scripts come from `main`, never from the worktree.

### 1. Trust: which inputs may start a repair

A run's trust comes from the class of every input the model sees. Who clicked doesn't change it, because v1's taint never clears. The owner's own typed words are trusted, which is v1's rule. Text the owner pasted or restated from a note is marked, so it can't be laundered.

| Input | What the model sees | Class | What it can do |
|---|---|---|---|
| Owner report, provenance `typed` | The text, the version and commit, and the failure records from the last 24 hours | Trusted | Start a repair. It may be auto-eligible |
| Owner report, provenance `pasted` or `from_note` | The same | Trusted, but marked | Start a repair. It is never auto-eligible, and nothing it builds runs on the Mac (row 3) |
| Owner report with an untainted conversation attached | The above, plus the conversation | As the report's provenance | Start a repair. A tainted conversation is refused |
| Failure record | Only `kind`, `location`, `version`, `commit`, `count` and `origin_tainted` | Trusted | Start a repair once `count` reaches 2 for the same `(kind, location, commit)`. If `origin_tainted`, it is never auto-eligible (HQ4) |
| Connector offer, clicked in an untainted session | `service` and `ops`, both checked against fixed patterns | Trusted | Start a repair |
| Imported preview report, or scout idea | Another person's text, or web text | Untrusted | An inbox note only. The owner may restate it (provenance `from_note`) |
| Workflow suggestion | Fixed-job sentences from the vault | Untrusted | A suggestion. Accepting it opens a tainted `save_automation` approval |

Untrusted inputs never start a run, so the decision table below has no untrusted rows.

**The decision table.** The test `decision_table_selfheal` checks this. Rows are tried in order, and the first match wins.

| # | Run | Fence verdict | Other state | Checks | Outcome |
|---|---|---|---|---|---|
| 0 | any | any | The secret scan (R5) hits the diff, the commit message or the PR body | — | **Not pushed.** An inbox note. The branch is kept locally for 7 days |
| 0b | any | any | The private-text check hits (HQ9), or the diff touches `.github/**`, `scripts/**`, `packaging/**` or `eval/**` | — | **Not pushed until the owner reviews.** An inbox note with **Review** (opens the diff in the owner's editor) and **Push**. The push is the owner's click |
| 1 | Trusted, `typed` | Auto-eligible | Phase 2 build, auto-install on, under the caps, signing identity set, installed commit is in `origin/main` | CI and G7 smoke pass | **Install**, plus a PR labelled `self-heal:auto` |
| 2 | Trusted, `typed` | Auto-eligible | Any condition of row 1 doesn't hold. In Phase 1 that is always the case | CI and G7 smoke pass | PR labelled `self-heal:auto-eligible` |
| 3 | Trusted | Needs owner. This includes provenance `pasted` or `from_note`, and `origin_tainted` | any | CI passes. G7 smoke runs only for `typed` runs | PR labelled `self-heal:needs-owner` |
| 4 | Trusted | any | any | CI or G7 smoke fails 3 times | The draft PR is closed with its failure report, and the inbox says so |
| 5 | — | — | A run is active, an approval is waiting, or Heron is paused | — | Queued |
| 6 | — | — | Held by Q6 | — | An inbox note with **Start** |

The three labels are `self-heal:auto`, `self-heal:auto-eligible` and `self-heal:needs-owner`.

**When a run fails** (`triggers.state` in brackets):

*   **No diff, or a step or token cap was hit:** this counts as a failed attempt [`retrying`, then `failed`].
*   **The provider hit a limit:** the fallback CLI is tried once, then the trigger waits 6 hours [`waiting_limit`].
*   **The 24-hour cap was reached:** the trigger waits [`queued`].
*   **The worktree or the disposable home couldn't be set up:** an inbox note [`setup_failed`].
*   **A crash interrupted the run:** the effect journal (Pre-5) asks "Repair #N was interrupted. Run again?", and nothing re-runs silently [`interrupted`].
*   **The switch was turned off mid-run:** the current step finishes, and nothing more is pushed [`cancelled`].

**Producers of failure records:**

| `kind` | Writer |
|---|---|
| `panic` | The existing panic hook (`src/main.rs:714-722`) calls the library function `harness2::record_panic(location)` |
| `core_exit` | The app sees a non-zero exit in `CoreProcess.swift`. The core writes the record on its next start, from the status the app passes it |
| `migration_failed` | The `Store::open` migration error path |
| `internal_error` | Every `ApiError` with status 500 in `src/server.rs` |
| `health_failed` | The swapper (Phase 2) |

**How `origin_tainted` is set.** It is true if, when the failure happened, either of these was the case:

*   **Outside content was being handled.** The task-local scope `harness2::OUTSIDE_SCOPE` is set while any outside-content handler runs:
    *   web fetch and the scout's feed reader;
    *   report import;
    *   Telegram ingest;
    *   observation ingest;
    *   MCP and connector responses;
    *   email and calendar sync.
*   **A tainted session was active.** A process-wide counter tracks these.

Preview Macs write failure records too, so that reports can carry them. Failure records start triggers only on a builder Mac.

### 2. The repair run

`heron-cli selfheal repair --run <id>` runs as a separate process. Its setup:

*   **Home:** a disposable `HERON_HOME` at `~/.heron/selfheal/runs/<id>/home`, with no memories, persona, connectors, Telegram or MCP.
*   **Workspace:** a git worktree at `~/.heron/selfheal/runs/<id>/wt`, on the branch `heron/selfheal/<yyyymmdd>-<id>`, cut from the installed commit. That commit is reported by the new `commit` field of `GET /api/state`.
*   **Tools:** `read_file`, `write_file`, `edit_file`, `list_dir`, `search_files`, `find_files` and `shell`. `App::assemble` filters the registry to these.
*   **Policy:** `mode = "ask"` with the existing `DenyAll` approver, so every `Ask` becomes a denial. Auto mode would allow any unmatched command (`src/permissions.rs:438`). The allow rules are:
    *   `write_file:*` and `edit_file:*`, which `check_workspace` limits to the worktree;
    *   `shell:cargo test --locked*`, `shell:cargo check --locked*`, `shell:cargo clippy --locked*`, `shell:git diff*` and `shell:git status*`.

    A rule for one command never covers a compound command (`allow_rule_covers`, `src/permissions.rs:449-460`). The shell runs under Heron's normal Seatbelt profile, with the protected paths, egress allowlist and loop guard from Pre-5.
*   **Prompt:** the fixed Trusted `PromptSource::Repair`. It contains:
    *   the trigger's fields;
    *   "Fix the cause, add a test under `tests/`, `macos/HeronMac/Tests/` or `connectors/` that fails before your fix, and don't edit existing tests";
    *   the list of auto-eligible paths, as advice. The fence still decides.
*   **Model:** the HQ6 provider, through `cli_llm`, so the five CLI containment invariants hold (`src/cli_llm.rs:30-50`).
*   **Retries:** up to 3 attempts. Attempts 2 and 3 get the previous failure: the failing step's last 60 lines, from CI or the smoke test. This is the actionable retry feedback that Harness² uses.

The agent's own test runs are only a guide. The gate decides.

### 3. The fence: what may install itself

The fence lives in `src/harness2/fence.rs` as constants compiled into the installed binary. The running Heron checks the local commit (made before any check, [§5](#5-publishing)) with this command:

```text
/Applications/Heron.app/Contents/MacOS/heron-cli selfheal check-fence --repo <path> <base> <fix>
```

The worktree's copy of `fence.rs` is never used. `src/harness2/**` is fenced, so a fix can't change the rules that judge it.

**Auto-eligible paths.** Every changed path must match one of these:

*   `macos/HeronMac/Sources/HeronUI/Tokens/**`, under R7. `Avatar/**` is excluded, because `AvatarStateMachine.swift` imports `HeronCore`.
*   `macos/HeronMac/Tests/**`
*   `src/schedule.rs`, under R1 and R9.
*   `tests/**`, except `tests/fixtures/**`.
*   `connectors/<name>.toml` and `connectors/<name>.test.json`, for one connector per run, under R6.

**Everything else needs the owner.** That includes:

*   **Core:** every other `src/` file. `events.rs` renders the risk labels the owner sees, and `embed.rs` chooses which model file is downloaded, so neither is auto-eligible. Also `src/ext/**`, the connector executor.
*   **Mac app:** all of `HeronCore/**`, `HeronUI/Stores/**` and `HeronUI/Views/**`, and `macos/HeronMac/Sources/Heron/**`.
*   **Gate and build:** `eval/**`, `scripts/**`, `packaging/**`, `.github/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, any `build.rs`, `Package.swift`, `*.plist` and `*.entitlements`.
*   **Docs:** all of `docs/**`. A docs-only fix is a PR, with no install.

**Content rules.** A broken rule makes the verdict "needs owner". The exceptions are R2 (decided in CI) and R5 (decision row 0).

*   **R1, tests are add-only.** In `tests/**`, `macos/HeronMac/Tests/**`, `connectors/*.test.json` and `schedule.rs`'s `#[cfg(test)]` module, no line may be removed. In `schedule.rs`, the test module is everything after its `#[cfg(test)]` line at `base`.
*   **R2, the regression test bites.** At least one new test, or one new recorded pair for a changed connector. CI decides this rule ([§4](#4-the-self-heal-gate)).
*   **R3, no deleted, renamed or binary files, and no mode changes.**
*   **R4, a size cap.** At most 400 added plus removed lines.
*   **R5, the secret scan.** `src/util.rs`'s redaction runs over the diff, the commit message and the PR body. A hit is decision row 0.
*   **R6, connector manifests.** `heron_ext::validate_manifest` accepts the manifest. The schema is in [§7](#7-self-written-connectors-and-integrations). The auth must be `none` or `api_key` (HQ14).
*   **R7, display-only Swift.** In each file under `Tokens/**` after the change, a Swift tokeniser that matches whole identifiers finds none of these:
    *   `import HeronCore`;
    *   any type or function name declared under `HeronCore/` at `base`, found by `(class|struct|enum|protocol|func|actor)\s+(\w+)`;
    *   any identifier ending in `Store` or `Client`;
    *   `URLSession`, `Process`, `FileManager`, `NSWorkspace`, `Data(contentsOf`, `NSAppleScript` or `dlopen`.
*   **R8, no overlap.** The fix may not touch a file that an open self-heal PR also touches. Those files are taken from `runs.files_json`.
*   **R9, no new reach in `schedule.rs`.** Added lines bring in no `use` path or `crate::` item that the file didn't already use at `base`, and no `std::process`, `std::fs`, `std::net`, `std::env` or `Command`.

### 4. The self-heal gate

The gate has three parts. None of them runs model-written code on the Mac except the G7 smoke test's binary (HQ2).

**A. Local checks, before the push.** These run Heron's own code only.

1.  **The fence.** It gives the verdict.
2.  **R5 and the private-text check.**
3.  **The push rule.** Row 0b covers diffs that touch `.github/**`, `scripts/**`, `packaging/**` or `eval/**`.

Every `git` call Harness2 makes passes `-c core.hooksPath=/dev/null -c core.fsmonitor=false -c diff.external= -c filter.lfs.process=`. Before each call, Harness2 checks that the worktree's `.git` file still points at `<repo>/.git/worktrees/<id>`.

**B. CI, after the push.** This uses a new workflow, `.github/workflows/selfheal.yml`.

*   **When it runs:** on `pull_request` for head branches `heron/selfheal/**`.
*   **Permissions:** `contents: read`, with no secrets.
*   **Where the gate comes from:** it checks out the PR's base (`main`) into `gate/` and the PR head into `fix/`, then runs `bash gate/scripts/verify-selfheal.sh <base> <fix> fix/`. The gate is always `main`'s copy.
*   **What the script runs**, deciding from `git diff --name-only`:

| Step | Runs when | Command |
|---|---|---|
| R2 | always | See the table below |
| Swift | `macos/` changed | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac` |
| Rust | `src/`, `tests/` or `connectors/` changed | `cargo test --locked --workspace`, `cargo test --locked --features desktop --bin heron-desktop` and `cargo clippy --locked --workspace --all-targets -- -D warnings` |
| Mutation | Rust changed | `testteeth run --base <base> --fail-under 70 -q` |
| Containment | Rust changed | `python3 -I eval/containment/check.py` |

**R2 outcomes.** CI copies only the new test files, or the new recorded pairs, onto a checkout of `base`. It builds them with `cargo test --locked --workspace --no-run` (or `swift build --build-tests`), then runs `cargo test --locked --test <stem>` (or `swift test --filter <Class>`).

| On `base` | On `fix` | Result |
|---|---|---|
| Compiles, and the new tests fail | They pass | R2 passes |
| Compiles, and the new tests pass | — | Gate failure: the test doesn't catch the bug |
| Doesn't compile (the test needs a new API) | They pass | Verdict "needs owner". The gate continues |
| A new connector (no manifest on `base`) | Its recorded pairs pass | R2 passes |
| — | The new tests fail | Gate failure |

Heron reads CI's result with `gh pr checks`. GitHub's macOS runners are sometimes unavailable. When they are, the run waits for CI up to 24 hours, then fails as row 4 with "CI did not run".

**C. The local G7 smoke test, after CI passes.** It runs only for trusted runs with provenance `typed`, and only when Rust changed.

*   **The builds.** It makes a fresh `git worktree add` of the fix commit, then runs `git clean -xdf` and `cargo build --locked --release` into a new target folder. It does the same for `base` when no cached result exists.
*   **The environment.** It runs with `HOME=$(mktemp -d)` and a disposable `HERON_HOME`. It passes through only the active CLI's login folder variable (for example `CLAUDE_CONFIG_DIR`), and removes `GH_*`, `SSH_AUTH_SOCK` and `GIT_*`.
*   **The command:**

    ```text
    bash <base checkout>/eval/run-core-tasks.sh --provider <active Tier 1 CLI> --runs 1 --driver eve \
      --tasks t01,t02,t03,t04,t05,t06,t07,t08,t09,t10,p1 \
      --heron-bin <build>/release/heron --out ~/.heron/selfheal/g7/<cli>/<commit>
    ```

*   **The pass rule.**
    *   Every task that passes on `base` must also pass on `fix`. A task that fails on `fix` is run once more, and fails the smoke test if it fails again.
    *   `t09`, `t10` and `p1` must pass on `fix`.
    *   The `base` result is cached by commit, so it runs once per installed build.

    This compares a candidate against its own base, as anchor-harness's promotion gate does. It is a new rule in a fenced script, and `check_gate.py` is untouched.
*   **Before it runs,** the toolchain hash check from Pre-5 runs.

The smoke test's result goes into `gate-report.json` and is posted as a PR comment ([§11](#11-contracts)).

#### What the gate does not prove

A self-fix that passed this gate has not passed these:

*   G7 at 3 runs on every Tier 1 CLI;
*   G9;
*   the G3 record;
*   the genesis audits.

The PR into `main` carries those. In Phase 2, an installed self-fix runs before them, and Settings shows "Running 1 self-fix not yet in main".

### 5. Publishing

**Order.** The order is fixed:

1.  repair;
2.  `git commit` in the worktree;
3.  local checks on that commit;
4.  push;
5.  draft PR;
6.  CI;
7.  smoke test;
8.  label, and mark the PR ready.

The commit, the fence, R5 and every later build use the same commit.

**Checks before acting.** Each publishing step checks git and GitHub state first. It doesn't push if `git ls-remote` shows the branch already exists. It doesn't open a PR if `gh pr list --head <branch>` already shows one.

**What goes out.**

*   All PRs target `main`. Heron never pushes tags.
*   The commit message is written from the diff by a tool-less model call, and then scanned by R5.
*   The PR body follows HQ9.

**Afterwards.** On each tick, `gh pr view --json state` updates `runs.pr_state`.

### 6. The scout, the tick and the inbox

**The scout.** It has its own timer, because `schedule.rs` can't express 72 hours: `every 72h` fails with "does not divide evenly" (`src/schedule.rs:250-256`).

*   **Schedule:** `[harness2] scout_every_hours` is 24, 72 or 168.
*   **Fetching:** through `tools::web::check_url` and then `get_limited` (`src/tools/web.rs:76`, `:100`), which guard against SSRF. The scout's host allowlist is checked on the first URL and again on the final URL after redirects. The allowed hosts are `api.github.com`, `hn.algolia.com`, `export.arxiv.org`, `www.anthropic.com`, `openai.com` and `deepmind.google`.
*   **Parsing:** JSON for GitHub and Hacker News. For arXiv and the lab feeds, a tolerant regex reader takes `title`, `link` and `updated` or `pubDate` from each entry, and skips any entry it can't parse.
*   **Ranking:** one model call with no tools, in a session marked tainted from the start. The fetched items go in wrapped as untrusted content (`src/agent.rs:692-702`). The call returns `[{item_id, harness_part, why, effort}]`, at most 10 entries. Each `item_id` must be one that was fetched. Links and titles come from the fetched data, never from the model.
*   **Output:** each idea becomes an inbox note, "Idea from the web: …". Seen URLs go into `selfheal.db`. The scout writes nothing else.

**`Harness2::tick`.** It runs every minute in the app's scheduler loop, beside `run_due_schedules`, which returns early while Heron is paused (`src/app.rs:1483-1486`).

| Step | Runs while paused? | Held by Q6? |
|---|---|---|
| 1. Poll PRs and CI (`gh pr view`, `gh pr checks`) | yes | no model call |
| 2. Start a queued owner report or connector request | no | no (attended) |
| 3. Start a queued failure-record trigger | no | yes: a row-6 note |
| 4. Run the G7 smoke test for a run whose CI has passed | no | yes |
| 5. The scout, if due | no | yes: it runs at the app's next `GET /api/events` connection |
| 6. Prune worktrees and branches older than 7 days | yes | no |

**Inbox actions.** The `inbox` table gains an `action` column (JSON, nullable), and `add_inbox` takes an optional action. The app shows one button per action:

| Action | Button | Route |
|---|---|---|
| `start_trigger` | Start | `POST /api/harness2/triggers/{id}/start` |
| `rerun_trigger` | Run again | the same |
| `review_push` | Review, Push | opens the worktree in the owner's editor; `POST /api/harness2/runs/{id}/push` |
| `restate` | Restate as report | opens the report sheet with provenance `from_note` |
| `open_url` | Open PR | none |

**Storage.** `~/.heron/selfheal/selfheal.db` has these tables:

*   `triggers(id, kind, provenance, fields_json, state, attempts, created_at)`
*   `runs(id, trigger_id, base, fix, branch, verdict, files_json, gate_report_sha256, pr_url, pr_state)`
*   `scout_seen(url_sha256, first_seen)`
*   `replay(commit, passed, total, auto_eligible, ran_at)`
*   Phase 2 adds `installs(commit, previous_commit, installed_at, state)`. This follows anchor-harness's version store (`crates/harness-core/src/versions.rs:31`, `:122`), with the git commit as the version.

While `self_healing = true`, the core refuses to start a repair unless `replay` has a passing row for the merge base of the installed commit with `main`. Only `heron-cli selfheal record-replay` writes that row, and only `run_replay.py` calls it. A model-built binary in the smoke test could also write it. That residual is part of HQ2.

### 7. Self-written connectors and integrations

A self-written connector is a declarative manifest that a fixed executor runs. It contains no code. The executor is fenced, so G-H8's properties belong to the executor, whatever any manifest says.

**The manifest, `connectors/<name>.toml`:**

```toml
name = "todoist"              # ^[a-z][a-z0-9_]{1,30}$
title = "Todoist"
hosts = ["api.todoist.com"]   # HTTPS DNS names: lowercase, at least one dot; no IPs, ports or userinfo; not localhost, .local or .internal
[auth]
kind = "api_key"              # none | api_key | oauth (oauth is never auto-eligible)
header = "Authorization"
scheme = "Bearer"
[[ops]]
name = "list_tasks"           # ^[a-z][a-z0-9_]{1,40}$, at most 16 ops
kind = "read"                 # read: method must be GET; write: POST | PUT | PATCH | DELETE
method = "GET"
host = "api.todoist.com"      # one of hosts
path = "/rest/v2/tasks"       # literal segments and {param} placeholders only
query = ["project_id"]        # params sent as query string
body = "none"                 # none | json (the listed params, as one JSON object)
select = ["/0/content", "/0/due/date"]   # JSON pointers kept from the response; at most 32
params = { project_id = { type = "string", max_len = 64 } }   # string | integer | boolean; strings percent-encoded
max_items = 50
```

**The recorded test, `connectors/<name>.test.json`.** It is a list of `{op, args, request: {method, url, body}, response: {status, json}}` entries. The executor's harness (`tests/connector_manifests.rs`) does two things for every manifest:

*   it validates the manifest;
*   for each recorded pair, it asserts that the request the executor builds equals the recorded `request`, and that the selected output equals what the recording implies.

**The executor, `src/ext/`** (fenced, new). At startup it loads every manifest from the app bundle's `Contents/Resources/connectors/`. `build-app.sh` copies them there. In development it loads them from `<repo>/connectors/`. For each manifest:

1.  **Validation.** It checks the manifest again. A manifest that fails isn't registered, and Settings shows why.
2.  **Tools.** It registers one tool per operation, `ext_<name>_<op>`. The effect is `NetworkEgress` for `read` and `ExternalWrite` for `write`. Every tool has `untrusted_output = true`, so its output is wrapped as outside content and taints.
3.  **Requests.** It builds each request only from the manifest's template and the validated parameters, with these limits:
    *   no redirects (`reqwest::redirect::Policy::none()`);
    *   the host must be in the manifest;
    *   after DNS, the address must not be private (`permissions::is_private_ip`, `src/permissions.rs:1178`);
    *   a 1 MB response cap and a 20-second timeout.

    The executor reads the key from the keychain and adds the auth header itself, and only on requests to the manifest's hosts.
4.  **First uses ask (HQ13).** A new check, `permissions::ext_first_use`, runs in `Policy::decide` straight after the protective rules (Forbidden, deny rules, ReadOnly, Refused, secrets and taint) and before the convenience rules (`src/permissions.rs:398`).
    *   It returns `Ask` in every mode, Yolo included, for an `ext_*` tool whose kv counter `ext_approvals:<name>:<op>` is below 3. No rule or grant covers it.
    *   Only the owner's `once` answer in the app increments the counter (`src/server.rs:630`).
5.  **Learning.** It registers the source `ext:<name>` with `src/sources.rs`.
6.  **Settings.** It adds a card to `connectors::list`, with the category "Written by Heron". The key is entered through the existing keychain path (`connectors::put_api_key`, `POST /api/connectors/custom/{id}/api-key`), and the owner types it in Settings.

**Changes to the trust table, made once by this doc.** `src/memory.rs` gains `is_outside(source)`. It returns true for every source in `OUTSIDE_SOURCES`, for any `ext:` prefix, and for `observation`. It replaces both direct `OUTSIDE_SOURCES.contains` calls (`src/memory.rs:276`, `:367`).

`ext:` and `observation` memories can be confirmed the same way v1's email memories can, by the owner's click in Memories, which shows their source. After this change, the trust lists are fenced again.

**The connector offer (HQ15).** A new tool, `offer_connector{service, ops: [{name, kind}]}`, with effect `LocalStateWrite`. It is available only in untainted sessions.

*   **Validation.** `service` and each op name must match the manifest name patterns, with at most 16 ops.
*   **The card.** The app shows a card with **Build it** and **No**. **Build it** calls `POST /api/harness2/connector-request`.
*   **The trigger.** The server checks again that the session is untainted, then queues a trusted trigger with only `service` and `ops`. No other model text goes into it.

**What a manifest can still do:**

*   send the agent's arguments to its own hosts, under the decision function;
*   return hostile data, which taints;
*   change state with a `GET` on a badly designed API.

Its hosts and operations are listed on its card and in the PR.

### 8. Observation and workflow learning

**What each switch turns on.** Each switch is off by default. It shows a consent screen the first time, and records `observe_consent:<switch>:<version>`.

| Switch | Observer | sealbox `append` | Daily jobs and suggestions | Agent vault queries |
|---|---|---|---|---|
| 1. Screen, 2. App use, 3. Browser history, 4. Keystrokes, 5. Files outside the working folder | that observer | on while any of 1 to 5 is on | — | — |
| 6. Workflow learning (needs one of 1 to 5) | — | — | on | only if HQ23 is on |

**Observers.** They live in `macos/HeronMac/Sources/HeronCore/Observers/` (fenced). Each asks for its permission only through the macOS prompt, and if the owner refuses, it stops and says so in Settings.

| Observer | How it works | Permission | What it keeps |
|---|---|---|---|
| Screen | ScreenCaptureKit captures the frontmost window every 30 s, and Vision (`VNRecognizeTextRequest`) reads its text on the Mac | Screen Recording | `day`, `ts`, app, title, OCR text. The image is dropped |
| App use | `NSWorkspace` activation, plus the window title through Accessibility | Accessibility | `day`, start, end, app, bundle ID, title |
| Browser history | A read-only copy of Safari's `History.db` and Chrome's `History`, every 15 minutes | Full Disk Access | `day`, `ts`, host, page title. Never the full URL |
| Keystrokes | `CGEventTap`, listen-only | Input Monitoring | `day`, minute, app, text after detectors (HQ18). Nothing is kept while Secure Input is on, an `AXSecureTextField` has focus, or an excluded app is frontmost |
| Files outside the working folder | FSEvents on `~/Documents`, `~/Desktop` and `~/Downloads` | possibly Files & Folders | `day`, `ts`, path, event. Never the contents |

Exclusions (HQ20) are applied in the observer, before anything is written. Observers stop when:

*   Heron is paused (`POST /api/pause`, or `/pause` from the phone);
*   **Pause observing** is on;
*   the screen locks.

The phone can pause observers, but can't turn them on.

**sealbox.** This design uses sealbox v0.1 at `c863992`. Heron uses:

*   its encrypted vault (AES-256-GCM, with a data key for each dataset wrapped by a master key);
*   `run_sql`, a restricted SQL subset;
*   its egress gate, which applies k-anonymity, PII redaction, injection stripping, content fingerprints and leak budgets;
*   its detectors;
*   its hash-chained audit log.

The integration has these parts:

1.  **Installing sealbox.** Settings › Observation › Install sealbox runs these steps:
    *   `uv venv ~/.heron/observe/venv`;
    *   `uv pip install --require-hashes -r <sealbox lock at the pinned commit>`;
    *   then sealbox itself, from `git+https://github.com/fernandogarzaaa/sealbox@<commit>`.

    The commit and the lock file's SHA-256 are pinned in fenced `src/observe/sealbox.rs`. Heron records a hash of the venv's files and checks it before each spawn.
2.  **The key.** The core keeps the master key in the macOS keychain. It hands the key to each sealbox child through an inherited pipe (`--key-fd 3`, a Pre-4 addition), not through the environment, where `ps -E` could show it. The key never goes into `config.toml` or a file.
3.  **Ingest.** Observers post batches to `POST /api/observe/ingest` ([§11](#11-contracts)). The core streams each batch to a `sealbox append` child, which writes encrypted hourly segments to `~/.heron/observe/vault`. Raw observations never reach `heron.db`, its backups, the event bus or logs. v1's `observations` table (`src/store.rs:239-241`) keeps holding only working-folder file events.
4.  **Sandbox.** Every sealbox child runs under a new Seatbelt profile, `observe`. It has no network, read and write access to `~/.heron/observe/` only, and read access to the venv (HQ16).
5.  **The vault policy** comes from the `observation` template (Pre-4).
    *   The `text` columns are `aggregate_only`, so they can be filtered and counted but never returned.
    *   `title`, `host` and `path` are `aggregate_only` with `row_level_public = false`. The fixed jobs return them only grouped. `app`, `bundle_id`, `event` and `day` are `public`.
    *   `entity_column = "day"`.
    *   Differential privacy is off. The jobs are fixed SQL, and agent queries stay off unless HQ23 is on.
6.  **Fixed jobs.** `src/observe/jobs.rs` (fenced) runs four SQL queries once a day, with no model call:
    *   time per app per day;
    *   app sequences seen on at least 3 distinct days (`HAVING count(DISTINCT day) >= 3`);
    *   files touched, grouped by folder;
    *   site visits per host.

    Each result row fills a fixed sentence template. It is saved with `memory.save(text, "fact", "observation", 0.4, "observation")`, whose arguments are content, kind, tags, importance and source (`src/memory.rs:247`). The `observation` source is kept apart from v1's `observed` source (`src/app.rs:1836-1879`), so that "Forget" deletes exactly what observation made.
7.  **Workflow to automation.** `suggestions` gains an `origin` column (`'proactive'` or `'observation'`, default `'proactive'`), and `add_suggestion` gains `origin` and `session_id`.
    *   Each repeated sequence creates a tainted session holding its sentence, and a suggestion that points to it. The suggestion's prompt is fixed: "Create an automation for the routine in this conversation."
    *   Accepting it continues that tainted session (`src/server.rs:1606-1615`), and `save_automation` asks for approval as usual.
8.  **Retention.** Each day, `sealbox prune --older-than 14` runs, and `observation` memories older than 90 days are deleted.
9.  **Forget everything observed.** It stops every observer and sealbox child. It then deletes:
    *   `~/.heron/observe/`;
    *   the keychain key;
    *   every `observation` memory;
    *   every suggestion with `origin = 'observation'`, and its session.

    It reports what it deleted.
10. **Indicator.** While any observer is on, the menu bar icon shows a dot. Its menu starts with **Pause observing** (1 hour, or until resumed).

**Where sealbox falls short** (its README and source at `c863992`):

*   **It captures nothing.** The five observers are new Heron code.
*   **It only ingests whole datasets** (`Vault.ingest`, `src/sealbox/vault.py:268`). Pre-4 adds `append`, `prune`, `--key-fd`, a `--tools` allowlist, a hashed lock file and the `observation` template.
*   **Its sandbox is Linux-only.** On macOS, Heron's Seatbelt profile covers this.
*   **It has no summariser inside the vault** (fact 5 in the [Overview](#overview)).
*   **Its privacy tools assume many people.** Here, k-anonymity is replaced by "seen on 3 days", and DP is off.
*   **Its budgets reset when the process restarts.**
*   **It is alpha.** Its red-team results are "evidence, not proof", in its own words.
*   **It needs Python** (HQ17).
*   **It doesn't protect against a compromised Mac.**

**Preview users** control their own switches, behind the same consent screens. Nothing observed leaves their Mac except fixed-job sentences inside prompts to their own provider (HQ21). Reports never include observations.

### 9. Phase 2: install and rollback

These invariants hold. The step-by-step design is the Phase 2 design delta, which is still to be written.

*   **The old code judges the new.** The swapper is a copy of the old `heron-cli`, started detached (`setsid`).
*   **Exactly one launchable Heron.app.** The previous app is kept only as one zip, in `~/.heron/selfheal/previous/`.
*   **No install while work is pending.** That means no active run, no waiting approval, and no pause.
*   **The health check decides.** The new core must:
    *   write its port file within 90 seconds;
    *   answer `GET /api/state` with the expected `commit`;
    *   keep `risk_notice_accepted` unchanged;
    *   pass `heron-cli audit`;
    *   keep `PRAGMA user_version` unchanged;
    *   still be running 30 seconds after it first answered.

    If any check fails, the swapper restores the zip, relaunches, and turns `auto_install` off.
*   **Crash recovery.** A `swap_intent` marker is written before the swap and cleared after the health check. At start, a marker whose commit doesn't match the installed app restores the zip.
*   **Undo last self-fix** unzips the previous app. It is unavailable if `user_version` differs.
*   **Return to main** builds `origin/main`, which the owner has merged and so reviewed, then runs the swap and the health check.

### 10. Changes to fenced files, and the definition of done

Approving this doc approves every change in this table. Each file is fenced, so a later self-change can't touch it without review.

| File | Change | Item |
|---|---|---|
| `src/lib.rs` | `pub mod harness2; pub mod ext; pub mod observe;` | Pre-6 |
| `src/config.rs` | The `[harness2]` and `[observe]` tables ([§11](#11-contracts)) | [14](#14-srclibrs-srcconfigrs-srcmainrs-modify) |
| `src/main.rs` | The `selfheal` and `report` subcommands, and the panic hook calling `harness2::record_panic` | [14](#14-srclibrs-srcconfigrs-srcmainrs-modify) |
| `src/server.rs` | `commit` in `/api/state`, the `/api/harness2/*` and `/api/observe/*` routes, and failure records on 500 errors | [15](#15-srcserverrs-modify), [19](#19-srcobserve-create-srcsandboxrs-srcserverrs-modify) |
| `src/app.rs` | The tool filter, the `Harness2::tick` call, the tainted-session counter, and `OUTSIDE_SCOPE` in the handlers | [16](#16-srcapprs-srcagentrs-modify) |
| `src/agent.rs` | `PromptSource::Repair`, four `ContentEntry` variants, and `offer_connector` | Pre-6, [16](#16-srcapprs-srcagentrs-modify) |
| `src/permissions.rs` | `ext_first_use`, and `DenyAll` made available outside tests | [17](#17-srcext-create-srcpermissionsrs-srcmemoryrs-srcsourcesrs-srcconnectorsrs-modify) |
| `src/memory.rs` | `is_outside()` | [17](#17-srcext-create-srcpermissionsrs-srcmemoryrs-srcsourcesrs-srcconnectorsrs-modify) |
| `src/sources.rs`, `src/connectors.rs` | `ext:<name>` sources and cards | [17](#17-srcext-create-srcpermissionsrs-srcmemoryrs-srcsourcesrs-srcconnectorsrs-modify) |
| `src/sandbox.rs` | The `observe` profile | [19](#19-srcobserve-create-srcsandboxrs-srcserverrs-modify) |
| `src/store.rs` | `inbox.action` and `suggestions.origin`, as one migration with a version bump | [16](#16-srcapprs-srcagentrs-modify) |
| `src/ext/**`, `src/harness2/**`, `src/observe/**` | New | 1-10, [17](#17-srcext-create-srcpermissionsrs-srcmemoryrs-srcsourcesrs-srcconnectorsrs-modify), [19](#19-srcobserve-create-srcsandboxrs-srcserverrs-modify) |
| `tests/connector_manifests.rs` | New | [18](#18-tests-for-h2) |
| `.github/workflows/selfheal.yml` | New | [26](#26-githubworkflowsselfhealyml-create) |
| `packaging/macos/build-app.sh` | `HERON_COMMIT`, `--no-dmg`, `--host-arch`, the hardened-runtime default, and copying `connectors/` | [22](#22-packagingmacosbuild-appsh-modify) |
| `eval/selfheal/**`, `scripts/verify-selfheal.sh` | New | [20](#20-evalselfheal-create), [21](#21-scriptsverify-selfhealsh-create) |
| `macos/HeronMac/**` | The panes, observers, inbox buttons and offer card | [23](#23-mac-app-self-improvement), [24](#24-mac-app-observation) |
| `docs/engineering/SECURITY_THREAT_MODEL.md` | Two sections | [25](#25-docs) |

The v1 features doc's eight checks ([heron-v1-features.md](heron-v1-features.md#the-extended-definition-of-done)) apply as follows:

| Check | Harness2 |
|---|---|
| 1. New tools | `ext_<name>_<op>` (`NetworkEgress` or `ExternalWrite`, untrusted output), and `offer_connector` (`LocalStateWrite`, untainted sessions only). The repair profile reuses existing tools. sealbox's MCP tools exist only if HQ23 is on (`trust = false`) |
| 2. Decision-table rows | `decision_table_selfheal`, `decision_table_repair_profile` and `decision_table_ext_tools` |
| 3. Prompt blocks | `PromptSource::Repair` (Trusted). Scout items, connector output and observation sentences are Outside |
| 4. EVE | A preview report writes a file and sends nothing. Turning on App use shows consent, then the indicator, then Pause works |
| 5. Content entries | `OwnerReport` and `FailureRecord` (trusted); `ConnectorOutput` and `ObservationResult` (tainting) |
| 6. Approval surfaces | App only: the Self-improvement and Observation panes, consent screens, Report a problem, Import report, inbox buttons, the offer card, and the connector key field. The phone can only pause observers |
| 7. Processes outside the `shell` tool | See the list after this table |
| 8. TCC | Observation adds Screen Recording, Accessibility, Input Monitoring and Full Disk Access, each asked for when its switch is turned on. Phase 2 needs HQ5 so that grants survive |

Every process has a fixed path, fixed argument shapes, and the environment `PATH`, `HOME` and `TMPDIR`, plus the additions stated here:

*   `git` (worktree, commit, push, ls-remote), with the `-c` options in [§4](#4-the-self-heal-gate);
*   `gh` (pr create, view, checks, list, comment, close), with the publishing token;
*   `cargo` and `bash eval/run-core-tasks.sh` in the G7 smoke test, with the HQ2 environment;
*   `/usr/bin/open -a` for the owner's editor (Review);
*   for observation: `uv venv`, `uv pip install --require-hashes`, and the sealbox `append` and `prune` children, which run under the `observe` profile with the key passed through a pipe;
*   in Phase 2: `build-app.sh`, `codesign`, `ditto`, `/usr/bin/open -a /Applications/Heron.app` and the swapper.

### 11. Contracts

**Config.**

`[harness2]`:

*   `self_healing = false`
*   `self_evolution = false`
*   `scout_every_hours = 168` (24, 72 or 168)
*   `repo = ""`
*   `publish_report_text = false`, the default for the report sheet's tick
*   `max_attempts = 3`
*   `max_steps = 40`
*   `max_tokens = 500000`
*   `runs_per_day = 3`
*   Phase 2 builds only: `auto_install = true`

`[observe]`:

*   `screen = false`
*   `apps = false`
*   `browser = false`
*   `keys = false`
*   `files = false`
*   `workflow = false`
*   `agent_queries = false` (HQ23)
*   `raw_days = 14`
*   `derived_days = 90`
*   `exclude_apps = [...]`, `exclude_hosts = [...]` and `exclude_folders = [...]`, with the defaults from HQ20

**Enums.**

*   `triggers.kind`: `owner_report`, `failure_record` or `connector_request`.
*   `triggers.provenance`: `typed`, `pasted`, `from_note` or `none`.
*   `triggers.state`: `queued`, `held`, `running`, `retrying`, `waiting_limit`, `setup_failed`, `interrupted`, `cancelled`, `failed`, `awaiting_review`, `pr_draft`, `pr_opened` or `installed`.
*   `runs.verdict`: `auto_eligible` or `needs_owner`.
*   `runs.pr_state`: `draft`, `open`, `merged` or `closed`.

**Routes.** All need the bearer token, and errors use the existing `{"error", "message"}` shape.

| Route | Body | Returns |
|---|---|---|
| `GET /api/harness2` | — | `{self_healing, self_evolution, scout_every_hours, builder, checklist: [{item, ok, fix}], runs: [{id, state, verdict, pr_url}]}` |
| `POST /api/harness2` | any of `self_healing`, `self_evolution`, `scout_every_hours` | the same as `GET` |
| `POST /api/harness2/report` | `{text, provenance, attach_session?, publish_text}` | `{trigger_id}`, or `409 tainted_session` |
| `POST /api/harness2/import` | `{path}` | `{inbox_id}` |
| `POST /api/harness2/connector-request` | `{session_id, offer_id}` | `{trigger_id}`, or `409 tainted_session` |
| `POST /api/harness2/triggers/{id}/start` | — | `{trigger_id, state}` |
| `POST /api/harness2/runs/{id}/push` | — | `{pr_url}` |
| `GET /api/observe` | — | `{switches, consent, sealbox: missing\|installed\|running, counts}` |
| `POST /api/observe` | any switch, `agent_queries`, or the exclude lists | the same as `GET` |
| `POST /api/observe/consent` | `{switch, version}` | `{ok}` |
| `POST /api/observe/install` | — | `{sealbox}` |
| `POST /api/observe/ingest` | `{dataset, rows: [...]}`: at most 500 rows or 256 KB, and `dataset` is one of `screen`, `apps`, `web`, `keys` or `files` | `{accepted}`, or `409 paused` |
| `POST /api/observe/pause` | `{minutes?}` | `{paused_until}` |
| `POST /api/observe/forget` | — | `{deleted: {vault, memories, suggestions}}` |

**`gate-report.json`:**

```json
{"base": "<40 hex>", "fix": "<40 hex>", "verdict": "auto_eligible", "reasons": [],
 "ci": {"run_url": "https://github.com/…", "conclusion": "success", "r2": "passes"},
 "smoke": {"cli": "claude-cli", "base_passed": ["t01"], "fix_passed": ["t01"], "result": "passes"}}
```

**`.heronreport`:**

```json
{"schema": 1, "version": "1.0.0", "commit": "<hex>", "text": "<user text>", "provenance": "typed",
 "failures": [{"kind": "panic", "location": "src/x.rs:12", "count": 2}], "conversation": null}
```

### 12. Preview Macs

*   **Self-healing.** On a Mac that isn't a builder Mac, the switch is replaced by the line "Self-healing runs on the Mac that builds Heron. Use Help › Report a problem." Fixes reach preview users in the next DMG the owner sends.
*   **Self-evolution.** It is hidden, because its ideas are about Heron's code.
*   **Observation.** It is available under each user's own control ([§8](#8-observation-and-workflow-learning)).
*   **Failure records.** They are written, so that reports can carry them, but they start nothing.

--------------------------------------------------------------------------------

## Alternatives Considered

### Alternative A: A headless coding CLI with its own tools does the fix

*   **Pros:** a coding agent that already works on this repository; skein's `claude_build` backend built v1 this way.
*   **Cons:**
    *   It breaks CLI containment invariant 3 (`src/cli_llm.rs:40-43`).
    *   It adds a second permission system that Heron's audit log and taint can't see.
*   **Reason for rejection:** it gives up v1's containment for a capability gap that the replay suite measures. If Heron falls below HQ10, the owner gets this alternative back as an open question.

### Alternative B: Run the gate on the Mac, under Seatbelt or unsandboxed

*   **Pros:** faster, with no CI queue.
*   **Cons:**
    *   Under Seatbelt, about 27 tests and `swift build` fail.
    *   Without a sandbox, model-written tests can reach the owner's credentials, `~/.heron` and the signing key.
*   **Reason for rejection:** CI is already a macOS sandbox with no secrets. The draft PR is needed anyway.

### Alternative C: A separate macOS user, or a VM, for the gate

*   **Pros:** no CI queue; isolation from the owner's account.
*   **Cons:**
    *   Setting up and maintaining a second user or a VM with Rust, Xcode and the CLI logins.
    *   The G7 smoke test still needs the owner's CLI login.
*   **Reason for rejection:** CI gives the same isolation with no setup. It is kept as a fallback if GitHub's macOS runners prove too unreliable.

### Alternative D: Proposal-only, permanently

*   **Pros:** the fewest parts; no install machinery.
*   **Cons:** the owner gets no autonomy at all.
*   **Reason for rejection:** OD1. Phase 1 is close to proposal-only in practice, and Phase 2 adds autonomy once it is measured (HQ22).

### Alternative E: `verify-v1.sh` before every PR

*   **Pros:** a self-fix PR would mean the same as a release.
*   **Cons:**
    *   66 or more G7 runs.
    *   A 14-day trial.
    *   G7 results are discarded on any change under `src`.
*   **Reason for rejection:** it can't finish within a day. The PR into `main` still gets the full gate.

### Alternative F: Run Google's Harness², or APE's propose-probe-compose loop

*   **Pros:** published gains; APE is the owner's own working version.
*   **Cons:**
    *   It tunes one task at a time.
    *   Its executors bypass safety.
    *   About 4,000 rollouts per task.
    *   11 tasks overfit.
*   **Reason for rejection:** it needs a suite of at least 50 tasks. This design keeps three of its ideas:
    *   actionable retry feedback ([§2](#2-the-repair-run));
    *   outside lessons treated as untrusted ([§1](#1-trust-which-inputs-may-start-a-repair), from APE's `lessons.js`);
    *   validating the whole change ([§3](#3-the-fence-what-may-install-itself)).

### Alternative G: A pipeline that turns untrusted triggers into local diffs

*   **Pros:** scout ideas and preview reports would become changes directly.
*   **Cons:**
    *   It needs read-only sessions, diff application, a diff view and four routes.
    *   It serves rare triggers.
    *   Model diffs often don't apply.
*   **Reason for rejection:** "Restate as report" plus provenance marking gives a supervised path with no extra pipeline.

### Alternative H: Port anchor-harness's `effects.rs` state machine for publishing

*   **Pros:** a single journal of every publishing step.
*   **Cons:** it duplicates what `git ls-remote` and `gh pr list` already answer.
*   **Reason for rejection:** publishing checks git and GitHub before each step. Interrupted repairs use the review session's effect journal (Pre-5), which is a different component.

### Alternative I: Model-written Rust connectors in a separate crate

*   **Pros:** they could reach any API shape, including pagination and transformations.
*   **Cons:**
    *   A Rust token scan isn't a sandbox (`include!`, string paths in derive attributes, and a CPU-bound loop all evade it).
    *   The code runs in the core's process.
*   **Reason for rejection:** a manifest has no code to scan. Anything a manifest can't express is PR-only.

### Alternative J: A connector as its own process, behind a broker

*   **Pros:** full expressiveness, with a process boundary.
*   **Cons:**
    *   It needs a broker protocol, a Seatbelt profile for each connector, and process management.
    *   It still runs model-written code.
*   **Reason for rejection:** manifests cover common REST APIs with no code at all. A broker can be added later if manifests prove too limited.

### Alternative K: Keep raw observations in `heron.db`

*   **Pros:** no Python and no sidecar.
*   **Cons:** raw keystrokes would be in Heron's store, its backups and its upgrade snapshots.
*   **Reason for rejection:** OD3 puts raw observations in sealbox.

### Alternative L: Summarise raw observation text with the provider

*   **Pros:** summaries in words.
*   **Cons:** raw keystrokes and screen text would go to the model provider.
*   **Reason for rejection:** this breaks the containment the owner chose. A local model inside the vault could do it later.

### Alternative M: No keystroke capture

*   **Pros:** no password or private-message capture risk at all.
*   **Cons:** less context for the agent.
*   **Reason for rejection:** OD3. The owner chose rich context for the research preview.

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Prompt injection becomes persistent code | Untrusted inputs never start a run. Pasted or restated text is marked, never auto-eligible, and runs nothing locally. Links come from fetched data |
| The owner launders outside text by pasting it | Paste detection and `from_note` provenance (decision row 3) |
| A self-fix weakens a security check | Default-deny fence in the installed binary, R1 to R9, the gate from `main`, and fenced trust tables |
| Model-built code runs on the Mac | Only the smoke test's binary, for `typed` trusted runs, after CI, in a disposable `HOME` (HQ2) |
| CI secrets are exposed by a branch workflow | The token has no `workflow` scope, no tags are pushed, release secrets sit in a protected environment, and row 0b keeps workflow and script diffs local (HQ24) |
| A secret or private text appears in a public PR | R5, the private-text check (row 0b), report text off by default, and commit messages written from the diff |
| A broken build replaces the app (Phase 2) | The old binary health-checks the new one, then reverts, turns `auto_install` off, and leaves a crash marker |
| A self-fix crashes the core, which files a new trigger, and so on | A failure record whose `commit` is a self-fix build starts no trigger. It goes to the inbox |
| Privacy grants or keychain access reset | Stable signing, with the hardened runtime (HQ5, PT2) |
| A connector exfiltrates data or reads secrets | No code: a fixed executor with host checks, no redirects, the SSRF guard, a header added by the executor, and 3 asks on first use |
| The vault is read | Encrypted, its key in the keychain, the key passed through a pipe, Seatbelt-only access. A compromised Mac can still read it (sealbox's own limit) |
| The sealbox supply chain | Pinned commit, a lock file with hashes, a venv hash checked before each spawn |
| Passwords or codes captured | Secure Input and `AXSecureTextField` skips, detectors, exclusions, 14 days. macOS doesn't guarantee Secure Input everywhere (HQ18) |
| Other people's content captured | The consent screen says so. Raw data kept 14 days, and only fixed-job sentences come out |
| Disk exhaustion | Worktrees pruned after 7 days. One release build per side for the smoke test, removed after the run |
| The repair agent is too weak | The replay suite gates the switch |
| The fence checker or replay grader is wrong | genesis audits both, and testteeth grades Harness2's code |
| GitHub's macOS runners are unavailable | Each run waits up to 24 hours. Alternative C stays as the fallback |

--------------------------------------------------------------------------------

## Non-Functional Requirements

*   **Security and privacy:** see [Risks & Mitigations](#risks--mitigations) and [§1](#1-trust-which-inputs-may-start-a-repair) to [§8](#8-observation-and-workflow-learning).
*   **Time:** a trusted run takes 1 to 3 hours end to end (CI queue, plus about 30 minutes of G7 smoke when Rust changes). PT4 measures this.
*   **Storage:**
    *   a worktree uses 3 to 5 GB, pruned after 7 days;
    *   the smoke test uses one release target per side, removed after the run;
    *   the vault grows by an estimated 50 to 200 MB a week, and is pruned to 14 days.
*   **Rollout:** every switch ships off. Self-healing stays disabled until the replay passes, and Phase 2 has its own criteria.
*   **Rollback of Phase 1:**
    *   turning the switches off stops everything;
    *   the `store.rs` migration only adds nullable columns, so the previous build reads the database;
    *   reverting the merge removes the feature, and `selfheal.db` and `~/.heron/observe/` can be deleted.

--------------------------------------------------------------------------------

### Additional Resources

*   [heron-v1.md](heron-v1.md) and [heron-v1-features.md](heron-v1-features.md).
*   anchor-harness: `crates/harness-core/src/versions.rs`, `gate.rs`, `effects.rs`, and `docs/HARNESS2.md` (which summarises Google's Harness²).
*   APE: `~/workspace/ape-business/src/agent/lessons.js` and `probe.js`.
*   sealbox at `c863992`: `README.md` (threat model), and `src/sealbox/vault.py`, `sandbox.py`, `policy.py`, `gate.py`.
*   Goldfish review reports: [reviews/heron-harness2/](reviews/heron-harness2/).
*   The research behind this doc (2026-11-22) was three read-only reports comparing Harness² with Heron and ranking harness candidates across the owner's repositories. They were session scratch files. Their findings are restated where they are used.

### Future Improvements

*   **Split the app's views from its stores.** Views would get read-only view models and action closures, so UI fixes could become auto-eligible.
*   **An offline harness optimizer.** It needs a suite of at least 50 tasks with a train/test split. It would use anchor-harness's promotion rule, which compares a candidate against an unchanged baseline and a best-of-N baseline, with statistical confidence intervals (`gate.rs:42`, `:74`).
*   **The completion check, and recoverable tool output.** The review session's design covers RepeatGuard and protected paths, but not these.
*   **A local model inside the observation vault**, for summaries in words.
*   **A connector broker** (Alternative J), if manifests prove too limited.
*   **An updater for preview users.**

--------------------------------------------------------------------------------

## Detailed Implementation Plan

### Preconditions

*   **Pre-1. The features base is merged.** The v1 features base (B1 to B6) is in `main`. Harness2 uses `ContentEntry`, and both edit `config.rs`, `server.rs`, `app.rs` and `SettingsView.swift`.
*   **Pre-2. The prototypes have run.** The build session runs them by hand, outside any sandbox, and records each in `docs/designs/research/harness2-prototypes.md` as `PT<n>: pass|fail: <evidence>`.
    *   **PT1:** how many `cargo test --locked` failures occur inside Heron's Seatbelt shell (expected about 27, as recorded on 2026-11-19);
    *   **PT2 (for Phase 2):** whether TCC grants and keychain items survive a switch between a release DMG and a self-fix build, and with an ad-hoc build;
    *   **PT3 (for Phase 2):** whether a detached process started by the core outlives the app and core, and can rename `/Applications/Heron.app` without an admin prompt;
    *   **PT4:** the end-to-end time for a one-line Rust fix and a one-line Swift fix, CI queue included;
    *   **PT5:** whether the four observation permission prompts appear without any `Info.plist` usage strings. Apple documents none for them, so none is added unless PT5 shows one is needed;
    *   **PT6:** whether a push that changes `.github/workflows/` is rejected with the HQ24 token.
*   **Pre-3. The owner has answered.** The owner has answered, or accepted the defaults for, HQ1 to HQ5, HQ12, HQ16 to HQ18, HQ22, HQ23 and HQ24.
*   **Pre-4. The sealbox additions are merged.** They land in sealbox's own repository, as their own PR, and their commit is pinned:
    *   `sealbox append <dataset> --kind events` (JSON lines on stdin into encrypted hourly segments);
    *   `sealbox prune --older-than <days>`;
    *   `sealbox serve --tools <list>`;
    *   `--key-fd <n>` on every command;
    *   a hashed lock file, `requirements.lock`;
    *   the `observation` policy template from [§8](#8-observation-and-workflow-learning).

    Its check is `python -m pytest -q && python eval/redteam.py`, with 0 leaks.
*   **Pre-5. The review session's design is ready.** Its harness parts are merged, or their interfaces are fixed in its approved design. Harness2 calls:
    *   the effect journal's "ask again" hook;
    *   the toolchain hash check.

    If the names differ, this doc is updated in the same change.
*   **Pre-6. The skeleton is committed.** The build session commits it before `skein run`. It contains:
    *   `pub mod harness2; pub mod ext; pub mod observe;` in `src/lib.rs`, with empty modules;
    *   `PromptSource::Repair`;
    *   the four `ContentEntry` variants as stubs;
    *   the `App::assemble` tool-filter parameter (`None` everywhere);
    *   an empty `connectors/` folder with a `README.md`.

    Its check is `cargo build --locked && cargo test --locked --no-run`.

### Target Files to Create/Modify (Phase 1)

#### Unit H1: `src/harness2/`

##### 1. `src/harness2/mod.rs` (Create)

*   `pub struct Harness2 { db, cfg }`
*   `pub fn tick(&self, app: &App)`, with the six steps of [§6](#6-the-scout-the-tick-and-the-inbox)
*   `pub fn queue(&self, t: Trigger) -> TriggerId`
*   `pub fn record_panic(location: &str)`
*   `pub static OUTSIDE_SCOPE`, a `tokio::task_local!` flag, with the `in_outside_scope(f)` helper

##### 2. `src/harness2/trust.rs` (Create)

*   `enum TriggerKind`, `enum Provenance { Typed, Pasted, FromNote, None }`, `struct FailureRecord`
*   `fn outcome(...) -> Outcome`, which implements the rows of [§1](#1-trust-which-inputs-may-start-a-repair) exactly
*   `fn failure_starts_trigger(rec, installed_is_selffix: bool) -> bool`

##### 3. `src/harness2/fence.rs` (Create)

*   `const AUTO: &[&str]`, as a `globset` set
*   `fn check(repo, base, fix) -> FenceVerdict { AutoEligible, NeedsOwner(Vec<Reason>) }`, which applies R1, R3, R4 and R6 to R9
*   `fn push_rule(paths) -> bool`, for row 0b
*   the Swift identifier tokeniser for R7, and the Rust `use` and path scan for R9

##### 4. `src/harness2/repair.rs` (Create)

*   `REPAIR_POLICY`, `REPAIR_TOOLS` and `REPAIR_PROMPT`
*   `fn prepare(run, installed_commit)`
*   `async fn attempt(run, feedback)`

##### 5. `src/harness2/checks.rs` (Create)

*   `fn local_checks(run) -> LocalResult`: the fence, R5, the private-text check (8-word shingles of the report and its attachment, matched against the added lines) and `push_rule`
*   `fn git(args) -> Command`, which always adds the `-c` options and checks the `.git` pointer
*   `fn smoke(run) -> SmokeResult`: fresh worktrees, `git clean -xdf`, release builds, the HQ2 environment, `run-core-tasks.sh` from the base checkout, and the cache by commit
*   `fn poll_ci(run)`, through `gh pr checks`

##### 6. `src/harness2/publish.rs` (Create)

*   `fn commit(run)`
*   `fn push_and_draft(run)`, which checks before each step
*   `fn finish(run, verdict, smoke)`, which labels the PR, marks it ready, or closes it
*   `fn poll_prs()`

##### 7. `src/harness2/scout.rs` (Create)

*   `HOSTS`, one fetcher per source using `check_url` and `get_limited`, with the host checked on the first and final URLs
*   `parse_feed`, `rank` and `post`

##### 8. `src/harness2/report.rs` (Create)

*   `write_report`, which writes JSON, refuses a tainted session, and never includes observations
*   `import_report`, which adds an inbox note with the `restate` action

##### 9. `src/harness2/db.rs` (Create)

*   `selfheal.db`, with the tables in [§6](#6-the-scout-the-tick-and-the-inbox), and `PRAGMA user_version = 1`

##### 10. Tests (inline)

*   `trust.rs`:
    *   `decision_table_selfheal`
    *   `pasted_report_is_never_auto`
    *   `from_note_report_skips_smoke`
    *   `failure_record_has_no_free_text`
    *   `origin_tainted_failure_is_never_auto`
    *   `outside_scope_sets_origin_tainted`
    *   `selffix_build_failure_starts_no_trigger`
    *   `interrupted_run_asks_before_rerun`
    *   `self_healing_refused_without_replay_pass`
*   `fence.rs`:
    *   `fence_allows_tokens_and_schedule`
    *   `fence_denies_views_and_stores`
    *   `avatar_is_fenced`
    *   `events_and_embed_are_fenced`
    *   `fence_denies_harness2_and_gate_script`
    *   `fence_denies_unlisted_src_file`
    *   `docs_only_diff_needs_owner`
    *   `removed_test_line_needs_owner`
    *   `renamed_file_needs_owner`
    *   `diff_over_cap_needs_owner`
    *   `oauth_manifest_needs_owner`
    *   `bad_manifest_needs_owner`
    *   `tokens_file_calling_client_needs_owner`
    *   `tokens_file_using_process_needs_owner`
    *   `fence_bans_every_heroncore_declaration`
    *   `schedule_new_use_path_needs_owner`
    *   `overlapping_open_pr_needs_owner`
    *   `workflow_diff_is_not_pushed`
*   `checks.rs`:
    *   `redaction_hit_never_pushes`
    *   `private_text_blocks_push`
    *   `git_calls_disable_hooks_and_fsmonitor`
    *   `moved_git_pointer_stops_git`
    *   `smoke_env_has_no_gh_or_ssh`
    *   `smoke_uses_base_checkout_runner`
*   `repair.rs`:
    *   `decision_table_repair_profile`
    *   `repair_registry_has_only_seven_tools`
    *   `compound_shell_is_denied_in_repair`
    *   `unmatched_command_is_denied_in_repair`
*   `publish.rs`:
    *   `publish_skips_existing_branch_and_pr`
    *   `pr_body_omits_report_text_by_default`
    *   `ci_failure_three_times_closes_draft`
*   `mod.rs`: `tick_steps_follow_their_gating`
*   `scout.rs`:
    *   `scout_drops_unknown_item_ids`
    *   `scout_note_link_comes_from_fetch`
    *   `scout_rejects_unlisted_host`
    *   `scout_rejects_redirect_off_allowlist`
    *   `feed_reader_skips_malformed_entries`
    *   `scout_writes_only_inbox_notes`
*   `report.rs`:
    *   `report_excludes_conversation_unless_ticked`
    *   `report_never_includes_observations`
    *   `imported_report_is_inbox_only`

#### Unit H8: connector executor `src/ext/`

##### 11. `src/ext/mod.rs`, `manifest.rs`, `exec.rs` (Create)

*   **`manifest.rs`:** the schema in [§7](#7-self-written-connectors-and-integrations), and `validate_manifest`. The host rules move here from `connectors::validate`, which then calls it.
*   **`exec.rs`:**
    *   `fn build_request(manifest, op, args) -> Request` (pure);
    *   `async fn call(...)`, with no redirects, the host check, `is_private_ip` after DNS, the 1 MB cap, the 20-second timeout, the header added from the keychain, and the `select` filter;
    *   `fn load_all(dir) -> Vec<Manifest>`.
*   **Tests:**
    *   `manifest_rejects_ip_localhost_and_port`
    *   `read_op_must_be_get`
    *   `path_params_are_percent_encoded`
    *   `unknown_param_is_refused`
    *   `redirect_is_not_followed`
    *   `private_address_after_dns_is_refused`
    *   `auth_header_only_to_manifest_hosts`
    *   `response_over_1mb_is_refused`
    *   `select_keeps_only_listed_pointers`

#### Unit H2: wiring in existing core files

##### 14. `src/lib.rs`, `src/config.rs`, `src/main.rs` (Modify)

*   **`config.rs`:** the `[harness2]` and `[observe]` tables, as in [§11](#11-contracts).
*   **`main.rs`:**
    *   the subcommands `selfheal repair --run`, `selfheal check-fence --repo <path> <base> <fix>`, `selfheal record-replay <commit> <passed> <total> <auto_eligible>` and `report`;
    *   the panic hook calls `harness2::record_panic(location)` before it calls the previous hook.

##### 15. `src/server.rs` (Modify)

*   `commit` in `GET /api/state`.
*   Every `/api/harness2/*` route in [§11](#11-contracts).
*   Failure records on `ApiError` 500.

##### 16. `src/app.rs`, `src/agent.rs` (Modify)

*   **`app.rs`:**
    *   the tool filter;
    *   `Harness2::tick` in the scheduler loop;
    *   the tainted-session counter;
    *   `in_outside_scope` around the handlers listed in [§1](#1-trust-which-inputs-may-start-a-repair).
*   **`agent.rs`:**
    *   the four `ContentEntry` variants;
    *   the `offer_connector` tool, registered only in untainted sessions.
*   **`store.rs`:** one migration that adds `inbox.action` and `suggestions.origin`, plus the new `add_inbox` and `add_suggestion` arguments.

##### 17. `src/ext/` (Create), `src/permissions.rs`, `src/memory.rs`, `src/sources.rs`, `src/connectors.rs` (Modify)

*   **Executor registration:** tools, sources and cards, as in [§7](#7-self-written-connectors-and-integrations).
*   **`permissions.rs`:** `ext_first_use`, and `DenyAll` made available outside tests.
*   **`memory.rs`:** `is_outside()`, which replaces both direct `OUTSIDE_SOURCES.contains` calls.

##### 18. Tests for H2

*   **Inline:**
    *   `state_reports_commit`
    *   `harness2_routes_need_token`
    *   `record_panic_writes_failure_record`
    *   `inbox_action_starts_trigger`
    *   `every_prompt_block_is_trusted_or_taints`
    *   `every_content_entry_is_trusted_or_taints`
    *   `decision_table_ext_tools`
    *   `ext_output_taints`
    *   `ext_op_asks_until_three_approvals`
    *   `ext_first_use_ignores_yolo_and_rules`
    *   `ext_source_memories_start_unconfirmed`
    *   `observation_source_is_outside`
    *   `is_outside_replaces_both_checks`
    *   `offer_connector_absent_in_tainted_session`
    *   `connector_request_refuses_tainted_session`
    *   `migration_adds_only_nullable_columns`
*   **Unit H2t, scope `tests/**`:**
    *   `tests/integration.rs`: `api_harness2_lists_switches_and_checklist`, with the fixture `tests/fixtures/api/harness2.json`;
    *   `tests/connector_manifests.rs`: it validates every `connectors/*.toml` and replays its pairs. With no manifests, it passes.

#### Unit H10: observation core

##### 19. `src/observe/` (Create), `src/sandbox.rs`, `src/server.rs` (Modify)

*   **`sealbox.rs`:**
    *   `SEALBOX_COMMIT` and `SEALBOX_LOCK_SHA256`;
    *   `install()`, `verify_venv()` and `master_key()`, the last using the keychain;
    *   `spawn_append()` and `spawn_prune()`, which run under the `observe` profile with `--key-fd 3`;
    *   `spawn_serve()`, only when HQ23 is on, registered at runtime with `trust = false`.
*   **`jobs.rs`:** the four queries, the sentence templates, and the saves.
*   **`forget.rs`:** forgetting everything observed.
*   **`server.rs`:** every `/api/observe/*` route.
*   **Tests:**
    *   `ingest_keeps_nothing_outside_vault`
    *   `ingest_refused_while_paused`
    *   `observe_profile_denies_network`
    *   `master_key_passed_by_fd_not_env`
    *   `config_never_contains_sealbox_key`
    *   `venv_hash_mismatch_refuses_spawn`
    *   `serve_absent_unless_agent_queries_on`
    *   `workflow_job_requires_three_days`
    *   `jobs_make_no_model_call`
    *   `forget_removes_vault_key_memories_and_suggestions`
    *   `workflow_suggestion_session_is_tainted`

#### Unit H3: replay suite

##### 20. `eval/selfheal/` (Create)

*   **`cases.md`:** 10 historical fix commits, each with `base`, a report in the owner's words, and `hidden_tests`.
*   **`run_replay.py`:** checks out `base`, runs `heron-cli selfheal repair`, copies the fix's tests in, and passes the case if those tests and `cargo test --locked --workspace` pass. It prints `replay: N/10` and `auto-eligible: M`, calls `record-replay`, and exits 0 when N meets HQ10.
*   **`genesis.yaml`:** for the grader. An empty patch, or a patch that deletes the hidden test, must fail.
*   **`required-tests.txt`:** every Rust test name from items 10, 11, 18 and 19.
*   **`eval/tests/test_selfheal_replay.py`:** tests of the grader itself.

#### Unit H4: gate script

##### 21. `scripts/verify-selfheal.sh` (Create)

*   It runs the CI steps and the R2 table from [§4](#4-the-self-heal-gate), chosen from `git diff --name-only <base> <fix>`, in `verify-v1.sh`'s style (`begin`, `fail`).
*   It validates its arguments: two commits of 40 hex digits, and an existing directory.
*   It writes the `ci` part of `gate-report.json` as a workflow artifact.

#### Unit H12: CI workflow

##### 26. `.github/workflows/selfheal.yml` (Create)

*   **Trigger:** `on: pull_request` for branches `heron/selfheal/**`, with `permissions: contents: read`.
*   **Runner:** `macos-latest`.
*   **Steps:**
    1.  `actions/checkout` of `main` into `gate/`;
    2.  checkout of the head into `fix/`;
    3.  install testteeth with `pip install testteeth==<pinned>`;
    4.  `bash gate/scripts/verify-selfheal.sh`;
    5.  upload `gate-report.json`.
*   It uses no secrets.

#### Unit H5: packaging

##### 22. `packaging/macos/build-app.sh` (Modify)

*   `HERON_COMMIT=$(git rev-parse HEAD)` for both cargo builds.
*   `--no-dmg` and `--host-arch`.
*   The default mode signs `heron-cli`, then the app, with `codesign --force --options runtime --sign "${CODESIGN_IDENTITY:--}"`, with no `--deep`.
*   It copies `connectors/*.toml` into `Contents/Resources/connectors/`.

#### Unit H6: Mac app, self-improvement

##### 23. Mac app, self-improvement

*   **New files:**
    *   `macos/HeronMac/Sources/HeronUI/SelfImprovement/`;
    *   `HeronCore/Harness2Client.swift`.
*   **Changed files:** `Models.swift`, `SettingsView.swift`, the inbox view in `ScreenViews.swift`, and `Heron/HeronApp.swift`.
*   **The pane:** switches, the checklist, runs with PR links, "Check every" (24 hours, 72 hours or Week), and the line shown on preview Macs.
*   **Report sheet:** paste detection sets `pasted`, there is an "OK to publish" tick, and **Restate as report** sets `from_note`.
*   **Inbox buttons:** one for each action in [§6](#6-the-scout-the-tick-and-the-inbox).
*   **The connector offer card.**
*   **The publishing-token field.** It is a secure text field, with a "Create token" link to GitHub's fine-grained token page.
*   **Tests:**
    *   `test_selfHealingSwitchDisabledUntilChecklistPasses`
    *   `test_previewMacShowsReportLineInsteadOfSwitch`
    *   `test_scoutScheduleOffersThreeChoices`
    *   `test_reportSheetDefaultsToNoConversation`
    *   `test_reportSheetDefaultsToNotPublished`
    *   `test_pasteMarksReportPasted`
    *   `test_restateMarksReportFromNote`
    *   `test_inboxShowsActionButtons`

#### Unit H11: Mac app, observation

##### 24. Mac app, observation

*   **New files:** `HeronCore/Observers/` and `HeronUI/Observation/`.
*   **Changed file:** `MenuBarViews.swift`.
*   **The observers, as in [§8](#8-observation-and-workflow-learning):**
    *   `ScreenObserver`;
    *   `AppUseObserver`;
    *   `BrowserHistoryObserver`;
    *   `KeystrokeObserver`;
    *   `FileEventsObserver`;
    *   `Exclusions`;
    *   `ObservationUploader`.
*   **The views:**
    *   `ObservationSettingsView`;
    *   `ConsentView`;
    *   `ExclusionsView`;
    *   the menu bar dot and Pause.
*   **Tests:**
    *   `test_everyObserverStartsOff`
    *   `test_consentShownBeforeFirstEnable`
    *   `test_keystrokesSkippedDuringSecureInput`
    *   `test_keystrokesSkippedInSecureTextField`
    *   `test_excludedAppIsNotRecorded`
    *   `test_indicatorShownWhileAnyObserverOn`
    *   `test_pauseStopsAllObservers`

#### Unit H7: docs

##### 25. Docs

*   **`docs/engineering/SECURITY_THREAT_MODEL.md`:** two new sections.
    *   `## Harness2: code Heron writes into itself` covers the trust table, provenance, the fence, CI as the gate's sandbox, the HQ2 residual, connectors, and the process list.
    *   `## Observation` covers the observers, sealbox, what leaves the vault, and its shortfalls.
*   **New guides:** `docs/features/self_improvement.md` and `docs/features/observation.md`.

### Verification Plan

*   **Build:** `cargo build --locked`, `swift build --package-path macos/HeronMac` and `bash packaging/macos/build-app.sh --unsigned`.
*   **Tests:** `cargo test --locked --workspace`, `cargo test --locked --test integration`, `cargo test --locked --test connector_manifests` and `swift test --package-path macos/HeronMac`. `verify` runs `require-tests.sh` over `eval/selfheal/required-tests.txt`.
*   **Lint:** `cargo clippy --locked --workspace --all-targets -- -D warnings`, and `actionlint .github/workflows/selfheal.yml` if it is installed.
*   **Test quality, with testteeth:** `testteeth run --base main --fail-under 70 -q`. Surviving mutants in `fence.rs`, `trust.rs`, `checks.rs`, `src/ext/` and `src/observe/` are killed, not justified.
*   **Evaluator audits, with genesis.** All must be SOUND:
    *   `genesis audit-evaluator eval/selfheal/genesis.yaml`, for the replay grader;
    *   a genesis claim for the fence checker, with the fence tests of item 10 as its cases;
    *   sealbox's `eval/redteam.py` at the pinned commit, with 0 leaks.
*   **Replay:** `run_replay.py` meets HQ10 on the owner's Mac.
*   **End to end, once, by hand:**
    *   CUJ H-1 with a planted bug in `src/schedule.rs`, through real CI;
    *   CUJ H-6 with recorded pairs;
    *   CUJ H-7 with App use only, then Forget;
    *   a test PR that changes `.github/workflows/`, which must be rejected (PT6).

## Work units

These cover Phase 1. The graph runs on a feature branch cut from `main` after Pre-1. Pre-2's prototype file, Pre-4's pinned sealbox commit and Pre-6's skeleton are committed before `skein run`.

*   **Scopes.** Scopes that share a prefix (`src/**` with `src/harness2/**` and `src/ext/**`; `macos/**` twice) are ordered by depends-on.
*   **Escaping.** In the check column, `\|` stands for a shell `|`.

| id | what to build | scope | depends on | check | model role |
|---|---|---|---|---|---|
| H1 | `src/harness2/` (items 1-10) | `src/harness2/**` | none | `cargo test --locked --lib harness2 && cargo clippy --locked --all-targets -- -D warnings && test "$(cargo test --locked --lib -- --list \| grep -c -E 'harness2::.*::(decision_table_selfheal\|pasted_report_is_never_auto\|failure_record_has_no_free_text\|outside_scope_sets_origin_tainted\|fence_denies_views_and_stores\|events_and_embed_are_fenced\|fence_denies_harness2_and_gate_script\|tokens_file_using_process_needs_owner\|fence_bans_every_heroncore_declaration\|schedule_new_use_path_needs_owner\|workflow_diff_is_not_pushed\|redaction_hit_never_pushes\|private_text_blocks_push\|smoke_env_has_no_gh_or_ssh\|unmatched_command_is_denied_in_repair\|scout_rejects_redirect_off_allowlist\|report_never_includes_observations): test')" -eq 17` | hardest tasks: opus high |
| H8 | `src/ext/` executor (item 11) | `src/ext/**` | none | `cargo test --locked --lib ext:: && test "$(cargo test --locked --lib -- --list \| grep -c -E 'ext::.*::(redirect_is_not_followed\|private_address_after_dns_is_refused\|auth_header_only_to_manifest_hosts\|read_op_must_be_get\|path_params_are_percent_encoded): test')" -eq 5` | hardest tasks: opus high |
| H2 | Wiring, executor registration, trust list, permissions, store migration, routes, subcommands (items 14-17, inline tests of 18) | `src/**` | H1, H8 | `cargo test --locked --lib --bins && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib -- state_reports_commit harness2_routes_need_token record_panic_writes_failure_record inbox_action_starts_trigger every_prompt_block_is_trusted_or_taints every_content_entry_is_trusted_or_taints decision_table_ext_tools ext_output_taints ext_op_asks_until_three_approvals ext_first_use_ignores_yolo_and_rules ext_source_memories_start_unconfirmed observation_source_is_outside is_outside_replaces_both_checks offer_connector_absent_in_tainted_session connector_request_refuses_tainted_session migration_adds_only_nullable_columns` | hardest tasks: opus high |
| H2t | Integration test, fixture, connector harness (item 18) | `tests/**` | H2 | `cargo test --locked --test integration && cargo test --locked --test connector_manifests && bash eval/require-tests.sh --test integration -- api_harness2_lists_switches_and_checklist && test -s tests/fixtures/api/harness2.json` | feature, refactoring: sonnet high |
| H10 | `src/observe/`, the `observe` profile, observe routes (item 19) | `src/**` | H2 | `cargo test --locked --lib --bins && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib -- ingest_keeps_nothing_outside_vault ingest_refused_while_paused observe_profile_denies_network master_key_passed_by_fd_not_env config_never_contains_sealbox_key venv_hash_mismatch_refuses_spawn serve_absent_unless_agent_queries_on workflow_job_requires_three_days jobs_make_no_model_call forget_removes_vault_key_memories_and_suggestions workflow_suggestion_session_is_tainted` | hardest tasks: opus high |
| H3 | Replay suite, grader, genesis evaluator, required-tests list (item 20) | `eval/**` | none | `python3 -I -m unittest discover -s eval/tests -t eval -p test_selfheal_replay.py && test "$(grep -c '^## case' eval/selfheal/cases.md)" -eq 10 && test -s eval/selfheal/required-tests.txt && genesis audit-evaluator eval/selfheal/genesis.yaml` | feature, refactoring: sonnet high |
| H4 | `scripts/verify-selfheal.sh` (item 21) | `scripts/**` | none | `bash -n scripts/verify-selfheal.sh && grep -q 'testteeth run --base' scripts/verify-selfheal.sh && grep -q 'eval/containment/check.py' scripts/verify-selfheal.sh && grep -q -- '--no-run' scripts/verify-selfheal.sh && grep -q 'gate-report.json' scripts/verify-selfheal.sh` | feature, refactoring: sonnet high |
| H12 | `.github/workflows/selfheal.yml` (item 26) | `.github/**` | H4 | `grep -q 'heron/selfheal/' .github/workflows/selfheal.yml && grep -qE '^permissions:' .github/workflows/selfheal.yml && grep -qE 'contents: read' .github/workflows/selfheal.yml && ! grep -q 'secrets\.' .github/workflows/selfheal.yml && grep -q 'gate/scripts/verify-selfheal.sh' .github/workflows/selfheal.yml` | feature, refactoring: sonnet high |
| H5 | `build-app.sh` (item 22) | `packaging/macos/**` | none | `bash -n packaging/macos/build-app.sh && grep -q -- '--no-dmg' packaging/macos/build-app.sh && grep -q -- '--host-arch' packaging/macos/build-app.sh && grep -q 'HERON_COMMIT' packaging/macos/build-app.sh && grep -q 'Resources/connectors' packaging/macos/build-app.sh && ! grep -qE '^[^#]*--deep' packaging/macos/build-app.sh` | feature, refactoring: sonnet high |
| H6 | Self-improvement pane, report sheet, inbox buttons, offer card, token field (item 23) | `macos/**` | H2t | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && test "$(swift test list --package-path macos/HeronMac \| grep -c 'SelfImprovementTests')" -eq 8` | feature, refactoring: sonnet high |
| H11 | Observers, Observation pane, consent, exclusions, indicator (item 24) | `macos/**` | H6, H10 | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && test "$(swift test list --package-path macos/HeronMac \| grep -c 'ObservationTests')" -eq 7` | hardest tasks: opus high |
| H7 | Threat-model sections and guides (item 25) | `docs/**` | H2, H10 | `grep -qx '## Harness2: code Heron writes into itself' docs/engineering/SECURITY_THREAT_MODEL.md && grep -qx '## Observation' docs/engineering/SECURITY_THREAT_MODEL.md && test -s docs/features/self_improvement.md && test -s docs/features/observation.md` | judgment and prose: opus high |
| verify | Full suite, required tests, test-quality gate, evaluator audit | `scripts/gates/harness2` (check only) | H1, H2, H2t, H3, H4, H5, H6, H7, H8, H10, H11, H12 | `cargo test --locked --workspace && bash eval/require-tests.sh --lib -- $(cat eval/selfheal/required-tests.txt) && cargo clippy --locked --workspace --all-targets -- -D warnings && swift test --package-path macos/HeronMac && python3 -I -m unittest discover -s eval/tests -t eval && genesis audit-evaluator eval/selfheal/genesis.yaml && testteeth run --base main --fail-under 70 -q` | feature, refactoring: sonnet high |

**Run notes for the build session:**

*   **Unsandboxed checks.** H1, H2, H2t, H8, H10 and `verify` need `cargo test` outside a sandbox. H6 and H11 need `swift build` outside one. The skein supervisor runs those checks.
*   **By-hand runs.** These run by hand after `verify`, with their output in the build PR, because they need the owner's CLI login, TCC grants, GitHub or the sealbox checkout:
    *   the replay go/no-go;
    *   the genesis fence claim;
    *   sealbox's red-team run;
    *   the end-to-end CUJs;
    *   PT6.
*   **Timeout.** Give `verify` `--verify-timeout 3600`.

--------------------------------------------------------------------------------

*Drafted with EGM*
