# Heron Harness2: self-healing, self-evolution, connectors and observation

*   **Author:** fernandogarzaaa (drafted with Claude)
*   **Status:** Approved (2026-11-22). The owner replied "accept them all" to HQ1-HQ24 and OD2/OD3; the reply was relayed by the main Heron session
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
| Replay suite | 10 historical bugs that the repair agent must fix again from a report alone, each graded by a hidden test written for the suite ([item 20](#20-evalselfheal-create)). |
| PT1 to PT6, Pre-1 to Pre-5 | Prototypes and preconditions ([Preconditions](#preconditions)). |
| H0 | The shared skeleton unit that every graph builds on ([Preconditions](#preconditions)) |
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
*   **OD2 (2026-11-22, confirmed by the owner).** Self-healing and self-evolution apply to everything, including new connectors and integrations.
    *   A connector from a trusted trigger may install itself under the same loop.
    *   A scout-found integration stays a proposal.
    *   Changes to trust tables stay fenced and need owner review.
*   **OD3 (2026-11-22, confirmed by the owner).** Keystroke capture stays in, as its own switch, contained by sealbox.
    *   **What it overruled.** The earlier recommendation was to log no raw keystrokes, because they hold passwords, 2FA codes and private messages. Workflow would have been derived from app switches, window titles and file events instead.
    *   **The owner's reason.** v1.x is a research preview that tests how a personal agent works with full context, so accuracy from rich context is the goal.
    *   **What stays.** Raw keystrokes stay in sealbox. Secure Input and password fields are never captured.

## Open Questions

**Answered 2026-11-22.** The owner accepted HQ1 to HQ24 at their proposed defaults ("accept them all"). The build uses those defaults. The text below keeps each question as it was asked, so the reasoning stays on record.

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
    *   Trust resets when the manifest's hash changes. This follows MCP's pinning, where a tool whose definition changed is treated as `Privileged` (`src/mcp.rs:204-211`).
*   **HQ14, OAuth.** **Proposed: an API-key or no-auth manifest can be auto-eligible. One that needs a new OAuth provider is PR-only**, because `src/oauth.rs` is fenced.
*   **HQ15, requesting a connector.** **Proposed: the report sheet gains a "Request a connector" mode with three fields: a service name, the operations wanted, and notes.** It queues an owner report. Heron doesn't offer connectors on its own in Phase 1.

**Observation: HQ16 to HQ21**

*   **HQ16, sealbox's sandbox.** **Proposed: Heron runs every sealbox process under its own Seatbelt profile, with no network and access to `~/.heron/observe/` only. sealbox's `run_python` is never used.** sealbox's own sandbox is Linux-only, and it falls back to an audit hook on macOS (sealbox `src/sealbox/sandbox.py:62-66`, `:119`).
*   **HQ17, sealbox on preview Macs.** **Proposed: observation is offered only where `uv` is installed.**
*   **HQ18, keystrokes.** **Proposed:**
    *   capture only while Secure Input is off, the focused element isn't an `AXSecureTextField`, and the frontmost app isn't excluded;
    *   store the text in chunks of 1 minute per app;
    *   run sealbox's detectors over each chunk before it is written, to drop card numbers, one-time codes and API-key shapes.

    macOS doesn't guarantee Secure Input in every app.
*   **HQ19, retention.** **Proposed: raw data for 14 days, and derived memories for 90 days.**
*   **HQ20, exclusions.** **Proposed** (the exact defaults are in §11):
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
    4.  CI runs `ci.yml` (build, tests, clippy, Swift, app build) and `selfheal.yml`. `selfheal.yml` checks that the new test fails on the old code and passes on the fix (R2), and runs testteeth.
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

API errors don't produce failure records. Every handler error becomes a 400 (`src/server.rs:81-85`), and most are user input, so records from them would trigger repairs for things that aren't bugs.

**Producers of failure records:**

| `kind` | Writer |
|---|---|
| `panic` | The existing panic hook (`src/main.rs:714-722`) calls `harness2::record_panic(location)` |
| `core_exit` | The app sees a non-zero exit (`CoreProcess.swift`). The core writes the record on its next start |
| `migration_failed` | The `Store::open` migration error path |
| `health_failed` | The swapper (Phase 2) |

**`origin_tainted`.** In Phase 1, it is true when a process-wide counter of active tainted sessions is above zero at the time of the failure. That is coarse but safe: it only stops auto-eligibility. Phase 2 adds a task-local scope, set by every outside-content handler, so that auto-install can rely on it.

### 2. The repair run

`heron-cli selfheal repair --run <id>` runs as a separate process:

*   **Home:** a disposable `HERON_HOME` at `~/.heron/selfheal/runs/<id>/home`. It has no memories, persona, connectors, Telegram or MCP.
*   **Workspace:** a git worktree at `~/.heron/selfheal/runs/<id>/wt`, on the branch `heron/selfheal/<yyyymmdd>-<id>`, cut from the installed commit (the new `commit` field of `GET /api/state`).
*   **Tools:** `read_file`, `write_file`, `edit_file`, `list_dir`, `search_files`, `find_files` and `shell`. `App::assemble_with_tools` filters the tool registry down to these.
*   **Policy:** `mode = "ask"` with the existing `DenyAll` approver, so every `Ask` becomes a denial. Auto mode would allow any unmatched command (`src/permissions.rs:438`). The allow rules are:
    *   `write_file:*` and `edit_file:*`. A write outside the worktree is `Privileged`, and allow rules never cover `Privileged` effects (`src/permissions.rs:108-113`, `:416`);
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
*   the genesis audits;
*   the real-CLI containment canary (`eval/containment/run.sh`). It needs every Tier 1 CLI logged in. The `ci.yml` unit tests for `cli_llm` containment do run on every PR.

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

**The scout** has its own timer, because `schedule.rs` can't express 72 hours: `every 72h` fails with "does not divide evenly" (`src/schedule.rs:250-257`). The interval is `[harness2] scout_every_hours`, set to 24, 72 or 168.

*   **Fetching.** It uses its own client, built the way `web.rs` builds its client:
    *   `permissions::PublicOnlyResolver` for DNS (`src/tools/web.rs:55`);
    *   `no_proxy`;
    *   a redirect policy that follows at most 5 redirects, and only to allowlisted hosts.

    The allowlisted hosts are `api.github.com`, `hn.algolia.com`, `export.arxiv.org`, `www.anthropic.com`, `openai.com` and `deepmind.google`.
*   **Parsing.** GitHub and Hacker News return JSON. The arXiv and lab feeds go through a tolerant regex reader that keeps `title`, `link`, and `updated` or `pubDate`, and skips any entry it can't parse.
*   **Ranking.** One model call, with no tools, in a session marked tainted from the start. The items go in wrapped as untrusted (`src/agent.rs:692-702`). The call returns `[{item_id, harness_part, why, effort}]`, at most 10 entries. Each `item_id` must be one that was fetched. Links and titles come from the fetched data, never from the model.
*   **Output.** Inbox notes titled "Idea from the web: …". Seen URLs go into `selfheal.db`. The scout writes nothing else.

**`Harness2::tick`** is spawned on every third pass of `App::scheduler_loop`, inside its `own_background_work()` branch, so only one Heron per Mac runs it (`src/app.rs:2134-2140`). The loop sleeps 20 seconds between passes, so that is about once a minute. Pausing is checked inside each step, because `run_due_schedules` returns early while paused (`src/app.rs:1483-1486`).

| Step | Runs while paused? | Held by Q6? |
|---|---|---|
| 1. Poll PRs and checks (`gh pr view`, `gh pr checks`) | yes | no (no model call) |
| 2. Start a queued owner report | no | no (attended) |
| 3. Start a queued failure-record trigger | no | yes: a row-6 note |
| 4. Run the scout, if due | no | yes: it runs at the app's next `GET /api/events` connection |
| 5. Prune worktrees and branches older than 7 days | yes | no |
| 6. Once a day: observation jobs and prune (graph 1c) | no | no (no model call). It runs on every Mac with observation on, preview Macs included |

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
name = "todoist"              # ^[a-z][a-z0-9_]{2,30}$; not a catalog key in src/connectors.rs
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
    *   Trust is stored in kv as `ext_trust:<name> = <manifest sha256>`, so a changed manifest is untrusted again, in the spirit of MCP's pinning (`src/mcp.rs:204-211`).
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
        *   **Which terms.** The terms are the titles of open tasks (`Store::list_tasks`) and confirmed project memories with source `user` or `onboarding`. That is at most 50 terms. A task title may have been written by the model (`task_add`), but a term only chooses what is counted, never what leaves the vault, so a bad term only makes a useless count.
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

| File | Change | Where |
|---|---|---|
| `src/lib.rs`, `src/config.rs` (config structs), `src/store.rs` (the next schema step, the new insert functions), `src/app.rs` (`assemble_with_tools`, tainted-session counter), `src/agent.rs` (`PromptSource::Repair`, content entries), `src/memory.rs` (`is_outside`) | The skeleton | 1a, unit H0 |
| `src/main.rs` | The `selfheal` and `report` subcommands, and `record_panic` in the panic hook | 1a, item 14 |
| `src/server.rs` | `commit` in `/api/state`, `action` on inbox rows, and the `/api/harness2/*` routes | 1a, item 15 |
| `src/app.rs` | The tick spawn, and the `core_exit` and `migration_failed` records | 1a, item 16 |
| `src/connectors.rs`, `src/sources.rs`, `src/server.rs` (trust route), `src/app.rs` (tool registration) | Manifest cards, `require_known`, `api_key`, `is_known`, and trust | 1b, item 17 |
| `src/sandbox.rs` | `Profile::Observe` | 1c, item 19 |
| `src/server.rs`, `src/app.rs`, `src/memory.rs` | The observe routes, the daily jobs, and the 90-day deletion | 1c, item 19 |
| `src/ext/**`, `src/harness2/**`, `src/observe/**` | New | items 1-11, 19 |
| `tests/**` | Re-recorded `state.json` and `connectors.json`, the integration tests, `connector_manifests.rs`, and the fake sealbox | items 17-19 |
| `.github/workflows/selfheal.yml` | New | 1a, item 26 |
| `packaging/macos/build-app.sh` | Argument loop, `--no-dmg`, `--host-arch`, `HERON_COMMIT`, hardened-runtime default signing, and the connectors copy | 1a, item 22 |
| `eval/selfheal/**`, `scripts/verify-selfheal.sh` | New | 1a, items 20-21 |
| `macos/HeronMac/**` | The panes, the `CoreClientProtocol` additions (in all three clients), the observers, the inbox buttons, `CoreProcess`'s exit variable, and the decoders | items 23, 24, 17 (H9) |
| `docs/engineering/SECURITY_THREAT_MODEL.md` | Three sections | items 25a, 17 (H7b), 25c |

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
    *   `replay_threshold = 6` (HQ10)
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
    *   `exclude_apps = ["com.1password.1password", "com.agilebits.onepassword7", "com.bitwarden.desktop", "com.lastpass.LastPass", "com.apple.keychainaccess", "com.apple.Passwords", "com.apple.systempreferences"]`. An app is also excluded when its bundle ID or display name contains `bank`, compared case-insensitively.
    *   `exclude_hosts = ["1password.com", "bitwarden.com", "lastpass.com", "accounts.google.com", "appleid.apple.com", "login.microsoftonline.com", "paypal.com"]`. A host is also excluded when any label contains `bank`, `health`, `patient` or `pharmacy`.
    *   `exclude_folders = ["~/Library", "~/.ssh", "~/.gnupg", "~/.heron"]`.

**Enums**

*   **`triggers.kind`:** `owner_report` or `failure_record`.
*   **`triggers.provenance`:** `typed`, `pasted`, `from_note` or `none`.
*   **`triggers.state`:** `queued`, `held`, `running`, `retrying`, `waiting_limit`, `setup_failed`, `interrupted`, `cancelled`, `failed`, `awaiting_review`, `pr_draft`, `pr_opened` or `installed`.
*   **`runs.verdict`:** `auto_eligible` or `needs_owner`.
*   **`runs.pr_state`:** `draft`, `open`, `merged` or `closed`.

**Routes.** Every route needs the bearer token. Errors use the existing shape, `{"error": "<text>"}` (`src/server.rs:75-78`). The status code tells the cases apart.

| Route | Body | Returns |
|---|---|---|
| `GET /api/harness2` | — | `{self_healing, self_evolution, scout_every_hours, builder, checklist: [{item, ok, fix}], runs: [{id, state, verdict, pr_url}]}`. The checklist items are `repo` (a checkout whose origin is this repository), `git`, `gh_token` (set, and `gh api user` works), `cargo`, `swift`, and `replay` (a passing row for the installed commit). Each has a `fix` sentence |
| `POST /api/harness2` | any of the three settings | as `GET` |
| `POST /api/harness2/report` | `{text, provenance, attach_session?, publish_text, connector?: {service, ops: [string], notes}}` | On a builder Mac, `{trigger_id}`. On any other Mac, `{path}` (the written `.heronreport`). `409` if the attached session is tainted |
| `POST /api/harness2/token` | `{token}` | `{ok}`. The token is stored in the OS vault, as `harness2_github_token` |
| `POST /api/connectors/custom/{id}/trust` | `{trusted: bool}` | `{id, trusted}` |
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
 "changed": ["src/schedule.rs", "tests/schedule_local_time.rs"]}
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
    *   It breaks CLI containment invariant 3 (`src/cli_llm.rs:38-42`).
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
    *   the new schema step raises `user_version` by one, and an older build refuses a newer database (`src/store.rs:610-616`). So rolling back the app also means restoring the `backups/heron-before-upgrade-from-v3-*` copy that Heron makes before it migrates;
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

**Naming.**

*   `harness2` is the Rust module.
*   `selfheal` is the CLI subcommand group, the database (`selfheal.db`) and the branch prefix (`heron/selfheal/`).
*   "Self-improvement" is the Settings pane. It holds the self-healing switch and the scout.
*   `ext` is the connector executor, and `observe` is the observation module.
*   Item numbers are stable IDs. A missing number is a removed item.
*   The cargo binary is `heron`. It ships as `heron-cli` only inside the app bundle (`packaging/macos/build-app.sh:73`).

### Preconditions

*   **Pre-1.** The v1 features base (B1 to B6) is merged into `main`. It adds `ContentEntry` and `docs/features/`.
*   **Pre-2.** The prototypes have been run by hand, outside any sandbox, and recorded in `docs/designs/research/harness2-prototypes.md` as `PT<n>: pass|fail: <evidence>`. Graph 1a needs PT1, PT4 and PT6. Graph 1c needs PT5. Phase 2 needs PT2 and PT3.
    *   **PT1.** Run `cargo test --locked` inside Heron's Seatbelt shell. It passes if at most 30 tests fail, and every failure is in a list recorded with the result.
    *   **PT2.** Test whether TCC grants and keychain items survive switching from the release DMG to a self-fix build signed under HQ5, and back again. Also test an ad-hoc build. Only the build change is under test: the schema is unchanged in both directions.
    *   **PT3.** Test whether a process started with `setsid` by the core outlives both the app and the core, and can rename `/Applications/Heron.app` with no admin prompt.
    *   **PT4.** Time a one-line Rust fix and a one-line Swift fix, from report to a labelled PR. This always passes; the times are recorded.
    *   **PT5.** Test whether the four observation permission prompts appear with no `Info.plist` usage strings. If one doesn't appear, H5 adds the key PT5 names.
    *   **PT6.** Check that a push changing `.github/workflows/`, made with the HQ24 token, is rejected. It is run again once `selfheal.yml` exists.
*   **Pre-3.** The owner has answered, or accepted the defaults for, HQ1 to HQ5, HQ10, HQ12, HQ16 to HQ18 and HQ22 to HQ24.
*   **Pre-4 (graph 1c only).** These sealbox additions are merged in sealbox's own repository, and the merge commit is pinned:
    *   `append <dataset> --kind events` reads JSON lines on stdin and writes encrypted hourly segments.
    *   `query <sql>` runs one SQL string through the gate and prints JSON.
    *   `prune --older-than <days>`.
    *   `--key-fd <n>` on every command.
    *   A hashed `requirements.lock`.
    *   An `observation` policy template.

    Its check is `python3 -m pytest -q && python3 eval/redteam.py`, with 0 leaks.
*   **Pre-5.** The review session's harness parts (PR #73) are merged into `main` before H1 starts, including the effect journal's ask-again hook and the toolchain hash check. H1 calls them by the names that PR gives them, and this doc is updated in the same change.
**Unit H0, the skeleton.** It is the first unit of graph 1a (scope `src/**`). It holds the parts that more than one unit needs:
    *   **`src/lib.rs`:** `pub mod harness2; pub mod ext; pub mod observe;`, each an empty module.
    *   **`src/config.rs`:**
        *   `Harness2Config` and `ObserveConfig` with every key and default in §11, under `[harness2]` and `[observe]`;
        *   validation that `scout_every_hours` is one of 24, 72 or 168.
    *   **`src/store.rs`:** the next schema step is appended to `STEPS` (`src/store.rs:190`). Pre-1 and PR #73 may each have added steps first, so it is whichever number comes next. The step adds the nullable columns `inbox.action` (TEXT) and `suggestions.origin` (TEXT, default `'proactive'`). It also adds two functions, and leaves the existing ones as wrappers so their callers don't change:
        *   `add_inbox_with_action(title, body, action: Option<&str>)`;
        *   `add_suggestion_with(title, why, prompt, origin: &str, session_id: Option<&str>)`.
    *   **`src/app.rs`:**
        *   `App::assemble_with_tools(cfg, workspace, store, llm, allow: Option<&[&str]>)`, with `assemble` kept as a wrapper that passes `None`;
        *   `agent::TAINTED_SESSIONS`, a process-global `AtomicUsize`, read through `App::tainted_sessions()`. It counts persisted `Session` values whose taint flag is on:
            *   It goes up when the flag turns from off to on (`agent.rs:692-702`), and when `Session::load` restores a tainted session.
            *   It goes down when such a `Session` is dropped.
            *   Ephemeral sub-agent sessions share their parent's flag and aren't counted.
        *   `harness2::DB`, a `OnceLock<SelfhealDb>` that `main` sets after `Config::load`. `record_panic` does nothing when it is unset.
    *   **`src/agent.rs`:**
        *   `PromptSource::Repair`, Trusted, with the heading `# Repair task`. `ALL` grows to 12, and the `trust()` and `headings()` matches gain the case.
        *   The `ContentEntry` variants `OwnerReport` and `FailureRecord` (trusted), and `ConnectorOutput` and `ObservationResult` (tainting).
    *   **`src/ext/manifest.rs`:**
        *   the manifest schema in §7;
        *   `validate_manifest(&Manifest) -> Result<()>`;
        *   `host_ok(url: &str) -> Result<()>`, which carries over every rule in `validate_endpoint` today (`src/connectors.rs:125-146`): HTTPS on port 443, a DNS name with no IP literal, no userinfo, query or fragment, the same length limit, and not `localhost`, `.local` or `.internal`.

        Graph 1a's fence (R6) uses these functions, and graph 1b builds the executor on top of them. The tests `manifest_rejects_ip_localhost_and_port`, `manifest_rejects_catalog_key_names` and `read_op_must_be_get` live here.
    *   **`src/memory.rs`:** `is_outside(source)`, which returns true for any source in `OUTSIDE_SOURCES`, any `ext:` prefix, and `observation`. It replaces both `OUTSIDE_SOURCES.contains` calls (`src/memory.rs:276`, `:367`).

    Its tests:

    *   `is_outside_replaces_both_checks`
    *   `tainted_sessions_counts_up_and_down`
    *   `next_schema_step_adds_only_nullable_columns`
    *   `insert_wrappers_keep_old_signatures`
    *   `assemble_with_tools_filters_registry`
    *   `repair_prompt_block_is_trusted`
    *   `manifest_rejects_ip_localhost_and_port`
    *   `manifest_rejects_catalog_key_names`
    *   `read_op_must_be_get`
    *   the existing `the_schema_cannot_change_without_a_new_version` and `every_prompt_block_is_trusted_or_taints`

    Its work-unit row is in graph 1a.

### Graph order

The graphs run in sequence: 1a, then 1b, then 1c. Each graph's branch is cut from `main` after the previous graph's PR is merged into `main`. Graph 1c also waits for Pre-4.

Running them in sequence avoids conflicts in `server.rs`, `config.rs` and `agent.rs`. It also makes sure that `eval/selfheal/required-tests.txt` and `check-required.sh` (both from H3) are on every later branch.

### Platform gating

`ci.yml` runs `cargo test` on Linux, macOS and Windows.

*   **Cross-platform:** everything in `src/harness2/` and `src/ext/`.
*   **macOS only:** `src/observe/` code and tests, plus the Seatbelt and keychain paths. They sit behind `#[cfg(target_os = "macos")]`.
*   **Elsewhere:** the observe routes return `501 {"error": "Observation needs macOS."}`.

### Target Files to Create/Modify

#### Graph 1a, unit H1: `src/harness2/` (scope `src/harness2/**`)

##### 1. `src/harness2/mod.rs` (Create)

*   `Harness2 { db: SelfhealDb, cfg: Harness2Config }`
*   `tick(&self, app: &App)`, which runs the five steps in §6. `App::scheduler_loop` spawns it on every third pass, inside its `own_background_work()` branch (`src/app.rs:2134`). The loop sleeps 20 seconds between passes, so this is about once a minute.
*   `queue(&self, t: Trigger) -> i64`, where `Trigger { kind: TriggerKind, provenance: Provenance, fields: serde_json::Value }`
*   `record_panic(location: &str)`, which writes or increments a `FailureRecord` keyed on `(kind, location, commit)`, with `origin_tainted` taken from `App::tainted_sessions() > 0`

##### 2. `src/harness2/trust.rs` (Create)

*   `TriggerKind { OwnerReport, FailureRecord }`
*   `Provenance { Typed, Pasted, FromNote, None }`
*   `FailureKind { Panic, CoreExit, MigrationFailed, HealthFailed }`
*   `outcome(class, verdict, state, ci) -> Outcome`, covering exactly the rows in §1
*   `failure_starts_trigger(rec, installed_is_selffix) -> bool`, which is true when `count >= 2` and the installed build isn't a self-fix

##### 3. `src/harness2/fence.rs` (Create)

*   `AUTO`, a `globset` set built from the §3 allowlist.
*   `check(repo, base, fix) -> FenceVerdict { AutoEligible, NeedsOwner(Vec<String>) }`. It applies R1, R3, R4, R6 and R8, reading the diff with `git diff --numstat --name-status -M <base> <fix>`.
*   `push_rule(paths) -> bool`, for row 0b.
*   The CLI wrapper `heron selfheal check-fence --repo <path> <base> <fix>`, item 14, prints `{"verdict": "auto_eligible"|"needs_owner", "reasons": [..]}`. It exits 0 when it reached a verdict and 2 on a usage error.

##### 4. `src/harness2/repair.rs` (Create)

*   `REPAIR_TOOLS`: `read_file`, `write_file`, `edit_file`, `list_dir`, `search_files`, `find_files` and `shell`.
*   `REPAIR_POLICY` (§2).
*   `REPAIR_PROMPT`.
*   `prepare(run, commit)`. It runs `git worktree add`, writes the disposable home's `config.toml`, and returns `setup_failed` when `commit` is null.
*   `attempt(run, feedback)`. It builds the app with `App::assemble_with_tools(..., Some(&REPAIR_TOOLS))` and `DenyAll`, then runs one turn.

##### 5. `src/harness2/checks.rs` (Create)

*   `local_checks(run)` runs the fence, R5, the private-text check and `push_rule`.
    *   **Private-text check.** Its input is the report's `fields.text` plus the attached transcript, if any. Both sides are normalised: lowercase, and any run of non-alphanumeric characters becomes one space. The check takes every 8-word shingle of the input and flags any shingle that appears in the diff's added lines.
*   `git(args) -> Command` always adds `-c core.hooksPath=/dev/null -c core.fsmonitor=false -c diff.external= -c filter.lfs.process=`, and checks that the worktree's `.git` file points at `<repo>/.git/worktrees/<id>`.
*   `poll_ci(run)` reads `gh pr checks <pr> --json name,state`. It needs every check to be `SUCCESS`, and needs a check named `selfheal-gate` to be present.

##### 6. `src/harness2/publish.rs` (Create)

*   `commit(run)`
*   `push_and_draft(run)`, which checks before each step (§5)
*   `finish(run)`, which labels the PR and marks it ready, or closes it with the failure report
*   `poll_prs()`

##### 7. `src/harness2/scout.rs` (Create)

*   `client()` builds a `reqwest` client with `dns_resolver(PublicOnlyResolver)` (`src/permissions.rs:1228`), `no_proxy()`, and a custom redirect policy. The policy follows at most 5 redirects, each to a host in `HOSTS`.
*   The fetchers request these exact URLs:
    *   `https://api.github.com/search/repositories?q=topic:llm-agent+topic:ai-agent+topic:agent-framework+topic:mcp+pushed:>{today-7d}&sort=stars&per_page=20`
    *   `https://hn.algolia.com/api/v1/search?tags=story&numericFilters=points>=100&query=agent`
    *   `http://export.arxiv.org/api/query?search_query=ti:agent+AND+(cat:cs.AI+OR+cat:cs.CL+OR+cat:cs.SE)&sortBy=submittedDate&max_results=20`
    *   `https://www.anthropic.com/news/rss.xml`
    *   `https://openai.com/news/rss.xml`
    *   `https://deepmind.google/blog/rss.xml`

    A feed URL that returns a 404 is skipped and logged.
*   `parse_feed(text) -> Vec<Item>`.
*   `rank(items)` makes the tool-less call in a tainted session.
*   `post(ideas)` writes one inbox note per idea with `add_inbox_with_action(title, body, None)`.

##### 8. `src/harness2/report.rs` (Create)

*   `write_report(text, provenance, attach: Option<SessionId>, out: &Path)` writes JSON (§11). A tainted session is refused. Observations are never included. It includes the failure records from the last 7 days, so the owner sees more context when reading another person's report. Owner-report triggers on the builder Mac read only the last 24 hours (§1).
*   `import_report(path)` adds an inbox note with action `{"kind":"restate","text":<report text>}`.

##### 9. `src/harness2/db.rs` (Create)

*   `selfheal.db` with the tables in §6, and `PRAGMA user_version = 1`.

##### 10. Tests (inline in each file)

*   **`trust.rs`:**
    *   `decision_table_selfheal`
    *   `pasted_report_is_never_auto`
    *   `from_note_report_is_never_auto`
    *   `origin_tainted_failure_is_never_auto`
    *   `selffix_build_failure_starts_no_trigger`
    *   `failure_record_has_no_free_text`
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
    *   `private_text_ignores_case_and_punctuation`
    *   `git_calls_disable_hooks_and_fsmonitor`
    *   `moved_git_pointer_stops_git`
    *   `ci_without_selfheal_check_is_not_passing`
*   **`repair.rs`:**
    *   `decision_table_repair_profile`
    *   `repair_registry_has_only_seven_tools`
    *   `compound_shell_is_denied_in_repair`
    *   `unmatched_command_is_denied_in_repair`
    *   `null_commit_is_setup_failed`
*   **`publish.rs`:**
    *   `publish_skips_existing_branch_and_pr`
    *   `pr_body_omits_report_text_by_default`
    *   `ci_failure_three_times_closes_draft`
    *   `ci_not_run_in_24_hours_closes_draft`
*   **`mod.rs`:**
    *   `tick_steps_follow_their_gating`
    *   `record_panic_writes_failure_record`
    *   `interrupted_run_asks_before_rerun` (through Pre-5's ask-again hook)
*   **`scout.rs`:**
    *   `scout_drops_unknown_item_ids`
    *   `scout_note_link_comes_from_fetch`
    *   `scout_rejects_unlisted_host`
    *   `scout_rejects_redirect_off_allowlist`
    *   `feed_reader_skips_malformed_entries`
    *   `scout_writes_only_inbox_notes`
*   **`report.rs`:**
    *   `report_excludes_conversation_unless_ticked`
    *   `report_with_tainted_session_is_refused`
    *   `report_never_includes_observations`
    *   `imported_report_is_inbox_only`

#### Graph 1a, unit H2: wiring (scope `src/**`)

##### 14. `src/main.rs` (Modify)

These subcommands use the standalone path, `run_standalone` (`src/main.rs:733`). They need no running app.

*   **`heron selfheal repair --run <id>`** runs one repair attempt.
*   **`heron selfheal replay --case <n> --worktree <path> --cases <file>`** queues an owner-report trigger from case `<n>`'s report, against the given worktree, then runs the repair.
*   **`heron selfheal check-fence --repo <path> <base> <fix>`** works as described in item 3.
*   **`heron selfheal record-replay <commit> <passed> <total> <auto_eligible>`** writes one row to `replay`.
*   **`heron report --text <text> [--out <path>]`** writes a `.heronreport` file (item 8).
*   **The panic hook** at `:714-722` calls `heron::harness2::record_panic(location)` before it calls the previous hook.

##### 15. `src/server.rs` (Modify)

*   `GET /api/state` gains `commit`: `option_env!("HERON_COMMIT")`, or JSON `null` when it isn't set.
*   It gets every `/api/harness2/*` route in §11.
*   `GET /api/inbox` rows gain `action`.
*   Errors keep the existing shape, `{"error": "<text>"}` (`src/server.rs:75-78`).

##### 16. `src/app.rs`, `src/agent.rs` (Modify)

*   **`app.rs`:** spawns the tick from item 1, holding `Harness2` in an `Arc` field on `App`.
*   **`main.rs`:**
    *   It writes a `core_exit` record at start when the app has passed `HERON_PREVIOUS_EXIT=<status>`. The app sets that variable when it restarts the core after a non-zero exit.
    *   It writes a `migration_failed` record when `Store::open` returns a migration error. `main` holds the config, so it can reach `harness2::DB`.
*   **`agent.rs`:** the `OwnerReport` and `FailureRecord` content entries are used for repair turns.

##### 18. Tests for the wiring

*   **Inline (`--lib`):**
    *   `state_reports_commit`
    *   `harness2_routes_need_token`
    *   `inbox_rows_carry_action`
    *   `inbox_action_starts_trigger`
    *   `owner_report_entry_is_trusted`
    *   `core_exit_env_writes_failure_record`
*   **Unit H2t (scope `tests/**`):**
    *   Re-record `tests/fixtures/api/state.json` (delete it, then run the suite once), because `/state` gains `commit` (`tests/integration.rs:1456`).
    *   Add `api_harness2_lists_switches_and_checklist`, with the fixture `tests/fixtures/api/harness2.json`.
    *   Add `api_state_reports_commit_key`, which asserts that `/state` has a `commit` key that is a string or null.

#### Graph 1a, unit H3: replay suite (scope `eval/**`)

##### 20. `eval/selfheal/` (Create)

*   **`cases.md`** lists 10 cases. Few existing fixes ship separable tests: of the 120 `fix` commits in `git log 39dbdf5`, none adds only new test files, because most Rust tests are inline. So each case comes with a hidden test written for the suite.
    *   **Which commits.** The cases are the first 10 commits in `git log 39dbdf5` whose subject starts with `fix`, whose changes outside tests are all under `src/` or `macos/HeronMac/Sources/`, and whose diff has at most 200 lines.
    *   **Format.** Each case is a `## case <n>` heading followed by four lines: `base: <sha>`, `fix: <sha>`, `report: <one sentence in the owner's words, without the fix>`, `hidden: eval/selfheal/cases/<n>.patch`, and `test: <selector>`. The selector is `cargo --lib <name>`, `cargo --test <stem>` or `swift <Class>`.
    *   **The hidden test.** Each `<n>.patch` adds one test: an integration test under `tests/`, a Swift test, or a `#[test]` appended to the changed file's test module. When applied with `git apply --3way`, it must fail on `base` and pass on `fix`.
    *   **Validation.** `run_replay.py --validate` checks both of those for every case. It runs by hand, because it builds 20 commits.
    *   **Replacing a case.** A patch that conflicts with an agent's fix makes that case `ungradable`. It is then replaced by the next qualifying commit.
*   **`run_replay.py`** works like this:
    1.  Build `target/release/heron` from HEAD.
    2.  For each case, run `git worktree add <tmp> <base>`.
    3.  Run `heron selfheal replay --case <n> --worktree <tmp> --cases eval/selfheal/cases.md`.
    4.  Apply `eval/selfheal/cases/<n>.patch` to `<tmp>` with `git apply --3way`.
    5.  The case passes if the case's `test:` selector passes and `cargo test --locked` passes in `<tmp>`. `selfheal replay` prints `{"verdict": "auto_eligible"|"needs_owner", "attempts": n}`, and the `verdict` values are counted to give `K`.

    It prints `replay: N/M` and `auto-eligible: K`. It calls `record-replay` with `git merge-base HEAD main` as the commit, which is the same key the core checks. It exits 0 when N is at least the threshold.
*   **Threshold.** It comes from `[harness2] replay_threshold`, default 6. That value is HQ10.
*   **`grade.py` and `genesis.yaml`.**
    *   **What is audited:** the pure function `grade(outcome: dict) -> bool` in `eval/selfheal/grade.py`. `run_replay.py` calls it with `{patch_empty, hidden_test_deleted, hidden_failed_on_base, hidden_passed_on_fix, suite_passed}`.
    *   **How:** `genesis.yaml` audits it with `--suite json`, shaped like `eval/genesis/pass-rules-json.yaml`, over the cases in `eval/selfheal/genesis_cases.json`.
    *   **The rule:** an empty patch, or one that deletes a hidden test, must grade `false`.
    *   **Required verdict:** `TRUSTED`.
*   **`required-tests.txt`** has one line per test: `<graph> <selection> <name>`, for example `1a --lib decision_table_selfheal` or `1a --test=integration api_harness2_lists_switches_and_checklist`. It covers unit H0 and items 10, 11, 17, 18 and 19.
*   **`check-required.sh <graph>`** groups the file's lines by selection, then runs `bash eval/require-tests.sh <selection> -- <names>` once for each group.
*   **`eval/tests/test_selfheal_replay.py`** tests the grader.
*   **`eval/tests/test_verify_selfheal.py`** builds a small git repository in a temp folder and checks each row of the R2 table against `scripts/verify-selfheal.sh`.
*   **`requirements-ci.txt`** is generated with `uv pip compile --generate-hashes` from `testteeth==<the version the owner's CLI reports today>`, so every dependency is pinned and hashed. Its first line is `# cargo-mutants==<the version cargo mutants --version reports on the owner's Mac>`, and the workflow reads that line.

#### Graph 1a, unit H4: gate script (scope `scripts/**`)

##### 21. `scripts/verify-selfheal.sh` (Create)

*   **Arguments:** `verify-selfheal.sh <base-sha> <fix-sha> <fix-dir>`. The two commits must be 40 hex digits each, and the directory must exist.
*   **Base checkout:** it makes one with `git -C <fix-dir> worktree add ../base <base-sha>`.
*   **R2:** it lists the new test files with `git diff --diff-filter=A --name-only <base> <fix> -- tests macos/HeronMac/Tests connectors`. The `<stem>` is the file name without its extension, and `<Class>` is the Swift file's name. It then applies the R2 table in §4 with `cargo test --locked --no-run` followed by `cargo test --locked --test <stem>`, or `swift build --build-tests` followed by `swift test --filter <Class>`.
*   **Mutation:** when Rust changed, `testteeth run --base <base> --fail-under 70 -q`.
*   **Output:** it writes `<fix-dir>/gate-report.json` (§11). The `r2` field is one of `passes`, `catches_nothing`, `no_compile_on_base`, `fails_on_fix` or `new_connector`.
*   **Exit codes:** 0 when the gate passes, which includes a `no_compile_on_base` verdict (the report records it); 1 on a gate failure; 2 on a usage error.
*   **Overrides:** it runs the commands named by `CARGO`, `SWIFT` and `TESTTEETH`, which default to `cargo`, `swift` and `testteeth`, so `test_verify_selfheal.py` can stub them.
*   **Reading the verdict:** `finish(run)` downloads the report with `gh run download --name gate-report`, takes R2's verdict from it, and stores its SHA-256 in `runs.gate_report_sha256`.

#### Graph 1a, unit H12: CI workflow (scope `.github/**`)

##### 26. `.github/workflows/selfheal.yml` (Create)

```yaml
on: pull_request
permissions: { contents: read }
jobs:
  gate:
    name: selfheal-gate
    if: startsWith(github.head_ref, 'heron/selfheal/')
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4      # main into gate/
        with: { ref: main, path: gate }
      - uses: actions/checkout@v4      # head into fix/
        with: { ref: "${{ github.event.pull_request.head.sha }}", path: fix, fetch-depth: 0 }
      - run: pip install --require-hashes -r gate/eval/selfheal/requirements-ci.txt
      - run: cargo install --locked cargo-mutants@<pin from requirements-ci.txt comment>
      - run: bash gate/scripts/verify-selfheal.sh "$(git -C fix merge-base origin/main HEAD)" "${{ github.event.pull_request.head.sha }}" fix
      - uses: actions/upload-artifact@v4
        with: { name: gate-report, path: fix/gate-report.json }
```

The workflow uses no secrets. The job's check name is `selfheal-gate`, which `poll_ci` requires (item 5).

#### Graph 1a, unit H5: packaging (scope `packaging/macos/**`)

##### 22. `packaging/macos/build-app.sh` (Modify)

*   **Arguments.** An argument loop replaces the one-argument check (`build-app.sh:16-23`). It takes at most one mode flag (`--sign` or `--unsigned`), plus any of `--no-dmg` and `--host-arch`, in any order.
*   **`--host-arch`.** It sets `TARGETS` to the host's target only (`arm64` → `aarch64-apple-darwin`), passes `--arch $(uname -m)` to SwiftPM, and copies the one cargo binary instead of running `lipo`.
*   **`--no-dmg`.** It skips step 6.
*   **`HERON_COMMIT`.** The cargo loop runs `HERON_COMMIT=$(git rev-parse HEAD) cargo build …`.
*   **Default mode.** The `codesign --deep` call (`build-app.sh:97`) becomes two calls, `heron-cli` first and then the app, each `codesign --force --options runtime --sign "${CODESIGN_IDENTITY:--}"`.
*   **Connectors.** `mkdir -p "$APP/Contents/Resources/connectors"`, then `if [ -d connectors ]; then find connectors -maxdepth 1 -name '*.toml' -exec cp {} "$APP/Contents/Resources/connectors/" \; ; fi`. This works when the folder is missing or empty; the first manifest creates it.

#### Graph 1a, unit H6: Mac app, self-improvement (scope `macos/**`)

##### 23. Mac app, self-improvement

*   **`CoreClientProtocol`** gains the `/api/harness2/*` calls. `CoreClient` implements them, `PreviewCoreClient` returns fixtures, and `ManagedCoreClient` (`HeronCore/ManagedCoreClient.swift:12`) forwards them.
*   **`Models.swift` and `Wire.swift`** gain `Harness2State`, `CoreWire.harness2` and the inbox `action`. `ModelDecodingTests.swift` gains decoders for `harness2.json` and the re-recorded `state.json`.
*   **`CoreProcess.swift`** sets `HERON_PREVIOUS_EXIT` when it restarts the core after a non-zero exit.
*   **`ContentStores.swift` (`InboxStore`) and the inbox view in `ScreenViews.swift`** show the action buttons (§6).
*   **`HeronUI/SelfImprovement/SelfImprovementSettingsView.swift`** has:
    *   the switches and the checklist;
    *   runs, with their PR links;
    *   "Check every", with the choices 1 day, 3 days and 1 week;
    *   the line shown on preview Macs;
    *   the publishing-token field, a secure field that saves through `POST /api/harness2/token`.
*   **`HeronUI/SelfImprovement/ReportSheet.swift`** has:
    *   paste detection, which sets `pasted`;
    *   the "OK to publish" tick;
    *   a "Request a connector" mode, with fields for the service, the operations and notes;
    *   "Restate as report", which sets `from_note`.

    On a preview Mac, it shows the share sheet for the `path` that the API returns.
*   **`SettingsView.swift`** links the pane. **`HeronApp.swift`** adds the Help › Report a problem and File › Import report menu items.
*   **Tests:** all in `macos/HeronMac/Tests/HeronUITests/SelfImprovementTests.swift`, and only there:
    *   `test_selfHealingSwitchDisabledUntilChecklistPasses`
    *   `test_previewMacShowsReportLineInsteadOfSwitch`
    *   `test_scoutScheduleOffersThreeChoices`
    *   `test_reportSheetDefaultsToNoConversation`
    *   `test_reportSheetDefaultsToNotPublished`
    *   `test_pasteMarksReportPasted`
    *   `test_restateMarksReportFromNote`
    *   `test_inboxShowsActionButtons`
    *   `test_connectorRequestModeSendsFields`

#### Graph 1a, unit H7a: docs (scope `docs/**`)

##### 25a. Docs for 1a

*   `docs/engineering/SECURITY_THREAT_MODEL.md` gains `## Harness2: code Heron writes into itself`. It covers:
    *   the trust table;
    *   provenance, and its limit (retyping);
    *   the fence;
    *   CI as the gate;
    *   the HQ2 residual;
    *   the process list.
*   New file: `docs/features/self_improvement.md`.

#### Graph 1b, unit H8: connector executor (scope `src/ext/**`)

##### 11. `src/ext/mod.rs`, `src/ext/manifest.rs`, `src/ext/exec.rs` (Create)

*   **`manifest.rs`** already exists from H0, with the schema in §7.
    *   **The name** must match `^[a-z][a-z0-9_]{2,30}$`, must not be a catalog key in `src/connectors.rs`, and must not be a `connectors.json` id.
*   **`exec.rs`:**
    *   `build_request(manifest, op, args) -> RequestSpec`, which is pure.
    *   `call(cfg, manifest, op, args)`. Its client uses `PublicOnlyResolver`, `no_proxy()`, `Policy::none()`, the host check, a 1 MB cap and a 20-second timeout. It takes the key as an argument, `key: Option<String>`, and adds the header itself. H8w passes `connectors::api_key(cfg, name)`. It applies the `select` filter to the response.
    *   `load_all() -> Vec<Manifest>`. It reads `current_exe()/../../Resources/connectors/` when that folder exists, and otherwise `[harness2] repo` + `/connectors/`.
*   **`mod.rs`:** `tools(manifests, trust) -> Vec<Tool>`.
*   **Tests:**
    *   `manifest_rejects_ip_localhost_and_port`
    *   `manifest_rejects_catalog_key_names`
    *   `read_op_must_be_get`
    *   `path_params_are_percent_encoded`
    *   `unknown_param_is_refused`
    *   `redirect_is_not_followed`
    *   `auth_header_only_to_manifest_hosts`
    *   `response_over_1mb_is_refused`
    *   `select_keeps_only_listed_pointers`

#### Graph 1b, unit H8w: connector wiring (scope `src/**`)

##### 17. Wiring for connectors

*   **`src/connectors.rs`:**
    *   `validate` calls `ext::manifest::host_ok` (from H0), and `validate_endpoint` is deleted.
    *   `list()` merges in the loaded manifests as cards. They are never written to `connectors.json`, and each card carries `source: "heron"` and `trusted`.
    *   `require_custom` becomes `require_known`, which accepts a custom id or a manifest name.
    *   `key_name` becomes `pub(crate)`.
    *   New `api_key(cfg, id)` reads the OS vault only, with no file fallback.
*   **`src/sources.rs`:**
    *   `is_known(name)` accepts `SOURCES` plus any `ext:<name>` for a loaded manifest.
    *   `learn_one` (`:449`) and the disconnect route (`src/server.rs:1561`) call it.
    *   The executor saves a read operation's output as `knowledge` rows under `ext:<name>`, following `src/sources.rs:353`.
*   **`src/server.rs`:** `POST /api/connectors/custom/{id}/trust {trusted: bool}` sets `ext_trust:<id>` to the manifest's SHA-256, or clears it.
*   **`src/app.rs`:** registers `ext::tools` at assembly.
*   **`src/agent.rs`:** the `ConnectorOutput` content entry.
*   **Tests:**
    *   `decision_table_ext_tools`
    *   `ext_output_taints`
    *   `ext_untrusted_until_owner_trusts`
    *   `ext_trust_resets_on_manifest_change`
    *   `manifest_connector_accepts_api_key`
    *   `api_key_reads_vault_only`
    *   `ext_source_memories_start_unconfirmed`
*   **Unit H8t (scope `tests/**`):**
    *   `tests/connector_manifests.rs` holds `every_manifest_validates` and `request_shapes_match`.
    *   The fixture manifest is `tests/fixtures/connectors/echo.toml`, with `echo.test.json`.
    *   `tests/fixtures/api/connectors.json` is re-recorded.
*   **Unit H9 (scope `macos/**`):**
    *   Settings gains a Connectors section in `HeronUI/Views/ConnectorsSettingsView.swift`. It shows the list from `GET /api/connectors`, the key field (the existing `POST|DELETE /api/connectors/custom/{id}/api-key` routes), and the Trust switch (the new route).
    *   `CoreClientProtocol` gains the calls for these routes.
    *   Its tests live in `Tests/HeronUITests/ConnectorsTests.swift`: `test_connectorCardShowsTrustSwitch` and `test_keyFieldIsSecure`.
*   **Unit H7b (scope `docs/**`):** the threat-model section gains the subsection `### Connectors`, and `docs/features/connectors.md` is added.

#### Graph 1c, unit H10: observation core (scope `src/**`)

##### 19. `src/observe/`, `src/sandbox.rs`, `src/server.rs`, `src/app.rs`, `src/memory.rs` (Create/Modify)

*   **`src/observe/sealbox.rs`:**
    *   `SEALBOX_COMMIT` and `SEALBOX_LOCK_SHA256`;
    *   `install()`;
    *   `verify_venv()`, which compares the venv's file-list hash with the one recorded at install;
    *   `master_key()`, from the `config` vault module (`src/config.rs:878`);
    *   `spawn_append()`, `query(sql) -> Value` and `prune(days)`.

    Each process passes the key with `--key-fd 3` over a pipe, and runs under `Profile::Observe`. If `sandbox::wrap` returns `None`, the spawn is refused.
*   **`src/sandbox.rs`:**
    *   New variant `Profile::Observe { dir, venv }`.
    *   Its SBPL starts from the shared prelude (`(allow default)`, and deny reads under `cfg.home`, `src/sandbox.rs:356-357`). It then adds, in this order:
        1.  `(deny network*)`;
        2.  `(deny file-write*)`;
        3.  `(allow file-read* file-write* (subpath dir))`;
        4.  `(allow file-read* (subpath venv))`.
        5.  `(allow file-read* (subpath base))`, where `base` is the `home =` line of `<venv>/pyvenv.cfg`. That line is the uv-managed interpreter's folder.
        6.  `(allow file-read* (literal sealbox_bin))`, the sealbox executable.

        In tests, `sealbox_bin` is the fake script.

        The later rules win, so `dir` and `venv` are reopened after the home deny.
*   **`src/observe/jobs.rs`:** the five queries in §8, the sentence templates, and `memory.save(text, "fact", "observation", 0.4, "observation")`.
    *   **Topic terms:** open task titles from `Store::list_tasks(Some("open"))`, plus confirmed memories with `source IN ('user','onboarding')` and kind `project`. That gives at most 50 terms.
*   **`src/observe/forget.rs`:** `forget_all() -> ForgetReport`. It stops the processes and deletes the vault folder, the vault key, the `observation` memories, and the suggestions with `origin = 'observation'` together with their sessions.
*   **`src/observe/mod.rs`:** ingest (it streams to `append`, and refuses while paused), plus the daily run of jobs and prune. `Harness2::tick` calls it from step 6 (§6), on every Mac with observation on.
*   **`src/server.rs`:** the `/api/observe/*` routes (§11).
*   **`src/memory.rs`:** deletes `observation` memories older than 90 days during the existing housekeeping pass.
*   **`src/agent.rs`:** the `ObservationResult` content entry.
*   **Test doubles:** H10 creates a fake `sealbox` at `src/observe/testdata/fake-sealbox`, a shell script that records its arguments. Tests point to it with `HERON_SEALBOX_BIN`, which only `#[cfg(test)]` code reads.
*   **Tests**, all `#[cfg(target_os = "macos")]`:
    *   `ingest_keeps_nothing_outside_vault`
    *   `ingest_refused_while_paused`
    *   `observe_profile_denies_network`
    *   `observe_spawn_refused_without_seatbelt`
    *   `master_key_passed_by_fd_not_env`
    *   `config_never_contains_sealbox_key`
    *   `venv_hash_mismatch_refuses_spawn`
    *   `workflow_job_requires_three_days`
    *   `topic_job_reads_only_tasks_and_user_memories`
    *   `jobs_make_no_model_call`
    *   `forget_removes_vault_key_memories_and_suggestions`
    *   `workflow_suggestion_session_is_tainted`
    *   `observation_source_is_outside`

#### Graph 1c, unit H11: Mac app, observation (scope `macos/**`)

##### 24. Mac app, observation

*   **`CoreClientProtocol`, `CoreClient`, `PreviewCoreClient` and `ManagedCoreClient`** gain the `/api/observe/*` calls.
*   **`HeronCore/Observers/`** holds `ScreenObserver`, `AppUseObserver`, `BrowserHistoryObserver`, `KeystrokeObserver`, `FileEventsObserver` and `ObservationUploader`.
    *   **Exclusions.** `HeronCore/Observers/Exclusions.swift` is shared by every observer.
    *   **Seams.** The observers take these protocols, so tests can fake them: `SecureInputProbe` (wrapping `IsSecureEventInputEnabled`), `FocusedElementProbe` (the focused element's AX role) and `FrontmostAppProbe`.
*   **`HeronUI/Observation/`** holds `ObservationSettingsView`, `ConsentView` and `ExclusionsView`.
*   **`MenuBarViews.swift`** gets the observing dot and Pause observing.
*   **Tests:** all in `macos/HeronMac/Tests/HeronUITests/ObservationTests.swift`:
    *   `test_everyObserverStartsOff`
    *   `test_consentShownBeforeFirstEnable`
    *   `test_keystrokesSkippedDuringSecureInput`
    *   `test_keystrokesSkippedInSecureTextField`
    *   `test_excludedAppIsNotRecorded`
    *   `test_indicatorShownWhileAnyObserverOn`
    *   `test_pauseStopsAllObservers`

#### Graph 1c, unit H7c: docs (scope `docs/**`)

##### 25c. Docs for 1c

*   The threat model gains `## Observation`. It covers the observers, sealbox, what leaves the vault, and sealbox's shortfalls.
*   New file: `docs/features/observation.md`.

### Verification Plan

*   **Build:** `cargo build --locked`, `swift build --package-path macos/HeronMac`, and `bash packaging/macos/build-app.sh --unsigned --no-dmg --host-arch`.
*   **Tests:**
    *   `cargo test --locked`, `cargo test --locked --test integration`, `cargo test --locked --test connector_manifests`, and `swift test --package-path macos/HeronMac`.
    *   Each graph's `verify` node runs `bash eval/selfheal/check-required.sh <graph>`.
*   **Lint:** `cargo clippy --locked --all-targets -- -D warnings`.
*   **Test quality (testteeth):** `testteeth run --base main --fail-under 70 -q`. Surviving mutants in `fence.rs`, `trust.rs`, `checks.rs`, `src/ext/` and `src/observe/` are killed, not justified. The fence's evidence is its mutation score; there is no separate genesis claim.
*   **Evaluator audit (genesis):** `genesis audit-evaluator eval/selfheal/genesis.yaml --suite json` must return `TRUSTED`.
*   **sealbox's red team** at the pinned commit: `python3 eval/redteam.py`, with 0 leaks.
*   **Replay:** `python3 -I eval/selfheal/run_replay.py` meets the threshold on the owner's Mac.
*   **End to end, once, by hand:**
    *   CUJ H-1 through real CI, with a planted bug in `schedule.rs`;
    *   CUJ H-6 with the fixture service;
    *   CUJ H-7 with App use, then Forget;
    *   a rerun of PT6.

## Work units

The graphs run in sequence, each merged to `main` before the next starts (see [Graph order](#graph-order)). Units in a graph whose scopes share a prefix are ordered by depends-on. In the check column, `\|` stands for a shell `|`.

**Graph 1a: self-healing to PR, and the scout**

| id | what to build | scope | depends on | check | model role |
|---|---|---|---|---|---|
| H0 | Skeleton (Preconditions, unit H0) | `src/**` | none | `cargo test --locked && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib -- is_outside_replaces_both_checks tainted_sessions_counts_up_and_down next_schema_step_adds_only_nullable_columns insert_wrappers_keep_old_signatures assemble_with_tools_filters_registry repair_prompt_block_is_trusted manifest_rejects_ip_localhost_and_port manifest_rejects_catalog_key_names read_op_must_be_get the_schema_cannot_change_without_a_new_version` | hardest tasks: opus high |
| H1 | `src/harness2/` (items 1-10) | `src/harness2/**` | H0 | `cargo test --locked --lib harness2:: && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib -- decision_table_selfheal pasted_report_is_never_auto failure_record_has_no_free_text origin_tainted_failure_is_never_auto fence_denies_views_and_stores events_and_embed_are_fenced fence_denies_harness2_and_gate_script workflow_diff_is_not_pushed redaction_hit_never_pushes private_text_blocks_push ci_without_selfheal_check_is_not_passing unmatched_command_is_denied_in_repair scout_rejects_redirect_off_allowlist report_never_includes_observations record_panic_writes_failure_record` | hardest tasks: opus high |
| H2 | Wiring (items 14-16, inline tests of 18) | `src/**` | H1 | `cargo test --locked --lib --bins && cargo clippy --locked --lib --bins -- -D warnings && bash eval/require-tests.sh --lib -- state_reports_commit harness2_routes_need_token inbox_rows_carry_action inbox_action_starts_trigger owner_report_entry_is_trusted core_exit_env_writes_failure_record` | hardest tasks: opus high |
| H2t | State fixture, integration test, fixture (item 18) | `tests/**` | H2 | `cargo test --locked --test integration && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --test integration -- api_harness2_lists_switches_and_checklist api_state_reports_commit_key && test -s tests/fixtures/api/harness2.json` | feature, refactoring: sonnet high |
| H3 | Replay suite, grader, genesis adapter, required-tests list, gate-script tests (item 20) | `eval/**` | none | `python3 -I -m unittest discover -s eval/tests -t eval -p 'test_selfheal_replay.py' && test "$(grep -c '^## case ' eval/selfheal/cases.md)" -eq 10 && test "$(ls eval/selfheal/cases/*.patch \| wc -l)" -eq 10 && test -s eval/selfheal/required-tests.txt && bash -n eval/selfheal/check-required.sh && genesis audit-evaluator eval/selfheal/genesis.yaml --suite json` | feature, refactoring: sonnet high |
| H4 | `scripts/verify-selfheal.sh` (item 21) | `scripts/**` | H3 | `bash -n scripts/verify-selfheal.sh && python3 -I -m unittest discover -s eval/tests -t eval -p 'test_verify_selfheal.py'` | feature, refactoring: sonnet high |
| H12 | `.github/workflows/selfheal.yml` (item 26) | `.github/**` | H4 | `grep -q "startsWith(github.head_ref, 'heron/selfheal/')" .github/workflows/selfheal.yml && grep -q 'fetch-depth: 0' .github/workflows/selfheal.yml && grep -q 'contents: read' .github/workflows/selfheal.yml && ! grep -q 'secrets\.' .github/workflows/selfheal.yml && grep -q 'verify-selfheal.sh "$(git -C fix merge-base' .github/workflows/selfheal.yml` | feature, refactoring: sonnet high |
| H5 | `build-app.sh` (item 22) | `packaging/macos/**` | none | `bash -n packaging/macos/build-app.sh && ! grep -qE '^[^#]*--deep' packaging/macos/build-app.sh && bash packaging/macos/build-app.sh --unsigned --no-dmg --host-arch && test -d dist/Heron.app/Contents/Resources/connectors && test ! -e dist/Heron-macos-universal.dmg` | feature, refactoring: sonnet high |
| H6 | Self-improvement pane, report sheet, inbox buttons, token field, client and model changes (item 23) | `macos/**` | H2t | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && test "$(swift test list --package-path macos/HeronMac \| grep -c 'SelfImprovementTests')" -eq 9` | feature, refactoring: sonnet high |
| H7a | 1a docs (item 25a) | `docs/**` | H2 | `grep -qx '## Harness2: code Heron writes into itself' docs/engineering/SECURITY_THREAT_MODEL.md && test -s docs/features/self_improvement.md` | judgment and prose: opus high |
| verify-1a | Suite, required tests, test quality, evaluator audit | `scripts/gates/harness2-1a` (check only) | H0, H1, H2, H2t, H3, H4, H5, H6, H7a, H12 | `cargo test --locked && cargo clippy --locked --all-targets -- -D warnings && bash eval/selfheal/check-required.sh 1a && swift test --package-path macos/HeronMac && genesis audit-evaluator eval/selfheal/genesis.yaml --suite json && testteeth run --base main --fail-under 70 -q` | feature, refactoring: sonnet high |

**Graph 1b: connectors**

| id | what to build | scope | depends on | check | model role |
|---|---|---|---|---|---|
| H8 | `src/ext/` (item 11) | `src/ext/**` | none | `cargo test --locked --lib ext:: && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib -- redirect_is_not_followed auth_header_only_to_manifest_hosts path_params_are_percent_encoded response_over_1mb_is_refused select_keeps_only_listed_pointers` | hardest tasks: opus high |
| H8w | Connector wiring (item 17) | `src/**` | H8 | `cargo test --locked --lib --bins && cargo clippy --locked --lib --bins -- -D warnings && bash eval/require-tests.sh --lib -- decision_table_ext_tools ext_output_taints ext_untrusted_until_owner_trusts ext_trust_resets_on_manifest_change manifest_connector_accepts_api_key api_key_reads_vault_only ext_source_memories_start_unconfirmed` | hardest tasks: opus high |
| H8t | Connector harness and fixtures | `tests/**` | H8w | `cargo test --locked --test connector_manifests && cargo test --locked --test integration && bash eval/require-tests.sh --test connector_manifests -- every_manifest_validates request_shapes_match` | feature, refactoring: sonnet high |
| H9 | Connectors section in Settings | `macos/**` | H8t | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && test "$(swift test list --package-path macos/HeronMac \| grep -c 'ConnectorsTests')" -eq 2` | feature, refactoring: sonnet high |
| H7b | 1b docs | `docs/**` | H8w | `grep -q '### Connectors' docs/engineering/SECURITY_THREAT_MODEL.md && test -s docs/features/connectors.md` | judgment and prose: opus high |
| verify-1b | Suite and test quality | `scripts/gates/harness2-1b` (check only) | H8, H8w, H8t, H9, H7b | `cargo test --locked && cargo clippy --locked --all-targets -- -D warnings && bash eval/selfheal/check-required.sh 1b && swift test --package-path macos/HeronMac && testteeth run --base main --fail-under 70 -q` | feature, refactoring: sonnet high |

**Graph 1c: observation**

| id | what to build | scope | depends on | check | model role |
|---|---|---|---|---|---|
| H10 | `src/observe/`, the `Observe` profile, observe routes (item 19) | `src/**` | none | `cargo test --locked --lib --bins && cargo clippy --locked --lib --bins -- -D warnings && bash eval/require-tests.sh --lib -- ingest_keeps_nothing_outside_vault ingest_refused_while_paused observe_profile_denies_network observe_spawn_refused_without_seatbelt master_key_passed_by_fd_not_env config_never_contains_sealbox_key venv_hash_mismatch_refuses_spawn workflow_job_requires_three_days topic_job_reads_only_tasks_and_user_memories jobs_make_no_model_call forget_removes_vault_key_memories_and_suggestions workflow_suggestion_session_is_tainted observation_source_is_outside` | hardest tasks: opus high |
| H10t | Re-recorded fixtures | `tests/**` | H10 | `cargo test --locked --test integration && cargo clippy --locked --all-targets -- -D warnings` | feature, refactoring: sonnet high |
| H11 | Observers, Observation pane, consent, exclusions, indicator (item 24) | `macos/**` | H10t | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && test "$(swift test list --package-path macos/HeronMac \| grep -c 'ObservationTests')" -eq 7` | hardest tasks: opus high |
| H7c | 1c docs (item 25c) | `docs/**` | H10 | `grep -qx '## Observation' docs/engineering/SECURITY_THREAT_MODEL.md && test -s docs/features/observation.md` | judgment and prose: opus high |
| verify-1c | Suite and test quality (the sealbox red team runs by hand) | `scripts/gates/harness2-1c` (check only) | H10, H10t, H11, H7c | `cargo test --locked && cargo clippy --locked --all-targets -- -D warnings && bash eval/selfheal/check-required.sh 1c && swift test --package-path macos/HeronMac && testteeth run --base main --fail-under 70 -q` | feature, refactoring: sonnet high |

**Run notes:**

*   **Sandboxing:** the skein supervisor runs every check that calls `cargo`, `swift`, `build-app.sh`, `testteeth` or `genesis` outside the sandbox (see the repository's memory notes on sandboxed builds).
*   **Steps run by hand.** These need the owner's CLI login, TCC grants, GitHub, or the sealbox checkout. They run after each `verify` node, and their output goes in that graph's PR:
    *   the replay run;
    *   sealbox's red team;
    *   the end-to-end CUJs;
    *   the PT6 rerun.
*   **Timeouts:** `verify` nodes run with `--verify-timeout 3600`.

--------------------------------------------------------------------------------

*Drafted with EGM*
