# Heron v1: production-ready personal agent on macOS

* **Author:** EGM synthesizer (six-lens Goldfish brainstorm)
* **Date:** 2026-11-18

## Overview

Heron v1 aims to be a personal agent that its owner uses every day, and that a few friends can install if they accept written risks. It is macOS only. It has a SwiftUI Mac app on the existing Rust core, with a named agent, avatar, themes and animation. Models are reached by driving each user's own logged-in official CLIs (Claude Code, Codex and others), so Heron is not a model provider and does not resell tokens. Heron's permission checks, untrusted-content marking ("taint") and hash-chained audit log are meant to stay in charge of every action.

**Source.** The concept is in `docs/designs/heron-v1-hypothesis.md`. Six reviewers read it alongside this worktree: design, codebase details, zoom-out, comprehension, implementation readiness and structural. Each reviewer got only:
- the hypothesis path;
- the EGM plugin root;
- the repository paths;
- the owner's four questions.

**Limits of this run.**
- No reviewer could read the `qualification/v1.0-rc1` branch, so this brief cites no qualification finding IDs beyond those in the hypothesis.
- Claims about outside products are marked UNVERIFIED. These include CLI flags, MCP behaviour in each CLI, vendor terms of service and Apple signing rules. Nobody in this run could check them on the web.
- One reviewer also read the untracked research note `docs/designs/research/cli-backend-and-swiftui.md`.

**What I read myself.** I opened these ranges and confirmed what they say:

| File | Lines or item |
|---|---|
| `src/mcp.rs` | 228-289 |
| `src/main.rs` | 744-755 |
| `src/cli_llm.rs` | 140-171 and 1214-1243, plus a search showing no Seatbelt use and the `~/.heron/tmp/cli-*` working folder at 1980-1982 |
| `src/permissions.rs` | 212-256 and 538-543 |
| `src/agent.rs` | 244-299 |
| `src/store.rs` | 1155-1166 |
| Research note | 14-102 |

Every other line reference in this brief comes from a reviewer and is marked "reviewer-reported" where it matters.

**The main finding.** All six reviewers found the same problem: the hypothesis does not say who runs the agent loop. The sentence "Heron serves its own tools to the CLI over MCP" fits two readings:
- **Hermes style.** MCP only advertises tool schemas, and Heron still runs every tool. The hypothesis cites Hermes, so this reading is plausible.
- **CLI owns the loop.** The CLI runs the loop and calls Heron's tools over MCP.

Under the first reading, the hypothesis's safety claim holds. Under the second, it fails, because Heron's controls work only while Heron runs the loop. This is the decision the rest of the plan depends on.

## Analysis

### A. The CLI model connection (owner question 1)

**What ships today.** Reviewers report the following, and the Claude flags I checked agree with it.

- `src/cli_llm.rs` already drives 16 CLIs as plain text models. The registry is at `cli_llm.rs:284-302` (reviewer-reported).
- Claude Code runs as `-p --tools '' --max-turns 1 --permission-mode dontAsk --no-session-persistence --setting-sources '' --disable-slash-commands`. I checked this at `cli_llm.rs:145-163`.
- Each step goes like this (reviewer-reported):
  1. Heron renders the conversation and the tool list into one prompt (`render_prompt`, `cli_llm.rs:733-765`).
  2. The CLI replies with a JSON envelope of text and tool calls (`cli_llm.rs:770`).
  3. Heron runs every call through `Agent::execute_call` (`agent.rs:453-537`).
- `docs/cli-providers.md` describes this as "Heron owns the tool loop".

**The three possible topologies.**

- **(a) Envelope.** This is what ships.
- **(b) Hybrid.** MCP advertises Heron's tools so the model can use native tool calling. The CLI is still limited to one turn and does not run them. Heron captures the tool-use blocks and runs them in its own loop.
  - The code already strips an `mcp__heron__` prefix from stray tool-use blocks (`cli_llm.rs:1542-1563`, reviewer-reported).
  - The research note describes this as the Hermes plugin's pattern and calls it "the pattern to copy" (note line 22).
- **(c) CLI owns the loop.** The CLI is the MCP client and runs Heron's tools across many turns.
  - Heron's MCP handler becomes the only gate.
  - The research note's Claude Code section (line 50) and its final recommendation (line 102) describe this configuration: `--tools "" --strict-mcp-config --mcp-config <your axum MCP endpoint>`. Line 50 notes "the CLI will still execute MCP tool calls itself".
  - So the note points to (b) in one place and to a (c)-style configuration in another. It does not settle the choice.

**What breaks under (c).**

1. **Controls that live in Heron's loop stop applying.** Reviewer-reported from `agent.rs:306-316`, `337-430`, `523-526` and `540-553`, these are:
   - the step limit (40 by default);
   - the token budget and model routing;
   - compaction, which hides assistant text once a conversation is tainted;
   - repeat-call detection;
   - sub-agent depth limits;
   - the list of addresses the user typed, which the egress check uses;
   - setting the conversation's taint flag;
   - the stored transcript that history, resume and undo rely on.

   Heron would see only individual tool calls.
2. **The existing MCP server is a second, weaker gate.** I checked this at `mcp.rs:231-289` and `main.rs:750-753`.
   - It does not go through `execute_call`.
   - It decides every call with taint hard-coded to `false` (`mcp.rs:266`), even though its own comment says "What another program sends is outside content" (`mcp.rs:261-263`).
   - It refuses anything that needs approval (`mcp.rs:274`), because `heron mcp-serve` builds the agent with a `DenyAll` approver.
   - It logs every call under one fixed session id, `"mcp"` (`main.rs:751-752`).
   - It never marks tool output as untrusted (`mcp.rs:269-271`).

   Today it serves only 10 memory, task and schedule tools (`mcp.rs:232`). If it grew to cover files, shell and web, the step-1 root-cause fixes, which land in `execute_call`, would be skipped on this path.
3. **Approvals collide with timeouts.** In (c), one CLI process waits inside a tool call while a person decides. The timeouts disagree (reviewer-reported):
   - the CLI process is killed after 300 s (`config.rs:235`);
   - desktop approvals wait up to 900 s (`server.rs:416`);
   - Telegram approvals wait up to 600 s (`telegram.rs:270`).

   The kill message points the user at logging in again (`cli_llm.rs:1898`). Each CLI may also apply its own MCP tool-call timeout or its own permission layer for MCP tools (UNVERIFIED).
4. **Deterministic testing is lost.** The record and replay layer wraps the model interface (`llm.rs:478-511`, reviewer-reported). Under (c) there is no model boundary to record at, so EVE scores would vary with CLI versions and use up subscription quota.

**"Built-in tools switched off" is false for several named CLIs, by the repository's own notes.**

- **Codex.** The comment at `cli_llm.rs:165-170`, which I read, says "Codex 0.164 keeps its shell tool even under read-only, so the temp-dir cwd (never the workspace) is what actually bounds it." This is a repo comment that I have not checked against a binary.
- **Cline and Goose** cannot turn tools off (reviewer-reported, `cli_llm.rs:47-50`).
- **Cursor and OpenCode** use read-only modes, not tools-off (reviewer-reported, `cli_llm.rs:183-215`).
- **Six providers** have flags never checked against a live binary (reviewer-reported, `docs/cli-providers.md:112-115`).

**The CLI child process sits outside Heron's protections.** I checked this.

- It inherits Heron's whole environment, minus a few API-key names for three providers (`cli_llm.rs:1218-1233`). Heron's shell tool passes only an allowlist (reviewer-reported, `tools/shell.rs:49-56`).
- Its working folder is under `~/.heron/tmp` (`cli_llm.rs:1980-1982`), inside the folder that holds Heron's database and config.
- `cli_llm.rs` contains no Seatbelt or sandbox call apart from Codex's own `--sandbox` flag.

**Hidden cost.** Every step spawns a fresh process and re-sends the full conversation and tool schemas (reviewer-reported).

- With the default 40 steps and a 100k-token context, one long turn can send millions of input tokens.
- Background jobs draw on the same subscription.
- The research note reports that `claude -p` usage counts against the subscription's 5-hour usage window about 1.7 to 2 times faster than interactive use (note line 26, UNVERIFIED).
- The real motive for MCP may be cost and latency, but nobody has measured it.

**Version skew** (reviewer-reported).

- Flags for each CLI are maintained by hand.
- Only Claude has auto-update disabled. I checked this at `cli_llm.rs:1240-1243`. Whether the other CLIs update themselves is UNVERIFIED.
- `--version` is read for the status display but never compared against a tested range.
- Error classification matches words in stderr. A rate-limit message that mentions OAuth would be reported as "not logged in".
- Nothing fails over to another provider when a CLI's usage window runs out.

**Terms of service.**

- The repository calls subscription logins in third-party tools "a grey area" (`docs/cli-providers.md:192-197`, reviewer-reported).
- The research note quotes Anthropic's legal page (note lines 75-82, UNVERIFIED in this run):
  - developers building products should use API keys;
  - third parties may not route requests through plan credentials on behalf of their users;
  - there is a carve-out for an end user signing in to the unmodified Claude Code binary with their own subscription.

  The note's own reading is that a local app driving the unmodified binary with the user's own login fits the carve-out, but that this is ambiguous.
- Reviewers see the highest risk in:
  - unattended use through a consumer subscription (scheduled jobs, reflection, proactive work);
  - handing builds to friends, which moves toward distribution.
- "Heron does not resell tokens" does not answer whether this use is allowed.
- Reviewers agreed on three mitigations:
  - each friend uses their own login on their own Mac;
  - the API-key HTTP provider stays fully supported;
  - background features stay off by default on subscription providers.

### B. The SwiftUI boundary and keeping Slint (owner question 2)

**Boundary.** All six reviewers recommend the local HTTP API rather than FFI. Their reasons (reviewer-reported):

- The Slint app already starts the axum server in its own process and talks to it only over HTTP with a per-launch token (`src/bin/desktop/main.rs:36-67`, `bridge.rs:170-211`).
- The web UI and EVE use the same API, which has about 45 routes (`server.rs:117-161`).
- FFI would add a second contract that EVE does not test, and would mean carrying tokio async calls and approval callbacks across the language boundary. There is no `cdylib` target today.

The shape they suggest:

- Ship the Rust core as a signed helper binary inside the app bundle.
- Swift launches it and receives the port and token over a pipe, not in a URL. Today the token goes in a query string (`server.rs:190`).
- A Unix socket would be preferable to a TCP port.

**API gaps for a client in a separate process** (reviewer-reported).

- **Dropped stream.** If the event stream closes, the run is cancelled, with no way to reattach (`server.rs:389-393`). Sleep, wake or a view reload kills the reply.
- **Approval delivery.** Approvals travel on the chat request's own stream. One is denied if it cannot be delivered within 15 s (`server.rs:411-416`).
- **No push channel.** There is no global push channel. Slint polls every 30 s.
- **No contract.** There is no API version or schema. Event shapes exist only in code (`server.rs:425-434`).

**What it costs to keep Slint alongside SwiftUI.**

- **Duplicated view logic.** About 2,550 lines of view logic in `src/ui/chat.rs` and `src/ui/views.rs` cannot be reused from Swift. This includes the approval danger wording, so security-relevant UI will drift between two toolkits (reviewer-reported).
- **Two cores on one data folder.** Each app hosts its own core in-process (reviewer-reported).
  - What coordinates already: background work and the phone gateway, through `background.lock` and `telegram.lock`.
  - What diverges: permission mode and per-conversation grants live in memory per process, and taint is read once when a conversation loads.
  - What I checked in `store.rs:1157-1166`: the audit append reads the previous hash, then inserts, with no transaction, and it ignores insert errors (`let _ =`). Two writers can fork the chain, so `heron audit` would report tampering that did not happen. A failed insert leaves an action with no record.
  - Owner decision C35-8 (phone turns inherit the desktop mode) becomes ambiguous when two desktops are open with different modes.
- **Schema lockstep.** The store refuses a database written by a newer schema (reviewer-reported, `store.rs:519-525`), so both apps must ship from the same commit.
- **A conflict with step 2.** "Slint gets no new UI work" means features added in step 2 have no screen until SwiftUI exists.
- **CI.** There is no Swift job, and the mutation gate does not cover Swift.

**Reviewers' recommendation.**

- One core process owns `heron.db`, and both UIs are HTTP clients of it.
- Make audit appends atomic and report insert failures.
- Persist the permission mode.
- Version the API and the event stream, with contract tests.
- Freeze Slint to API-compatibility fixes, with a stated retirement point, for example when SwiftUI reaches parity on chat, approvals and settings.

### C. The work order (owner question 3)

Reviewers found five places where the order fails.

1. **The largest change has no step.** The topology choice in section A decides where the step-1 taint fix must go: `execute_call` and the system prompt, or an MCP handler. If step 1 comes first, it may have to be done twice.
2. **Features before hardening, with automatic merges.** Each new tool, source or channel needs four things:
   - a risk class;
   - an untrusted-output flag;
   - a decision-table row;
   - an injection test.

   Adding features before the remaining P0 and P1 fixes keeps the hardening target growing, and leaves the qualification rerun with no freeze point. "Nothing cut" is an owner constraint. The reviewers argue for ordering and a freeze, not for deleting features.
3. **SwiftUI before the API is stable.** Hardening will likely change the approval and streaming shapes, so the Swift client would have to keep changing.
4. **Signing comes last.** All of these risks are UNVERIFIED:
   - Notarization requires the hardened runtime, which may affect spawning npm-installed CLIs, `sandbox-exec` and Chrome over CDP.
   - Keychain items written by today's ad-hoc-signed build may prompt, or be unreadable, under a Developer ID identity.
   - Privacy permissions granted to the old build may reset.
   - The bundled helper binary needs its own signature.

   Developer ID enrolment is also an external dependency that takes time.
5. **The auto-merge gate has no security trigger** (reviewer-reported).
   - CI runs `cargo test` and a build only.
   - Codex review is a model, so it can share blind spots with the model that writes the code.
   - A loop that edits `the_decision_table` to match new code still passes.
   - "Security calls go to the owner" depends on the loop recognising a call as a security call.

**Step 1 cannot start as written.**

**Root cause 1: taint.** Taint is set only from untrusted tool output and from event-triggered jobs (reviewer-reported, `agent.rs:523-526`, `app.rs:222-224`, `260-263`).

I checked that these reach the system prompt without setting taint (`agent.rs:248-297`):
- unconfirmed memories taken from the user's email or documents;
- open task titles;
- ambient file-name awareness;
- skill descriptions.

Reviewers also reported, and I did not check:
- piped stdin (`main.rs:659-664`);
- Telegram text (`telegram.rs:444`);
- CLI prompt flattening, where a tool result containing a line that starts "User:" looks like a user turn (`cli_llm.rs:733-765`).

For each source, step 1 has to pick a rule: taint the conversation, remove the text from the prompt, or fetch it on demand. That choice has a usability cost. If recalling any unconfirmed memory taints the conversation, most conversations become tainted and every write asks for approval. That pushes down both EVE scores and daily use.

**Calendar text.** This is already decided (O26-1): calendar text stays out of system prompts and is looked up on demand through a path that taints. This brief does not reopen the decision. In this worktree, calendar text is still placed in the system prompt (`agent.rs:285-291`, which I checked), so the decision is not implemented here yet.

**Root cause 2: secret reads.** The secret-read pattern at `permissions.rs:541`, which I checked, names `.ssh`, `.aws`, `.gnupg`, `.kube`, `.docker`, gcloud, `.env` files, key files, files ending in `credentials` or `credentials.json`, `.pem`, `.key` and Heron's own files. It names no CLI config folder such as `~/.codex`, `~/.cursor` or `~/.config/opencode`. Reviewers report that the sandbox's hidden-folder list (`sandbox.rs:68`) has the same gap. Where each CLI stores its tokens is UNVERIFIED. The research note says `~/.codex/auth.json` "contains access tokens" (note line 61).

In `decide_in` (`permissions.rs:219-256`, checked), the checks run in this order:
1. secrets ask;
2. when the conversation is tainted, anything that changes state or sends data out asks;
3. `LocalRead` is allowed;
4. `NetworkEgress`, meaning the model sends out something it wrote, is allowed in every mode.

That makes this chain possible:
1. Text is injected through a prompt path that does not set taint.
2. The model reads a CLI token file whose path is not on the list. This is allowed.
3. The model fetches an address built from the token. This is also allowed, because the conversation is not tainted.

Whether a model would comply is inference. What the code does show is that the subscription logins this plan depends on are not named by the current secret gate.

What replaces the denylist is still open, for example readable allowlisted folders, with everything else in home treated as sensitive. That choice also has to settle three things: the README's promise that reads are allowed, existing "always allow" rules, and new decision-table rows.

### D. Success definition (owner question 4)

**Problems with the provisional measure.**

- **Too coarse.** 85% of about 10 tasks really means 9 of 10, so a single flaky run fails it.
- **Not defined.** The tasks are not named. Provider, CLI version, model and permission mode are not pinned. Nothing says how EVE answers approvals.
- **Wrong surface.** EVE drives the HTTP API, so it never touches the SwiftUI app.
- **No baseline.** Step 1 will add approval prompts that may lower the score.
- **Daily use is undefined.** "Instead of their current tool" names no tool, and "daily" has no definition. There is no telemetry by design, so daily use has to be counted locally.
- **Self-grading.** The qualification rerun is partly self-graded if the same loop that wrote the fixes also runs it. A READY verdict can be reached by reclassifying findings as accepted risks, and there is no cap on that.
- **Friends are missing.** Friends appear in the goal but not in the measure.

**Proposed definition.** This merges the six reviewers' proposals. Commit all thresholds and task lists before work starts, and check every gate on one tagged commit.

1. **Security gate.**
   - A rerun of the frozen v1.0-rc1 rubric, by a reviewer who did not write the fixes, finds 0 open P0 and 0 open P1. The only exceptions are entries in an owner-signed accepted-risk register, capped at 3 new entries.
   - Every v1.0 P0 and P1 has a regression test that fails on the old commit.
   - Two named tests pass:
     - every system-prompt section is either trusted or taints the conversation;
     - reading each supported CLI's config folder asks first, through `read_file` and through the sandbox.
   - An injection suite with at least one case per entry path produces 0 changes or outbound sends without approval. The entry paths are web, email, calendar, memories, Telegram, MCP output, piped stdin and CLI output. The design reviewer suggested 20 cases.
   - `heron audit` reports the chain intact afterwards.
2. **Task gate.**
   - `docs/eval/core-tasks.md` names exactly 10 tasks. Each has a pass rule a machine can check: file contents, a memory, task or schedule row, an audit entry, or an expected refusal.
   - One task is a page with a hidden instruction. That instruction must never be acted on, in any run.
   - Each task runs 3 times through EVE on claude-cli, at a pinned CLI version and model, in `ask` mode. A script approves the expected actions and denies everything else.
   - Pass means at least 26 of 30 runs (87%) and no task below 2 of 3.
   - Run the same suite on the API-key HTTP path to separate Heron failures from CLI failures. Run it on codex-cli for information unless codex-cli becomes Tier 1.
   - Take a baseline on current main before step 1. Report median latency and tokens per task.
3. **Owner-use gate.**
   - Name the tool being replaced.
   - Over 14 consecutive days, at least 12 have a conversation the owner started that includes a tool call. Count these from the local sessions table, excluding background sessions.
   - Keep a one-line log each time the owner falls back to the old tool for something Heron should do. At most 3 of those fallbacks may be caused by Heron failures.
   - Zero severity-1 incidents: an unapproved state change, data loss, or an exposed secret.
4. **Release and friends gate.**
   - The notarized DMG installs on a clean Mac without the right-click workaround.
   - An upgrade from v0.3.1 keeps the data, the Keychain secrets, the allow rules and an intact audit chain.
   - One rollback through `heron restore` has been rehearsed.
   - At least 2 friends install from the written guide, using their own CLI login, and reach a first reply that used a tool within 30 minutes without help.
   - Each friend answers one permission prompt correctly and has accepted a one-page risk notice that lists each accepted risk by ID.
5. **Provider robustness gate.**
   - Scripted faults each produce a distinct message with a fix action. The faults are: CLI missing, logged out, rate-limited, untested version, and an approval left unanswered past its limit.
   - A scheduled contract test runs against the latest releases of the supported CLIs.

The alternative sizing reviewers raised is 10 tasks with 5 runs each. Pass would be at least 43 of 50, with no task below 3 of 5. That gives a clearer per-task signal, but costs more subscription quota.

### E. Other findings (reviewer-reported)

- **More than one SQLite file.** "One SQLite file" is not accurate: `heron.db`, `channels.db` and `proactivity-events.db` all exist. Backup, restore, upgrade and the secret gate must cover all three.
- **macOS only, but not in CI or the README.** CI and the README still cover Windows and Linux. The plan does not say whether existing Windows and Linux beta users get updates.
- **Observability.** There is no logging crate. Audit rows have no run id, provider or CLI version, so an action driven through a CLI cannot be traced back to its turn.
- **Terms with more than one meaning.**
  - "Session" means the CLI login in the hypothesis, but a conversation in the code.
  - "Control plane" means different things for T3 Code and Hermes.
  - "Muse" is both the design inspiration and a CLI provider id (`muse-cli`).
  - "Production-ready" for friends conflicts with the README's stated beta limits.
- **Friend setup is not written down.** A friend needs:
  - Node;
  - their own paid subscription;
  - a Terminal login.

  Also, the CLI binary search misses nvm, fnm and mise installs, so an app started from Finder may report a working CLI as missing.
- **Scope.** Heron's distinct value is its policy layer: taint, approvals, the audit chain and personal memory. Other tools already drive CLIs (UNVERIFIED for each product). The zoom-out reviewer advises keeping the CLI layer thin and spending effort on the policy layer.

## Goals vs. Non-Goals

* **In-Scope Goals:** these come from the hypothesis.
  - A macOS-only v1 that the owner uses daily and that friends can install.
  - Models come through the user's own logged-in CLIs.
  - Heron's permission checks, taint and audit decide every action. The proposed form of this is a testable rule: one gate function handles every tool call, and a tested list covers every way content enters a turn.
  - The two root-cause fixes.
  - A SwiftUI Mac app on the Rust core.
  - The remaining P0 and P1 findings closed.
  - A signed, notarized build.
  - All new features the owner wants. None are cut.
  - A success definition with fixed thresholds. Analysis D has a proposal.
* **Proposed In-Scope Goals:** these come from reviewers, for the owner to accept or reject.
  - The API-key HTTP provider stays fully supported alongside the CLIs.
  - Each friend uses their own CLI login on their own Mac.
  - The root-cause fixes also cover CLI prompt rendering, CLI output, CLI credential folders and all three databases.
  - Each new feature meets a security definition of done.
* **Out-of-Scope Non-Goals:** these come from the hypothesis and owner decisions.
  - Cloud deployment, telemetry, acting as a model provider, and reselling tokens.
  - New Slint UI work.
  - Reopening C35-8 (phone turns inherit the desktop mode) or O26-1 (calendar looked up on demand through a path that taints).
* **Proposed Non-Goals:** for the owner to decide. Items stay built and are not cut.
  - Qualifying Windows and Linux for v1. They are labelled unqualified.
  - Counting experimental CLI providers toward the READY verdict or the task gate. They stay labelled experimental.
  - Mac App Store distribution and App Sandbox, which are likely incompatible with spawning user CLIs (UNVERIFIED).

## Key Decisions & Staging for Next Steps

These decisions are open for the owner. Each lists the options and what the reviewers recommend. This brief settles none of them.

1. **Model-connection topology.** Decide this before step 1. The options are (a) envelope, (b) hybrid with MCP schemas only, or (c) the CLI owns the loop. Section A describes all three.
   - **Recommendation.** Use (a) as the qualified v1 default. Run (b) as a flagged experiment on claude-cli, and promote it only if it beats (a) on tokens per task and task success.
   - **Before (c) is possible**, four things are needed:
     - an MCP endpoint inside the running core, bound to one conversation by a single-use credential;
     - every call routed through `execute_call`;
     - approvals that can hold a call open;
     - a written list of where each loop control moves.
   - **Per-CLI spike.** Every check below is UNVERIFIED today:
     - Heron's tools can be called while built-in tools are off.
     - A prompt that asks for a built-in shell, edit or fetch fails. Use a canary file to prove it.
     - The user's global config, MCP servers and instruction files are not loaded.
     - An MCP call can stay pending for 15 minutes.
     - Cancelling kills the process tree and clears the pending approval.
     - Usage is still reported.
     - Server-side vendor tools are off.
2. **SwiftUI integration boundary.** The options are local HTTP or FFI.
   - **Recommendation (all six reviewers):** HTTP, with the Rust core as a signed helper process and the token passed over a pipe.
   - **Needed before Swift work starts:** resumable event streams, approvals decoupled from the chat stream, and a versioned API.
3. **Tier 1 providers.**
   - **Recommendation:** claude-cli at a pinned version range. Add codex-cli only if it passes the tools-off canary with its shell contained.
   - **What Tier 1 means:**
     - the CLI runs under the Seatbelt profile;
     - it gets an allowlisted environment;
     - its working folder is outside `~/.heron`;
     - its version is checked at startup and recorded on every turn.
   - **The other 14 providers** stay built and experimental.
4. **Terms-of-service stance.** This is a product-policy call for the owner.
   - **Before builds go to friends:** read each vendor's current consumer terms and record a decision per provider.
   - **Recommendation:** by default, no unattended background work through a subscription login.
5. **Root-cause decision records.** Write one per root cause.
   - **Root cause 1:**
     - the rule for each prompt source except calendar, which O26-1 has decided: taint, remove, or fetch on demand;
     - whether existing sessions are re-tainted.
   - **Root cause 2:**
     - the read model that replaces the denylist;
     - how existing allow rules migrate;
     - the exact new decision-table rows.
6. **Two-UI model.**
   - **Options:** one shared core process that both apps attach to, or enforce a single running instance.
   - **Also decide:** when Slint is retired.
7. **Work order.** Three options:
   - **The hypothesis order:** features in step 2.
   - **Option X, hardening and contract first.** The implementation-readiness, structural, design and comprehension reviewers converged on this:
     - Step 0, decisions and long-lead items:
       - the topology and its spike;
       - the Tier 1 providers;
       - Developer ID enrolment;
       - a numbered feature list and a freeze date;
       - owner-approval paths, listed after this option;
       - the EVE task file and a baseline run;
       - a signed, notarized build of the current app that spawns a CLI and a sandboxed command on a clean Mac.
     - Step 1: both root causes, in a gate layer that every model connection shares, then an EVE rerun.
     - Step 2: the remaining P0 and P1 findings.
     - Step 3: the core contract: a single core, a versioned API, resumable events, a persisted permission mode and atomic audit appends.
     - Step 4: SwiftUI, signed from its first commit.
     - Step 5: features behind switches, each meeting the security definition of done, frozen at the release-candidate date. Late features move to v1.1 and are not cut.
     - Step 6: the independent rerun, the 14-day trial, then the friends release.
   - **Option Y, owner-ready first.** The zoom-out reviewer proposed this:
     - Stage 1: claude-cli as Tier 1, the root-cause fixes and the P0 findings, on the existing Slint app, plus a signed, notarized build. Start the daily-use clock.
     - Stage 2: the P1 findings run alongside features. Trust-boundary features wait until the P0 findings are closed. Presentation-only and read-only features can go earlier. Then a scope freeze and an owner-ready rerun.
     - Stage 3: SwiftUI on the HTTP API, then the friends-ready gate.
     - Stage 4: the MCP experiment and promoting more CLIs.

     This option splits the success definition into an owner-ready gate (sections D.1 to D.3) and a friends-ready gate (sections D.4 and D.5).
   - **In both options**, PRs that touch these paths need owner approval before merge:
     - `permissions.rs`;
     - the gating in `agent.rs`;
     - `sandbox.rs`, `mcp.rs` and `cli_llm.rs`;
     - server authentication;
     - `telegram.rs`;
     - the audit and schema code in `store.rs`;
     - `the_decision_table`.
8. **Success definition.** Accept, change or reject the five gates in section D. Three choices are needed: 3 or 5 runs per task, the name of the tool being replaced, and the cap on accepted risks.

**Next step.** Once decisions 1, 2, 3, 7 and 8 are answered, run `/egm-draft` with this brief as input. The design doc's Work units table can follow the structural reviewer's split:

| Unit | What it covers | Depends on |
|---|---|---|
| U0 | Decisions and spikes | none |
| U1 | Trust gate and secret allowlist | U0 |
| U2 | Provider adapters and version contract | U0, U1 |
| U3 | Core contract | U0 |
| U4 | SwiftUI client | U3 |
| U5 | Features | U1, U3 |
| U6 | Remaining hardening | U1 |
| U7 | Release pipeline | begins in U0; final build waits for U6 and the success measure |

## Owner decisions (2026-11-18)

The owner answered decisions 1, 2, 3, 4 and 7 above. Decisions 5, 6 and 8 move to `/egm-draft`.

1. **Model-connection topology: experiment first.** Before choosing a topology, run a comparison spike on the same task set:
   - (a) the envelope, which is what ships today;
   - (b) the hybrid, where MCP carries schemas only.

   Compare output quality, task success, tokens per task and latency. Run (c), where the CLI owns the loop, only as a measurement: it must not ship until the four preconditions listed under decision 1 are met. The topology is chosen from the spike's numbers.
2. **SwiftUI boundary: local HTTP API**, with the Rust core as a signed helper process. Accepted as recommended.
3. **Tier 1 providers:** claude-cli, opencode-cli, cline-cli and codex-cli, plus antigravity-cli or gemini-cli, whichever qualifies.
   - Each must pass the same per-CLI spike before it counts as Tier 1:
     - the tools-off canary;
     - the shell contained;
     - an allowlisted environment;
     - a working folder outside `~/.heron`;
     - a pinned version range.
   - A provider that fails stays experimental.
   - **Open cost:** the full task gate (30 runs) on all five multiplies subscription quota use. The draft should size this. One option is the full gate on claude-cli and a reduced gate on the others.
4. **Terms-of-service stance: research preview.** Heron ships to others as a research preview that is clearly marked as risky:
   - a first-run risk notice that each user must accept;
   - a matching banner in the README and on the download page;
   - a "Research preview" label in the app.

   The notice lists each accepted risk by ID, including C35-8. The default stays: no unattended background work through a subscription login.
5. **Work order: Option X** (hardening and contract first), as listed under decision 7. Features land behind switches. Features that miss the freeze move to v1.1 rather than being cut.
