# Heron auto router: Haiku, Sonnet or Opus for each turn

*   **Author:** fernandogarzaaa (drafted with Claude)
*   **Status:** Draft
*   **Date:** 2026-11-23
*   **Read at:** `main` = `c9dd12c`. Line numbers below are from that commit.
*   **Related:**
    *   [PR #84](https://github.com/fernandogarzaaa/HERON-Project/pull/84), the harness-safety design doc (`heron-harness-safety.md`, not merged), defines `RepeatGuard`. This doc can use its nudge as an escalation signal, but doesn't need it.
    *   [PR #72](https://github.com/fernandogarzaaa/HERON-Project/pull/72) (open) makes the Mac app show the active model in Settings.
    *   [heron-computer-use.md](heron-computer-use.md), drafted alongside this doc. Both docs edit the step loop in `src/agent.rs` (around `:522-566`), so whichever builds second rebases on the other.

--------------------------------------------------------------------------------

## Overview

The owner asked: "is a complicated task routed through Opus or just Sonnet? I think we should have an auto router." Today the answer is "just Sonnet". This doc adds a router that picks one of three tiers for every turn:

*   **Quick** (Haiku 5.5 on Claude): short replies and background chores such as memory learning and summaries.
*   **Standard** (Sonnet 5.5): the default.
*   **Deep** (Opus 5.5): multi-step work, planning, coding and hard reasoning.

The router also moves a turn up a tier partway through when the turn proves hard (§3). It respects the token caps and a new daily Opus allowance (§4.3), shows the chosen model and the reason in the chat, lets the user pin a model for a chat, and works with OpenRouter as well as the Claude CLI.

**The eval runs first.** Five setups need no new code: Haiku, Sonnet or Opus alone, and two two-tier configs of today's router. The eval measures all five against the router's rules before the router is wired in. If one of them wins, the owner gets that config change, and the wiring is not built (§5.6). The build has six units: E0 (the eval), R1 (the rules, with no wiring), G (the gate), then R2 (the core wiring), R3 (API tests) and R4 (the Mac app), which wait for G.

The router replaces the body of the existing `usage::route` (`src/usage.rs:203`). It does not add a second router.

## Definitions

*   A **tier** is `quick`, `standard` or `deep`. A tier is not a model. Each provider maps the three tiers to its own model ids (§1).
*   A **turn** is one call to `Agent::run_turn` (`src/agent.rs:468`). A **step** is one model call inside that turn. With the `claude-cli` provider, each step is a separate `claude -p` process (`src/cli_llm.rs:247-264`, `--max-turns 1`), so the model can change between steps.
*   **Escalation** is a move to a higher tier between two steps of the same turn.
*   A **pin** is the user's choice of one tier for a chat session. It turns the router off for that session.
*   A **typed turn** is a turn whose input the user typed: `POST /api/chat`, the REPL, or a Telegram message from the owner. Schedules, workflow steps, proactive takeover and sub-agents are not typed turns.
*   A **tool error** is a tool call whose tool ran and failed: it returned an error or timed out. A call the policy or the user refused, a call with unreadable arguments, and a call to an unknown tool are not tool errors.
*   Steps are counted from 0, as the loop does (`for step in 0..max_steps`, `src/agent.rs:504`). "Step 2" is the third model call.

**Terms from other Heron docs**, used here with their meaning there:

*   From [heron-v1.md](heron-v1.md): **taint** (a session is tainted once untrusted content, such as a web page, an email or a tool's untrusted output, enters it; it stays tainted), `Effect`, **Q7** (paths whose pull requests wait for the owner), and **G9**, heron-v1's owner trial, which runs until 2026-12-05.
*   **Depth 0** is a top-level turn; a sub-agent started by the `delegate` tool runs at depth 1 or more. **Proactive takeover** is Heron acting on its own suggestion (`act_on_suggestion`, `src/app.rs:2027-2071`). A **workflow `ask` step** is a one-shot model call inside a saved workflow (`src/app.rs:2240`).
*   The **budget levels** are `Level::Normal`, `Careful`, `Minimal` and `Spent` (`src/usage.rs:87-123`), shown to the user as "Normal", "Economy", "Strict economy" and "Allowance used up" (`src/usage.rs:36-40`).
*   From [heron-v1-features.md](heron-v1-features.md), "The extended definition of done": **the eight checks** each feature answers (the table below adds two rows, as that doc does), and **EVE**, the test harness that drives Heron's HTTP API as a user would.
*   **genesis** audits a grader: its `json` suite tries known tricks against a verifier and reports SOUND when none works. **testteeth** grades tests by mutation testing.
*   Build terms: a **work unit** is one row of the Work units table, built by one agent in its own git worktree under the **skein** task runner, and done when its **check** exits 0. The **skein supervisor** is the session that runs the graph; it runs the cargo and Swift checks itself, because they can't run inside a node's sandbox (nested Seatbelt and local ports are refused there). The **model role** column names the model tier that builds the unit. `eval/require-tests.sh` fails if a named test is missing from the cargo test list.

## The Problem

1.  **Nothing reaches Opus.** With `provider = "claude-cli"` and an empty `[llm] cli_model`, `argv` falls back to `"sonnet"` (`src/cli_llm.rs:1689-1695`). The owner's config has `cli_model` empty, so every turn runs on Sonnet 5.5, however hard it is.
2.  **The existing router routes only down, and only when configured.** `usage::route` (`src/usage.rs:203-221`) does nothing unless `fast_model` is set and differs from `model`. When it is set, requests that `looks_simple` (280 characters or fewer, at most 4 lines, no code fence, none of the "heavy" words, `src/usage.rs:188-199`) go to `fast_model`, and the turn moves back to the main model after 2 steps (`src/agent.rs:522`). There is no tier above the main model.
3.  **Background chores don't use a cheap model by default.** Compaction, memory learning, reflection, suggestions, workflow `ask` steps and source learning all call `cfg.fast_model()` (`src/agent.rs:806`, `:831`, `src/app.rs:1864`, `:1987`, `:2240`, `src/sources.rs:498`). With `fast_model` empty, that returns `llm.model`, an HTTP id such as `anthropic/claude-sonnet-5.5` (`src/config.rs:282`, `:818-819`). On `claude-cli`, the request's model wins over `cli_model` (`src/cli_llm.rs:2236-2244`), and `claude_native_model` strips the prefix (`:428-450`). So the chores run on Sonnet. The README suggests setting `fast_model` to Haiku (`README.md:188`), but nothing does it by default.
4.  **The chat names the wrong model.** `Event::Model` names `cfg.llm.model` when the main model is chosen (`src/agent.rs:525`, `:527`), but the CLI runs `cli_model` or `"sonnet"`. The Mac app decodes the event and drops it (`macos/HeronMac/Sources/HeronCore/CoreEvent.swift:27`, `Wire.swift:419`, `Sources/HeronUI/Stores/ChatAndApprovalsStores.swift:328`).
5.  **The user can't choose a model for one chat.** Sessions have no model column (`src/store.rs:194-196`), `POST /api/chat` takes only `session_id` and `message` (`src/server.rs:521-524`), and the Mac Model tab picks a provider only (`macos/HeronMac/Sources/HeronUI/Views/SettingsView.swift:164-205`). The only global way to pick one model is `routing = "off"` plus `cli_model`.

### The config-only options, and why they may not be enough

*   **Two tiers with Opus:** `cli_model = "opus"` and `fast_model = "haiku"`. Today's router already sends simple requests to Haiku, moves them to Opus after 2 steps, and runs the chores on Haiku. Everything else runs on Opus.
*   **Two tiers with Sonnet:** `cli_model = "sonnet"` and `fast_model = "haiku"`. Cheap, but nothing reaches Opus.

On list prices (Anthropic, 2026-11-03), Haiku 5.5 costs $0.10 / $0.50 per million tokens in and out, Sonnet 5.5 $2 / $10, and Opus 5.5 $4 / $20. Haiku is 20 times cheaper than Sonnet, but Opus is only twice Sonnet. So most of the saving comes from the quick tier, which the two-tier config already gives. What the router adds is a Sonnet middle tier for everyday turns, an Opus tier chosen by what the request asks for, a move from Sonnet to Opus mid-turn, a pin, and the right model name in the chat. The eval (§5) has to show the routing parts are worth building.

## Goals

*   **G1.** The router's rules pass the gate defined in §5.6, against the five setups that need no new code. If they don't, the wiring (R2 to R4) is not built. Before RQ1 turns the router on by default, the built router passes the same gate in a live run (§5.6).
*   **G2.** Every turn at depth 0 gets a tier from the router, unless the session is pinned. The router decides from the request and the budget, with no model call.
*   **G3.** Background chores run on the quick tier. On `claude-cli`, the quick tier is Haiku unless `fast_model` says otherwise.
*   **G4.** A turn moves up one tier when a fixed signal fires (§3). It moves at most twice and never moves down.
*   **G5.** The router chooses the deep tier only while the token budget level is Normal (`src/usage.rs:113-121`) and the daily Opus allowance has tokens left (§4.3). At Minimal and Spent it keeps today's rule: quick, with no escalation. Two explicit owner choices outrank this, as `routing = "off"` already does (`src/usage.rs:209-211`): `routing = "off"`, and a session the owner pinned. A pinned session's turns say so in the budget notice (§4.1).
*   **G6.** Only a typed turn in an untainted session can be routed to deep by its words. Outside content can't choose Opus.
*   **G7.** The chat shows the real model name and a one-line reason at the start of each turn and at each escalation, in the Mac app too.
*   **G8.** The user can pin a chat session to Auto, Quick, Standard or Deep from the Mac app. The pin survives a restart.
*   **G9.** The router works for `claude-cli` with built-in defaults, and for the HTTP provider (OpenRouter and other OpenAI-compatible endpoints) and `codex-cli` from model keys the owner sets. A provider with one model runs as today.

## Non-Goals

*   **A model call to classify requests.** A Haiku classifier adds a step to every turn. It is Alternative B, and a Future Improvement if the rules can't meet G1.
*   **Routing between providers.** That is `fallback_models` (`src/config.rs:88`).
*   **Moving a turn down a tier.** A turn that escalated stays up.
*   **Fable 5.x.** `claude_native_model` knows `fable` (`src/cli_llm.rs:437`), but it is not a tier.
*   **Dollar figures in the app.** Caps stay in tokens. The Usage tab already lists each model's tokens per day (`SettingsView.swift:315-340`). Dollar estimates live only in the eval report.
*   **Effort levels.** No tier passes `--effort`, so every model runs at the CLI's default. The labelling run measures what ships. A per-tier effort is a Future Improvement with its own label run.
*   **Remembering the last turn's tier.** A follow-up to a deep turn is routed on its own words. Escalation catches a follow-up that proves hard.

## Open Questions

These are the owner's calls. Each has a proposed default, and the build uses the default unless the owner changes it. The technical unknowns are in Prototype checks, below.

*   **RQ1, the router on by default for `claude-cli`. Proposed: yes, once the gate in §5.6 passes.** `[agent] routing` already defaults to `"auto"` (`src/config.rs:329`). "Once the gate passes" means both the replay gate (unit G) and the live gate (unit L). After this change, `"auto"` on `claude-cli` sends hard typed turns to Opus and chores to Haiku with no config edit. The alternative is a new value, `routing = "tiers"`, that the owner turns on.
*   **RQ2, how the caps count Opus tokens. Proposed: as today, one token is one token on every tier.** The alternative weights each token by its tier's list price relative to Sonnet (Opus 2, Haiku 0.05). That would change what the owner's existing caps mean.
*   **RQ3, a floor for tainted sessions. Proposed: the router never starts a tainted session on quick by itself (rule 5).** The owner's quick pin and the Minimal and Spent levels (rules 1 and 3) still route quick. The reason is an assumption, not a measurement: a smaller model may be more likely to follow instructions injected through web pages or email. Nothing in this repository compares injection rates across tiers.
*   **RQ4, eval spend during heron-v1's G9 owner trial (until 2026-12-05). Proposed: the labelling run (§5.2, 600 turns) waits until after 2026-12-05, or until the owner says go.** At list prices it is about $80, from the §5.7 turn shapes (unverified); on a subscription login it is quota, not dollars. The live run after R2 (§5.6) adds about 120 turns. The gate needs the labels, so the wiring waits too. Everything else in the eval runs without model calls.
*   **RQ5, the pin. Proposed: per chat session, set from the composer.** The owner asked for an override. A global "always this model" already exists as `routing = "off"` plus `cli_model`, so there is no second global setting.
*   **RQ7, a daily Opus allowance. Proposed: `[agent] deep_tokens_per_day = 1000000`.** The daily and monthly caps default to 0, meaning no limit (`src/config.rs:327-328`), and with no limit the budget level is always Normal (`src/usage.rs:57-60`). So on an install with no caps, nothing would limit how often the router picks Opus. The allowance is about 13 deep turns of the §5.7 tool-turn shape a day. When it is used up, the router stops choosing deep until the next day; a deep pin still runs. 0 means no allowance limit.
*   **RQ6, OpenRouter ids. Proposed: no built-in defaults for the HTTP provider.** The owner sets `fast_model`, `model` and the new `deep_model`. OpenRouter's ids for the 5.5 models are not verified in this repository, and a wrong id fails every turn.

## Prototype checks

These run before the labelling run. Each records its output in the eval PR.

*   **PT-R1.** `claude -p --model haiku` and `claude -p --model opus` both answer with Heron's fixed flags (`src/cli_llm.rs:247-264`) under the owner's subscription login, through `claude_native_model`'s `[1m]` ids.
*   **PT-R2.** Whether the CLI's `stream-json` output reports the model that answered. The result is recorded only; acting on it is a Future Improvement.
*   **PT-R3.** Prompt-cache reuse across steps on one model, and on the first step after a model switch. Heron reads only `input_tokens` and `output_tokens` from the CLI (`src/cli_llm.rs:1427-1450`), so cache reads are invisible today. PT-R3 reads the raw `usage` object.

## Critical User Journeys (CUJs)

### CUJ 1: A quick question

*   **Given:** `provider = "claude-cli"`, `routing = "auto"`, an unpinned, untainted session, budget level Normal.
*   **When:** the user types "what's on my calendar tomorrow?"
*   **Then:** the turn runs on Haiku 5.5, and the chat shows "Haiku 5.5 · a short question". If a tool call fails, or the turn reaches step 2 (its third model call), it moves to Sonnet, and the chat shows "Sonnet 5.5 · needed more steps".

### CUJ 2: A coding task

*   **Given:** the same setup.
*   **When:** the user types "refactor the mail sync so retries back off, and add tests".
*   **Then:** the turn runs on Opus 5.5. The chat shows "Opus 5.5 · asks to change code".

### CUJ 3: A standard turn that proves hard

*   **Given:** the same setup.
*   **When:** the user asks "why did last night's backup job fail?", and two of the turn's tool calls fail.
*   **Then:** the router starts the turn on standard. After the second failure, before the next step, the turn moves to Opus, and the chat shows "Opus 5.5 · two tool calls failed". The rest of the turn stays on Opus.

### CUJ 4: The budget runs low

*   **Given:** 25% or less of the daily cap is left (level Careful).
*   **When:** the user asks a coding question.
*   **Then:** the router picks standard, and the chat shows "Sonnet 5.5 · Opus is paused while the allowance is low". At Minimal or Spent, the router picks quick with no escalation, as it does today.

### CUJ 5: The user pins Opus

*   **Given:** a chat session.
*   **When:** the user opens the model menu in the composer and picks "Deep (Opus 5.5)".
*   **Then:** every turn in that session runs on Opus, at every budget level, and the chat shows "Opus 5.5 · pinned". Other sessions stay on Auto.

### CUJ 6: A schedule that reads an email

*   **Given:** a schedule fires on an arriving email that says "refactor everything step by step".
*   **When:** the scheduled turn runs.
*   **Then:** it is not a typed turn, so its words don't route it to deep. It starts on standard (the session is tainted, RQ3), and only escalation can move it up, within the caps.

### CUJ 7: OpenRouter

*   **Given:** `provider = "http"`, `base_url` set to OpenRouter, and `fast_model`, `model` and `deep_model` set to three model ids.
*   **When:** the user types a coding task.
*   **Then:** the request goes out with `deep_model`. With `deep_model` empty there is no deep tier: `fast_model` is quick and `model` is standard, as today. The new parts still apply: a quick turn also moves up on a tool error, tainted sessions skip rule 5, and the reasons read as in §2.

## Technical Plan (Architecture)

```text
 typed: POST /api/chat, REPL, Telegram owner text ──▶ Agent::run_turn_typed ─┐
 generated: schedule, workflow, takeover ──────────▶ Agent::run_turn ────────┤
                                                                              ▼
   Budget::now ──▶ level        session.model_pin        session.tainted
        │                              │                        │
        ▼                              ▼                        ▼
   router::decide(&TierMap, &Inputs { text, typed, level, pin, tainted, routing_off })
        ──▶ Decision { tier, reason, may_escalate }
        │
        ▼  before each later step
   router::escalate(&TierMap, &Decision, current, &TurnSignals { step, tool_errors_since_move, nudged, wrote_files, level, deep_left })
        ──▶ Option<(Tier, reason)>        (Event::Model on each change)
        │
        ▼
   TierMap::model(tier) ──▶ ChatRequest { model: Some(id) }
        ├─▶ CliLlm (claude-cli): --model <alias>
        └─▶ HttpLlm: "model": <id>

 background chores ──▶ cfg.fast_model() = TierMap::model(Tier::Quick)
 delegate children ──▶ the parent's current tier, not routed again
```

### 1. Tiers and the tier map

A new module, `src/router/`, holds `enum Tier { Quick, Standard, Deep }` and `struct TierMap`. `TierMap::for_config(&Config)` reads only existing keys plus one new one, `[llm] deep_model`:

| Tier | `claude-cli` | `http` | `codex-cli` and other CLIs |
|---|---|---|---|
| quick | `fast_model`, or `haiku` when empty | `fast_model` | `fast_model` |
| standard | `cli_model`, or `sonnet` when empty | `model` | `cli_model` |
| deep | `deep_model`, or `opus` when empty | `deep_model` | `deep_model` |

*   An empty cell (other than the `claude-cli` defaults) means the tier is **absent**. `TierMap::model(tier)` returns the nearest present tier below it, or above it when nothing is below.
*   A tier whose model equals the standard model is absent too. So `deep_model = "sonnet"` on `claude-cli` turns the deep tier off. On `claude-cli`, models are compared after `claude_native_model` (`src/cli_llm.rs:428-450`), so `sonnet`, `claude-sonnet-5.5`, `claude-sonnet-5-5` and `anthropic/claude-sonnet-5.5` are equal. Elsewhere they are compared as written.
*   An empty standard model on a CLI other than `claude-cli` means "the CLI's default model": no `--model` flag, as today (`src/cli_llm.rs:1697-1698`). Its display name is "the CLI's default model".
*   A map with only one distinct model is **inactive**: the router picks standard, sends no `Event::Model`, and behaves as today (`src/agent.rs:523`).
*   `TierMap::display(tier)` gives the chat name: "Haiku 5.5", "Sonnet 5.5" and "Opus 5.5" for any model that `claude_native_model` turns into `claude-haiku-5-5[1m]`, `claude-sonnet-5-5[1m]` or `claude-opus-5-5[1m]`, and the raw id for anything else.
*   **The one behaviour change in config:** on `claude-cli`, an empty `fast_model` now means Haiku, not the main model. That is Problem 3's fix. RQ1 covers it.
*   `Config::fast_model()` (`src/config.rs:818`) keeps its signature and returns the quick tier's id, so its six callers move to Haiku with no other change.

### 2. Choosing the tier at the start of a turn

`router::decide(&TierMap, &Inputs) -> Decision` is a pure function. The first matching rule wins.

| # | Condition | Tier | Escalates | Reason shown |
|---|---|---|---|---|
| 1 | The owner pinned the session | the pin | no | "pinned" |
| 2 | `routing = "off"`, or the map is inactive | standard | no | none: no event, as today |
| 3 | Budget level Minimal or Spent | quick | no | "the token allowance is nearly used up" (today's text) |
| 3a | A `delegate` child | the parent's current tier | no | none: a child's events are dropped |
| 4 | A typed turn in an untainted session that asks for deep work (below) | deep; standard at Careful or when the Opus allowance is used up | yes | "asks to change code", "asks for a plan", "a long, detailed request" or "a hard reasoning question"; otherwise "Opus is paused while the allowance is low" |
| 5 | `looks_simple(text)` (`src/usage.rs:188`), and the session is not tainted | quick | yes | "a short question" |
| 6 | otherwise | standard | yes | "an everyday request" |

**Deep work (rule 4)** is any of:

*   a code fence, or three or more file paths;
*   one of the deep words, matched case-insensitively on word boundaries: `refactor`, `implement`, `debug`, `migrate`, `architect`, `design doc`, `implementation plan`, `migration plan`, `step by step`, `proof`, `think hard`, `root cause`, `write tests`, `fix the bug`. Bare `plan` and `prove` are left out, because "plan my weekend" and "prove I paid" are everyday requests;
*   1,500 characters or more;
*   three or more numbered or bulleted requirements.

The deep words live in one `const` in `src/router/rules.rs`, next to `looks_simple` and its `HEAVY` list, which move there from `src/usage.rs`. The eval tunes both lists.

**Why only typed, untainted turns (G6).** Every other turn's input can carry outside text:

*   a triggered schedule wraps the event into its prompt (`src/app.rs:1579-1583`);
*   proactive takeover runs a brief that Heron wrote from what it observed (`src/app.rs:2063`);
*   workflow steps pass a framed prompt (`src/app.rs:2253`);
*   a tainted session's later messages may repeat what a page said.

Those turns are routed by rules 5 and 6 only, so their words can't buy an Opus turn. They can still escalate on harness signals (§3), within the caps.

**Typed turns.** A new method, `Agent::run_turn_typed`, sets `typed = true` for one turn and calls `run_turn`. Three callers switch to it: `/api/chat` (`src/server.rs:601`), the REPL (`src/main.rs:289`) and Telegram (`src/telegram.rs:564`). A forwarded Telegram message already taints the session (`src/telegram.rs:458-461`), so rule 4 skips it. Every other caller keeps `run_turn`, so a new caller is untyped unless someone decides otherwise.

**Tainted sessions (RQ3).** Rule 5 doesn't apply, so the router's own choice for a tainted session is standard or higher. Rules 1 and 3 come first and can still give quick.

**Sub-agents.** A `delegate` child (`src/agent.rs:712-735`) runs on its parent's current tier (rule 3a) and is not routed again, because its task text is written by the model. A child of an owner-pinned session gets the pin through rule 1. Otherwise rule 3 still comes first, so a child at Minimal or Spent runs on quick. Its `Event::Model` stays dropped (`src/agent.rs:732`).

### 3. Moving up mid-turn

Before each step after the first, the loop calls `router::escalate(&TierMap, &Decision, current: Tier, &TurnSignals) -> Option<(Tier, &'static str)>`. It is pure. The loop holds `current`, a move only goes up from `current`, and at most one move happens per step, so each tier boundary can be crossed only once. `TurnSignals { step, tool_errors_since_move, nudged, wrote_files, level }` is filled from data the loop already has:

*   `tool_errors_since_move` counts tool errors (Terms) one per call, since the turn started or since its last move, whichever is later. An error made on quick therefore never counts toward the move to deep.
*   To count them, `execute_call` (`src/agent.rs:613`) returns its text plus a `CallOutcome`: `Ran`, `ToolFailed` (an `Err` or a timeout from the tool), `Refused` (policy or user), `BadArguments` or `UnknownTool`. Only `ToolFailed` is a tool error. A user who declines two approvals does not move a turn to Opus.

| From | Signal | To | Reason shown |
|---|---|---|---|
| quick | step 2 reached, or step 4 at Careful (today's `escalate_after`) | standard | "needed more steps" |
| quick | one tool error | standard | "a tool call failed" |
| standard | two tool errors since the turn started or moved | deep | "two tool calls failed" |
| standard | the repeat nudge fired (`src/agent.rs:558-566`; `RepeatGuard`'s nudge once PR #84 is built) | deep | "it was going in circles" |
| standard | step 8 reached (the ninth model call), and the turn has run `write_file`, `edit_file` or `shell` | deep | "a long change to files" |

*   No escalation when `Decision.may_escalate` is false (rules 1 to 3a).
*   No move to deep unless the budget level is Normal and the Opus allowance has tokens left. The loop reads the budget again before each step (`Budget::now`, one small query), so a long turn that crosses into Careful stops moving up.
*   No move into an absent tier: if the map has no deep tier, a standard turn never moves, and no event is sent.
*   **A failing command counts.** The shell returns `Ok` with `[exit code N]` appended when a command fails (`src/tools/shell.rs:124-134`), so `execute_call` reports a non-zero exit as `ToolFailed` too. A failing build or test is the coding case where Opus helps most.
*   **The delegate tool's own errors:** "maximum delegation depth reached" is `Refused`; a child turn that fails is `ToolFailed`.
*   **A tier the provider refuses.** If a step on quick or deep fails with a provider-limit or unknown-model error (`names_a_provider_limit`, `src/cli_llm.rs:2435-2436`; HTTP 404 or 429 on the HTTP provider), the loop runs the same step again on standard, marks that tier unavailable for 30 minutes (kv `tier_down:<tier>`), and sends a notice: "Opus isn't available right now, so Heron is using Sonnet." Today such an error ends the turn (`src/agent.rs:529`).
*   Each move sends `Event::Model` with the new model and reason.
*   **Outside content can cause tool errors**, for example a page that makes every fetch fail. The worst case is one Sonnet-to-Opus move per turn, at Normal budget only.
*   The prompt cache is per model, so the first step on the new model pays full input price for the whole transcript. PT-R3 measures this.
*   `budget.max_steps` stays per turn (`src/usage.rs:130-146`). An escalation does not reset the step count.
*   The closing call at the step limit (`src/agent.rs:580`) uses the turn's current tier.

### 4. Controls and visibility

#### 4.1 The pin

*   **Store.** `sessions` gains a nullable column, `model_pin TEXT`, holding `quick`, `standard` or `deep`. Null means Auto.
*   **API.** `POST /api/sessions/{id}/model` with `{"pin": "auto"|"quick"|"standard"|"deep"}` sets it; `auto` writes null.
    *   It answers 200 with `{"model_pin": "quick"|"standard"|"deep"|null, "tiers": [{"tier": "quick", "name": "Haiku 5.5"}, …]}`, listing only present tiers in order.
    *   It answers 400 for an unknown value or a tier this provider doesn't have, and 404 for an unknown session.
    *   It is accepted while a turn is running; the pin applies from the next turn.
    *   `GET /api/sessions/{id}` returns the same `model_pin` and `tiers` fields.
*   **Mac app.** A menu in the composer: "Auto", "Quick (Haiku 5.5)", "Standard (Sonnet 5.5)", "Deep (Opus 5.5)". The label shows the current choice.
*   **Telegram and the REPL** use the session's pin if it has one. They have no command to set it.
*   **A pin outranks the budget**, as `routing = "off"` does today. At Careful and below, a pinned session's budget notice adds: "This chat is pinned to Opus 5.5; set it to Auto to save tokens." Spent still lets turns run, as today (`src/usage.rs:171`).

#### 4.2 The event

`Event::Model` gains two fields, `{ name, why, tier, pinned }`, and its JSON form becomes `{"type":"model","name","why","tier","pinned"}` (`src/events.rs:192`). The JSON fields are additive, so the web UI (`assets/index.html:727`) keeps working. The Rust matches in the REPL (`src/main.rs:229`) and the Slint UI (`src/bin/desktop/models.rs:76`) name `{ name, why }` and gain `..`.

*   `name` is `TierMap::display(tier)`: the model that runs, not `cfg.llm.model` (Problem 4). `tier` is `"quick"`, `"standard"` or `"deep"`. `pinned` is true when rule 1 chose the tier.
*   It is sent at step 0 whenever the map is active, and at each escalation.
*   **The Mac app** shows it as a line above the reply, for example "Opus 5.5 · asks to change code", in the secondary text style.

#### 4.3 The caps and the Usage tab

*   The daily and monthly caps and the budget levels stay as they are (`src/usage.rs:87-123`). Tokens count the same on every tier (RQ2).
*   Background chores keep their own ledger, `bg_tokens:YYYY-MM-DD` (`src/app.rs:1889-1910`), and run only at Normal. On Haiku they spend less from the same allowance (150,000 tokens a day by default, `src/config.rs:326`).
*   Proactive takeover keeps its per-run `token_limit` (`src/app.rs:2046`).
*   **The Opus allowance (RQ7).** A ledger beside `bg_tokens`: kv `deep_tokens:YYYY-MM-DD`, charged with each deep step's tokens, and checked by rule 4 and by escalation. `[agent] deep_tokens_per_day`, 1,000,000 by default, 0 for no limit. Pinned turns are charged but never stopped by it.
*   Usage is recorded per model already (`src/llm.rs:457-481`, the `usage(day, model, …)` table at `src/store.rs:526`), and the Usage tab lists each row with its model (`SettingsView.swift:315-340`). Once requests carry the tier's model, the tab shows the Haiku, Sonnet and Opus split with no change.
*   `GET /api/usage`'s `Report.routing` (`src/usage.rs:234`) now means "the map is active and routing is not off". `Report.main_model` (`:251`) becomes the standard tier's display name instead of `cfg.llm.model`, which fixes Problem 4 in the Usage tab. `Report` gains `deep_model`, `deep_today` and `deep_limit`, and `POST /api/usage` accepts it beside `fast_model` (`src/server.rs:1267-1287`).

### 5. The eval

#### 5.1 The cases

`eval/router/cases.md` holds 40 cases in the format of `eval/core-tasks.md`: a `## rNN: <title>` heading and one JSON block each, with machine-checkable pass rules (`eval/pass_rules.py`: `file_contains`, `file_present`, `file_absent`, `reply_matches`, `no_request`).

*   `r01`-`r10`: the ten core tasks `t01`-`t10`.
*   `r11`-`r20`, quick: questions with one checkable answer in a fixture file (`reply_matches`). Three of them are near misses for the deep words, such as "plan my weekend from calendar.txt".
*   `r21`-`r30`, standard: one- or two-tool tasks on fixtures (read, summarise into a file, rename).
*   `r31`-`r40`, deep: fixture repositories with a failing test to fix, a refactor whose test must still pass, a plan written to a file with required sections.

Every fixture uses demo data only. Every case is a typed turn in a new, untainted session.

**The word lists are frozen before labelling.** R1 fixes `DEEP_WORDS` and `HEAVY` before the labelling run, and the gate scores those lists. Any later change to either list is scored on a second set, `r41`-`r60` (20 held-out cases, labelled in their own run), not on the 40 it was tuned against.

#### 5.2 The labelling run

`eval/run_core_tasks.py` gains four options that write keys into the config it generates (`config_toml`, `eval/run_core_tasks.py:176-183`): `--cli-model`, `--fast-model`, `--deep-model` and `--routing`. Environment variables can't be used, because `core_env` strips `HERON_*` (`:186-190`).

`eval/router/label.py` runs every case 3 times under each setup below, through `run_core_tasks.py --provider claude-cli`, on today's `main`. No router code is needed.

| Setup | Options | Role |
|---|---|---|
| Haiku only | `--routing off --cli-model haiku --fast-model haiku` | label |
| Sonnet only | `--routing off --cli-model sonnet --fast-model sonnet` | label, baseline |
| Opus only | `--routing off --cli-model opus --fast-model opus` | label, baseline |
| Two tiers with Opus | `--cli-model opus --fast-model haiku` (today's router) | baseline |
| Two tiers with Sonnet | `--cli-model sonnet --fast-model haiku` (today's router) | baseline |

That is 40 cases × 5 setups × 3 runs = 600 turns (RQ4). In the single-model setups, `--fast-model` matches `--cli-model`, so memory learning after each turn runs on the same model. Otherwise it would run on `llm.model`, which is Sonnet (`src/config.rs:282`, `:818-819`), and a Sonnet learning call would be priced into every Haiku turn.

A case's **sufficient tier** is the lowest tier (quick for the Haiku setup, standard for Sonnet, deep for Opus) that passes at least 2 of its 3 runs. A case no tier passes is dropped from the routing metrics and listed.

**Where the numbers come from.** Today a run record has one `tokens` total (`eval/run_core_tasks.py:459-461`, from the last `usage` event, `eval/eve_adapter.py:161-163`) and no wall time. E0 adds two fields to each run record:

*   `wall_ms`: the time from sending the turn to its `done` event, measured by the runner.
*   `models`: one `{"model", "prompt", "completion"}` per model, read after the run from the `usage` table in that run's own Heron home (`usage(day, model, prompt, completion, calls)`, `src/store.rs:526`), which the runner keeps under `--artifacts`. This gives the per-model split the two-tier baselines need.

**The file formats** (all JSON, committed):

*   `eval/router/labels.json`: `{"cases": {"r01": {"setups": {"haiku": [Run, Run, Run], "sonnet": […], "opus": […], "two_opus": […], "two_sonnet": […]}}, …}, "sufficient": {"r01": "quick"|"standard"|"deep"|null, …}}`, where `Run` is `{"pass": bool, "ttft_ms": int|null, "wall_ms": int, "models": [{"model", "prompt", "completion"}]}`.
*   `eval/router/replay.json`: an array `[{"id": "r01", "completion": "{\"tier\": \"quick\"}"}, …]`.
*   The text the replay routes is each case's `prompt` field from `cases.md`, with `{web}` replaced by `http://fixture.invalid`.

#### 5.3 The replay

The router's rules are a pure function, so they are scored without model calls. Unit R1 includes an ignored test, `router::replay_cases`, run with `cargo test --locked --lib router::replay_cases -- --ignored`. It reads each case's text from `eval/router/cases.md`, runs `decide` with `typed = true`, an untainted session and level Normal, and writes `eval/router/replay.json`: one `{"id", "completion"}` per case, where `completion` is the string `{"tier": "…"}`.

`eval/router/score.py` passes each completion and its label to `eval/genesis/router_verifier.py`, the verifier that genesis audits (§5.5). The verifier writes `{"correct": bool, "direction": "exact"|"under"|"over"|"invalid"}`. genesis reads `correct`. `score.py` reads `direction` for the under- and over-routing rates. One parser decides both, so a completion the audit would reject can't count in the metrics.

#### 5.4 The metrics

| Metric | Definition |
|---|---|
| Under-routing rate | cases where the router's tier is below the sufficient tier |
| Over-routing to deep | cases routed deep whose sufficient tier is quick or standard |
| Pass rate | for the router: the label runs' pass rate at the tier it chose, per case. For a baseline: its own runs |
| Cost per passed task | the sum over runs of tokens in and out × list price, divided by the passed runs. `eval/router/prices.json` holds the three 5.5 prices. `score.py` maps each `usage.model` string the way `claude_native_model` does (strip `anthropic/` and `[1m]`, map `haiku`, `sonnet`, `opus`, dots to dashes), and the provider id `claude-cli` (a call with no model, `src/cli_llm.rs:2459-2463`) to Sonnet. Any other name is reported as unpriced, which fails the gate |
| Latency | median and 90th percentile of `ttft_ms` and wall time |

**The router's mid-turn escalation is not in its numbers**, because its pass rate and cost come from pinned runs, while the two-tier baselines run live with today's escalation. That tilts the comparison in two directions. On pass rate it favours the baselines, because escalation can rescue a router turn. On cost it favours the router, because a quick case that would move to Sonnet at step 2 is priced as Haiku throughout. The live run (§5.6) measures the built router with its escalation before it is turned on by default.

#### 5.5 genesis

genesis audits the grader, not the router. `eval/genesis/router.yaml` follows `eval/genesis/limit-classifier.yaml`. Its dataset, `eval/genesis/router-cases.jsonl`, has one row per labelled case with the sufficient tier as the reference. `eval/genesis/router_verifier.py` accepts only the strict answer `{"tier": "<tier>"}` equal to the reference. The `json` suite checks that the verifier can't be passed by duplicate keys, prose around the JSON, extra fields or a schema-only check. Before the labelling run, `router-cases.jsonl` is built from 12 hand-labelled seed rows (`eval/router/seed-labels.json`), so the audit runs in CI from the start. `genesis audit-evaluator eval/genesis/router.yaml --suite json` must exit 0 (SOUND).

#### 5.6 The gate

The baselines are the five setups in §5.2: three single models and two two-tier configs. Pass rate is passed runs over all runs of the cases that count (up to 120). One case's worth is 3 runs. `python3 -I eval/router/score.py --gate` exits 0 only when all of these hold:

*   the router's under-routing rate is 10% or less of the cases that count;
*   its pass rate is at least the highest baseline pass rate minus 3 runs' worth;
*   for every baseline whose pass rate is at least the router's minus 3 runs' worth, the router's cost per passed task is at most 85% of that baseline's.

The gate is unit G in the Work units table. R2, R3 and R4 depend on it. If it fails, the report names the winning baseline, and the owner's next step is that config change, not this build.

**The live run.** After R2, `label.py --setup router` runs every case 3 times on the built router, with escalation, and adds those runs to `labels.json` as setup `router`. `score.py --gate --live` applies the same three conditions to the live runs in place of the replay. RQ1's default-on waits for it. A 3-run tolerance is one case's worth: it absorbs one case flipping from pass to fail between runs, not more.

#### 5.7 Estimated cost and latency

These figures are **estimates, unverified**, from list prices and an assumed turn shape. The labelling run replaces them.

*   **Assumed shape:** a 6-step tool turn sends about 12,000 input tokens per step (the whole transcript, `render_prompt`, `src/cli_llm.rs:878`) and returns 400. A quick reply is 1 step of 8,000 in and 150 out.
*   **Per turn, with no caching:**

    | Turn | Haiku 5.5 | Sonnet 5.5 | Opus 5.5 |
    |---|---|---|---|
    | Quick reply (8,000 in, 150 out) | $0.0009 | $0.018 | $0.035 |
    | 6-step tool turn (72,000 in, 2,400 out) | $0.008 | $0.168 | $0.336 |

*   **An escalation** from Sonnet to Opus before step 3 of a 6-step turn (steps 0 to 5) runs steps 3 to 5 on Opus, and step 3 pays full price for the whole transcript.
*   **With a CLI subscription login** these dollars are notional. The real limits are the plan's quotas, which Heron can't see.
*   **Latency:** smaller models usually return the first token sooner, but the only baseline in this repository (`eval/baseline/summary.json`) is Sonnet.

## Alternatives Considered

### Alternative A: The two-tier config (`cli_model = "opus"`, `fast_model = "haiku"`)

*   **Pros:** No code. Today's router already routes simple requests to Haiku and moves them up after 2 steps, and the chores run on Haiku.
*   **Cons:** Every turn that isn't "simple" runs on Opus, including everyday requests Sonnet handles. No Sonnet tier, no Opus on demand from a Sonnet turn, no pin, and the chat still names the wrong model.
*   **Decision:** Not rejected up front. It is a baseline, and the gate (§5.6) builds the router only if the router beats it.

### Alternative B: A Haiku classifier call before each turn

*   **Pros:** Reads meaning, not keywords.
*   **Cons:** One extra model call on every turn, which slows the quick replies it is meant to speed up. Its decisions vary between runs, so the eval can't replay them for free. A classifier that reads outside content can be steered by it.
*   **Reason for rejection:** The rules are free, deterministic and replayable. If they can't pass the gate after tuning, a classifier is the next step.

### Alternative C: Let the model ask for a bigger model

*   **Pros:** The model knows when it is stuck.
*   **Cons:** An `escalate` tool is a lever that outside content can pull directly, and models call such tools unevenly.
*   **Reason for rejection:** Harness signals are harder to steer, and each one is bounded to one move per boundary at Normal budget (§3).

### Alternative D: The CLI's own model modes or `--fallback-model`

*   **Pros:** Built into Claude Code.
*   **Cons:** Heron runs one step per process with the CLI's tools off (`--tools ""`, `--max-turns 1`), so the CLI never sees a multi-step turn. Only `claude-cli` has them, which leaves out OpenRouter and Codex.
*   **Reason for rejection:** The tier decision belongs where the turn loop is: in Heron.

### Alternative E: A second router beside `usage::route`

*   **Pros:** No risk to today's behaviour.
*   **Cons:** Two routers can disagree, and both would send `Event::Model`.
*   **Reason for rejection:** Rules 2, 3 and 5 carry today's rules over, with the changes listed in CUJ 7, so one function replaces the other.

--------------------------------------------------------------------------------

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| The word lists under-route or over-route (for example "plan my weekend" going to Opus) | The lists are frozen before labelling; near-miss cases in r11-r30; the gate's under-routing bar; tuning after labelling needs the held-out cases (§5.1) |
| Outside content causes tool errors to force escalation | Only typed, untainted turns route deep by words (G6); escalation moves at most one tier per boundary, deep only at Normal with allowance left (§3) |
| The Opus or Haiku tier is refused by the provider (plan limit, unknown model) | The step retries on standard, the tier is skipped for 30 minutes, and a notice says why (§3) |
| Opus use grows with no caps set | The daily Opus allowance (RQ7) |
| A switch of model loses the prompt cache | One switch per boundary; PT-R3 measures the cost; it is in the live run's numbers |
| The label runs vary from run to run | 3 runs per setup and case, a 2-of-3 vote, and a tolerance of one case's worth of runs; the live run repeats the gate on the built router |
| A wrong OpenRouter id fails every turn | No built-in HTTP defaults (RQ6); the HTTP provider's `fallback_models` chain still applies (`src/llm.rs:376-378`) |
| Plan quotas on a subscription login are invisible to Heron | Recorded in §5.7; the Opus allowance bounds the share Heron itself chooses |

## Non-Functional Requirements

*   **Security and privacy.** No new tool, host, process or process argument. Outside content can't route a turn to deep (G6), and tainted sessions get a floor (RQ3).
*   **Performance.** `router::decide` runs word matches, two counts and a length check on one message, with no model call. It should take well under a millisecond; R1's tests don't time it.
*   **Rollout and rollback.** `routing = "off"` restores "always the main model". On the HTTP provider, leaving `deep_model` empty keeps today's behaviour. On `claude-cli`, `fast_model = "sonnet"` and `deep_model = "sonnet"` together restore today's single model. The schema change adds one nullable column, which older builds ignore.

## The eight checks

The checks from heron-v1-features.md, plus its two extra rows (Switch and default, Q7).

| Check | Answer |
|---|---|
| Switch and default | Core key `[agent] routing` (`"auto"` default, `"off"`). RQ1 decides whether `"auto"` routes to Opus on `claude-cli` by default |
| New tools | none |
| Decision-table rows | none: no new `Effect` |
| Prompt blocks | none. The budget guidance block is unchanged |
| EVE | New task `eval/eve/router.yaml` (written in E0, run by hand after R2 with a live provider): over `/api/chat`, a quick question's `model` event has tier `quick`, a coding request's has `deep`, and after `POST /api/sessions/{id}/model` with `standard` the next turn's event has `pinned: true`. The Mac model line and composer menu are covered by R4's tests and the owner's manual pass |
| Content entries | none new. Only a typed turn's own text is matched against the deep words |
| Approval surfaces | none |
| Processes | none new. `claude` gets a different `--model` value per tier, from the same fixed argv (`src/cli_llm.rs:247-264`) |
| TCC | none |
| Q7 | yes: the schema step in `store.rs` |

--------------------------------------------------------------------------------

## Additional Resources

*   `src/usage.rs:183-221`: today's `Route`, `looks_simple` and `route`, and the test at `:330-339` that pins "off beats economy".
*   `src/agent.rs:486-580`: the turn loop, the step-0 event and the escalation step.
*   `src/cli_llm.rs:247-264`, `:428-450`, `:1689-1700`: the `claude -p` argv, `claude_native_model`, and the `"sonnet"` fallback.
*   `eval/run_core_tasks.py`, `eval/pass_rules.py`, `eval/genesis/limit-classifier.yaml`: the harness, rules and genesis pattern the eval reuses.
*   Anthropic's price list, 2026-11-03: Haiku 5.5 $0.10 / $0.50, Sonnet 5.5 $2 / $10, Opus 5.5 $4 / $20 per million tokens in / out (the source for every price in this doc).

## Future Improvements

*   **A classifier model call** (Alternative B), if the rules can't pass the gate.
*   **Effort per tier** (for example Opus at `high`, Haiku at `low`), with its own label run.
*   **A dollar estimate in the Usage tab**, if the owner wants one.
*   **Learning from pins:** a session re-pinned to deep after an Auto turn is a labelled example.
*   **Acting on PT-R2:** if the CLI reports the model that answered, log any mismatch with the model asked for.

--------------------------------------------------------------------------------

## Detailed Implementation Plan

### Pre-steps

1.  Record the owner's answers to RQ1 to RQ6, or note that the defaults stand.
2.  Commit three small pieces on the feature branch first, so R1 stays inside its scope: `pub mod router;` in `src/lib.rs`, an empty `src/router/mod.rs`, and the `[llm] deep_model` field (empty by default) in `src/config.rs`. Build E0 and R1 (no owner spend). Merge E0.
3.  Run PT-R1 to PT-R3, then the labelling run (§5.2) on the owner's go (RQ4). Commit `labels.json`.
4.  Only then start unit G, and R2 to R4 after it.
5.  After R2 builds, run the live run (§5.6) on the owner's go, commit the new `labels.json` rows, and only then let unit L and V finish. The branch with R2 isn't merged before L passes, so RQ1's default never ships untested.
6.  PR #84 is not required. The "going in circles" signal uses the existing repeat check (`src/agent.rs:558-566`). When PR #84's `RepeatGuard` lands, it replaces that check in one place.

### Target Files to Create/Modify

#### 1. `eval/router/`, `eval/genesis/`, `eval/run_core_tasks.py`, `eval/tests/` (Create and Modify, E0)

*   `eval/run_core_tasks.py`: the `wall_ms` and `models` fields in each run record (§5.2), and `--cli-model`, `--fast-model`, `--deep-model` and `--routing`, written by `config_toml` as `cli_model`, `fast_model` and `deep_model` under `[llm]` and `routing` under `[agent]`. Today's core ignores the unknown `deep_model` key: no struct in `src/config.rs` uses `deny_unknown_fields`.
*   `eval/eve/router.yaml`: the EVE task in the eight-checks table.
*   `eval/router/cases.md` (40 cases), `eval/router/fixtures/` (demo data), `eval/router/prices.json`, `eval/router/seed-labels.json` (12 rows), `eval/router/label.py` (§5.2), `eval/router/score.py` (§5.3 to §5.6, with `--gate`), `eval/router/README.md`.
*   `eval/genesis/router.yaml`, `eval/genesis/router_verifier.py`, `eval/genesis/build_router_cases.py` (writes `router-cases.jsonl` from `labels.json`, or from the seed file when there is no `labels.json`), `eval/genesis/router-cases.jsonl`.
*   `eval/tests/test_router_eval.py`: the case file's structure, the new harness options in the generated config, the `models` split read from a fixture `heron.db`, sufficient-tier arithmetic, the verifier's `direction` values, metric arithmetic on a fixed `labels.json` and `replay.json`, each of the gate's three conditions, and the verifier's known-good and known-bad answers.

#### 2. `src/router/mod.rs`, `src/router/rules.rs` (Create, R1)

*   **`mod.rs`:** `enum Tier { Quick, Standard, Deep }` with `as_str` and `parse`; `struct TierMap { quick: Option<String>, standard: String, deep: Option<String> }` with `for_config`, `model(Tier) -> &str`, `display(Tier) -> String`, `is_active() -> bool` and `present() -> Vec<Tier>`; `struct Inputs<'a> { text: &'a str, typed: bool, level: Level, pin: Option<Tier>, parent_tier: Option<Tier>, tainted: bool, routing_off: bool, deep_left: bool, down: &'a [Tier] }`; `struct Decision { tier: Tier, reason: &'static str, may_escalate: bool }`; `fn decide(&TierMap, &Inputs) -> Decision`; `struct TurnSignals { step: usize, tool_errors_since_move: usize, nudged: bool, wrote_files: bool, level: Level, deep_left: bool }`; `fn escalate(&TierMap, &Decision, current: Tier, &TurnSignals) -> Option<(Tier, &'static str)>`.
*   **`rules.rs`:** `looks_simple` and `HEAVY`, copied from `src/usage.rs` (R2 deletes the old copy); `DEEP_WORDS`; `fn asks_deep(text) -> Option<&'static str>` (returns the reason); `fn count_paths`; `fn count_requirements`. Word-boundary, case-insensitive matching.
*   **Tests (inline):** `decide_follows_rule_order` (one case per row of §2), `pin_beats_every_budget_level`, `off_beats_spent_budget`, `minimal_and_spent_route_quick_without_escalation`, `careful_caps_deep_at_standard`, `untyped_turn_never_routes_deep_by_words`, `tainted_typed_turn_never_routes_deep_by_words`, `tainted_session_skips_rule_5`, `absent_tier_falls_to_nearest`, `deep_equal_to_standard_is_absent`, `single_model_map_is_inactive`, `escalate_quick_at_step_two`, `escalate_quick_on_tool_error`, `escalate_standard_on_two_errors`, `escalate_standard_on_nudge`, `escalate_standard_on_long_file_turn`, `escalate_crosses_each_boundary_once`, `escalate_skips_deep_below_normal`, `no_escalation_when_pinned`, `deep_words_match_on_word_boundaries`, `bare_plan_is_not_deep`, `delegate_child_yields_to_minimal`, `deep_allowance_used_up_routes_standard`, `escalate_skips_absent_tier`, `escalate_skips_deep_when_allowance_used_up`, `down_tier_is_skipped`, `tier_map_claude_cli_defaults`, `tier_map_http_uses_existing_keys`.
*   **The replay test:** `#[ignore] fn replay_cases()` (§5.3), reading `eval/router/cases.md` and writing `eval/router/replay.json`.

#### 3. `src/usage.rs` (Modify, R2)

*   `route()`, `Route` and `looks_simple` are deleted: after R2 the agent calls `router::decide` directly, and nothing else calls `route` (its only caller is `src/agent.rs:492`). The test at `:330-339` goes with them; R1's `off_beats_spent_budget` and `minimal_and_spent_route_quick_without_escalation` cover what it pinned.
*   `Report` gains `deep_model`; `routing` takes the meaning in §4.3.

#### 4. `src/config.rs` (Modify, R2)

*   A commented `deep_model` line in the config template next to `fast_model` (`:959`). The field itself came with the pre-steps.
*   `[agent] deep_tokens_per_day: u64`, 1,000,000 by default (RQ7).
*   `Config::fast_model()` changes from `-> &str` to `-> String` and returns `TierMap::for_config(self).model(Tier::Quick).to_string()`. Its six callers take the `String`.
*   Tests: `deep_model_defaults_empty`, `fast_model_is_haiku_on_claude_cli_when_empty`, `deep_tokens_per_day_defaults_to_one_million`.

#### 5. `src/agent.rs` (Modify, R2)

*   `run_turn_typed(&self, session, input, sink)`, which sets a `typed` flag for this turn and calls `run_turn`.
*   `Session` gains `model_pin: Option<Tier>`, filled by `Session::load` from the `sessions` row. At turn start (`:492`): build the `TierMap`, read `session.model_pin` and `session.tainted`, and call `router::decide`.
*   In the step loop (`:522-528`): replace the `escalate_after` filter with `router::escalate`, fed from counters the loop keeps (tool errors from `execute_call` results, the repeat nudge at `:558-566`, and whether `write_file`, `edit_file` or `shell` ran). Send `Event::Model { name, why, tier, pinned }` at step 0 when the map is active, and on every move.
*   The closing call (`:580`) uses the current tier's model.
*   `delegate` (`:712-735`) passes the parent's current tier to the child: `Session::ephemeral` gains a `parent_tier` argument, which feeds rule 3a, and the parent's owner pin, which feeds rule 1.
*   `execute_call` returns a `CallOutcome` (§3), with a non-zero `[exit code N]` from the shell counted as `ToolFailed`.
*   The provider-refusal retry on standard and the `tier_down:<tier>` marks (§3).
*   The `deep_tokens:YYYY-MM-DD` ledger, charged after each deep step (§4.3). The child's `Event::Model` stays dropped by its sink (`:732`).
*   `Event::Model` gains `tier: String` and `pinned: bool`.
*   Tests: `turn_start_sends_model_event_with_real_name`, `escalation_changes_request_model_between_steps` (with `ScriptedLlm`, asserting `req.model` per step), `background_compaction_uses_quick_tier`, `delegate_child_runs_on_parent_tier`, `run_turn_is_untyped_by_default`, `shell_nonzero_exit_is_tool_failed`, `declined_approval_is_not_tool_failed`, `refused_tier_retries_on_standard`, `deep_steps_charge_the_allowance`, `pinned_notice_names_the_pin`.

#### 6. `src/events.rs` (Modify, R2)

*   The `model` event's JSON adds `tier` and `pinned` (`:192`). Test: `model_event_json_has_tier_and_pinned`.

#### 7. `src/store.rs` (Modify, R2, Q7)

*   The next schema step adds `sessions.model_pin TEXT`, nullable, with `set_model_pin(id, Option<Tier>)` and `model_pin(id)`.
*   Tests: `schema_step_adds_nullable_pin_column`, `model_pin_round_trips`.

#### 8. `src/server.rs`, `src/main.rs`, `src/telegram.rs` (Modify, R2)

*   `server.rs`: `/api/chat` calls `run_turn_typed` (`:601`). `POST /api/sessions/{id}/model`, and `model_pin` and `tiers` in `GET /api/sessions/{id}`, behind the token check. `POST /api/usage` accepts `deep_model`.
*   `main.rs`: the REPL calls `run_turn_typed` (`:289`), and the `Event::Model` match at `:229` gains `..`. The same `..` goes on the match in `src/bin/desktop/models.rs:76`.
*   `telegram.rs`: the owner's messages call `run_turn_typed` (`:564`).
*   Tests: `session_pin_route_needs_token`, `session_pin_rejects_unknown_tier`, `chat_route_runs_typed_turn`.

#### 9. `src/app.rs`, `src/sources.rs` (Tests only, R2)

*   No call-site changes: reflection (`app.rs:1864`), suggestions (`:1987`), workflow `ask` (`:2240`) and `learn_one` (`sources.rs:498`) already call `cfg.fast_model()`, which now returns the quick tier.
*   Test: `background_calls_request_quick_tier` (one `ScriptedLlm` assertion per call site).

#### 10. `tests/integration.rs`, `tests/fixtures/api/` (Modify, R3)

*   `api_session_pin_round_trip`, `api_model_event_carries_tier`, and re-recorded `tests/fixtures/api/` files for sessions, usage and the event stream.

#### 11. Mac app (Modify, R4)

*   `Sources/HeronCore/CoreEvent.swift:27`: `case model(name: String, why: String, tier: String?, pinned: Bool)`.
*   `Sources/HeronCore/Wire.swift`: decode the new event fields (`:419`), and `model_pin` and `tiers` on sessions.
*   `Sources/HeronCore/CoreClient.swift`: `setModelPin(session:pin:)` in the client protocol and all three clients.
*   `Sources/HeronUI/Stores/ChatAndApprovalsStores.swift`: keep the event (`:328`) as the turn's model line instead of dropping it.
*   `Sources/HeronUI/Avatar/AvatarStateMachine.swift:81`: the `.model` case takes the new associated values.
*   `Sources/HeronUI/Views/ChatView.swift`: the model line above each reply, and the composer menu.
*   Tests (`macos/HeronMac/Tests/`): `test_modelEventDecodesTierAndPinned`, `test_modelLineShowsNameAndReason`, `test_composerMenuListsPresentTiersOnly`, `test_pinMenuCallsSetModelPin`.

#### 12. Documents (Modify, R2)

*   `README.md` (near `:188`): `deep_model`, the new meaning of an empty `fast_model` on `claude-cli`, and what `routing = "auto"` does.

### Verification Plan

*   **Build:** `cargo build --locked`, `cargo clippy --locked --all-targets -- -D warnings`, `swift build --package-path macos/HeronMac`.
*   **Tests:** `cargo test --locked` (run outside the Bash sandbox, where about 27 unrelated tests need local ports and nested Seatbelt), `swift test --package-path macos/HeronMac`, `python3 -I -m unittest discover -s eval/tests -t eval`.
*   **Test quality (testteeth):** `testteeth run --base main --fail-under 70` on the Rust changes. `src/router/` is pure logic and should have no surviving mutants. testteeth has no Swift support here, so the Swift tests are checked by name (`swift test list | grep`) and in the owner's manual pass.
*   **Evaluator audit (genesis):** `genesis audit-evaluator eval/genesis/router.yaml --suite json` exits 0.
*   **The gate:** `python3 -I eval/router/score.py --gate` exits 0 on the committed `labels.json` and `replay.json`.
*   **EVE:** the task in the eight-checks table.

## Work units

Units whose scopes share a prefix (`src/**` and `src/router/**`, `eval/**` and `eval/router/**`) are ordered by depends-on. In the check column, `\|` stands for a shell `|`. Cargo and Swift checks run in the skein supervisor, outside the node sandbox, as on earlier Heron builds.

| id | what to build | scope | depends on | check | model role |
|---|---|---|---|---|---|
| E0 | Eval: harness options, cases, fixtures, label and score scripts, genesis spec and verifier, tests (item 1) | `eval/**` | none | `python3 -I -m unittest discover -s eval/tests -t eval -p 'test_router_eval.py' && test "$(grep -c '^## r[0-9][0-9]: ' eval/router/cases.md)" -eq 40 && genesis audit-evaluator eval/genesis/router.yaml --suite json` | feature, refactoring: sonnet high |
| R1 | Router module, rules and the replay test (item 2) | `src/router/**` | none | `cargo test --locked --lib router:: && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib -- decide_follows_rule_order off_beats_spent_budget minimal_and_spent_route_quick_without_escalation careful_caps_deep_at_standard untyped_turn_never_routes_deep_by_words tainted_typed_turn_never_routes_deep_by_words escalate_crosses_each_boundary_once escalate_skips_deep_below_normal escalate_skips_absent_tier bare_plan_is_not_deep delegate_child_yields_to_minimal deep_allowance_used_up_routes_standard tier_map_claude_cli_defaults replay_cases` | feature, refactoring: sonnet high |
| G | The gate: replay written, gate passes on the committed labels (§5.6) | `eval/router/**` | E0, R1, and a committed `eval/router/labels.json` | `test -s eval/router/labels.json && cargo test --locked --lib router::replay_cases -- --ignored && python3 -I eval/router/score.py --gate` | judgment and prose: opus high |
| R2 | Wiring: usage, config, agent loop, typed turns, events, store, routes, README (items 3-9, 12) | `src/**` | G | `cargo test --locked --lib --bins && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib --bins -- fast_model_is_haiku_on_claude_cli_when_empty escalation_changes_request_model_between_steps background_calls_request_quick_tier delegate_child_runs_on_parent_tier run_turn_is_untyped_by_default shell_nonzero_exit_is_tool_failed declined_approval_is_not_tool_failed refused_tier_retries_on_standard deep_steps_charge_the_allowance model_event_json_has_tier_and_pinned schema_step_adds_nullable_pin_column session_pin_route_needs_token chat_route_runs_typed_turn` | hardest tasks: opus high |
| R3 | Integration tests and API fixtures (item 10) | `tests/**` | R2 | `cargo test --locked --test integration && bash eval/require-tests.sh --test integration -- api_session_pin_round_trip api_model_event_carries_tier` | feature, refactoring: sonnet high |
| R4 | Mac app: model line, composer pin menu (item 11) | `macos/HeronMac/**` | R2 | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && for t in test_modelEventDecodesTierAndPinned test_modelLineShowsNameAndReason test_composerMenuListsPresentTiersOnly test_pinMenuCallsSetModelPin; do swift test list --package-path macos/HeronMac \| grep -q "$t" \|\| exit 1; done` | feature, refactoring: sonnet high |
| L | The live run: the built router's own runs pass the gate (§5.6) | `eval/router/**` | R2, and the owner's go for about 120 more turns | `test -s eval/router/labels.json && python3 -I eval/router/score.py --gate --live` | judgment and prose: opus high |
| V | Verify the whole branch | (none: reads only) | R3, R4, L | `cargo test --locked && swift test --package-path macos/HeronMac && python3 -I -m unittest discover -s eval/tests -t eval && genesis audit-evaluator eval/genesis/router.yaml --suite json && python3 -I eval/router/score.py --gate && testteeth run --base main --fail-under 70 -q` | judgment and prose: opus high |

--------------------------------------------------------------------------------

*Drafted with EGM*
