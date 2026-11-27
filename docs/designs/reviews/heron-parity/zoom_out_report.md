Status: resolved

# Stage 5, Initial Zoom Out: annotated review report

**Target:** `docs/designs/heron-parity.md` (full file, draft commit `0ae86a9`)
**Reviewer:** `egm-zoom-out` (fresh context). It received the doc, the persona and standard paths, the research notes, the predecessor designs and the code paths.
**Verdict from the reviewer:** changes required (1 Blocker, 8 Major, 6 Nit).
**Resolution:** 14 accepted as proposed. M5 partly accepted. 0 contested.

## Findings and resolution

### [B1] `egm-zoom-out`, Blocker: memory scope enforced in the wrong layer; the write path breaks isolation
*   **Discrepancy:** a filter added to the read paths misses three write paths:
    *   `Memory::save_detailed` dedupes and supersedes across every row (`src/memory.rs:268-381`);
    *   `forget` deletes by raw id (`src/tools/personal.rs:120`);
    *   `replaces` reads by raw id.

    The model could also choose "remember for everyone".
*   **AI suggestion:** PROPOSE FIX. Verified in the code (`memory.rs:268` scans every vector; `list_memories(None, 3000)`).
*   **Edit:** §1.4 is rewritten. Each turn gets a `Memory` handle bound to `(scope, agent id)`. Every store memory query takes the scope: search, `core`, `list`, `get`, `delete`, the near-duplicate scan, supersede and the raw-id lookups. Only the owner moves a memory to the shared pool, from the Memory screen, and the model never picks the pool. New tests cover the write paths: `dedupe_never_touches_other_pool`, `forget_refuses_other_pool_id`, `supersede_stays_in_pool`.

### [M1] Major: action rules add a second rule system
*   **Discrepancy:** the plan adds a kv store, a new matcher and `Tool::facts` next to the existing allow and deny rules. The structured allows don't keep the shell chaining guard. Recipient and amount facts have no Phase 1 tool.
*   **AI suggestion:** PROPOSE FIX.
*   **Edit:**
    *   Phase 1 adds an `ask` list to the existing rule store (`config.toml` `permissions.ask` with a kv mirror, as deny has) and the existing glob matcher, which keeps `allow_rule_covers`'s chaining guard.
    *   Agent rules go through `Scope`.
    *   The Settings "Never" section becomes "Rules" (never, ask, allow).
    *   `Tool::facts`, recipient, domain and amount matching move to P2, with the first send tool.
    *   Unit A2 is removed, and its work is folded into A3.

### [M2] Major: delegate-to-agent, schedules and workflows as an agent have no Phase 1 user
*   **AI suggestion:** PROPOSE FIX.
*   **Edit:** `delegate {agent}`, the roster block, `schedules.agent` and the workflow `agent` step move to the P2 goals delta. Phase 1 keeps three things: untrusted `delegate` output (PR #75), auditing the `delegate` call, and a child inheriting its parent agent's scope.

### [M3] Major: "surface" duplicates `Origin`
*   **AI suggestion:** PROPOSE FIX.
*   **Edit:** the surface is derived from `Origin` by one function, `surface_of(origin)`. P1 adds `Origin::Voice` only. Channel origins are left to the P2 delta.

### [M4] Major: Phase 1 CUJs use sending and mail tools that P1 doesn't have
*   **Discrepancy:** `draft_email` exists only on `feat/v1-email_drafts` (F8), and sending is P2.
*   **AI suggestion:** PROPOSE FIX.
*   **Edit:**
    *   CUJs P-1, P-3 and P-4 are rewritten with tools on `main`: `shell`, `web_fetch` and the file tools.
    *   The `email` and `calendar` groups hold F8's `draft_email` and the calendar tools when those merge, and are empty before then.

### [M5] Major: seven reversals aren't needed for Phase 1
*   **AI suggestion:** PROPOSE FIX, partly. The brief requires this doc to decide explicitly whether a mobile app is in scope, and full parity means the owner should see every reversal coming.
*   **Edit:** the reversals now sit in two groups:
    *   **decided by approving this doc:** R4 (microphone) and R1, as "mobile is a later doc";
    *   **direction, decided in the owning delta:** R2, R3, R5, R6, R7 and R8.

### [M6] Major: speculative columns and groups
*   **AI suggestion:** PROPOSE FIX.
*   **Edit:**
    *   Removed from P1: `agents.model`, `guest_replies`, `pack:` provenance, `schedules.agent` and the `computer` group.
    *   P1 `surfaces` values are `app` and `voice`. Each later delta adds its own columns and values.

### [M7] Major: a second state machine next to `AvatarStateMachine`
*   **AI suggestion:** PROPOSE FIX. Verified: `AvatarState` has `idle, thinking, awaitingApproval, speaking, error` (`macos/HeronMac/Sources/HeronUI/Avatar/AvatarStateMachine.swift:7`).
*   **Edit:** live voice adds `listening` and `hearing` to `AvatarState` and feeds it voice events. Matrix row 2b now records the existing avatar.

### [M8] Major: Telegram's behaviour in Phase 1 is undefined
*   **AI suggestion:** PROPOSE FIX.
*   **Edit:** in P1, Telegram stays default-agent-only, with no `@Name` addressing and no change to its approval text. R8 moves to the P2 channels delta.

### [N1] Nit: two transcribers, one switched off
*   **AI suggestion:** PROPOSE FIX.
*   **Edit:** P1 builds `SFOnDeviceTranscriber` and a file-backed test source behind the `Transcriber` protocol. `SpeechAnalyzer` is left to a later voice delta.

### [N2] Nit: `CancelToken` in `ToolCtx`
*   **AI suggestion:** PROPOSE FIX.
*   **Edit:** the token lives with the run in `ServerState.running`, and is passed to `run_turn`. Tools never see it.

### [N3] Nit: timing plumbing
*   **AI suggestion:** PROPOSE FIX.
*   **Edit:** the app writes `voice-timing.jsonl` (the last 50 turns) in its own support folder. Settings › Voice reads it, and so does the gate. The core routes and the kv ring are dropped.

### [N4] Nit: preconditions and P0 differ
*   **AI suggestion:** PROPOSE FIX.
*   **Edit:** P0 and the Preconditions now list the same items: feature base, F1, F7, PR #75 and PR #84.

### [N5] Nit: later-phase sections read as decided
*   **AI suggestion:** PROPOSE FIX.
*   **Edit:** §4 to §6 are headed "Direction", and their decisions are owned by the named delta.

### [N6] Nit: `DraftConnectorOutput` duplicates Harness2's entry
*   **AI suggestion:** PROPOSE FIX.
*   **Edit:** the row is removed, and the table points to Harness2's `ext:` sources.
