# Stage 5 Zoom-Out Review: heron-v1.md

**Target:** `docs/designs/heron-v1.md`, lines 1-711, at commit 55b33ad.
**Reviewer:** `egm-zoom-out`, read only.
**Count:** 1 Blocker, 10 Major, 6 Nit.
**Checks:** the main finding's code claims (`sandbox.rs`, `app.rs` `run_parked`, `build-app.sh`, `release.yml`, `background.lock`) were spot-checked against the code in this worktree, and they hold.

**Context sufficiency:** passes. Overall the design goes after root causes, keeps Heron as the owner of the agent loop, and reuses the HTTP API. The findings below are about where it falls short of its own stated aims, or rebuilds things that already exist.

## Blocker

### The allowlist is enforced only in read_file, and the OS sandbox stays a denylist - Severity: Blocker
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** G2; Technical Plan §1 "A readable-locations allowlist"; U1 items 5 and 9.
* **Discrepancy Description**: Only the `Refused` list goes into Seatbelt. The shell profile is `(allow default)` with a `deny file-read*` line per hidden path (`sandbox.rs:101-108`), so the `Ask` tier ("everything else under `$HOME`") stays readable by any sandboxed command. In `auto` mode, `ProcessExec` runs without asking, so a command such as `python -c "open(...)"` reads `Ask` paths freely. The claim that "`read_file` and the sandboxed shell agree" is false for the `Ask` tier.
* **Rationale & Recommendation**:
  * **Option (a):** give the shell profile deny-default reads under `$HOME`, using the `ReadRoots` allowed list. That is the same profile shape the design already proposes for the CLI child. `Ask` paths then fail in the sandbox, and the model uses `read_file`, which can ask.
  * **Option (b):** keep `(allow default)`, reword G2 and §1 so they claim only the `Refused` tier at the OS layer, and record the gap as an accepted risk.
  * Either way, test deny-default Seatbelt reads in the U2 spike.

## Major

### "One gate" misses an execution site and adds a context type that already exists - Severity: Major
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** §1 "Today" table and "Change"; U1 item 5 `Gate`; U1 M-7.
* **Discrepancy Description**: Three places execute tools, not two:
  * `agent.rs`, through `decide_in` with real taint;
  * `mcp.rs:269`, through `decide_in(..., false)`;
  * `app.rs` `run_parked`, through the taint-free `decide()` at `app.rs:397`.

  `ToolCtx` (`tools/mod.rs:21-28`) already carries `session_id` and the shared `tainted` flag, so a new `SessionCtx` and a `Gate` struct that only wraps `decide_in` are extra layers.
* **Rationale & Recommendation**:
  * Delete `Policy::decide()` and the caller-supplied `tainted: bool` parameter.
  * Make the one decision function read taint from `&ToolCtx`.
  * MCP becomes a `ToolCtx` whose taint flag starts set.
  * M-7 then follows from the root fix instead of needing its own patch.

### Resumable `/events` doesn't fix the root problem: the run still dies with the `/chat` stream - Severity: Major
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** §3 events and approvals; U3 `server.rs`; the `api_events_resume_from_sequence` test.
* **Discrepancy Description**: A run aborts in three cases:
  * when `ChatEventStream` is dropped (`server.rs:373-393`);
  * when its queue fills (`server.rs:477-491`);
  * when `WebApprover` can't push an approval onto the same stream within 15 s, in which case it denies the approval (`server.rs:409-414`).

  A client that reconnects to `/events` would replay a run that was already cancelled. The design also leaves open whether the replay buffer is kept in memory or in the database.
* **Rationale & Recommendation**:
  * `POST /chat` starts a run and returns `{session_id, run_id}`.
  * Runs live independently of any connection, and only `/cancel` stops one.
  * All events, approvals included, go out on one bus that `/events` streams. `/chat` SSE becomes a filtered view of it for Slint and the web UI.
  * The replay buffer is a bounded in-memory ring.
  * A test checks that a reply survives a client disconnect.

### The process model differs from the accepted boundary and has no port discovery - Severity: Major
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** the diagram; §3 token handoff; §4 packaging; U3 and U4 helper.
* **Discrepancy Description**: The design registers the core as an `SMAppService` LaunchAgent with a token file. It doesn't record that it moved away from the pipe approach in Alternatives. The core binds a random port (`server::start(app, "127.0.0.1", 0)`), and nothing says how the Swift app learns it. The menu bar extra already keeps the app alive. Scheduled work through a subscription login is off by default (Q6).
* **Rationale & Recommendation**:
  * **Simplest for v1:** the Swift app launches the bundled core as a child process and reads `{port, token}` from a pipe, and the LaunchAgent is deferred.
  * **If the LaunchAgent stays:** use one discovery file holding the port and the token, and define how the app handles a stale file and restarts the core after an update.

### The bundle executables `Heron` and `heron` collide on a case-insensitive filesystem - Severity: Major
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** the diagram; §4 packaging.
* **Discrepancy Description**: `packaging/macos/build-app.sh:17-25` records that v0.3.0 shipped a broken bundle for exactly this reason, and guards against it with `cmp -s`. The design repeats the mistake.
* **Rationale & Recommendation**: Ship the core as `Contents/MacOS/heron-cli` or `Contents/Helpers/heron-core`, and keep the guard.

### The release pipeline rebuilds the existing build script and workflow - Severity: Major
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** §6 Release CI; U7 and U7b.
* **Discrepancy Description**: Two pieces already exist:
  * `packaging/macos/build-app.sh` builds the universal app, signs it and makes `dist/Heron-macos-universal.dmg`;
  * `.github/workflows/release.yml` has a `macos` job and a `publish` job, which writes `SHA256SUMS.txt`. The README installers check against it.

  A second workflow would build twice, and it would change the artifact name the installers expect.
* **Rationale & Recommendation**:
  * Extend `build-app.sh` with the Xcode build, notarization and stapling behind `--sign` and `--unsigned`.
  * Change the `macos` job in `release.yml`.
  * Add the unsigned PR build and the upgrade test to `ci.yml`.
  * Keep the artifact name and the checksum flow.

### Two-UI machinery is built ahead of Q3, and `core.lock` duplicates `background.lock` - Severity: Major
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** the diagram; §3 "One core"; the non-goal of no new Slint work; U3 `app.rs`; the attach test.
* **Discrepancy Description**:
  * Giving the Slint app an attach path is new Slint work, which contradicts the non-goal.
  * v1 is macOS-only, and the Swift app is the qualified UI there.
  * `background.lock` (`App::own_background_work`, `app.rs:670-688`) already gives single-owner coordination on `main`.
  * The design doesn't say what CLI subcommands do while a core is running.
* **Rationale & Recommendation**:
  * Drop the Slint attach path for v1.
  * Make "SwiftUI is the only macOS UI" the proposed answer to Q3.
  * Reuse the `background.lock` pattern.
  * State how CLI subcommands behave while a core runs.

### Open questions are answered inside the plan - Severity: Major
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** Q1-Q3; §1; §3; U1 items 5 and 7; Preconditions.
* **Discrepancy Description**:
  * §3 settles Q3.
  * U1 settles the Q2 migration.
  * §1 cites a Q2 description that doesn't exist.

  A reader can't tell which parts are proposals for the owner and which are commitments.
* **Rationale & Recommendation**: Give each open question a recommended answer, mark the plan text "proposed, pending Q-n", and describe the Q2 migration in place.

### Tainting every workspace read is a major policy change buried at file level - Severity: Major
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** U1 `src/tools/fs.rs`; §1 (absent); Alternatives (absent).
* **Discrepancy Description**: Marking every workspace file read as untrusted changes a lot:
  * any conversation that reads a file becomes tainted;
  * every later write then asks;
  * conversation-scoped grants and standing allow rules stop applying.

  This reverses the README contract ("Reading: allowed") and its stated known limit. It probably lowers the G7 and G9 scores.
* **Rationale & Recommendation**:
  * Raise it to §1 and Q1, and weigh the narrower options:
    * taint only files not written by Heron or the user in this session;
    * taint only files outside a trusted root;
    * rely on M-1's egress and credential-read asks.
  * Measure the effect against the U0 baseline before making it the default.

### Topology spike code goes into production paths, and its removal is coupled to U3 - Severity: Major
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** §2 topology flag; U2 `cli_llm.rs` and `main.rs`; the U2, U2s and U3 rows.
* **Discrepancy Description**: The spike ships a flag, a hidden subcommand and hybrid code to `main`, and leaves the cleanup to the API-contract unit. Two existing pieces could be reused instead:
  * `mcp::serve` already runs a stdio JSON-RPC loop with `tools/list` (`mcp.rs:228-289`);
  * `cli_llm.rs:1680-1690` already maps native `tool_use` blocks.
* **Rationale & Recommendation**:
  * Run the spike on a branch or behind a `topology-spike` cargo feature, and merge only the winner, in U2.
  * Make the schema stub a "refuse every call" option of `mcp::serve`.
  * Take the cleanup out of U3.

## Nit

### `[features]` scaffolding is built before any feature exists - Severity: Nit
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** §5; U5.
* **Discrepancy Description**: Per-ability switches already exist (`/tools/{name}` and `set_tool`), and Q8 has no feature list yet.
* **Rationale & Recommendation**: Keep the security definition of done. Defer the `[features]` table until a listed feature needs a switch.

### The schema change doesn't follow store.rs's numbered migrations - Severity: Nit
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** U2 `store.rs`; NFR rollout.
* **Discrepancy Description**: `store.rs` uses numbered `STEPS` with `has_column()`, and a version bump makes older builds refuse the database. The new columns are outside `audit_hash`, so the hash chain doesn't protect them.
* **Rationale & Recommendation**: Specify a `to_v3` step. State whether the new columns go into the hash input. Note that rolling back to v0.3.1 requires `heron restore`.

### `probe_version()` duplicates the existing version probe - Severity: Nit
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** §2 version contract; U2 `cli_llm.rs`.
* **Discrepancy Description**: `probe_status()` already returns `ProviderStatus.version` (`cli_llm.rs:538-615`).
* **Rationale & Recommendation**: Compare that value with `CliSpec.supported` instead of adding a second probe.

### `heron usage --trial` is a product subcommand for a one-off measurement - Severity: Nit
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** §7.
* **Discrepancy Description**: G9 needs one 14-day count, and no work unit owns this command.
* **Rationale & Recommendation**: Use a read-only SQL script, `eval/owner-use.sh`.

### A stored permission mode creates a second source of truth - Severity: Nit
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** §3; U3 `server.rs`.
* **Discrepancy Description**: The mode already lives in `[permissions] mode` in `config.toml` (`config.rs:176-178`). `set_tool` already writes to that config file.
* **Rationale & Recommendation**: Have `set_mode` write the mode to the config file, the same way `set_tool` does.

### ACCEPTED_RISKS.md is set up separately from the threat model, and the entry points go beyond the CUJs - Severity: Nit
* **Reviewer Lens:** `egm-zoom-out`
* **Affected Scope:** §6 risk notice; §4 entry points; U4 tests.
* **Discrepancy Description**:
  * `docs/engineering/SECURITY_THREAT_MODEL.md` and `REMEDIATION_MATRIX.md` already exist, and a separate register can drift from them.
  * The hotkey panel isn't in any CUJ.
  * Snapshot tests of every screen cost a lot of upkeep while the UI keeps changing.
* **Rationale & Recommendation**:
  * Have each risk entry cite its threat-model ID.
  * Move the hotkey panel to the Q8 feature list.
  * Limit snapshot tests to the approval sheet and the risk notice.
