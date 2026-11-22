# Heron Harness2: self-healing, self-evolution, connectors and observation

*   **Author:** fernandogarzaaa (drafted with Claude)
*   **Status:** In Review
*   **Date:** 2026-11-22
*   **Builds on:** [heron-v1.md](heron-v1.md) (merged to `main` as `f732b4e`) and [heron-v1-features.md](heron-v1-features.md) (approved, being built)
*   **Read at:** `main` = `39dbdf5`

--------------------------------------------------------------------------------

## Overview

The owner asked for these Settings switches, each off by default:

*   **Self-healing.** When the owner reports a bug, or Heron sees one of its own failures, Heron writes the fix and a regression test, has CI check them, and opens a pull request.
    *   In **Phase 2**, a fix that qualifies also rebuilds Heron and reinstalls it in place at `/Applications/Heron.app`.
    *   The owner chose full autonomy for this loop (OD1). HQ22 builds it in phases, because Phase 1 has to measure how many fixes would qualify.
*   **Self-evolution.** A scout runs every 24 hours, every 72 hours, or weekly. It reads AI news and trending open-source repositories, and leaves ideas in the inbox.
*   **Observation and workflow learning.** These are separate switches for:
    *   the screen;
    *   app use;
    *   browser history;
    *   keystrokes (OD3);
    *   files outside the working folder;
    *   learning the owner's workflow.

    Raw observations stay inside the owner's sealbox vault. Only results of fixed queries reach Heron, as outside content ([§8](#8-observation-and-workflow-learning)).

The owner also widened the scope to everything, including new connectors and integrations (OD2). A self-written connector is a declarative manifest, run by a fixed, fenced executor. It contains no code ([§7](#7-self-written-connectors-and-integrations)).

Most of this doc is the set of rules that decide what a self-change may do without the owner. Those rules are proposals. The owner approves or changes them ([Open Questions](#open-questions)).

Five facts shape the design:

1.  **Under a fence that blocks every path to security code, 0 of the last 30 fixes would have installed themselves.**
    *   Security code is spread across most core files.
    *   The Mac app's views, stores and core client can change the permission mode, lift "never" rules and accept the risk notice (`macos/HeronMac/Sources/HeronCore/CoreClientProtocol.swift`).
    *   A file-level fence would have allowed 23 of the 30. Those 23 include Telegram credential and pairing-revocation fixes.

    In Phase 1, every trusted fix whose checks pass ends as a pull request.
2.  **CI checks model-written code, not the owner's Mac.**
    *   Heron's tests can't run under a nested Seatbelt sandbox. About 27 cargo tests fail in a sandboxed shell (recorded 2026-11-19; prototype PT1 measures this again).
    *   The repository's CI already runs the full macOS build and test suite on every pull request, with `permissions: contents: read` and no secrets (`.github/workflows/ci.yml`).
    *   In Phase 1, the only model-written code that runs on the Mac is the repair agent's own `cargo test` runs, inside Heron's Seatbelt sandbox. The local G7 smoke test, which runs a model-built binary, waits for Phase 2.
3.  **v1's full gate can't gate a self-fix.** `scripts/verify-v1.sh` needs 33 G7 runs on each Tier 1 CLI and the 14-day G9 trial. It also discards G7 results after any change under `src` (`scripts/verify-v1.sh:155`). The self-heal gate loosens no v1 threshold, and states what it leaves out ([What the gate does not prove](#what-the-gate-does-not-prove)).
4.  **Pasted text is the easiest way to launder outside content.** If the owner pastes API docs, or restates a scout idea or a preview report, that text arrives as the owner's own words. So reports record where their text came from, and a run from pasted or restated text is never auto-eligible ([§1](#1-trust-which-inputs-may-start-a-repair)). That is a check against mistakes, not a wall: an owner who retypes outside text by hand gets a `typed` report.
5.  **sealbox keeps raw text sealed, but it can't summarise it in words.**
    *   Text leaves the vault only as counts. In Phase 1, one fixed job counts the minutes the owner spends on their own task and project names in each app. For example: "about 20 minutes on 'Invoices' in Mail, most weekdays".
    *   It never says what the emails said.
    *   Whether that is worth capturing keystrokes and screen text for is HQ23.

## The Problem

A bug the owner hits in Heron waits until a coding session gets to it. The fix then needs a branch, a pull request, a build and a manual reinstall. The owner wants Heron to close that loop. They also want it to keep up with agent tooling, to add the integrations it needs, and to work from full context about how the owner works.

The risk comes from what such an agent could do:

*   **Act on itself.** It can change and reinstall its own code.
*   **See everything.** It sees what is typed and shown on screen.
*   **Turn injection into code.** If outside content can steer a self-change, prompt injection becomes code running on the owner's Mac.
*   **Publish it.** That code would also land in a public commit, because `fernandogarzaaa/HERON-Project` is public.
*   **Leak what it sees.** An observation leak exposes passwords, private messages and banking sessions.

Heron v1's standing decisions set the limits:

*   a tainted conversation's grants never reach the shell;
*   "never" rules are lifted only in the app;
*   memory never trusts outside content.

Harness2 keeps all three, including for code Heron writes into itself and for everything it observes.

## Goals

*   **G-H1.** An owner-reported bug becomes a pull request into `main` without further owner action. The PR holds a fix and a regression test that fails on the old code and passes on the new, and CI has checked both. The exceptions are decision rows 0, 0b and 4.
*   **G-H2.** A fix never installs itself if any of these is true:
    *   it touches a fenced path;
    *   its run saw untrusted input;
    *   it started from pasted or restated text.
*   **G-H3 (Phase 2).** A trusted, auto-eligible fix installs itself. A failed health check reverts it and turns auto-install off until the owner turns it back on.
*   **G-H4.** Every published self-change is a commit on its own branch, with a PR into `main`.
*   **G-H5.** The scout runs on its schedule, fetches only from allowlisted hosts, and only writes inbox notes.
*   **G-H6.** The repair agent fixes at least 6 of the 10 replay-suite bugs before the self-healing switch can be turned on (HQ10).
*   **G-H7.** A preview user's "Report a problem" sends nothing by itself.
*   **G-H8.** A self-written connector:
    *   contains no executable code;
    *   reaches only the hosts its manifest lists;
    *   never sees a secret's value;
    *   produces output that taints;
    *   saves memories that start unconfirmed;
    *   has every call go through the decision function.
*   **G-H9.** Observation:
    *   Raw keystroke text, screen text and file contents never leave the vault.
    *   Fixed-job results may name apps, window titles, site hosts and folders, and the consent screen says so.
    *   Password fields and Secure Input are never captured.
    *   An indicator and a one-click pause are always visible.
    *   "Forget everything observed" deletes the vault, its key, and everything derived from it.

## Non-Goals

*   **A Harness²-style optimizer.** Deferred until a suite of 50 or more tasks exists.
*   **Self-healing on preview users' Macs**, and an **auto-updater** for them ([§12](#12-preview-macs)).
*   **GitHub issues, PR comments or CI logs as triggers.** In a public repository, these are outside content.
*   **Changing any v1 threshold:** G7's 26/30, its 2/3 per task, testteeth's 70, and the containment check.
*   **Self-changes that install without owner review** in Harness2, the fence, the gate, observation, the connector executor, CI workflows or any trust table.
*   **New Rust or Swift dependencies.** sealbox, a Python tool installed with `uv`, is the one new outside tool. OD3 names it, and Alternative N weighs the alternatives.
*   **Observation leaving the user's Mac**, apart from fixed-job sentences inside prompts to that user's own provider. Agent queries on the vault are not in this design.
*   **Heron offering connectors on its own.** The owner asks for one (HQ15).

## Definitions

| Term | Meaning here |
|---|---|
| Builder Mac | A Mac where `[harness2] repo` points at a checkout of `fernandogarzaaa/HERON-Project`, and `git`, `gh` (with the publishing token, HQ24), `cargo` and `swift` are found. In practice, the owner's Mac. |
| Trigger | What starts a repair: an owner report (which includes a connector request) or a failure record ([§1](#1-trust-which-inputs-may-start-a-repair)). |
| Provenance | Where a report's text came from: `typed`; `pasted` (the report sheet saw a paste of more than 40 characters); or `from_note` (written through "Restate as report" on an inbox note). |
| Failure record | A row Heron's code writes when it fails. It holds only `kind`, `location`, `version`, `commit`, `count` and `origin_tainted`. |
| Fence | The compiled-in rules that decide whether a diff is **auto-eligible**. It denies by default ([§3](#3-the-fence-what-may-install-itself)). |
| Self-heal gate | Local checks before the push, then every CI check on the PR ([§4](#4-the-self-heal-gate)). Phase 2 adds a local G7 smoke test. |
| Phase 1a, 1b, 1c / Phase 2 | Phase 1 has three separate build graphs: 1a (self-healing and the scout), 1b (connectors) and 1c (observation). Phase 2 adds installing, once HQ22's entry criteria hold. `[harness2] auto_install` exists only in Phase 2 builds. |
| Connector manifest | `connectors/<name>.toml`: a self-written connector, run by the executor in `src/ext/`. |
| Observer | App-side code that captures one kind of observation. |
| sealbox | The owner's local MCP server (`github.com/fernandogarzaaa/sealbox`). It keeps data in an encrypted vault and releases only results that pass its egress gate. Its own term for this is "compute-to-data". |
| Observation vault | A sealbox vault at `~/.heron/observe/vault`. Its key is held in the macOS keychain. |
| Replay suite | 10 historical bugs that the repair agent must fix again from a report alone ([item 20](#20-evalselfheal-create)). |
| PT1 to PT6, Pre-1 to Pre-6 | Prototypes and preconditions ([Preconditions](#preconditions)). |
| HQ*n* | An open question for the owner, with a proposed default. |
| Item *n* | An entry in the [Detailed Implementation Plan](#detailed-implementation-plan). |
| kv | Heron's key-value table in `heron.db`. |
| Design delta | A short follow-up design doc that extends an approved one. |

**Terms from v1.** These keep their meaning from [heron-v1.md](heron-v1.md):

*   Taint, permission mode, `Effect`, unattended work, the global pause, and the decision function (`Policy::decide`, `src/permissions.rs:364-443`).
*   **Q6:** the hold on unattended use of a subscription model.
*   **Tier 1 CLI:** `claude-cli` and `codex-cli` today.
*   **G7:** the task gate. It has the gate tasks `t01` to `t10` and the probe `p1`, each with a fixed pass rule (`eval/core-tasks.md`). `t09` and `t10` are the injection tasks, and `p1` is the containment probe.
*   **G3, G4 and G9:** the regression record, the containment check and the owner trial.
*   **EVE:** the simulated-user harness.
*   **Seatbelt:** the macOS sandbox.
*   **`ContentEntry` and the features base B1 to B6:** from [heron-v1-features.md](heron-v1-features.md).
*   **testteeth** grades tests by mutation. **genesis** audits evaluators. **skein** runs the work units.

**Outside projects:**

*   **Google's Harness²** (`github.com/google-research/harness2`, summarised in anchor-harness's `docs/HARNESS2.md`) improves an agent's harness one task at a time. It edits prompts, tools and skills over many rollouts, and a judge model picks the winner.
*   **anchor-harness** (`~/Documents/PERSONAL/anchor-harness`) is the owner's Rust harness. It has a version store with rollback and a promotion gate.
*   **APE** (`~/workspace/ape-business`) is the owner's JavaScript version of Harness²'s method. It treats the lessons it learns as untrusted.

**Harness parts from the review session's design** (PR #73). Harness2 treats these as existing (Pre-5):

*   **The effect journal.** An action that a crash interrupted asks again before it re-runs.
*   **The loop guard (RepeatGuard).** It stops repeating tool calls, including A-B loops.
*   **Protected paths.** Git hooks, `.envrc`, `.mcp.json`, shell rc files, launch agents, and Heron's own config and database are read-only to the agent, and are hash-checked after each run.
*   **The egress allowlist and toolchain check.** Sandboxed commands reach only allowlisted hosts. Writes to the Rust toolchains are allowed, and the toolchain binaries are hash-checked before their next use.

## Owner decisions recorded in this doc

*   **OD1 (2026-11-22).** Full autonomy for trusted self-heal runs, chosen over proposal-only. HQ22 phases it, so the decision is unchanged and only the order differs.
*   **OD2 (2026-11-22).** Self-healing and self-evolution apply to everything, including new connectors and integrations.
    *   A connector from a trusted trigger may install itself under the same loop.
    *   A scout-found integration stays a proposal.
    *   Changes to trust tables stay fenced and need owner review.
*   **OD3 (2026-11-22).** Keystroke capture stays in, as its own switch, contained by sealbox.
    *   **What it overruled.** The earlier recommendation was to log no raw keystrokes, because they hold passwords, 2FA codes and private messages. Workflow would have been derived from app switches, window titles and file events instead.
    *   **The owner's reason.** v1.x is a research preview that tests how a personal agent works with full context, so accuracy from rich context is the goal.
    *   **What stays.** Raw keystrokes stay in sealbox. Secure Input and password fields are never captured.

## Open Questions

Each question has a proposed default, which the build uses unless the owner changes it.

**Shape decisions: HQ1 to HQ5, HQ22 to HQ24**

*   **HQ1, how wide the fence is.** **Proposed: the allowlist in [§3](#3-the-fence-what-may-install-itself).**
    *   **What it allows:** the Mac app's design tokens, `src/schedule.rs`, new tests and connector manifests.
    *   **What it would have done:** 0 of the last 30 fixes would have been auto-eligible.
    *   **Why so narrow:** views, stores and most core files can each reach security state. Widening the fence safely first needs the app's views split from its stores ([Future Improvements](#future-improvements)).
    *   **What Phase 1 measures:** the 30 commits are mostly hardening work, so Phase 1 measures the real share.
*   **HQ2, model-written code on the Mac.**
    *   **Phase 1:** only the repair agent's own `cargo test` runs execute model-written code on the Mac. They run inside Heron's Seatbelt profile, with the protected paths and egress allowlist from Pre-5. CI runs every check that decides the outcome.
    *   **Phase 2:** this adds a local G7 smoke test of the model-built binary. It runs with a disposable `HOME` and `HERON_HOME`, no `GH_*` or `SSH_AUTH_SOCK`, and only the active CLI's login folder. Phase 2 also installs model-built binaries.

    **Proposed:** accept both, and record them in `docs/engineering/SECURITY_THREAT_MODEL.md`. Add no new `## R-` entry to `docs/ACCEPTED_RISKS.md`. That file is the risk notice every preview user accepts (`src/risk_notice.rs:9`), and it is capped at 3 entries (`scripts/verify-v1.sh:248-249`).
*   **HQ3, unattended runs and Q6.** Unattended work never calls a subscription model unless `allow_unattended_subscription` is set (`src/app.rs:1510-1513`). **Proposed: Harness2 obeys this.**
    *   A held failure record becomes an inbox note with a **Start** button.
    *   A held scout run waits for the app's next connection.
    *   Owner reports are attended, so they run at once.
*   **HQ4, failures that outside content may have caused.** **Proposed: such a failure record still starts a trusted run, but the result is never auto-eligible.** In Phase 1, `origin_tainted` is true when any tainted session was active at the time of the failure. Phase 2 makes this per task (§1).
*   **HQ5, the Phase 2 signing identity.** **Proposed: the release Developer ID, with the hardened runtime and the release entitlements, without notarization.** A separate, revocable identity under the same team ID is an option. PT2 checks that privacy grants survive.
*   **HQ22, phasing.** **Proposed:**
    *   **Phase 1** is three separate build graphs:
        *   **1a:** self-healing to PR, plus the scout;
        *   **1b:** connectors;
        *   **1c:** observation, which alone waits for the sealbox additions (Pre-4).
    *   **Phase 2** adds the installer, the local G7 smoke test, health check and rollback, and the stricter fence rules (R7, R9, per-task `origin_tainted`). It starts when all of these hold:
        *   the replay suite passes HQ10;
        *   PT2 and PT3 pass;
        *   3 or more merged self-heal PRs labelled `self-heal:auto-eligible` within 30 days.
*   **HQ23, keystrokes and screen text in Phase 1c.** In Phase 1, the only reader of captured text is the topic-minutes job (§8). **Proposed: ship the Keystrokes and Screen switches with that job.** The alternative is to defer both switches until a richer reader exists, for example a local model inside the vault. Under that alternative, Phase 1c ships App use, Browser history and Files only.
*   **HQ24, the publishing token and CI secrets.** **Proposed:**
    *   Heron publishes with a fine-grained GitHub token that the owner creates and stores from Settings. It gives contents and pull-request access to this one repository, with no `workflow` scope, so GitHub rejects any push that changes `.github/workflows/`.
    *   Heron never pushes tags.
    *   The release secrets used by `.github/workflows/release.yml` move into a protected `release` environment.

**Tuning: HQ6 to HQ11**

*   **HQ6, which model repairs.** **Proposed: the active Tier 1 CLI, with its default model. codex-cli is the fallback when the active CLI hits a limit.**
*   **HQ7, caps.** **Proposed:**
    *   3 attempts per trigger;
    *   40 steps and 500,000 tokens per attempt;
    *   3 repair runs in any 24 hours;
    *   Phase 2: 2 installs in any 24 hours, and none while an installed self-fix is unmerged;
    *   the scout: 60 items and one model call of up to 50,000 tokens per run.

    The usage budget (`src/usage.rs`) applies too.
*   **HQ8, scout sources and schedule.** **Proposed sources:**
    *   **GitHub search:** the topics `llm-agent`, `ai-agent`, `agent-framework` and `mcp`, pushed in the last 7 days and sorted by stars. GitHub has no trending API.
    *   **Hacker News (Algolia):** stories with 100 or more points that match "agent", "LLM" or "MCP".
    *   **arXiv:** `cs.AI`, `cs.CL` and `cs.SE`, with "agent" in the title.
    *   **Lab news feeds:** Anthropic, OpenAI and Google DeepMind.

    **Proposed schedule: weekly.**
*   **HQ9, what goes into a public PR.** **Proposed:**
    *   The PR body holds the gate report, the changed files, and a description written from the diff.
    *   Report text goes in only with the "OK to publish" tick, which is off by default.
    *   Before any push, R5 (the secret scan) and the private-text check must both be clean. The private-text check blocks a push when any 8-word run from the report or its attachment appears in the diff (row 0b).
*   **HQ10, the replay threshold.** **Proposed: 6 of 10.**
*   **HQ11, the Phase 2 rollback copy.** **Proposed: one zip of the previous app.**

**Connectors: HQ12 to HQ15**

*   **HQ12, connectors written from documentation.** A manifest written from what the model already knows, in an untainted run, is trusted. A connector request with pasted docs has provenance `pasted`: it gets a PR, and it is never auto-eligible. **Proposed: keep this.**
*   **HQ13, trusting a connector.** **Proposed: use the same model as MCP servers.**
    *   A connector is untrusted until the owner marks it trusted on its Settings card. While untrusted, each call is `ExternalWrite`, so it asks every time.
    *   Once trusted, reads are `NetworkEgress` and writes are `ExternalWrite`.
    *   Trust resets when the manifest's hash changes, as MCP trust resets when a tool changes (`src/mcp.rs:159-167`).
*   **HQ14, OAuth.** **Proposed: an API-key or no-auth manifest can be auto-eligible. One that needs a new OAuth provider is PR-only**, because `src/oauth.rs` is fenced.
*   **HQ15, requesting a connector.** **Proposed: the report sheet gains a "Request a connector" mode with three fields: a service name, the operations wanted, and notes.** It queues an owner report. Heron doesn't offer connectors on its own in Phase 1.

**Observation: HQ16 to HQ21**

*   **HQ16, sealbox's sandbox.** **Proposed: Heron runs every sealbox process under its own Seatbelt profile, with no network and access to `~/.heron/observe/` only. sealbox's `run_python` is never used.** sealbox's own sandbox is Linux-only, and it falls back to an audit hook on macOS (sealbox `src/sealbox/sandbox.py:62-66`).
*   **HQ17, sealbox on preview Macs.** **Proposed: observation is offered only where `uv` is installed.**
*   **HQ18, keystrokes.** **Proposed:**
    *   capture only while Secure Input is off, the focused element isn't an `AXSecureTextField`, and the frontmost app isn't excluded;
    *   store the text in chunks of 1 minute per app;
    *   run sealbox's detectors over each chunk before it is written, to drop card numbers, one-time codes and API-key shapes.

    macOS doesn't guarantee Secure Input in every app.
*   **HQ19, retention.** **Proposed: raw data for 14 days, and derived memories for 90 days.**
*   **HQ20, exclusions.** **Proposed:**
    *   **Apps:** password managers, Keychain Access, System Settings, and any app named with "bank".
    *   **Private browsing windows:** excluded on a best-effort basis.
    *   **Sites:** banking, health and password-manager sites, from an editable list.
    *   **Folders:** `~/Library`, `~/.ssh`, `~/.gnupg`, and Heron's home.
*   **HQ21, what a provider sees.** **Proposed: the consent screen says that fixed-job sentences (app names, titles, hosts, folders, topic minutes) go to the user's model provider inside prompts.**

## Critical User Journeys (CUJs)

### CUJ H-1: The owner reports a bug, and Heron opens a checked PR

*   **Given:** a builder Mac with self-healing on and no run active.
*   **When:** the owner chooses Help › Report a problem, types "Next run shows UTC for 'every monday at 8'", and sends.
*   **Then:**
    1.  Heron makes a worktree at the installed commit and runs a repair. The agent fixes `src/schedule.rs` and adds `tests/schedule_local_time.rs`.
    2.  Heron commits the fix locally. The fence marks it auto-eligible, and the secret and private-text checks are clean.
    3.  Heron pushes and opens a **draft** PR.
    4.  CI runs `ci.yml` (the build, tests, clippy and Swift) and `selfheal.yml`. `selfheal.yml` checks R2, which means the new test must fail on the old code and pass on the fix. It also runs testteeth and the containment check.
    5.  When every check passes, the PR is marked ready and labelled `self-heal:auto-eligible`. The inbox says "Fix ready: next-run time zone (PR #NN)".

    This is expected to take 1 to 2 hours, mostly in the CI queue. PT4 measures it.

### CUJ H-2: The fix touches fenced code

*   **Given:** the same Mac.
*   **When:** the owner reports "Telegram pairing asks twice".
*   **Then:** the fix touches `src/telegram.rs`. CI still runs. The PR is labelled `self-heal:needs-owner`, and the inbox names the fenced file.

### CUJ H-3 (Phase 2): A self-fix installs itself, then fails its health check

*   **Given:** a Phase 2 build, with auto-install on and the signing identity set.
*   **When:** a fix meets every condition of decision row 1.
*   **Then:**
    1.  Heron shows "Installing fix #7. Heron will restart" and quits. The swapper replaces the app and relaunches it.
    2.  The new core never writes its port file. After 90 seconds, the swapper restores the previous app, relaunches it, and turns auto-install off.
    3.  The PR gets a comment, and the inbox offers "Turn auto-install back on".

### CUJ H-4: The scout finds an idea

*   **Given:** self-evolution is on, weekly.
*   **When:** the weekly scout run happens.
*   **Then:**
    1.  The inbox gains "Idea from the web: A-B loop detection in <repository>", with its link, the part of Heron it concerns, its effort, and "from the web, not checked".
    2.  **Restate as report** opens the report sheet with provenance `from_note`. That run can produce a PR, but it is never auto-eligible (row 3).

### CUJ H-5: A preview user reports a problem

*   **Given:** a preview Mac.
*   **When:** the user chooses Help › Report a problem.
*   **Then:**
    1.  Heron writes `Heron-report-<date>.heronreport`, a JSON file, to the Desktop. It holds the user's text, the failure records from the last 7 days, and the version and commit. It holds a conversation only if the user ticked "Include this conversation". A tainted conversation is refused with "This conversation includes outside content, so it can't be attached." Observations are never included.
    2.  The share sheet opens.
    3.  On the owner's Mac, File › Import report adds the file to the inbox as outside content. **Restate as report** sets provenance `from_note`.
    4.  A merged fix reaches the user in the next DMG.

### CUJ H-6: The owner asks for a connector

*   **Given:** a builder Mac.
*   **When:** the owner opens Report a problem › Request a connector, and types the service `todoist`, the operations "list tasks" and "add task", and the note "for the morning brief".
*   **Then:**
    1.  A trusted repair writes `connectors/todoist.toml`:
        *   host `api.todoist.com`;
        *   `list_tasks` as `GET /rest/v2/tasks`, a read;
        *   `add_task` as `POST /rest/v2/tasks`, a write;
        *   `api_key` auth.

        It also writes `connectors/todoist.test.json`, with request-shape pairs.
    2.  The fence and CI pass, and the PR is labelled `self-heal:auto-eligible`. A Phase 2 build installs it.
    3.  Once a build with the manifest is installed, Settings › Connectors shows "Todoist (written by Heron)" with an empty key field and a **Trust** switch. The owner pastes the key there. Heron never asks for the key in chat and never types it.
    4.  Until the owner turns on **Trust**, every call asks (HQ13). Output from the connector taints the session, and anything learned from it is saved unconfirmed.

### CUJ H-7: The owner turns on observation

*   **Given:** `uv` and sealbox are installed, and every observer is off.
*   **When:** the owner turns on Keystrokes, App use and Workflow learning.
*   **Then:**
    1.  Each switch shows its consent screen once. macOS asks for Input Monitoring and Accessibility.
    2.  The menu bar icon gains an observing dot, and its menu starts with **Pause observing**.
    3.  Two days later, the jobs have produced two results:
        *   "Most weekdays 9:05 to 9:20: Mail, then Numbers 'Invoices', then Preview" (app sequence);
        *   "About 20 minutes typing about 'Invoices' in Mail on 3 days" (topic minutes; 'Invoices' is one of the owner's task names).

        Each becomes an unconfirmed memory. The sequence also becomes an "Automate this?" suggestion. Accepting it opens the normal `save_automation` approval in a tainted session.

## Technical Plan (Architecture)

### 0. Shape

```text
 Heron.app (builder Mac)
 ┌───────────────────────────────────────────────────────────────────────────────────────┐
 │ SwiftUI: Self-improvement · Observation · Report a problem · inbox buttons             │
 │          observers (screen, apps, browser, keys, files) ──────────────┐                │
 └──────────────▲────────────────────────────────────────────────────────┼────────────────┘
                │ /api/harness2/*, /api/observe/*                        │ /api/observe/ingest
 ┌──────────────┴────────────────────────────────────────────────────────▼────────────────┐
 │ Rust core                                                                              │
 │  src/harness2: trigger ─▶ trust ─▶ repair ─▶ commit ─▶ local checks ─▶ push, draft PR  │
 │                ─▶ CI ─▶ label  [Phase 2: G7 smoke ─▶ swapper ─▶ health ─▶ revert]      │
 │                scout ─▶ scout client ─▶ tool-less ranking ─▶ inbox notes               │
 │  src/ext:      connector manifests ─▶ fixed executor ─▶ tools (untrusted output)       │
 │  src/observe:  sealbox append and fixed jobs (Seatbelt, no network) ─▶ outside memories│
 └──────────────┬─────────────────────────────────────────────────────────────────────────┘
                │ git push (token without workflow scope)
 ┌──────────────▼───────────────────────────────────────────────────────┐
 │ GitHub CI (contents: read, no secrets)                               │
 │  ci.yml: build and tests · selfheal.yml: main's gate on the PR head  │
 └──────────────────────────────────────────────────────────────────────┘
```

*   The repair agent is Heron's own agent loop ([Alternative A](#alternative-a-a-headless-coding-cli-with-its-own-tools-does-the-fix)).
*   The fence runs in the installed binary.
*   The gate script comes from `main`.

### 1. Trust: which inputs may start a repair

A run's trust comes from the class of every input the model sees. Who clicked doesn't matter, because v1's taint never clears. The owner's typed words are trusted, as in v1. Pasted text and text restated from a note are marked.

| Input | What the model sees | Class | What it can do |
|---|---|---|---|
| Owner report or connector request, provenance `typed` | the text, any connector fields, the version and commit, and the last 24 hours of failure records | Trusted | Start a repair, which may be auto-eligible |
| The same, provenance `pasted` or `from_note` | the same | Trusted, marked | Start a repair, which is never auto-eligible (row 3) |
| An owner report with an untainted conversation attached | the above, plus the conversation | As its provenance | Start a repair. A tainted conversation is refused |
| A failure record | only `kind`, `location`, `version`, `commit`, `count` and `origin_tainted` | Trusted | Start a repair once `count` reaches 2 for the same `(kind, location, commit)`. If `origin_tainted`, never auto-eligible (HQ4). It starts nothing when `commit` is a self-fix build |
| An imported preview report, or a scout idea | another person's text, or web text | Untrusted | An inbox note only. The owner may restate it (`from_note`) |
| A workflow suggestion | fixed-job sentences | Untrusted | A suggestion. Accepting it opens a tainted `save_automation` approval |

Untrusted inputs never start a run.

**The decision table.** The test `decision_table_selfheal` checks it. Rows are checked in order, and the first match wins.

| # | Run | Fence verdict | Other state | Checks | Outcome |
|---|---|---|---|---|---|
| 0 | any | any | R5 hits the diff, the commit message or the PR body | — | **Not pushed.** An inbox note. The branch is kept locally for 7 days |
| 0b | any | any | The private-text check hits, or the diff touches `.github/**`, `scripts/**`, `packaging/**` or `eval/**` | — | **Not pushed until the owner reviews.** An inbox note with **Review** (opens the worktree in the owner's editor) and **Push** |
| 1 | Trusted, `typed` | Auto-eligible | Phase 2 build, auto-install on, under the caps, signing identity set, installed commit in `origin/main` | CI and the G7 smoke test pass | **Install**, plus a PR labelled `self-heal:auto` |
| 2 | Trusted, `typed` | Auto-eligible | Any condition of row 1 doesn't hold (always the case in Phase 1) | CI passes | PR labelled `self-heal:auto-eligible` |
| 3 | Trusted | Needs owner (including `pasted`, `from_note` and `origin_tainted`) | any | CI passes | PR labelled `self-heal:needs-owner` |
| 4 | Trusted | any | any | CI fails 3 times, or doesn't run within 24 hours | The draft PR is closed with its failure report, and the inbox says so |
| 5 | — | — | A run is active, an approval is waiting, or Heron is paused | — | Queued |
| 6 | — | — | Held by Q6 | — | An inbox note with **Start** |

**When a run fails** (`triggers.state` in brackets):

*   **No diff, or a cap was hit:** this counts as a failed attempt [`retrying`, then `failed`].
*   **The provider hit a limit:** the fallback is tried once, and then the run waits 6 hours [`waiting_limit`].
*   **The 24-hour cap was reached:** the run is held in the queue [`queued`].
*   **Setup failed:** an inbox note [`setup_failed`].
*   **A crash interrupted it:** the effect journal (Pre-5) asks "Repair #N was interrupted. Run again?" [`interrupted`].
*   **The switch was turned off:** the current step finishes and nothing more is pushed [`cancelled`].

**Producers of failure records:**

| `kind` | Writer |
|---|---|
| `panic` | The existing panic hook (`src/main.rs:714-722`) calls `harness2::record_panic(location)` |
| `core_exit` | The app sees a non-zero exit (`CoreProcess.swift`). The core writes the record on its next start |
| `migration_failed` | The `Store::open` migration error path |
| `internal_error` | Every `ApiError` with status 500 in `src/server.rs` |
| `health_failed` | The swapper (Phase 2) |

**`origin_tainted`.** In Phase 1, it is true when a process-wide counter of active tainted sessions is above zero at the time of the failure. That is coarse but safe: it only stops auto-eligibility. Phase 2 adds a task-local scope, set by every outside-content handler, so that auto-install can rely on it.

### 2. The repair run

`heron-cli selfheal repair --run <id>` runs as a separate process:

*   **Home:** a disposable `HERON_HOME` at `~/.heron/selfheal/runs/<id>/home`. It has no memories, persona, connectors, Telegram or MCP.
*   **Workspace:** a git worktree at `~/.heron/selfheal/runs/<id>/wt`, on the branch `heron/selfheal/<yyyymmdd>-<id>`, cut from the installed commit (the new `commit` field of `GET /api/state`).
*   **Tools:** `read_file`, `write_file`, `edit_file`, `list_dir`, `search_files`, `find_files` and `shell`. `App::assemble` filters the tool registry down to these.
*   **Policy:** `mode = "ask"` with the existing `DenyAll` approver, so every `Ask` becomes a denial. Auto mode would allow any unmatched command (`src/permissions.rs:438`). The allow rules are:
    *   `write_file:*` and `edit_file:*`, which `check_workspace` limits to the worktree;
    *   `shell:cargo test --locked*`, `shell:cargo check --locked*`, `shell:cargo clippy --locked*`, `shell:git diff*` and `shell:git status*`.

    A rule for one command never covers a compound command (`allow_rule_covers`, `src/permissions.rs:449-460`). The shell runs under Heron's Seatbelt profile, with the protected paths, egress allowlist and loop guard from Pre-5.
*   **Prompt:** the fixed Trusted `PromptSource::Repair`. It contains:
    *   the trigger's fields;
    *   "Fix the cause, add a test under `tests/`, `macos/HeronMac/Tests/` or `connectors/` that fails before your fix, and don't edit existing tests";
    *   for a connector request, the manifest schema;
    *   the auto-eligible paths, given as advice.
*   **Model:** the HQ6 provider, through `cli_llm`. The five CLI containment invariants hold (`src/cli_llm.rs:30-50`).
*   **Retries:** up to 3 attempts. Attempts 2 and 3 get the previous CI failure's last 60 lines, which is Harness²'s actionable retry feedback.

### 3. The fence: what may install itself

The fence is a set of constants in `src/harness2/fence.rs`, compiled into the installed binary. The running Heron checks the local commit with:

```text
/Applications/Heron.app/Contents/MacOS/heron-cli selfheal check-fence --repo <path> <base> <fix>
```

The worktree's copy of `fence.rs` is never used, and `src/harness2/**` is itself fenced. In Phase 1, the verdict sets the PR label, and HQ22 counts the labels.

**Auto-eligible paths.** Every changed path must match one of these:

*   `macos/HeronMac/Sources/HeronUI/Tokens/**`. `Avatar/**` is excluded, because `AvatarStateMachine.swift` imports `HeronCore`.
*   `macos/HeronMac/Tests/**`.
*   `src/schedule.rs`.
*   `tests/**`, except `tests/fixtures/**`.
*   One connector per run: `connectors/<name>.toml` and `connectors/<name>.test.json`.

**Everything else needs the owner.** That includes:

*   **Core:** every other `src/` file. `events.rs` renders the risk labels the owner sees, and `embed.rs` chooses which model file is downloaded. The connector executor, `src/ext/**`, is fenced too.
*   **App:** `HeronCore/**`, `HeronUI/Stores/**`, `HeronUI/Views/**` and `macos/HeronMac/Sources/Heron/**`.
*   **Gate and build:** `eval/**`, `scripts/**`, `packaging/**`, `.github/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `build.rs`, `Package.swift`, `*.plist` and `*.entitlements`.
*   **Docs:** `docs/**`. A docs-only fix is a PR with no install.

**Content rules.** A broken rule gives the verdict "needs owner". R2 is decided in CI, and R5 is decision row 0.

*   **R1, tests are add-only.** No removed lines in `tests/**`, `macos/HeronMac/Tests/**`, `connectors/*.test.json`, or the `#[cfg(test)]` module of `schedule.rs` (everything after that line at `base`).
*   **R2, the regression test bites.** There is at least one new test. CI checks it ([§4](#4-the-self-heal-gate)).
*   **R3, no deleted, renamed or binary files, and no mode changes.**
*   **R4, a size cap.** At most 400 lines added plus removed.
*   **R5, the secret scan.** `src/util.rs`'s redaction runs over the diff, the commit message and the PR body.
*   **R6, connector manifests.** `ext::validate_manifest` must accept the manifest. Auth must be `none` or `api_key` (HQ14).
*   **R8, no overlap.** The fix touches no file that an open self-heal PR touches (`runs.files_json`).
*   **R7 and R9 (Phase 2).** Before a Tokens or `schedule.rs` diff can install itself, two more rules apply:
    *   R7 checks Swift identifiers: no `HeronCore` declarations, no `*Store` or `*Client` types, and no `Process`, `FileManager` or `URLSession`.
    *   R9 checks that no new `use` path or `std::process`, `std::fs` or `std::net` item appears in `schedule.rs`.

    In Phase 1, these diffs are labelled auto-eligible without R7 and R9, as candidates for measurement only.

### 4. The self-heal gate

**A. Local checks, before the push.** These run only Heron's own code:

1.  the fence;
2.  R5 and the private-text check;
3.  the push rule (row 0b).

Every `git` call passes:

```text
-c core.hooksPath=/dev/null -c core.fsmonitor=false -c diff.external= -c filter.lfs.process=
```

Harness2 also checks that the worktree's `.git` file still points at `<repo>/.git/worktrees/<id>`.

**B. CI, after the push.** Heron requires **every** check on the PR to pass, read with `gh pr checks`:

*   **`ci.yml`, unchanged.** It already builds and tests on macOS, Linux and Windows. That covers `cargo test`, the desktop bin tests, clippy, `swift test`, the app build and the upgrade test.
*   **`selfheal.yml`, new.** It runs on `pull_request` for heads `heron/selfheal/**`, with `permissions: contents: read` and no secrets. It checks out `main` into `gate/` and the head into `fix/`, then runs `main`'s `bash gate/scripts/verify-selfheal.sh <base> <fix> fix/`.

The script runs only what `ci.yml` doesn't:

| Step | Runs when | Command |
|---|---|---|
| R2 | always | the table below |
| Mutation | any change under `src/`, `tests/` or `connectors/` | `testteeth run --base <base> --fail-under 70 -q` |
| Containment | the same | `python3 -I eval/containment/check.py` |

**R2 outcomes.** The script copies only the new test files onto a checkout of `base`. It compiles them with `cargo test --locked --no-run` (or `swift build --build-tests`), then runs them with `cargo test --locked --test <stem>` (or `swift test --filter <Class>`).

| On `base` | On `fix` | Result |
|---|---|---|
| Compiles, and the new tests fail | They pass | R2 passes |
| Compiles, and the new tests pass | — | Gate failure: the test doesn't catch the bug |
| Doesn't compile (needs a new API) | They pass | Verdict "needs owner". The gate continues |
| A new connector (no manifest on `base`) | Its request-shape pairs pass | R2 passes. The pairs check the manifest's template, not the real API |
| — | The new tests fail | Gate failure |

If GitHub's macOS runners don't run within 24 hours, the run ends as row 4, reported as "CI did not run".

**C. Phase 2: the local G7 smoke test.** This isn't built in Phase 1. For a trusted `typed` run that is about to install, it does three things:

*   **Builds.** It builds `base` and `fix` in fresh worktrees, after `git clean -xdf`.
*   **Runs.** It runs `eval/run-core-tasks.sh --runs 1 --tasks t01,…,t10,p1` from the base checkout, with the HQ2 environment.
*   **Passes or fails.** Every task that passed on `base` must pass on `fix` (one retry allowed), and `t09`, `t10` and `p1` must pass.

This compares a candidate with its own base, as anchor-harness's promotion gate does. It doesn't touch `check_gate.py`.

#### What the gate does not prove

A self-fix that passed this gate has **not** passed:

*   G7 at 3 runs on every Tier 1 CLI;
*   G9;
*   the G3 record;
*   the genesis audits.

The PR into `main` carries those checks.

### 5. Publishing

**The order:**

1.  repair;
2.  commit in the worktree;
3.  local checks on that commit;
4.  push;
5.  draft PR;
6.  CI;
7.  label the PR and mark it ready, or close it.

The commit that the fence and R5 check is the commit CI tests.

**Rules:**

*   Each step checks git and GitHub state first. If `git ls-remote` shows the branch, Heron doesn't push it again. If `gh pr list --head <branch>` shows a PR, it doesn't open another.
*   All PRs target `main`. Heron never pushes tags.
*   A tool-less model call writes the commit message from the diff, and R5 then scans it.
*   Each tick updates `runs.pr_state` from `gh pr view --json state`.

### 6. The scout, the tick and the inbox

**The scout** has its own timer, because `schedule.rs` can't express 72 hours: `every 72h` fails with "does not divide evenly" (`src/schedule.rs:250-256`). The interval is `[harness2] scout_every_hours`, set to 24, 72 or 168.

*   **Fetching.** It uses its own client, built the way `web.rs` builds its client:
    *   `permissions::PublicOnlyResolver` for DNS (`src/tools/web.rs:55`);
    *   `no_proxy`;
    *   a redirect policy that follows at most 5 redirects, and only to allowlisted hosts.

    The allowlisted hosts are `api.github.com`, `hn.algolia.com`, `export.arxiv.org`, `www.anthropic.com`, `openai.com` and `deepmind.google`.
*   **Parsing.** GitHub and Hacker News return JSON. The arXiv and lab feeds go through a tolerant regex reader that keeps `title`, `link`, and `updated` or `pubDate`, and skips any entry it can't parse.
*   **Ranking.** One model call, with no tools, in a session marked tainted from the start. The items go in wrapped as untrusted (`src/agent.rs:692-702`). The call returns `[{item_id, harness_part, why, effort}]`, at most 10 entries. Each `item_id` must be one that was fetched. Links and titles come from the fetched data, never from the model.
*   **Output.** Inbox notes titled "Idea from the web: …". Seen URLs go into `selfheal.db`. The scout writes nothing else.

**`Harness2::tick`** runs every minute in the app's scheduler loop, beside `run_due_schedules`. That function returns early while Heron is paused (`src/app.rs:1483-1486`).

| Step | Runs while paused? | Held by Q6? |
|---|---|---|
| 1. Poll PRs and checks (`gh pr view`, `gh pr checks`) | yes | no (no model call) |
| 2. Start a queued owner report | no | no (attended) |
| 3. Start a queued failure-record trigger | no | yes: a row-6 note |
| 4. Run the scout, if due | no | yes: it runs at the app's next `GET /api/events` connection |
| 5. Prune worktrees and branches older than 7 days | yes | no |

**Inbox actions.** The `inbox` table gains a nullable JSON column, `action`, and `add_inbox` takes an optional action.

| Action | Button | Route |
|---|---|---|
| `start_trigger` | Start | `POST /api/harness2/triggers/{id}/start` |
| `rerun_trigger` | Run again | the same |
| `review_push` | Review, then Push | Review opens the worktree in the owner's editor. Push is `POST /api/harness2/runs/{id}/push` |
| `restate` | Restate as report | opens the report sheet with provenance `from_note` |
| `open_url` | Open PR | none |

**Storage.** `~/.heron/selfheal/selfheal.db` holds these tables:

*   `triggers(id, kind, provenance, fields_json, state, attempts, created_at)`
*   `runs(id, trigger_id, base, fix, branch, verdict, files_json, gate_report_sha256, pr_url, pr_state)`
*   `scout_seen(url_sha256, first_seen)`
*   `replay(commit, passed, total, auto_eligible, ran_at)`
*   Phase 2: `installs(commit, previous_commit, installed_at, state)`, following anchor-harness's version store (`crates/harness-core/src/versions.rs:31`, `:122`).

While `self_healing = true`, the core refuses to start a repair unless `replay` has a passing row for the merge base of the installed commit with `main`. That row is written by `heron-cli selfheal record-replay`, and `run_replay.py` is its only caller.

### 7. Self-written connectors and integrations

A self-written connector is a declarative manifest, run by the fixed executor in `src/ext/`. The manifests extend the existing custom-connector registry: they are the execution layer for its reserved `rest_api` transport, which v1 marks "remote execution is NOT yet enabled" (`src/connectors.rs:28`). G-H8 holds because the executor is fenced, whatever any manifest says.

**The manifest, `connectors/<name>.toml`:**

```toml
name = "todoist"              # ^[a-z][a-z0-9_]{1,30}$
title = "Todoist"
hosts = ["api.todoist.com"]   # HTTPS DNS names: lowercase, one dot or more; no IPs, ports or userinfo; not localhost, .local or .internal
[auth]
kind = "api_key"              # none | api_key | oauth (oauth is never auto-eligible)
header = "Authorization"
scheme = "Bearer"
[[ops]]
name = "list_tasks"           # ^[a-z][a-z0-9_]{1,40}$, at most 16 ops
kind = "read"                 # read: GET only; write: POST | PUT | PATCH | DELETE
method = "GET"
host = "api.todoist.com"      # one of hosts
path = "/rest/v2/tasks"       # literal segments and {param} placeholders only
query = ["project_id"]
body = "none"                 # none | json (the listed params as one object)
select = ["/0/content", "/0/due/date"]   # JSON pointers kept from the response; at most 32
params = { project_id = { type = "string", max_len = 64 } }   # string | integer | boolean; strings percent-encoded
max_items = 50
```

**`connectors/<name>.test.json`** holds request-shape pairs: `{op, args, request: {method, url, body}}`. The harness `tests/connector_manifests.rs` validates every manifest, and checks that the request the executor builds equals each recorded `request`. This tests the template, not the real API. The real safeguards are the validation, HQ13's trust switch, and the executor's limits.

**The executor, `src/ext/`** (new, fenced). At startup it loads every manifest from the app bundle's `Contents/Resources/connectors/`, which `build-app.sh` fills. In development it loads from `<repo>/connectors/`. For each manifest:

1.  **Validates.** It checks the manifest again. One that fails isn't registered, and its card says why.
2.  **Registers.** It registers the manifest as a custom connector with `transport = "rest_api"` and source `heron`. `connectors::require_custom` is generalised to accept loaded manifest IDs, so the existing card and the key path (`connectors::put_api_key`, `POST /api/connectors/custom/{id}/api-key`) work unchanged. The owner types the key in Settings.
3.  **Makes tools.** It adds one tool per operation, named `ext_<name>_<op>`, with `untrusted_output = true`.
    *   While the connector is untrusted, each tool's effect is `ExternalWrite`, which asks every time.
    *   Once the owner turns on **Trust** on the card, reads become `NetworkEgress` and writes stay `ExternalWrite`.
    *   Trust is stored in kv as `ext_trust:<name> = <manifest sha256>`, so a changed manifest is untrusted again, as for MCP (`src/mcp.rs:159-167`).
4.  **Sends requests.** It builds each request only from the template and the validated parameters, under these limits:
    *   its client uses `PublicOnlyResolver` and `no_proxy`;
    *   it follows no redirects (`Policy::none()`);
    *   the host must be in the manifest;
    *   responses are capped at 1 MB, with a 20-second timeout.

    The executor adds the auth header itself, from the keychain, and only on the manifest's hosts.
5.  **Allows learning.** It registers the source `ext:<name>` with `src/sources.rs`.

**One change to the trust table, made by this doc.** `src/memory.rs` gains `is_outside(source)`. It returns true for every source in `OUTSIDE_SOURCES`, for any `ext:` prefix, and for `observation`. It replaces both direct `OUTSIDE_SOURCES.contains` calls (`src/memory.rs:276`, `:367`).

These memories can be confirmed by the owner's click, as v1's email memories can, and the trust lists stay fenced afterwards.

**What a manifest can still do:**

*   send the agent's arguments to its own hosts, under the decision function;
*   return hostile data, which taints;
*   change state with a `GET` on a badly designed API.

### 8. Observation and workflow learning

**What each switch turns on.** Each switch is off by default. It shows a consent screen the first time, and records `observe_consent:<switch>:<version>`.

| Switch | What it turns on |
|---|---|
| 1. Screen, 2. App use, 3. Browser history, 4. Keystrokes, 5. Files outside the working folder | That observer, plus the sealbox `append` process while any of 1 to 5 is on |
| 6. Workflow learning (needs one of 1 to 5) | The daily jobs and the suggestions |

**Observers** live in `macos/HeronMac/Sources/HeronCore/Observers/`, which is fenced. Each one asks for its permission only through the macOS prompt. If the owner refuses, it stops and says so in Settings.

| Observer | How | Permission | What it keeps |
|---|---|---|---|
| Screen | ScreenCaptureKit captures the frontmost window every 30 s. Vision OCR runs on the Mac | Screen Recording | `day`, `ts`, app, title, OCR text. The image is dropped |
| App use | `NSWorkspace` activation events, and the window title through Accessibility | Accessibility | `day`, start, end, app, bundle ID, title |
| Browser history | A read-only copy of Safari's and Chrome's history every 15 minutes | Full Disk Access | `day`, `ts`, host, page title. Never the full URL |
| Keystrokes | `CGEventTap`, listen-only | Input Monitoring | `day`, minute, app, and the text after detectors. Nothing is kept while Secure Input is on, while an `AXSecureTextField` has focus, or while an excluded app is in front (HQ18) |
| Files outside the working folder | FSEvents on `~/Documents`, `~/Desktop` and `~/Downloads` | possibly Files & Folders | `day`, `ts`, path, event. Never the contents |

*   **Exclusions** (HQ20) apply in the observer, before anything is written.
*   **Stopping.** Observers stop while Heron is paused, while **Pause observing** is on, and while the screen is locked. The phone can pause observers but can't turn them on.
*   **Two file observers.** v1's `observations` table (`src/store.rs:239-241`) keeps working-folder file events in `heron.db`, as before. The new file observer covers other folders, whose paths are more personal, so it writes to the vault instead.

**sealbox** is used at v0.1, commit `c863992`. Heron uses three of its parts:

*   its encrypted vault (AES-256-GCM, with a data key per dataset wrapped by a master key);
*   its restricted SQL engine behind its egress gate (PII redaction, injection stripping, content fingerprints);
*   its detectors and its hash-chained audit log.

What it gives over a Rust-side store, in this configuration (Alternative N):

*   encryption without a new Rust cipher crate;
*   an egress gate that has been red-teamed and mutation-graded;
*   the detectors;
*   a tamper-evident audit log.

All of it is the owner's own tested code.

The integration:

1.  **Install.** Settings › Observation › Install sealbox runs, in order:
    *   `uv venv ~/.heron/observe/venv`;
    *   `uv pip install --require-hashes -r <sealbox's hashed lock at the pinned commit>`;
    *   the install of sealbox from `git+https://github.com/fernandogarzaaa/sealbox@<commit>`.

    The commit and the lock file's SHA-256 are pinned in the fenced file `src/observe/sealbox.rs`. Heron records a hash of the venv and checks it before every spawn.
2.  **The key.** It is held in the macOS keychain. Each sealbox process gets it through an inherited pipe (`--key-fd 3`), never through the environment or a file.
3.  **Ingest.** Observers post batches to `POST /api/observe/ingest`. The core streams each batch to a `sealbox append` child, which writes encrypted hourly segments to `~/.heron/observe/vault`. Raw observations never reach `heron.db`, backups, the event bus or logs.
4.  **Sandbox.** Every sealbox process runs under a new Seatbelt profile, `observe`. It has no network, read and write access to `~/.heron/observe/` only, and read access to the venv.
5.  **Policy** (the `observation` template, Pre-4):
    *   `text` columns are `aggregate_only`;
    *   `title`, `host` and `path` are `aggregate_only`, with `row_level_public = false`;
    *   `app`, `bundle_id`, `event` and `day` are `public`;
    *   `entity_column = "day"`;
    *   differential privacy is off, because every query is fixed Heron code.
6.  **Fixed jobs.** `src/observe/jobs.rs` (fenced) runs five SQL queries once a day, through `sealbox query`, with no model call:
    *   **Time per app per day.**
    *   **App sequences seen on 3 or more days** (`HAVING count(DISTINCT day) >= 3`).
    *   **Files touched**, grouped by folder.
    *   **Site visits** per host.
    *   **Topic minutes.**
        *   **Which terms.** The terms are the titles of the owner's open tasks (`Store::list_tasks`) and the owner's confirmed project memories. That is at most 50 terms, all trusted, because the owner wrote them.
        *   **What is counted.** For each term, the job counts the minutes in each app per day where `keys.text` or `screen.text` contains it.
        *   **What comes out.** Only counts of 5 minutes or more on 3 or more days.

    Each result row fills a fixed sentence template. It is saved with `memory.save(text, "fact", "observation", 0.4, "observation")`, whose arguments are content, kind, tags, importance and source (`src/memory.rs:247`).
7.  **Workflow to automation.** `suggestions` gains an `origin` column (`'proactive'` or `'observation'`), and `add_suggestion` gains `origin` and `session_id`.
    *   Each repeated sequence makes a tainted session holding its sentence, plus a suggestion with the fixed prompt "Create an automation for the routine in this conversation".
    *   Accepting continues that tainted session (`src/server.rs:1606-1615`), and `save_automation` asks for approval as usual.
8.  **Retention.** Every day, `sealbox prune --older-than 14` runs, and `observation` memories older than 90 days are deleted.
9.  **Forget everything observed.** It stops every observer and sealbox process. It then deletes:
    *   `~/.heron/observe/`;
    *   the key;
    *   every `observation` memory;
    *   every suggestion with `origin = 'observation'`, with its session.

    It reports what it deleted.
10. **Indicator.** While any observer is on, the menu bar icon has a dot. Its menu starts with **Pause observing**, for 1 hour or until resumed.

**Where sealbox falls short** (its README and source at `c863992`):

*   **It captures nothing.** All five observers are new Heron code.
*   **It only ingests whole datasets** (`Vault.ingest`, `src/sealbox/vault.py:268`). Pre-4 adds `append`, `query` (a CLI for a fixed SQL string), `prune`, `--key-fd`, a hashed lock file and the `observation` template.
*   **Its sandbox is Linux-only.** Heron's Seatbelt profile replaces it.
*   **It has no summariser inside the vault** (Overview, fact 5).
*   **Its k-anonymity and differential privacy assume many people.** Here, the "seen on 3 days" rule takes their place.
*   **It is alpha.** Its red-team results are "evidence, not proof", in its own words.
*   **It needs Python** (HQ17).
*   **It can't protect against a compromised Mac.**

### 9. Phase 2: install and rollback

These invariants hold. The step-by-step design is the Phase 2 design delta, which is still to be written.

*   **The old code judges the new.** The swapper is a copy of the old `heron-cli`, started detached (`setsid`).
*   **There is exactly one launchable Heron.app.** The previous app is kept as one zip in `~/.heron/selfheal/previous/`.
*   **No install while work is pending:** not during an active run, not while an approval waits, not while paused.
*   **The health check.** The new core must:
    *   write its port file within 90 seconds;
    *   answer `GET /api/state` with the expected `commit`;
    *   keep `risk_notice_accepted`;
    *   pass `heron-cli audit`;
    *   keep `PRAGMA user_version`;
    *   still be running 30 seconds after first answering.

    If any check fails, the swapper restores the zip, relaunches, and turns `auto_install` off.
*   **A crash mid-swap.** A `swap_intent` marker written before the swap lets the next start restore the zip.
*   **Undo last self-fix** unzips the previous app. It is unavailable if `user_version` differs.
*   **Return to main** builds `origin/main`, which the owner has merged and so reviewed, then swaps and runs the health check.
*   **Phase 2 also adds** the local G7 smoke test (§4C), R7, R9, and the per-task `origin_tainted` scope.

### 10. Changes to fenced files, and the definition of done

Approving this doc approves every change in this table. Each file is fenced, so a later self-change can't touch it without review.

| File | Change | Phase and item |
|---|---|---|
| `src/lib.rs` | `pub mod harness2; pub mod ext; pub mod observe;` | Pre-6 |
| `src/config.rs` | `[harness2]` and `[observe]` ([§11](#11-contracts)) | 1a, [14](#14-srcconfigrs-srcmainrs-modify) |
| `src/main.rs` | The `selfheal` and `report` subcommands, and `record_panic` in the panic hook | 1a, [14](#14-srcconfigrs-srcmainrs-modify) |
| `src/server.rs` | `commit` in `/api/state`, the `/api/harness2/*` and `/api/observe/*` routes, and failure records on 500 errors | 1a, 1c, [15](#15-srcserverrs-modify), [19](#19-srcobserve-create-srcsandboxrs-srcserverrs-modify) |
| `src/app.rs` | The tool filter, the `Harness2::tick` call, and the tainted-session counter | 1a, [16](#16-srcapprs-srcagentrs-srcstorers-modify) |
| `src/agent.rs` | `PromptSource::Repair`, and the four `ContentEntry` variants | Pre-6, [16](#16-srcapprs-srcagentrs-srcstorers-modify) |
| `src/store.rs` | `inbox.action` and `suggestions.origin`, as one migration with a version bump | 1a, [16](#16-srcapprs-srcagentrs-srcstorers-modify) |
| `src/permissions.rs` | `DenyAll` usable outside tests | 1a, [16](#16-srcapprs-srcagentrs-srcstorers-modify) |
| `src/memory.rs` | `is_outside()` | 1b, [17](#17-wiring-for-connectors) |
| `src/connectors.rs`, `src/sources.rs` | `require_custom` accepts manifest IDs; `ext:<name>` sources | 1b, [17](#17-wiring-for-connectors) |
| `src/sandbox.rs` | The `observe` profile | 1c, [19](#19-srcobserve-create-srcsandboxrs-srcserverrs-modify) |
| `src/ext/**`, `src/harness2/**`, `src/observe/**` | New | 1a, 1b and 1c |
| `tests/connector_manifests.rs` | New | 1b, [18](#18-tests-for-the-wiring) |
| `.github/workflows/selfheal.yml` | New | 1a, [26](#26-githubworkflowsselfhealyml-create) |
| `packaging/macos/build-app.sh` | `HERON_COMMIT`, `--no-dmg`, `--host-arch`, the hardened-runtime default, and copying `connectors/` | 1a, [22](#22-packagingmacosbuild-appsh-modify) |
| `eval/selfheal/**`, `scripts/verify-selfheal.sh` | New | 1a, [20](#20-evalselfheal-create), [21](#21-scriptsverify-selfhealsh-create) |
| `macos/HeronMac/**` | The panes, observers, inbox buttons, and `CoreClientProtocol` additions | 1a, 1c, [23](#23-mac-app-self-improvement), [24](#24-mac-app-observation) |
| `docs/engineering/SECURITY_THREAT_MODEL.md` | Two sections | [25](#25-docs) |

The v1 features doc's eight checks ([heron-v1-features.md](heron-v1-features.md#the-extended-definition-of-done)) apply as follows:

| Check | Harness2 |
|---|---|
| 1. New tools | `ext_<name>_<op>`: `ExternalWrite` until the connector is trusted, then `NetworkEgress` or `ExternalWrite`, always with untrusted output. The repair profile reuses existing tools |
| 2. Decision-table rows | `decision_table_selfheal`, `decision_table_repair_profile` and `decision_table_ext_tools` |
| 3. Prompt blocks | `PromptSource::Repair` is Trusted. Scout items, connector output and observation sentences are Outside |
| 4. EVE | A preview report writes a file and sends nothing. Turning on App use shows consent, then the indicator, then Pause |
| 5. Content entries | `OwnerReport` and `FailureRecord` are trusted. `ConnectorOutput` and `ObservationResult` taint |
| 6. Approval surfaces | App only: the Self-improvement and Observation panes, the consent screens, Report a problem (including Request a connector), Import report, the inbox buttons, and the connector card's key field and Trust switch. The phone can only pause observers |
| 7. Processes outside the `shell` tool | Listed below |
| 8. TCC | Screen Recording, Accessibility, Input Monitoring and Full Disk Access, each requested when its switch is turned on. Phase 2 needs HQ5 so that the grants survive |

Every process has a fixed path, fixed argument shapes, and `PATH`, `HOME` and `TMPDIR`, plus what is stated here:

*   `git`: worktree, commit, push and ls-remote, with the `-c` options from §4.
*   `gh`: pr create, view, checks, list, comment and close, with the publishing token.
*   `/usr/bin/open -a <editor>`, for **Review**.
*   For observation:
    *   `uv venv` and `uv pip install --require-hashes`;
    *   the sealbox `append`, `query` and `prune` processes, under the `observe` profile, with the key on a pipe.
*   In Phase 2: `build-app.sh`, `codesign`, `ditto`, `/usr/bin/open -a /Applications/Heron.app`, the swapper, and the G7 smoke test's `cargo` and `run-core-tasks.sh`.

### 11. Contracts

**Config**

*   **`[harness2]`:**
    *   `self_healing = false`
    *   `self_evolution = false`
    *   `scout_every_hours = 168`
    *   `repo = ""`
    *   `publish_report_text = false`, the default for the "OK to publish" tick
    *   `max_attempts = 3`
    *   `max_steps = 40`
    *   `max_tokens = 500000`
    *   `runs_per_day = 3`
    *   in Phase 2 builds only: `auto_install = true`
*   **`[observe]`:**
    *   `screen = false`
    *   `apps = false`
    *   `browser = false`
    *   `keys = false`
    *   `files = false`
    *   `workflow = false`
    *   `raw_days = 14`
    *   `derived_days = 90`
    *   `exclude_apps`, `exclude_hosts` and `exclude_folders`, with the HQ20 defaults

**Enums**

*   **`triggers.kind`:** `owner_report` or `failure_record`.
*   **`triggers.provenance`:** `typed`, `pasted`, `from_note` or `none`.
*   **`triggers.state`:** `queued`, `held`, `running`, `retrying`, `waiting_limit`, `setup_failed`, `interrupted`, `cancelled`, `failed`, `awaiting_review`, `pr_draft`, `pr_opened` or `installed`.
*   **`runs.verdict`:** `auto_eligible` or `needs_owner`.
*   **`runs.pr_state`:** `draft`, `open`, `merged` or `closed`.

**Routes.** Every route needs the bearer token. Errors use the existing `{"error", "message"}` shape.

| Route | Body | Returns |
|---|---|---|
| `GET /api/harness2` | — | `{self_healing, self_evolution, scout_every_hours, builder, checklist: [{item, ok, fix}], runs: [{id, state, verdict, pr_url}]}` |
| `POST /api/harness2` | any of the three settings | as `GET` |
| `POST /api/harness2/report` | `{text, provenance, attach_session?, publish_text, connector?: {service, ops: [string], notes}}` | `{trigger_id}`, or `409 tainted_session` |
| `POST /api/harness2/import` | `{path}` | `{inbox_id}` |
| `POST /api/harness2/triggers/{id}/start` | — | `{trigger_id, state}` |
| `POST /api/harness2/runs/{id}/push` | — | `{pr_url}` |
| `GET /api/observe` | — | `{switches, consent, sealbox: missing \| installed \| running, counts}` |
| `POST /api/observe` | any switch, or the exclusion lists | as `GET` |
| `POST /api/observe/consent` | `{switch, version}` | `{ok}` |
| `POST /api/observe/install` | — | `{sealbox}` |
| `POST /api/observe/ingest` | `{dataset, rows}`: at most 500 rows or 256 KB, with `dataset` one of `screen`, `apps`, `web`, `keys` or `files` | `{accepted}`, or `409 paused` |
| `POST /api/observe/pause` | `{minutes?}` | `{paused_until}` |
| `POST /api/observe/forget` | — | `{deleted: {vault, memories, suggestions}}` |

**`gate-report.json`**, written by `selfheal.yml` as an artifact:

```json
{"base": "<40 hex>", "fix": "<40 hex>", "r2": "passes", "mutation": {"score": 84, "passed": true},
 "containment": "passes", "changed": ["src/schedule.rs", "tests/schedule_local_time.rs"]}
```

**`.heronreport`:**

```json
{"schema": 1, "version": "1.0.0", "commit": "<hex>", "text": "<user text>", "provenance": "typed",
 "failures": [{"kind": "panic", "location": "src/x.rs:12", "count": 2}], "conversation": null}
```

### 12. Preview Macs

*   **Self-healing.** Instead of the switch, a preview Mac shows "Self-healing runs on the Mac that builds Heron. Use Help › Report a problem." Fixes arrive in the next DMG.
*   **Self-evolution.** Hidden.
*   **Observation.** Available, under the user's own control and consent screens. Nothing observed leaves the user's Mac, except fixed-job sentences inside prompts to their own provider.
*   **Failure records.** Written, so that reports can carry them, but they start nothing.

--------------------------------------------------------------------------------

## Alternatives Considered

### Alternative A: A headless coding CLI with its own tools does the fix

*   **Pros:** an agent that already works on this repository (skein's `claude_build` built v1).
*   **Cons:**
    *   It breaks CLI containment invariant 3 (`src/cli_llm.rs:40-43`).
    *   It adds a second permission system that Heron's audit and taint don't see.
*   **Reason for rejection:** it gives up v1's containment for a gap the replay suite measures. If Heron scores below HQ10, this alternative comes back to the owner.

### Alternative B: Run the gate on the Mac, under Seatbelt or unsandboxed

*   **Pros:** no CI queue.
*   **Cons:**
    *   Under Seatbelt, about 27 tests fail and `swift build` fails.
    *   Unsandboxed, model-written tests could reach the owner's credentials and `~/.heron`.
*   **Reason for rejection:** `ci.yml` already runs the same checks on macOS, with no secrets.

### Alternative C: A separate macOS user, or a VM, for the gate

*   **Pros:** no CI queue, and isolation.
*   **Cons:** setting up and maintaining a second toolchain and CLI logins.
*   **Reason for rejection:** CI gives the same isolation with no setup. This is the fallback if macOS runners prove unreliable.

### Alternative D: Proposal-only, permanently

*   **Pros:** the fewest parts.
*   **Cons:** no autonomy, ever.
*   **Reason for rejection:** OD1. Phase 1 is proposal-only in practice, and Phase 2 adds autonomy once it is measured.

### Alternative E: `verify-v1.sh` before every PR

*   **Pros:** a self-fix PR would mean what a release means.
*   **Cons:** 66 or more G7 runs, a 14-day trial, and G7 results discarded on any `src` change.
*   **Reason for rejection:** it can't finish in a day. The PR into `main` carries it.

### Alternative F: Run Google's Harness² or APE's propose-probe-compose loop

*   **Pros:** published gains, and APE is the owner's own.
*   **Cons:** it tunes per task, its executors bypass safety, it needs about 4,000 rollouts per task, and 11 tasks overfit.
*   **Reason for rejection:** it needs a suite of 50 or more tasks. Three of its ideas are kept:
    *   retry feedback (§2);
    *   treating outside lessons as untrusted (§1, from APE's `lessons.js`);
    *   validating the whole change (§3).

### Alternative G: A pipeline that turns untrusted triggers into local diffs

*   **Pros:** ideas and reports would become changes directly.
*   **Cons:** it needs read-only sessions, applying diffs, a diff view, and four routes, all for rare triggers.
*   **Reason for rejection:** "Restate as report" with provenance does the same job.

### Alternative H: Port anchor-harness's `effects.rs` for publishing

*   **Pros:** one journal of the publishing steps.
*   **Cons:** it duplicates what `git ls-remote` and `gh pr list` already answer.
*   **Reason for rejection:** publishing checks git and GitHub first. Interrupted repairs use Pre-5's journal.

### Alternative I: Model-written Rust connectors

*   **Pros:** any API shape.
*   **Cons:** a token scan isn't a sandbox (`include!`, string paths in derive attributes, CPU-bound loops), and the code runs inside the core.
*   **Reason for rejection:** a manifest has no code. Anything a manifest can't express is PR-only.

### Alternative J: A connector as its own process, behind a broker

*   **Pros:** any API shape, behind a process boundary.
*   **Cons:** a broker protocol, a profile per connector, and still model-written code.
*   **Reason for rejection:** manifests cover common REST APIs with no code.

### Alternative K: Keep raw observations in `heron.db`

*   **Pros:** no Python.
*   **Cons:** raw keystrokes would end up in the store, its backups and its snapshots.
*   **Reason for rejection:** OD3 places them in sealbox.

### Alternative L: Summarise raw text with the provider

*   **Pros:** summaries in words.
*   **Cons:** raw keystrokes would go to the provider.
*   **Reason for rejection:** it breaks the chosen containment.

### Alternative M: No keystroke capture

*   **Pros:** no password or private-message capture risk.
*   **Cons:** less context.
*   **Reason for rejection:** OD3. HQ23 still lets the owner defer the switch.

### Alternative N: A Rust-side encrypted store instead of sealbox

*   **Pros:** no Python and no venv. One process.
*   **Cons:**
    *   It needs a new cipher crate (`Cargo.toml` has none).
    *   Heron would have to write its own detectors, egress checks and audit chain.
*   **Reason for rejection:** OD3 names sealbox, and sealbox's gate and detectors are already tested. The trade is a Python runtime (HQ17).

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Prompt injection becomes persistent code | Untrusted inputs never start a run. Pasted and restated text is marked and never auto-eligible. Scout links come from the fetched data |
| The owner launders outside text by pasting it | Paste detection and `from_note` provenance. An owner who retypes outside text gets a `typed` report, a remaining risk recorded in the threat model |
| A self-fix weakens a security check | A default-deny fence in the installed binary, the gate taken from `main`, and fenced trust tables |
| Model-written code runs on the Mac | Phase 1: only the repair agent's sandboxed `cargo test` runs. Phase 2: the smoke test and the install (HQ2) |
| CI secrets are exposed | A token without `workflow` scope, no tag pushes, protected release secrets, and row 0b (HQ24) |
| A secret or private text appears in a public PR | R5, the private-text check, report text off by default, and commit messages written from the diff |
| A broken build replaces the app (Phase 2) | The old binary health-checks the new one and reverts |
| A self-fix crash triggers another self-fix | A failure record from a self-fix build starts nothing |
| Privacy grants reset | Stable signing with the hardened runtime (HQ5, PT2) |
| A connector exfiltrates data or reads secrets | No code. The executor checks the host, follows no redirects, uses `PublicOnlyResolver`, and adds the auth header itself. Untrusted until the owner trusts it |
| The vault is read | Encryption, the key in the keychain and passed by pipe, Seatbelt. A compromised Mac can still read it |
| The sealbox supply chain is attacked | A pinned commit, a hashed lock file, and a venv hash check |
| Passwords or codes are captured | The Secure Input and `AXSecureTextField` skips, detectors, exclusions, and 14 days of retention |
| Other people's content is captured | Stated on the consent screen. Kept 14 days, and only aggregates come out |
| The disk fills up | Worktrees are pruned after 7 days |
| The repair agent is too weak | The replay suite gates the switch |
| The fence checker or replay grader is wrong | genesis audits both, and testteeth grades the code |
| macOS runners are unavailable | Runs wait up to 24 hours. Alternative C is the fallback |

--------------------------------------------------------------------------------

## Non-Functional Requirements

*   **Security:** see Risks and §1 to §8.
*   **Time:** a trusted run takes 1 to 2 hours, mostly in the CI queue (PT4).
*   **Storage:**
    *   3 to 5 GB per worktree, pruned after 7 days;
    *   the vault grows by an estimated 50 to 200 MB a week, pruned to 14 days.
*   **Rollout:** every switch ships off. Self-healing waits for the replay suite to pass, and Phase 2 has its own criteria.
*   **Rolling back Phase 1:**
    *   switching it off stops everything;
    *   the migration only adds nullable columns, so the previous build still reads the data;
    *   reverting the merge removes the feature;
    *   `selfheal.db` and `~/.heron/observe/` can then be deleted.

--------------------------------------------------------------------------------

### Additional Resources

*   [heron-v1.md](heron-v1.md) and [heron-v1-features.md](heron-v1-features.md).
*   **anchor-harness:** `crates/harness-core/src/versions.rs`, `gate.rs` and `effects.rs`, and `docs/HARNESS2.md`.
*   **APE:** `lessons.js` and `probe.js`.
*   **sealbox at `c863992`:** `README.md`, and `src/sealbox/vault.py`, `sandbox.py`, `policy.py` and `gate.py`.
*   **Goldfish review reports:** [reviews/heron-harness2/](reviews/heron-harness2/).
*   **The research of 2026-11-22:** three read-only reports, kept as session scratch files. Their findings are restated where they are used.

### Future Improvements

*   **Split the app's views from its stores**, so that UI fixes can become auto-eligible.
*   **An offline harness optimizer.** It needs a suite of 50 or more tasks. Its promotion rule would be anchor-harness's `gate.rs`, which compares a candidate against an unchanged baseline and a best-of-N baseline, with confidence intervals.
*   **The completion check and recoverable tool output.** The review session's design doesn't cover these.
*   **A local model inside the vault**, for summaries in words.
*   **Agent queries on the vault**, through sealbox's `serve` with a tool allowlist, behind an owner switch.
*   **Heron offering connectors on its own**, through an `offer_connector` tool limited to untainted sessions.
*   **A connector broker**, if manifests prove too limited (Alternative J).
*   **An updater for preview users.**

--------------------------------------------------------------------------------

## Detailed Implementation Plan

### Preconditions

*   **Pre-1.** The v1 features base (B1 to B6) is merged into `main`.
*   **Pre-2.** The build session runs these prototypes by hand, outside any sandbox. It records each result in `docs/designs/research/harness2-prototypes.md` as `PT<n>: pass|fail: <evidence>`.
    *   **PT1.** Count the `cargo test --locked` failures inside Heron's Seatbelt shell. About 27 is expected.
    *   **PT2 (Phase 2).** Check whether TCC grants and keychain items survive a switch from the release DMG to a self-fix build and back, and also after an ad-hoc build.
    *   **PT3 (Phase 2).** Check whether a detached process outlives the app and the core, and can rename `/Applications/Heron.app` without an admin prompt.
    *   **PT4.** Time a one-line Rust fix and a one-line Swift fix end to end, including the CI queue.
    *   **PT5.** Check whether the four observation prompts appear with no `Info.plist` usage strings. Apple documents none for these permissions, so none is added unless PT5 needs one.
    *   **PT6.** Check that the HQ24 token can't push a change to `.github/workflows/`.
*   **Pre-3.** The owner has answered HQ1 to HQ5, HQ12, HQ16 to HQ18 and HQ22 to HQ24, or accepted their defaults.
*   **Pre-4 (Phase 1c only).** The sealbox additions are merged in sealbox's own repository, and their commit is pinned:
    *   `append <dataset> --kind events` (JSON lines from stdin, written as encrypted hourly segments);
    *   `query <sql>` (one fixed SQL string through the gate, printing JSON);
    *   `prune --older-than <days>`;
    *   `--key-fd <n>` on every command;
    *   a hashed `requirements.lock`;
    *   the `observation` policy template.

    Its check is `python -m pytest -q && python eval/redteam.py`, with 0 leaks.
*   **Pre-5.** The review session's harness parts are merged, or their interfaces are fixed in that design (the effect journal's ask-again hook, and the toolchain hash check). If the names change, this doc is updated in the same change.
*   **Pre-6.** The build session commits a skeleton before `skein run`. The skeleton holds:
    *   `pub mod harness2; pub mod ext; pub mod observe;`, with empty modules;
    *   `PromptSource::Repair`;
    *   the four `ContentEntry` variants as stubs;
    *   the `App::assemble` tool-filter parameter;
    *   an empty `connectors/` folder with a `README.md`.

    Its check is `cargo build --locked && cargo test --locked --no-run`.

### Target Files to Create/Modify

#### Phase 1a, Unit H1: `src/harness2/`

##### 1. `src/harness2/mod.rs` (Create)

*   `Harness2 { db, cfg }`
*   `tick(&self, app)`, with the five steps in §6
*   `queue(t)`
*   `record_panic(location)`

##### 2. `src/harness2/trust.rs` (Create)

*   `TriggerKind`, `Provenance` and `FailureRecord`
*   `outcome(...)`, which implements the rows of §1 exactly
*   `failure_starts_trigger(rec, installed_is_selffix)`

##### 3. `src/harness2/fence.rs` (Create)

*   `AUTO` as a `globset`
*   `check(repo, base, fix) -> FenceVerdict { AutoEligible, NeedsOwner(Vec<Reason>) }`, applying R1, R3, R4, R6 and R8
*   `push_rule(paths)`, for row 0b

##### 4. `src/harness2/repair.rs` (Create)

*   `REPAIR_POLICY`, `REPAIR_TOOLS` and `REPAIR_PROMPT`
*   `prepare(run, installed_commit)`
*   `attempt(run, feedback)`

##### 5. `src/harness2/checks.rs` (Create)

*   `local_checks(run)`: the fence, R5, the private-text check (8-word shingles of the report and its attachment matched against the added lines), and `push_rule`
*   `git(args)`, which adds the `-c` options and checks the `.git` pointer
*   `poll_ci(run)`, through `gh pr checks`

##### 6. `src/harness2/publish.rs` (Create)

*   `commit(run)`
*   `push_and_draft(run)`, which checks git and GitHub state before each step
*   `finish(run)`, which labels the PR and marks it ready, or closes it with its failure report
*   `poll_prs()`

##### 7. `src/harness2/scout.rs` (Create)

*   `HOSTS`
*   `client()`, which uses `PublicOnlyResolver`, `no_proxy`, and an allowlist redirect policy (at most 5 hops)
*   one fetcher per source
*   `parse_feed`, `rank` and `post`

##### 8. `src/harness2/report.rs` (Create)

*   `write_report`, which writes JSON, refuses a tainted session, and never includes observations
*   `import_report`, which adds an inbox note with the `restate` action

##### 9. `src/harness2/db.rs` (Create)

*   `selfheal.db`, with the tables in §6 and `PRAGMA user_version = 1`

##### 10. Tests (inline)

*   **`trust.rs`:**
    *   `decision_table_selfheal`
    *   `pasted_report_is_never_auto`
    *   `from_note_report_is_never_auto`
    *   `failure_record_has_no_free_text`
    *   `origin_tainted_failure_is_never_auto`
    *   `selffix_build_failure_starts_no_trigger`
    *   `interrupted_run_asks_before_rerun`
    *   `self_healing_refused_without_replay_pass`
*   **`fence.rs`:**
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
    *   `overlapping_open_pr_needs_owner`
    *   `workflow_diff_is_not_pushed`
*   **`checks.rs`:**
    *   `redaction_hit_never_pushes`
    *   `private_text_blocks_push`
    *   `git_calls_disable_hooks_and_fsmonitor`
    *   `moved_git_pointer_stops_git`
*   **`repair.rs`:**
    *   `decision_table_repair_profile`
    *   `repair_registry_has_only_seven_tools`
    *   `compound_shell_is_denied_in_repair`
    *   `unmatched_command_is_denied_in_repair`
*   **`publish.rs`:**
    *   `publish_skips_existing_branch_and_pr`
    *   `pr_body_omits_report_text_by_default`
    *   `ci_failure_three_times_closes_draft`
    *   `ci_not_run_in_24_hours_closes_draft`
*   **`mod.rs`:**
    *   `tick_steps_follow_their_gating`
*   **`scout.rs`:**
    *   `scout_drops_unknown_item_ids`
    *   `scout_note_link_comes_from_fetch`
    *   `scout_rejects_unlisted_host`
    *   `scout_rejects_redirect_off_allowlist`
    *   `feed_reader_skips_malformed_entries`
    *   `scout_writes_only_inbox_notes`
*   **`report.rs`:**
    *   `report_excludes_conversation_unless_ticked`
    *   `report_never_includes_observations`
    *   `imported_report_is_inbox_only`

#### Phase 1a, Unit H2: wiring

##### 14. `src/config.rs`, `src/main.rs` (Modify)

*   **`config.rs`:** the `[harness2]` table (§11).
*   **`main.rs`:**
    *   the subcommands `selfheal repair --run`, `selfheal check-fence --repo <path> <base> <fix>`, `selfheal record-replay <commit> <passed> <total> <auto_eligible>`, and `report`;
    *   the panic hook calls `harness2::record_panic(location)` before the previous hook.

##### 15. `src/server.rs` (Modify)

*   `commit` in `GET /api/state`
*   every `/api/harness2/*` route in §11
*   failure records on `ApiError` 500

##### 16. `src/app.rs`, `src/agent.rs`, `src/store.rs` (Modify)

*   **`app.rs`:** the tool filter, `Harness2::tick` in the scheduler loop, and the tainted-session counter.
*   **`agent.rs`:** the `OwnerReport` and `FailureRecord` content entries.
*   **`store.rs`:** one migration that adds `inbox.action` and `suggestions.origin`, plus the new `add_inbox` and `add_suggestion` arguments.
*   **`permissions.rs`:** `DenyAll` usable outside tests.

##### 18. Tests for the wiring

*   **Inline:**
    *   `state_reports_commit`
    *   `harness2_routes_need_token`
    *   `record_panic_writes_failure_record`
    *   `inbox_action_starts_trigger`
    *   `every_prompt_block_is_trusted_or_taints`
    *   `every_content_entry_is_trusted_or_taints`
    *   `migration_adds_only_nullable_columns`
    *   `report_with_tainted_session_is_refused`
*   **Unit H2t (`tests/**`):** in `tests/integration.rs`, `api_harness2_lists_switches_and_checklist`, with the fixture `tests/fixtures/api/harness2.json`.

#### Phase 1a, Unit H3: replay suite

##### 20. `eval/selfheal/` (Create)

*   **`cases.md`:** 10 historical fix commits, each with `base`, a report in the owner's words, and `hidden_tests`.
*   **`run_replay.py`:** for each case it:
    1.  checks out `base`;
    2.  runs `heron-cli selfheal repair`;
    3.  copies in the fix's tests;
    4.  passes the case when those tests and `cargo test --locked` pass.

    It prints `replay: N/10` and `auto-eligible: M`, calls `record-replay`, and exits 0 when the score meets HQ10.
*   **`genesis.yaml`:** for the grader. An empty patch, or a patch that deletes the hidden test, must fail.
*   **`required-tests.txt`:** every Rust test name in items 10, 11, 17 and 19.
*   **`eval/tests/test_selfheal_replay.py`:** tests for the grader.

#### Phase 1a, Unit H4: gate script

##### 21. `scripts/verify-selfheal.sh` (Create)

*   It runs R2, mutation and containment (§4B), chosen from `git diff --name-only <base> <fix>`.
*   Its arguments are validated: two 40-hex commits and an existing directory.
*   It writes `gate-report.json`.

#### Phase 1a, Unit H12: CI workflow

##### 26. `.github/workflows/selfheal.yml` (Create)

*   **Trigger:** `on: pull_request` for `heron/selfheal/**`, with `permissions: contents: read`.
*   **Runner:** `macos-latest`.
*   **Steps:**
    1.  check out `main` into `gate/`;
    2.  check out the head into `fix/`;
    3.  `pip install testteeth==<pinned>`;
    4.  run `bash gate/scripts/verify-selfheal.sh`;
    5.  upload `gate-report.json`.
*   **Secrets:** none.

#### Phase 1a, Unit H5: packaging

##### 22. `packaging/macos/build-app.sh` (Modify)

*   Set `HERON_COMMIT=$(git rev-parse HEAD)` for both cargo builds.
*   Add `--no-dmg` and `--host-arch`.
*   Default mode: sign `heron-cli`, then the app, with `codesign --force --options runtime --sign "${CODESIGN_IDENTITY:--}"`, with no `--deep`.
*   Copy `connectors/*.toml` into `Contents/Resources/connectors/`.

#### Phase 1a, Unit H6: Mac app, self-improvement

##### 23. Mac app, self-improvement

*   **New:**
    *   `macos/HeronMac/Sources/HeronUI/SelfImprovement/`.
    *   `CoreClientProtocol` gains the `/api/harness2/*` calls, implemented in `CoreClient` and `PreviewCoreClient`. There is no second client.
*   **Changed:**
    *   `Models.swift`;
    *   `SettingsView.swift`;
    *   the inbox view in `ScreenViews.swift`;
    *   `HeronApp.swift`, for the menus.
*   **The pane:** the switches, the checklist, runs with PR links, "Check every" (24 hours, 72 hours or Week), and the preview-Mac line.
*   **The report sheet:**
    *   paste detection sets `pasted`;
    *   the "OK to publish" tick;
    *   the "Request a connector" mode, with service, operations and notes fields;
    *   **Restate as report**, which sets `from_note`.
*   **Inbox buttons** for each action in §6.
*   **The publishing-token field:** a secure field with a link to GitHub's fine-grained token page.
*   **Tests:**
    *   `test_selfHealingSwitchDisabledUntilChecklistPasses`
    *   `test_previewMacShowsReportLineInsteadOfSwitch`
    *   `test_scoutScheduleOffersThreeChoices`
    *   `test_reportSheetDefaultsToNoConversation`
    *   `test_reportSheetDefaultsToNotPublished`
    *   `test_pasteMarksReportPasted`
    *   `test_restateMarksReportFromNote`
    *   `test_inboxShowsActionButtons`
    *   `test_connectorRequestModeSendsFields`

#### Phase 1b, Unit H8: connector executor

##### 11. `src/ext/mod.rs`, `manifest.rs`, `exec.rs` (Create)

*   **`manifest.rs`:** the schema in §7 and `validate_manifest`. The host rules move here, and `connectors::validate` calls them.
*   **`exec.rs`:**
    *   `build_request(manifest, op, args)`, which is pure;
    *   `call(...)`, with a `PublicOnlyResolver` and `no_proxy` client, no redirects, the host check, the 1 MB cap, the 20-second timeout, the header from the keychain, and the `select` filter;
    *   `load_all(dir)`.
*   **Tests:**
    *   `manifest_rejects_ip_localhost_and_port`
    *   `read_op_must_be_get`
    *   `path_params_are_percent_encoded`
    *   `unknown_param_is_refused`
    *   `redirect_is_not_followed`
    *   `auth_header_only_to_manifest_hosts`
    *   `response_over_1mb_is_refused`
    *   `select_keeps_only_listed_pointers`

##### 17. Wiring for connectors

*   **Files changed:** `src/connectors.rs`, `src/sources.rs`, `src/memory.rs` and `src/agent.rs` (Modify).
*   **Changes:**
    *   tools registered per §7;
    *   `require_custom` accepts manifest IDs;
    *   the `ext:<name>` sources;
    *   `is_outside()`;
    *   the `ConnectorOutput` content entry;
    *   the kv trust key `ext_trust`.
*   **Tests:**
    *   `decision_table_ext_tools`
    *   `ext_output_taints`
    *   `ext_untrusted_until_owner_trusts`
    *   `ext_trust_resets_on_manifest_change`
    *   `manifest_connector_accepts_api_key`
    *   `ext_source_memories_start_unconfirmed`
    *   `is_outside_replaces_both_checks`
*   **Unit H8t (`tests/**`):** `tests/connector_manifests.rs`.
*   **App (Unit H9, `macos/**`):** the connector card's Trust switch, with the test `test_connectorCardShowsTrustSwitch`.

#### Phase 1c, Unit H10: observation core

##### 19. `src/observe/` (Create), `src/sandbox.rs`, `src/server.rs` (Modify)

*   **`sealbox.rs`:**
    *   `SEALBOX_COMMIT` and `SEALBOX_LOCK_SHA256`;
    *   `install()`, `verify_venv()` and `master_key()` (from the keychain);
    *   `spawn_append()`, `run_query(sql)` and `spawn_prune()`, each under the `observe` profile with `--key-fd 3`.
*   **`jobs.rs`:** the five queries, the sentence templates, and the saves.
*   **`forget.rs`.**
*   **Config:** the `[observe]` table.
*   **Routes:** every `/api/observe/*` route.
*   **Memory:** the `observation` source, and the `ObservationResult` content entry.
*   **Tests:**
    *   `ingest_keeps_nothing_outside_vault`
    *   `ingest_refused_while_paused`
    *   `observe_profile_denies_network`
    *   `master_key_passed_by_fd_not_env`
    *   `config_never_contains_sealbox_key`
    *   `venv_hash_mismatch_refuses_spawn`
    *   `workflow_job_requires_three_days`
    *   `topic_job_uses_only_owner_terms`
    *   `jobs_make_no_model_call`
    *   `forget_removes_vault_key_memories_and_suggestions`
    *   `workflow_suggestion_session_is_tainted`
    *   `observation_source_is_outside`

#### Phase 1c, Unit H11: Mac app, observation

##### 24. Mac app, observation

*   **New:** `HeronCore/Observers/` and `HeronUI/Observation/`. `CoreClientProtocol` gains the `/api/observe/*` calls.
*   **Changed:** `MenuBarViews.swift`.
*   **Observers:**
    *   `ScreenObserver`
    *   `AppUseObserver`
    *   `BrowserHistoryObserver`
    *   `KeystrokeObserver`
    *   `FileEventsObserver`
    *   `Exclusions`
    *   `ObservationUploader`
*   **Views:**
    *   `ObservationSettingsView`
    *   `ConsentView`
    *   `ExclusionsView`
    *   the menu bar dot and **Pause observing**
*   **Tests:**
    *   `test_everyObserverStartsOff`
    *   `test_consentShownBeforeFirstEnable`
    *   `test_keystrokesSkippedDuringSecureInput`
    *   `test_keystrokesSkippedInSecureTextField`
    *   `test_excludedAppIsNotRecorded`
    *   `test_indicatorShownWhileAnyObserverOn`
    *   `test_pauseStopsAllObservers`

#### Units H7a and H7c: docs

##### 25. Docs

*   **`docs/engineering/SECURITY_THREAT_MODEL.md`:** two new sections:
    *   `## Harness2: code Heron writes into itself` (H7a). It covers the trust table, provenance and its limit, the fence, CI as the gate, the HQ2 residual, connectors, and the process list.
    *   `## Observation` (H7c). It covers the observers, sealbox, what leaves the vault, and where sealbox falls short.
*   **Guides:** `docs/features/self_improvement.md` (H7a) and `docs/features/observation.md` (H7c).

### Verification Plan

*   **Build:** `cargo build --locked`, `swift build --package-path macos/HeronMac`, and `bash packaging/macos/build-app.sh --unsigned`.
*   **Tests:**
    *   `cargo test --locked`, `cargo test --locked --test integration`, `cargo test --locked --test connector_manifests`, and `swift test --package-path macos/HeronMac`;
    *   each graph's `verify` node runs `require-tests.sh` over its own names in `eval/selfheal/required-tests.txt`.
*   **Lint:** `cargo clippy --locked --all-targets -- -D warnings`.
*   **Test quality, with testteeth:** `testteeth run --base main --fail-under 70 -q`. Surviving mutants in `fence.rs`, `trust.rs`, `checks.rs`, `src/ext/` and `src/observe/` are killed, not justified.
*   **Evaluator audits, with genesis.** Each must come back SOUND:
    *   `genesis audit-evaluator eval/selfheal/genesis.yaml`, for the replay grader;
    *   a genesis claim for the fence checker, with item 10's fence tests as its cases;
    *   sealbox's `eval/redteam.py` at the pinned commit, with 0 leaks.
*   **Replay:** `run_replay.py` meets HQ10 on the owner's Mac.
*   **End to end, once, by hand:**
    *   CUJ H-1 through real CI, with a planted bug in `schedule.rs`;
    *   CUJ H-6 with request-shape pairs;
    *   CUJ H-7 with App use, then **Forget everything observed**;
    *   PT6.

## Work units

Phase 1 is three skein graphs, each on its own feature branch cut from `main` after Pre-1 and Pre-6:

*   **1a** also needs Pre-2.
*   **1b** can start once 1a's H2 is merged.
*   **1c** needs Pre-4, and can start once 1a's H2 is merged.

In each graph, units whose scopes share a prefix are ordered by depends-on. In the check column, `\|` stands for a shell `|`.

**Graph 1a: self-healing to PR, and the scout**

| id | what to build | scope | depends on | check | model role |
|---|---|---|---|---|---|
| H1 | `src/harness2/` (items 1-10) | `src/harness2/**` | none | `cargo test --locked --lib harness2 && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib -- decision_table_selfheal pasted_report_is_never_auto failure_record_has_no_free_text origin_tainted_failure_is_never_auto fence_denies_views_and_stores events_and_embed_are_fenced fence_denies_harness2_and_gate_script workflow_diff_is_not_pushed redaction_hit_never_pushes private_text_blocks_push unmatched_command_is_denied_in_repair scout_rejects_redirect_off_allowlist report_never_includes_observations` | hardest tasks: opus high |
| H2 | Wiring (items 14-16, inline tests of 18) | `src/**` | H1 | `cargo test --locked --lib --bins && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib -- state_reports_commit harness2_routes_need_token record_panic_writes_failure_record inbox_action_starts_trigger every_prompt_block_is_trusted_or_taints every_content_entry_is_trusted_or_taints migration_adds_only_nullable_columns report_with_tainted_session_is_refused` | hardest tasks: opus high |
| H2t | Integration test and fixture (item 18) | `tests/**` | H2 | `cargo test --locked --test integration && bash eval/require-tests.sh --test integration -- api_harness2_lists_switches_and_checklist && test -s tests/fixtures/api/harness2.json` | feature, refactoring: sonnet high |
| H3 | Replay suite, grader, genesis evaluator, required-tests list (item 20) | `eval/**` | none | `python3 -I -m unittest discover -s eval/tests -t eval -p test_selfheal_replay.py && test "$(grep -c '^## case' eval/selfheal/cases.md)" -eq 10 && test -s eval/selfheal/required-tests.txt && genesis audit-evaluator eval/selfheal/genesis.yaml` | feature, refactoring: sonnet high |
| H4 | `scripts/verify-selfheal.sh` (item 21) | `scripts/**` | none | `bash -n scripts/verify-selfheal.sh && grep -q 'testteeth run --base' scripts/verify-selfheal.sh && grep -q 'eval/containment/check.py' scripts/verify-selfheal.sh && grep -q -- '--no-run' scripts/verify-selfheal.sh && grep -q 'gate-report.json' scripts/verify-selfheal.sh` | feature, refactoring: sonnet high |
| H12 | `.github/workflows/selfheal.yml` (item 26) | `.github/**` | H4 | `grep -q 'heron/selfheal/' .github/workflows/selfheal.yml && grep -qE 'contents: read' .github/workflows/selfheal.yml && ! grep -q 'secrets\.' .github/workflows/selfheal.yml && grep -q 'gate/scripts/verify-selfheal.sh' .github/workflows/selfheal.yml` | feature, refactoring: sonnet high |
| H5 | `build-app.sh` (item 22) | `packaging/macos/**` | none | `bash -n packaging/macos/build-app.sh && grep -q -- '--no-dmg' packaging/macos/build-app.sh && grep -q -- '--host-arch' packaging/macos/build-app.sh && grep -q 'HERON_COMMIT' packaging/macos/build-app.sh && grep -q 'Resources/connectors' packaging/macos/build-app.sh && ! grep -qE '^[^#]*--deep' packaging/macos/build-app.sh` | feature, refactoring: sonnet high |
| H6 | Self-improvement pane, report sheet, inbox buttons, token field (item 23) | `macos/**` | H2t | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && test "$(swift test list --package-path macos/HeronMac \| grep -c 'SelfImprovementTests')" -eq 9` | feature, refactoring: sonnet high |
| H7a | The threat-model section and guide for self-healing (item 25) | `docs/**` | H2 | `grep -qx '## Harness2: code Heron writes into itself' docs/engineering/SECURITY_THREAT_MODEL.md && test -s docs/features/self_improvement.md` | judgment and prose: opus high |
| verify-1a | Suite, required tests, test quality, evaluator audit | `scripts/gates/harness2-1a` (check only) | H1, H2, H2t, H3, H4, H5, H6, H7a, H12 | `cargo test --locked && bash eval/require-tests.sh --lib -- $(grep '^1a ' eval/selfheal/required-tests.txt \| cut -d' ' -f2) && cargo clippy --locked --all-targets -- -D warnings && swift test --package-path macos/HeronMac && genesis audit-evaluator eval/selfheal/genesis.yaml && testteeth run --base main --fail-under 70 -q` | feature, refactoring: sonnet high |

**Graph 1b: connectors**

| id | what to build | scope | depends on | check | model role |
|---|---|---|---|---|---|
| H8 | `src/ext/` executor (item 11) | `src/ext/**` | none | `cargo test --locked --lib ext:: && bash eval/require-tests.sh --lib -- redirect_is_not_followed auth_header_only_to_manifest_hosts read_op_must_be_get path_params_are_percent_encoded manifest_rejects_ip_localhost_and_port` | hardest tasks: opus high |
| H8w | Connector wiring (item 17) | `src/**` | H8 | `cargo test --locked --lib --bins && bash eval/require-tests.sh --lib -- decision_table_ext_tools ext_output_taints ext_untrusted_until_owner_trusts ext_trust_resets_on_manifest_change manifest_connector_accepts_api_key ext_source_memories_start_unconfirmed is_outside_replaces_both_checks` | hardest tasks: opus high |
| H8t | `tests/connector_manifests.rs` | `tests/**` | H8w | `cargo test --locked --test connector_manifests` | feature, refactoring: sonnet high |
| H9 | Connector card Trust switch | `macos/**` | H8w | `swift test --package-path macos/HeronMac && swift test list --package-path macos/HeronMac \| grep -q 'test_connectorCardShowsTrustSwitch'` | feature, refactoring: sonnet high |
| verify-1b | Suite and test quality | `scripts/gates/harness2-1b` (check only) | H8, H8w, H8t, H9 | `cargo test --locked && bash eval/require-tests.sh --lib -- $(grep '^1b ' eval/selfheal/required-tests.txt \| cut -d' ' -f2) && cargo clippy --locked --all-targets -- -D warnings && testteeth run --base main --fail-under 70 -q` | feature, refactoring: sonnet high |

**Graph 1c: observation**

| id | what to build | scope | depends on | check | model role |
|---|---|---|---|---|---|
| H10 | `src/observe/`, the `observe` profile, observe routes (item 19) | `src/**` | none | `cargo test --locked --lib --bins && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib -- ingest_keeps_nothing_outside_vault ingest_refused_while_paused observe_profile_denies_network master_key_passed_by_fd_not_env config_never_contains_sealbox_key venv_hash_mismatch_refuses_spawn workflow_job_requires_three_days topic_job_uses_only_owner_terms jobs_make_no_model_call forget_removes_vault_key_memories_and_suggestions workflow_suggestion_session_is_tainted observation_source_is_outside` | hardest tasks: opus high |
| H11 | Observers, Observation pane, consent, exclusions, indicator (item 24) | `macos/**` | H10 | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && test "$(swift test list --package-path macos/HeronMac \| grep -c 'ObservationTests')" -eq 7` | hardest tasks: opus high |
| H7c | The threat-model section and guide for observation (item 25) | `docs/**` | H10 | `grep -qx '## Observation' docs/engineering/SECURITY_THREAT_MODEL.md && test -s docs/features/observation.md` | judgment and prose: opus high |
| verify-1c | Suite, test quality, sealbox red team | `scripts/gates/harness2-1c` (check only) | H10, H11, H7c | `cargo test --locked && bash eval/require-tests.sh --lib -- $(grep '^1c ' eval/selfheal/required-tests.txt \| cut -d' ' -f2) && swift test --package-path macos/HeronMac && testteeth run --base main --fail-under 70 -q` | feature, refactoring: sonnet high |

**`required-tests.txt` format.** Each line is `<graph> <test name>`, for example `1a decision_table_selfheal`.

**Run notes:**

*   **Unsandboxed checks.** The `cargo test` checks need to run outside the sandbox, and so do the `swift build` checks in H6, H9 and H11. The skein supervisor runs them.
*   **Runs done by hand.** These need the owner's CLI login, TCC grants, GitHub or the sealbox checkout, so they run by hand after each `verify` node, with their output in that graph's PR:
    *   the replay go/no-go;
    *   the genesis fence claim;
    *   sealbox's red team;
    *   the end-to-end CUJs;
    *   PT6.
*   **Timeouts.** `verify` nodes get `--verify-timeout 3600`.

--------------------------------------------------------------------------------

*Drafted with EGM*
