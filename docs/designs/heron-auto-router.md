# Heron auto router: Haiku, Sonnet or Opus for each turn

*   **Author:** fernandogarzaaa (drafted with Claude)
*   **Status:** In Review (resolved, awaiting owner approval)
*   **Date:** 2026-11-23. Review resolved 2026-11-23; findings and decisions in [the consolidated review report](reviews/heron-auto-router/consolidated_review_report.md).
*   **Read at:** `main` = `c9dd12c`. Line numbers below are from that commit. `main` has since gained #72 (the Mac app shows the active model in Settings), #84 and #85 (the sidebar can pin a conversation, stored as kv `pinned:{id}`); none of them changes this design, but line numbers in `src/` may have shifted.
*   **Related:**
    *   [heron-harness-safety.md](heron-harness-safety.md), the harness-safety design (PR #84, on `main`, approved but not built), defines `RepeatGuard`. This doc can use its nudge as an escalation signal, but doesn't need it.
    *   [PR #72](https://github.com/fernandogarzaaa/HERON-Project/pull/72) (merged) shows the active model in the Mac app's Settings. This doc adds the per-turn model line in the chat.
    *   [heron-computer-use.md](heron-computer-use.md), drafted alongside this doc. Both docs edit the step loop in `src/agent.rs` (around `:522-566`), so whichever builds second rebases on the other.

--------------------------------------------------------------------------------

## Overview

The owner asked: "is a complicated task routed through Opus or just Sonnet? I think we should have an auto router." Today the answer is "just Sonnet". This doc adds a router that picks one of three tiers for every turn:

*   **Quick** (Haiku 5.5 on Claude): short replies and background chores such as memory learning and summaries.
*   **Standard** (Sonnet 5.5): the default.
*   **Deep** (Opus 5.5): multi-step work, planning, coding and hard reasoning.

The router also moves a turn up a tier partway through when the turn proves hard (§3). It respects the token caps and a new daily Opus allowance (§4.3), shows the chosen model and the reason in the chat, lets the user pin a model for a chat, and works with OpenRouter as well as the Claude CLI.

**The eval runs first.** Five setups need no new code: Haiku, Sonnet or Opus alone, and two two-tier configs of today's router. The eval measures all five against the router's rules before the router is wired in. If one of them wins, the owner gets that config change, and the wiring is not built (§5.6). The build has nine units: E0 (the eval), R1 (the rules, with no wiring), G (the gate), then R2 (the core wiring), R3 (API tests), R4 (the Mac app) and R5 (the web Usage page), which wait for G, then L (the live run of the built router) and V (verify).

The router takes over from the existing `usage::route` (`src/usage.rs:203`): its rules move into `src/router/`, and `usage::route` is deleted. There is never a second router.

## Definitions

*   A **tier** is `quick`, `standard` or `deep`. A tier is not a model. Each provider maps the three tiers to its own model ids (§1).
*   A **turn** is one call to `Agent::run_turn` (`src/agent.rs:468`). A **step** is one model call inside that turn. With the `claude-cli` provider, each step is a separate `claude -p` process (`src/cli_llm.rs:247-264`, `--max-turns 1`), so the model can change between steps.
*   **Escalation** is a move to a higher tier between two steps of the same turn.
*   A **pin** is the user's choice of one tier for a chat session. It turns the router off for that session. It is not the sidebar's conversation pin from #85 (kv `pinned:{id}`); the Mac menu says "Model", never "Pin".
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

*   **G1.** The router's rules pass the gate defined in §5.6, against the five setups that need no new code. If they don't, the wiring (R2 to R5) is not built. Before RQ1 turns the router on by default, the built router passes the same gate in a live run (§5.6).
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
*   **RQ6, OpenRouter ids. Proposed: no built-in defaults for the HTTP provider.** The owner sets `fast_model`, `model` and the new `deep_model`. OpenRouter's ids for the 5.5 models are not verified in this repository, and a wrong id fails every turn.
*   **RQ7, a daily Opus allowance. Proposed: `[agent] deep_tokens_per_day = 1000000`.** The daily and monthly caps default to 0, meaning no limit (`src/config.rs:327-328`), and with no limit the budget level is always Normal (`src/usage.rs:57-60`). So on an install with no caps, nothing would limit how often the router picks Opus. The allowance is about 13 deep turns of the §5.7 tool-turn shape a day. When it is used up, the router stops choosing deep until the next day; a deep pin still runs. 0 means no allowance limit.

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
   router::decide(&TierMap, &Inputs)      (fields: Detailed Implementation Plan, item 2)
        ──▶ Decision { tier, reason, may_escalate }
        │
        ▼  before each later step
   router::escalate(&TierMap, &Decision, current, &TurnSignals)
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
*   A tier whose model equals the standard model is absent too. So `deep_model = "sonnet"` on `claude-cli` turns the deep tier off. Models are compared by a **model key**: on `claude-cli`, `claude_native_model(id)` (`src/cli_llm.rs:428-450`) with dots turned into dashes and any `[1m]` removed, so `sonnet`, `claude-sonnet-5.5`, `claude-sonnet-5-5` and `anthropic/claude-sonnet-5.5` all have the key `claude-sonnet-5-5`. `claude_native_model` alone isn't enough, because it keeps the user's spelling (`:441-449`). Elsewhere the key is the id as written. The same key is used by `is_active`, the absence checks, `display`, and `Budget`'s match of today's deep rows.
*   An empty standard model on a CLI other than `claude-cli` means "the CLI's default model": no `--model` flag, as today (`src/cli_llm.rs:1697-1698`). Its display name is "the CLI's default model".
*   A map with only one distinct model is **inactive**: the router picks standard, sends no `Event::Model`, and behaves as today (`src/agent.rs:523`).
*   `TierMap::display(tier)` gives the chat name: "Haiku 5.5", "Sonnet 5.5" and "Opus 5.5" for the model keys `claude-haiku-5-5`, `claude-sonnet-5-5` and `claude-opus-5-5` (with or without an `anthropic/` prefix on the HTTP provider), and the raw id for anything else. `router::display_model(id)` does the same for any id, such as the model a fallback answered with.
*   **The one behaviour change in config:** on `claude-cli`, an empty `fast_model` now means Haiku, not the main model. That is Problem 3's fix. RQ1 covers it.
*   `Config::fast_model()` (`src/config.rs:818`) returns the quick tier's id, so its six callers move to Haiku. Its return type changes from `&str` to `String`, because the id is computed; the callers already pass it on as an owned or borrowed string, so each changes by at most a `&`.

### 2. Choosing the tier at the start of a turn

`router::decide(&TierMap, &Inputs) -> Decision` is a pure function. The first matching rule wins.

| # | Condition | Tier | Escalates | Reason shown |
|---|---|---|---|---|
| 1 | The owner pinned the session, or this is a `delegate` child | the pin, or the parent's current tier | no | "pinned"; a child's events are dropped |
| 2 | `routing = "off"`, or the map is inactive | standard | no | none: no event, as today |
| 3 | Budget level Minimal or Spent | quick | no | "the token allowance is nearly used up" (today's text) |
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

**Sub-agents.** A `delegate` child (`src/agent.rs:712-735`) runs on its parent's current tier (rule 1) and is not routed again, because its task text is written by the model. The parent's tier already reflects its pin and its budget level, which the parent reads again before each step. Its `Event::Model` stays dropped (`src/agent.rs:732`).

### 3. Moving up mid-turn

Before each step after the first, the loop calls `router::escalate(&TierMap, &Decision, current: Tier, &TurnSignals) -> Option<(Tier, &'static str)>`. It is pure. The loop holds `current`, a move only goes up from `current`, and at most one move happens per step, so each tier boundary can be crossed only once. `TurnSignals` (its fields are in item 2) is filled from data the loop already has:

*   `tool_errors_since_move` counts tool errors (Terms) one per call, since the turn started or since its last move, whichever is later. An error made on quick therefore never counts toward the move to deep.
*   To count them, `execute_call` (`src/agent.rs:613`) returns its text plus `tool_failed: bool`, true only for an `Err` or a timeout from the tool itself (and a non-zero shell exit, below). A user who declines two approvals does not move a turn to Opus.

| From | Signal | To | Reason shown |
|---|---|---|---|
| quick | step 2 reached, or step 4 at Careful (today's `escalate_after`) | standard | "needed more steps" |
| quick | one tool error | standard | "a tool call failed" |
| standard | two tool errors since the turn started or moved | deep | "two tool calls failed" |
| standard | the repeat nudge fired (`src/agent.rs:558-566`; `RepeatGuard`'s nudge once PR #84 is built) | deep | "it was going in circles" |
| standard | step 8 reached (the ninth model call), and the turn has run `write_file`, `edit_file` or `shell` | deep | "a long change to files" |

*   No escalation when `Decision.may_escalate` is false (rules 1 to 3).
*   No move to deep unless the budget level is Normal and the Opus allowance has tokens left. The loop reads the budget again before each step (`Budget::now`, one small query), so a long turn that crosses into Careful stops moving up.
*   No move into an absent tier: if the map has no deep tier, a standard turn never moves, and no event is sent.
*   **A failing command counts.** The shell returns `Ok` with `[exit code N]` appended when a command fails (`src/tools/shell.rs:124-134`), so `execute_call` sets `tool_failed` for a non-zero exit too. A failing build or test is the coding case where Opus helps most.
*   **The delegate tool's own errors:** "maximum delegation depth reached" is not a tool error; a child turn that fails is.
*   **A tier the provider refuses.** Model fallback stays in the `Llm` layer, where the HTTP provider already has it: `HttpLlm` tries the request's model, then `llm.model` (the standard tier), then `fallback_models` (`src/llm.rs:116-117`, `:376-379`). `CliLlm` gets the same two-step chain: when a step on a model other than the standard one fails with a provider-limit or unknown-model error (`names_a_provider_limit`, `src/cli_llm.rs:2435-2436`), it runs the step once more on the standard model. Today that error ends the turn (`src/agent.rs:529`). Either way, `resp.model` says which model answered. `ChatResponse` gains `fell_back: bool`, set by `HttpLlm` when it answered with a model other than the request's, and by `CliLlm` on its retry. When it is true, the loop sends `Event::Model` with the answering model's display name and the reason "Opus wasn't available", so the chat never names a model that didn't run. Comparing `resp.model` with the request instead would fire on every step, because test and demo models answer as `"scripted"`, `"demo"` or `"replay"` (`src/llm.rs:609`, `:689`, `:726`) and a CLI with no model answers with its provider id (`src/cli_llm.rs:2459-2463`).
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
    *   `GET /api/sessions/{id}` returns the same `model_pin` and `tiers` fields, and `GET /api/settings` returns `tiers` too, for a chat that has no session yet.
    *   A new chat has no session until its first message (`ChatAndApprovalsStores.swift:184-191`), so `POST /api/chat` accepts an optional `model_pin` and stores it on the session it creates.
*   **Mac app.** A menu in the composer: "Auto", "Quick (Haiku 5.5)", "Standard (Sonnet 5.5)", "Deep (Opus 5.5)". The label shows the current choice.
*   **Telegram and the REPL** use the session's pin if it has one. They have no command to set it.
*   **A pin outranks the budget**, as `routing = "off"` does today. At Careful and below, a pinned session's budget notice adds: "This chat is pinned to <model>. Set it to Auto to let Heron choose." with `<model>` the pinned tier's display name. Spent still lets turns run, as today (`src/usage.rs:171`).

#### 4.2 The event

`Event::Model` gains two fields, `{ name, why, tier, pinned }`, and its JSON form becomes `{"type":"model","name","why","tier","pinned"}` (`src/events.rs:192`). The JSON fields are additive, so the web UI (`assets/index.html:727`) keeps working. The Rust matches that name `{ name, why }` gain `..`: the REPL (`src/main.rs:229`) and `tests/integration.rs:651`. The Slint UI has its own event type (`src/bin/desktop/bridge.rs:30-63`) and needs no change.

*   `name` is `TierMap::display(tier)`: the model that runs, not `cfg.llm.model` (Problem 4). `tier` is `"quick"`, `"standard"` or `"deep"`. `pinned` is true when rule 1 chose the tier.
*   It is sent at step 0 whenever the map is active, and at each escalation.
*   **The Mac app** shows it as a line above the reply, for example "Opus 5.5 · asks to change code", in the secondary text style.

#### 4.3 The caps and the Usage tab

*   The daily and monthly caps and the budget levels stay as they are (`src/usage.rs:87-123`). Tokens count the same on every tier (RQ2).
*   Background chores keep their own ledger, `bg_tokens:YYYY-MM-DD` (`src/app.rs:1889-1910`), and run only at Normal. On Haiku they spend less from the same allowance (150,000 tokens a day by default, `src/config.rs:326`).
*   Proactive takeover keeps its per-run `token_limit` (`src/app.rs:2046`).
*   **The Opus allowance (RQ7).** `[agent] deep_tokens_per_day`, 1,000,000 by default, 0 for no limit. No new ledger: Heron already records every call per day and per model (`store.record_usage`, `src/llm.rs:476-479`; `usage_per_model`, `src/store.rs:1670-1675`). `Budget` sums today's rows whose model is the deep tier's id and exposes `deep_left`, which rule 4 and escalation read. Pinned deep turns are recorded in the same rows, so they count, but they are never stopped by it.
*   Usage is recorded per model already (`src/llm.rs:457-481`, the `usage(day, model, …)` table at `src/store.rs:526`), and the Usage tab lists each row with its model (`SettingsView.swift:315-340`). Once requests carry the tier's model, the tab shows the Haiku, Sonnet and Opus split with no change.
*   `GET /api/usage`'s `Report.routing` (`src/usage.rs:234`) now means "the map is active and routing is not off". `Report.main_model` (`:251`) keeps the raw id, because the web and Slint Usage tabs compare it with `fast_model` (`assets/index.html:1016`, `src/bin/desktop/views_slint.rs:1344`). `Report` gains `main_model_name` (the standard tier's display name, for Problem 4), `routing_available` (the map is active), and `deep_model` (the raw id), and `POST /api/usage` accepts it beside `fast_model` (`src/server.rs:1267-1287`).

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

`eval/run_core_tasks.py` gains three options that write keys into the config it generates (`config_toml`, `eval/run_core_tasks.py:176-183`): `--cli-model`, `--fast-model` and `--routing`, plus `--fixtures-root` (Detailed Implementation Plan, item 1). Environment variables can't be used, because `core_env` strips `HERON_*` (`:186-190`).

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

**Where the numbers come from.** Today a run record has one `tokens` total (`eval/run_core_tasks.py:459-461`, from the last `usage` event, `eval/eve_adapter.py:161-163`) and no wall time. E0 adds three optional fields to each run record:

*   `wall_ms`: the time from sending the turn to its `done` event, measured by the direct driver.
*   `tier`: the tier named by the run's first `model` event, or null. Only the live run (§5.6) uses it.
*   `models`: one `{"model", "prompt", "completion"}` per model, read after the run from the `usage` table in that run's own Heron home (`usage(day, model, prompt, completion, calls)`, `src/store.rs:526`), which the runner keeps under `--artifacts`. This gives the per-model split the two-tier baselines need.

**The file formats** (all JSON, committed):

*   `eval/router/labels.json`: `{"cases": {"r01": {"setups": {"haiku": [Run, Run, Run], "sonnet": […], "opus": […], "two_opus": […], "two_sonnet": […]}}, …}, "sufficient": {"r01": "quick"|"standard"|"deep"|null, …}}`, where `Run` is `{"pass": bool, "ttft_ms": int|null, "wall_ms": int, "tier": str|null, "models": [{"model": str, "prompt": int, "completion": int}]}`. The live run adds a sixth setup, `"router"`.
*   `eval/router/replay.json`: an array `[{"id": "r01", "completion": "{\"tier\": \"quick\"}"}, …]`.
*   The text the replay routes is each case's `prompt` field from `cases.md`, with `{web}` replaced by `http://fixture.invalid`.

#### 5.3 The replay

The router's rules are a pure function, so they are scored without model calls. Unit R1 includes an ignored test, `router::replay::tests::replay_cases`, run with `cargo test --locked --lib router::replay::tests::replay_cases -- --ignored`. It reads each case's text from `eval/router/cases.md`, runs `decide` with `typed = true`, an untainted session and level Normal, and writes `eval/router/replay.json`: one `{"id", "completion"}` per case, where `completion` is the string `{"tier": "…"}`.

`eval/router/score.py` passes each completion and its label to `eval/genesis/router_verifier.py`, the verifier that genesis audits (§5.5). The verifier writes the `{"pass", "score", "reward"}` that every genesis verifier in this repository writes (`eval/genesis/limit_verifier.py`), plus `"direction": "exact"|"under"|"over"|"invalid"`. genesis reads `pass`. `score.py` reads `direction` for the under- and over-routing rates. One parser decides both, so a completion the audit would reject can't count in the metrics.

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

The gate is unit G in the Work units table. R2 to R5 depend on it. If it fails, the report names the winning baseline, and the owner's next step is that config change, not this build.

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
| The Opus or Haiku tier is refused by the provider (plan limit, unknown model) | The provider layer falls back to the standard model, as `HttpLlm` already does; `Event::Model` names the model that answered (§3) |
| Opus use grows with no caps set | The daily Opus allowance (RQ7) |
| A switch of model loses the prompt cache | One switch per boundary; PT-R3 measures the cost; it is in the live run's numbers |
| The label runs vary from run to run | 3 runs per setup and case, a 2-of-3 vote, and a tolerance of one case's worth of runs; the live run repeats the gate on the built router |
| A wrong OpenRouter id fails every turn | No built-in HTTP defaults (RQ6); the HTTP provider's `fallback_models` chain still applies (`src/llm.rs:376-378`) |
| Plan quotas on a subscription login are invisible to Heron | Recorded in §5.7; the Opus allowance bounds the share Heron itself chooses |

## Non-Functional Requirements

*   **Security and privacy.** No new tool, host, process or process argument (the CLI fallback reruns the same argv with the standard model). Outside content can't route a turn to deep (G6), and tainted sessions get a floor (RQ3).
*   **Performance.** `router::decide` runs word matches, two counts and a length check on one message, with no model call. It should take well under a millisecond; R1's tests don't time it.
*   **Rollout and rollback.** `routing = "off"` restores "always the main model". On the HTTP provider, leaving `deep_model` empty keeps today's behaviour. On `claude-cli`, `fast_model = "sonnet"` and `deep_model = "sonnet"` together restore today's single model. The schema change adds one nullable column in a new schema version. An older build refuses a database with a newer version (`src/store.rs:609-616`), so going back to an older build means restoring the backup taken before the upgrade (`heron restore`).

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
| Q7 | yes: the schema step in `store.rs`, and the fallback to the standard model in `cli_llm.rs` |

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

1.  Record the owner's answers to RQ1 to RQ7, or note that the defaults stand.
2.  Commit these pieces on the feature branch first, so every unit stays inside its scope:
    *   `pub mod router;` in `src/lib.rs` and an empty `src/router/mod.rs`;
    *   the `[llm] deep_model: String` field (empty by default) in `src/config.rs`;
    *   `..` on the `Event::Model { name, why }` match in `tests/integration.rs:651`, so R2's new event fields still compile there (`..` is allowed when no fields remain);
    *   a `turn_tier: Arc<Mutex<Option<String>>>` field on `ToolCtx` (`src/tools/mod.rs:20-31`), holding a tier's `as_str` name, added to every `ToolCtx { … }` literal as `Default::default()`: `src/agent.rs:302`, `src/mcp.rs:321`, `:356`, `:397`, `src/permissions.rs:627`, the test literals in `src/tools/*.rs`, and `tests/integration.rs:350`. It is a string so the commit needs nothing from R1.
3.  Build E0 and R1 (no model calls). Merge E0.
4.  Run PT-R1 to PT-R3, then the labelling run (§5.2), after owner approval under RQ4. Commit `eval/router/labels.json`, and regenerate and commit `eval/genesis/router-cases.jsonl` from it with `python3 -I eval/genesis/build_router_cases.py`.
5.  Start unit G. If G fails, stop: its report names the winning setup, and the owner's next step is that config change. G never edits the scorer, the cases or the labels to pass.
6.  Build R2, then R3, R4 and R5.
7.  Run the live run (§5.6), after owner approval for about 120 more turns, and commit the new `labels.json` rows. Then units L and V. The branch is not merged before L passes, so RQ1's default never ships untested.
8.  After V, the build session updates `README.md` near `:188`: `deep_model`, `deep_tokens_per_day`, the new meaning of an empty `fast_model` on `claude-cli`, and what `routing = "auto"` now does.
9.  PR #84 is not required. The "going in circles" signal uses the existing repeat nudge (`src/agent.rs:558-566`). When PR #84's `RepeatGuard` lands, it replaces that check in one place.
10. **Out of scope for this build:** tuning the word lists after labelling, and the held-out cases `r41`-`r60` that tuning would need (§5.1). If the gate fails narrowly, that is a follow-up with its own labelling run.

### Target Files to Create/Modify

#### 1. The eval (Create and Modify, E0)

Files: `eval/run_core_tasks.py`, `eval/genesis/json_verifier.py`, `eval/router/`, `eval/genesis/`, `eval/eve/router.yaml`, `eval/tests/`.

*   **`eval/run_core_tasks.py`:**
    *   Options `--cli-model`, `--fast-model` and `--routing`, written by `config_toml` (`:176-183`) as `cli_model` and `fast_model` under `[llm]` and `routing` under `[agent]`.
    *   Option `--fixtures-root`, defaulting to today's `dirname(tasks_file)/fixtures` (`:629`).
    *   Three optional fields in each run record: `wall_ms` (from sending the turn to its `done` event, measured by the direct driver), `models` (one `{"model": str, "prompt": int, "completion": int}` per row of the run's own `heron.db` `usage` table, kept under `--artifacts`), and `tier` (from the run's first `model` event, or null).
*   **`eval/genesis/json_verifier.py`:** `RECORD_KEYS` (`:46`) gains `wall_ms`, `models` and `tier` as optional keys. `SCHEMA` stays 1, because the fields are additive. Its tests gain one record with and one without them.
*   **`eval/router/cases.md`:** 40 cases in the `eval/core-tasks.md` format (§5.1), read with `tasks_md.load_tasks` and checked with `pass_rules.validate_rule`. `eval/check_tasks.py` is not reused: it hard-codes `t01`-`t10`.
*   **`eval/router/fixtures/`:** demo-data fixtures for `r11`-`r40`, plus copies of `eval/fixtures/t01`-`t10` and `eval/fixtures/web/`, so one `--fixtures-root` serves every case.
*   **`eval/router/label.py`:** runs one setup (§5.2) with `run_core_tasks.py --provider claude-cli --driver direct --tasks-file eval/router/cases.md --fixtures-root eval/router/fixtures`, with a separate `--out` and `--artifacts` folder per setup, because each run deletes its own run folder first. `--setup router` runs the built router: `--routing auto` with no `--cli-model` or `--fast-model`, using the binary given by `--heron-bin`. It merges the records into `labels.json` (format in §5.2).
*   **`eval/router/score.py`:** sufficient tiers, the §5.4 metrics, and `--gate` (§5.6). `--gate --live` uses the `router` setup's runs, and its under-routing rate compares each run's `tier` with the case's sufficient tier. Prices come from `eval/router/prices.json`, with `usage.model` mapped as §5.4 says.
*   **`eval/genesis/router_verifier.py`:** follows `eval/genesis/limit_verifier.py`, importing `strict_loads`, `Reject`, `normalize_task` and `ENVELOPE_KEYS` from `json_verifier`. It writes the standard `{"pass", "score", "reward"}` that genesis reads, plus `"direction": "exact"|"under"|"over"|"invalid"`, which `score.py` reads.
*   **`eval/genesis/router.yaml`**, **`eval/genesis/build_router_cases.py`** (the `build_limit_cases.py` pattern; reads `labels.json`, or `eval/router/seed-labels.json` when there is none), **`eval/genesis/router-cases.jsonl`**, **`eval/router/seed-labels.json`** (12 hand-labelled rows), **`eval/router/prices.json`**, **`eval/router/README.md`**.
*   **`eval/eve/router.yaml`:** the EVE task in The eight checks.
*   **`eval/tests/test_router_eval.py`:** `cases_file_has_forty_valid_cases`, `harness_options_reach_the_generated_config`, `models_split_reads_a_fixture_heron_db`, `sufficient_tier_is_lowest_passing_two_of_three`, `verifier_direction_values`, `verifier_known_good_and_bad_answers`, `metrics_on_fixed_labels_and_replay`, `gate_fails_on_each_condition`, `live_gate_reads_run_tiers`, `label_setup_router_passes_no_model_options`, `prices_map_aliases_and_provider_id`.

#### 2. `src/router/mod.rs` (Modify), `src/router/rules.rs`, `src/router/replay.rs` (Create), R1

*   **`mod.rs`:**
    *   `enum Tier { Quick, Standard, Deep }` with `as_str` and `parse`.
    *   `struct TierMap { quick: Option<String>, standard: String, deep: Option<String> }` with `for_config(&Config)`, `model(Tier) -> &str`, `display(Tier) -> String`, `is_active() -> bool`, `present() -> Vec<Tier>`.
    *   `fn model_key(provider: &str, id: &str) -> String` (§1) and `fn display_model(provider: &str, id: &str) -> String`.
    *   `struct Inputs<'a> { text: &'a str, typed: bool, level: Level, pin: Option<Tier>, tainted: bool, routing_off: bool, deep_left: bool }`.
    *   `struct Decision { tier: Tier, reason: &'static str, may_escalate: bool }`; `fn decide(&TierMap, &Inputs) -> Decision`.
    *   `struct TurnSignals { step: usize, tool_errors_since_move: usize, nudged: bool, wrote_files: bool, level: Level, deep_left: bool }`; `fn escalate(&TierMap, &Decision, current: Tier, &TurnSignals) -> Option<(Tier, &'static str)>`.
    *   `Level` is `crate::usage::Level`, unchanged.
*   **`rules.rs`:**
    *   `looks_simple` and `HEAVY`, copied from `src/usage.rs:188-199` unchanged. `HEAVY` keeps its substring matching (its entries are stems such as `analy` and `summar`); R2 deletes the old copy.
    *   `DEEP_WORDS` (§2), matched case-insensitively on word boundaries.
    *   `fn asks_deep(text) -> Option<&'static str>`, which checks in this order and returns the first match's reason:

        | Order | Trigger | Reason |
        |---|---|---|
        | 1 | a code fence, three or more paths, or one of `refactor`, `implement`, `debug`, `migrate`, `write tests`, `fix the bug` | "asks to change code" |
        | 2 | one of `architect`, `design doc`, `implementation plan`, `migration plan`, `step by step` | "asks for a plan" |
        | 3 | one of `proof`, `think hard`, `root cause` | "a hard reasoning question" |
        | 4 | 1,500 characters or more, or three or more requirements | "a long, detailed request" |

    *   A **path** is a whitespace-separated token that contains `/` and not `://`, or ends in `.rs`, `.py`, `.ts`, `.js`, `.swift`, `.go`, `.java`, `.md`, `.toml`, `.json` or `.yaml`. A **requirement** is a line that, after leading spaces, starts with `-`, `*`, `•`, or digits followed by `.` or `)`.
*   **`replay.rs`:** `pub fn parse_cases(markdown: &str) -> Vec<(String, String)>` (id and `prompt`, with `{web}` replaced by `http://fixture.invalid`), and `#[cfg(test)] mod tests` holding `#[ignore] fn replay_cases()`, which reads `eval/router/cases.md` and writes `eval/router/replay.json` (§5.3). Its test path is `router::replay::tests::replay_cases`.
*   **Tests (inline in each file):** `decide_follows_rule_order` (one case per row of §2), `pin_beats_every_budget_level`, `off_beats_spent_budget`, `minimal_and_spent_route_quick_without_escalation`, `careful_caps_deep_at_standard`, `untyped_turn_never_routes_deep_by_words`, `tainted_typed_turn_never_routes_deep_by_words`, `tainted_session_skips_rule_5`, `absent_tier_falls_to_nearest`, `deep_equal_to_standard_is_absent`, `single_model_map_is_inactive`, `model_key_matches_every_spelling`, `escalate_quick_at_step_two`, `escalate_quick_on_tool_error`, `escalate_standard_on_two_errors`, `escalate_standard_on_nudge`, `escalate_standard_on_long_file_turn`, `escalate_crosses_each_boundary_once`, `escalate_skips_deep_below_normal`, `escalate_skips_absent_tier`, `escalate_skips_deep_when_allowance_used_up`, `no_escalation_when_pinned`, `deep_words_match_on_word_boundaries`, `bare_plan_is_not_deep`, `asks_deep_reason_order`, `paths_and_requirements_are_counted`, `looks_simple_keeps_todays_examples` (the prompts of `src/usage.rs:319-332`), `deep_allowance_used_up_routes_standard`, `tier_map_claude_cli_defaults`, `tier_map_http_uses_existing_keys`, `parse_cases_reads_an_inline_sample`, and the ignored `replay_cases`.

#### 3. `src/usage.rs` (Modify, R2)

*   Delete `route()`, `Route`, `looks_simple`, `HEAVY` and the test `simple_requests_go_to_the_cheaper_model_and_real_work_does_not` (`:312-340`). Their only caller is `src/agent.rs:492`; R1's tests cover what that test asserted.
*   `Report` (`:225-257`): `routing` means "the map is active and routing is not off"; it gains `routing_available`, `main_model_name` and `deep_model` (§4.3). `main_model` and `fast_model` stay raw ids.
*   `Budget` gains `deep_left: bool`: true when `deep_tokens_per_day` is 0, or when the tokens in today's `usage` rows whose model key equals the deep tier's key are below `deep_tokens_per_day`. `Budget` is serialised, so `/api/usage` shows it.
*   `Budget::notice` (`:165`) takes the pinned tier's display name, if any, and adds the §4.1 sentence at Careful and below.
*   Tests: `deep_left_sums_todays_deep_rows`, `deep_left_is_true_without_a_limit`, `pinned_notice_names_the_pin`.

#### 4. `src/config.rs` (Modify, R2)

*   `[agent] deep_tokens_per_day: u64`, 1,000,000 by default (RQ7).
*   The config template (`:947` onwards, "these are the defaults"): a `deep_model = ""` line after `fast_model` (`:959`), `deep_tokens_per_day = 1000000`, and new comments for `fast_model` (`:959`, "empty = Haiku on claude-cli, else the main model") and `routing` (`:984`). The doc comments at `:89` and `:217-218` change the same way.
*   `Config::fast_model()` returns `String` (§1).
*   Tests: `deep_model_defaults_empty`, `fast_model_is_haiku_on_claude_cli_when_empty`, `deep_tokens_per_day_defaults_to_one_million`.

#### 5. `src/llm.rs` and `src/cli_llm.rs` (Modify, R2, Q7 for `cli_llm.rs`)

*   **`llm.rs`:** `ChatResponse` (`:76-80`) gains `fell_back: bool`. `HttpLlm::chat` sets it when the answering model isn't the request's (`:376-379`). `ScriptedLlm` (`:698-726`) answers with `req.model` when one is given, else `"scripted"`, and a test helper makes one response say it fell back.
*   **`cli_llm.rs`:**
    *   The two places that turn a limit failure into text (`:2422`, `:2435`) return a typed `CliLimit(String)` error inside the `anyhow::Error`, and a new `CliUnknownModel(String)` is returned when the CLI's error text contains "model" together with "not found", "invalid model", "unknown model" or "does not exist".
    *   `CliLlm::chat` (`:2203-2210`) downcasts the error from `turn_in`. For either type, on a call whose model key isn't the standard tier's, and only if no text was streamed yet (the guard `HttpLlm` has at `src/llm.rs:381`), it runs the call once more with the standard tier's id given explicitly, and returns `fell_back: true`.
    *   Tests: `cli_falls_back_to_standard_on_limit`, `cli_falls_back_to_standard_on_unknown_model`, `cli_does_not_retry_after_streamed_text`, each with a fake CLI script (`fake_bin`, `:2554`).

#### 6. `src/agent.rs` (Modify, R2)

*   **Typed turns:** `run_turn` becomes a thin wrapper over a private `run_turn_inner(&self, session, input, sink, typed: bool)`; `run_turn_typed` calls it with `true`. A parameter rather than an `Agent` field, so sub-agents made with `self.clone()` (`:720`) don't inherit it. Session origin (`store.session_origin`) isn't used, because it belongs to the session, and the owner can type into a session a schedule created.
*   **Session state:** `Session` gains `model_pin: Option<Tier>`, filled by `Session::load` from the `sessions` row, and `Session::ephemeral` gains a `fixed_tier: Option<Tier>` argument, which rule 1 treats as a pin.
*   **Turn start (`:492`):** build the `TierMap`, read `Budget` (with `deep_left`), `session.model_pin` and `session.tainted`, and call `router::decide`.
*   **Step loop:** the bound stays `max_steps`. Before each step after the first, read `Budget` again and call `router::escalate`. Replace the `escalate_after` filter (`:522-528`). Send `Event::Model { name, why, tier, pinned }` at step 0 when the map is active, on every move, and when a response has `fell_back` (§3).
*   **Tool errors:** `execute_call` (`:613`) returns `(String, bool)`, the bool being `tool_failed`. Its callers change with it: the parallel `join_all` at `:548-556` and the loop that pushes results, and `call_tool` (`:590-598`), which workflows use and which keeps returning `String`. The shell tool (`src/tools/shell.rs:124-134`) reports a non-zero exit through a `ToolResult` flag rather than through its text, so a command that prints `[exit code 1]` isn't misread. `delegate` (`:712-742`) returns `(String, bool)`: false for "maximum delegation depth reached", true when the child turn fails.
*   **Delegation:** the loop stores its current tier where `delegate` can read it: `ToolCtx.turn_tier` (added in pre-step 2), set to the current tier's name before each step. `delegate` passes it as the child's `fixed_tier`. The child's `Event::Model` stays dropped (`:732`).
*   **The closing call** (`:580`) uses the current tier's model.
*   **Background calls:** compaction (`:806`) and memory learning (`:831`) already call `cfg.fast_model()`.
*   `Event::Model` (`:28`) gains `tier: String` and `pinned: bool`.
*   Tests: `turn_start_sends_model_event_with_real_name`, `escalation_changes_request_model_between_steps` (with `ScriptedLlm`, asserting `req.model` per step), `budget_is_read_again_before_each_step`, `background_compaction_uses_quick_tier`, `delegate_child_runs_on_parent_tier`, `run_turn_is_untyped_by_default`, `shell_nonzero_exit_is_tool_failed`, `declined_approval_is_not_tool_failed`, `fallback_sends_model_event_with_answering_model`, `no_fallback_event_without_fell_back`.

#### 7. `src/tools/mod.rs`, `src/tools/shell.rs` (Modify, R2)

*   `ToolCtx.turn_tier` and its literals came with pre-step 2; nothing else changes in `mod.rs`.
*   `shell.rs`: the failed-exit flag (item 6).

#### 8. `src/events.rs` (Modify, R2)

*   The `model` event's JSON adds `tier` and `pinned` (`:192`). Test: `model_event_json_has_tier_and_pinned`.

#### 9. `src/store.rs` (Modify, R2, Q7)

*   A new schema step, `to_v4`, appended to `STEPS` (`:185-190`): `ALTER TABLE sessions ADD COLUMN model_pin TEXT`. `set_model_pin(id, Option<Tier>)` and `model_pin(id)`.
*   Update `the_schema_cannot_change_without_a_new_version` (`:2067-2074`) to version 4 and its new hash, and the upgrade test that asserts `schema_version() == 3` (`:2040`).
*   Tests: `schema_step_adds_nullable_pin_column`, `model_pin_round_trips`.

#### 10. `src/server.rs`, `src/main.rs`, `src/telegram.rs`, `src/bin/desktop/views_slint.rs` (Modify, R2)

*   **`server.rs`:** `ChatBody` (`:522-525`) gains `model_pin: Option<String>`. `/api/chat` stores it on a new session and calls `run_turn_typed` (`:601`). `POST /api/sessions/{id}/model` (§4.1). `model_pin` and `tiers` in `GET /api/sessions/{id}` (`:458`); `tiers` and `deep_model` in `GET /api/settings` (`:302`). `POST /api/usage` (`:1267-1287`) accepts `deep_model`, and its `routing` flag no longer depends on `fast_model` being set.
*   **`main.rs`:** the REPL calls `run_turn_typed` (`:289`), and the `Event::Model` match at `:229` gains `..`.
*   **`telegram.rs`:** the owner's messages call `run_turn_typed` (`:564`).
*   **`views_slint.rs`:** the Usage tab enables its routing switch from `routing_available` and sends the switch's state as it is, instead of `routing: false` when `fast_model` is empty (`:1342-1348`).
*   Tests: `session_pin_route_needs_token`, `session_pin_rejects_unknown_tier`, `session_pin_rejects_absent_tier`, `chat_route_runs_typed_turn`, `chat_accepts_model_pin_for_new_session`, `usage_routing_flag_ignores_empty_fast_model`.

#### 11. `src/app.rs`, `src/sources.rs` (Tests only, R2)

*   No call-site changes: reflection (`app.rs:1864`), suggestions (`:1987`), workflow `ask` (`:2240`) and `learn_one` (`sources.rs:498`) already call `cfg.fast_model()`.
*   Test: `background_calls_request_quick_tier` (one `ScriptedLlm` assertion per call site).

#### 12. `tests/integration.rs`, `tests/fixtures/api/` (Modify, R3)

*   Rewrite `usage_is_recorded_and_shapes_model_choice_and_effort` (`:621-702`): every request now carries an explicit model (the standard tier sends `Some("big")`, not `None`), and the reasons at `:666-667` become "a short question" and "needed more steps".
*   New: `api_session_pin_round_trip`, `api_model_event_carries_tier`, `api_chat_with_model_pin`.
*   Re-record `tests/fixtures/api/session.json` (gains `model_pin` and `tiers`) and `settings.json` (gains `tiers` and `deep_model`). No usage fixture is added.

#### 13. Mac app (Modify, R4)

*   `Sources/HeronCore/CoreEvent.swift:27`: `case model(name: String, why: String, tier: String? = nil, pinned: Bool = false)`. The defaults keep `ModelDecodingTests.swift:292` and `AvatarStateMachineTests.swift:132` compiling unchanged, and `.model` patterns with no bindings (`AvatarStateMachine.swift:81`, `ChatAndApprovalsStores.swift:328`) need no change.
*   `Sources/HeronCore/Models.swift`: `SessionDetail` (`:126-141`) gains `modelPin: String?` and `tiers: [TierOption]` (empty by default); `TierOption { tier: String, name: String }`; `CoreSettings` gains `tiers`; `ChatMessage` (`:83`) gains `modelLine: String?`. `Wire.swift:103` and `PreviewData.swift:9` pass the new fields.
*   `Sources/HeronCore/Wire.swift`: decode the event fields (`:419`), `model_pin` and `tiers` (both optional).
*   `CoreClientProtocol.swift`, `CoreClient.swift`, `ManagedCoreClient.swift`, `PreviewCoreClient.swift`: `func setModelPin(session: String, pin: String) async throws -> SessionDetail`, and `send(…, modelPin: String?)` for a new chat.
*   `Sources/HeronUI/Stores/ChatAndApprovalsStores.swift`: keep the event (`:328`) as the turn's `modelLine` instead of dropping it; hold a new chat's menu choice until its first `send`, then pass it as `modelPin`.
*   `Sources/HeronUI/Views/ChatView.swift`: the model line above each reply, and the composer menu, filled from the session's `tiers`, or from `CoreSettings.tiers` before the first message.
*   Tests (`macos/HeronMac/Tests/`): `test_modelEventDecodesTierAndPinned`, `test_modelLineShowsNameAndReason`, `test_composerMenuListsPresentTiersOnly`, `test_pinMenuCallsSetModelPin`, `test_newChatSendsHeldPin`.

#### 14. The web UI (Modify, R5)

*   `assets/index.html`: the Usage page enables the routing switch from `routing_available` and stops comparing `fast_model` with `main_model` (`:1016-1022`); it shows `main_model_name`.

### Verification Plan

*   **Build:** `cargo build --locked`, `cargo clippy --locked --all-targets -- -D warnings`, `swift build --package-path macos/HeronMac`.
*   **Tests:** `cargo test --locked`, `swift test --package-path macos/HeronMac`, `python3 -I -m unittest discover -s eval/tests -t eval`. The cargo suite runs outside the Claude Code Bash sandbox, because inside it some unrelated tests fail for lack of local ports and nested Seatbelt.
*   **Test quality (testteeth):** `testteeth run --base main --fail-under 70`, run by the skein supervisor. Aim for no surviving mutants in `src/router/`, which is pure logic; the check enforces only the 70% floor. testteeth has no Swift support here, so the Swift tests are checked by name and in the owner's manual pass.
*   **Evaluator audit (genesis):** `genesis audit-evaluator eval/genesis/router.yaml --suite json` exits 0.
*   **The gates:** `python3 -I eval/router/score.py --gate` and `--gate --live` exit 0 on the committed labels.
*   **EVE:** the EVE row of The eight checks.

## Work units

Units whose scopes share a prefix are ordered by depends-on. In the check column, `\|` stands for a shell `|`. Each check names every test its items list. The skein supervisor runs the cargo and Swift checks (Definitions).

| id | what to build | scope | depends on | check | model role |
|---|---|---|---|---|---|
| E0 | The eval: harness options and fields, cases and fixtures, label and score scripts, genesis spec and verifier, EVE task, tests (item 1) | `eval/**` | none | `python3 -I -m unittest discover -s eval/tests -t eval && test "$(grep -c '^## r[0-9][0-9]: ' eval/router/cases.md)" -eq 40 && genesis audit-evaluator eval/genesis/router.yaml --suite json` | feature, refactoring: sonnet high |
| R1 | Router module, rules, case parser and replay test (item 2) | `src/router/**` | E0 | `cargo test --locked --lib router:: && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib -- decide_follows_rule_order pin_beats_every_budget_level off_beats_spent_budget minimal_and_spent_route_quick_without_escalation careful_caps_deep_at_standard untyped_turn_never_routes_deep_by_words tainted_typed_turn_never_routes_deep_by_words tainted_session_skips_rule_5 absent_tier_falls_to_nearest deep_equal_to_standard_is_absent single_model_map_is_inactive model_key_matches_every_spelling escalate_quick_at_step_two escalate_quick_on_tool_error escalate_standard_on_two_errors escalate_standard_on_nudge escalate_standard_on_long_file_turn escalate_crosses_each_boundary_once escalate_skips_deep_below_normal escalate_skips_absent_tier escalate_skips_deep_when_allowance_used_up no_escalation_when_pinned deep_words_match_on_word_boundaries bare_plan_is_not_deep asks_deep_reason_order paths_and_requirements_are_counted looks_simple_keeps_todays_examples deep_allowance_used_up_routes_standard tier_map_claude_cli_defaults tier_map_http_uses_existing_keys parse_cases_reads_an_inline_sample replay_cases` | feature, refactoring: sonnet high |
| G | The gate: write the replay and score it on the committed labels; on failure, stop and report (§5.6, pre-step 5) | `eval/router/replay.json` | E0, R1, and committed `labels.json` | `test -s eval/router/labels.json && cargo test --locked --lib router::replay::tests::replay_cases -- --ignored && test -s eval/router/replay.json && python3 -I eval/router/score.py --gate` | judgment and prose: opus high |
| R2 | Wiring: usage, config, llm and CLI fallback, agent loop, tools, events, store, routes, Slint Usage tab (items 3-11) | `src/**` | G | `cargo test --locked --lib --bins && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib --bins -- deep_left_sums_todays_deep_rows deep_left_is_true_without_a_limit pinned_notice_names_the_pin deep_model_defaults_empty fast_model_is_haiku_on_claude_cli_when_empty deep_tokens_per_day_defaults_to_one_million cli_falls_back_to_standard_on_limit cli_falls_back_to_standard_on_unknown_model cli_does_not_retry_after_streamed_text turn_start_sends_model_event_with_real_name escalation_changes_request_model_between_steps budget_is_read_again_before_each_step background_compaction_uses_quick_tier delegate_child_runs_on_parent_tier run_turn_is_untyped_by_default shell_nonzero_exit_is_tool_failed declined_approval_is_not_tool_failed fallback_sends_model_event_with_answering_model no_fallback_event_without_fell_back model_event_json_has_tier_and_pinned schema_step_adds_nullable_pin_column model_pin_round_trips session_pin_route_needs_token session_pin_rejects_unknown_tier session_pin_rejects_absent_tier chat_route_runs_typed_turn chat_accepts_model_pin_for_new_session usage_routing_flag_ignores_empty_fast_model background_calls_request_quick_tier` | hardest tasks: opus high |
| R3 | Integration tests and API fixtures (item 12) | `tests/**` | R2 | `cargo test --locked --test integration && bash eval/require-tests.sh --test integration -- usage_is_recorded_and_shapes_model_choice_and_effort api_session_pin_round_trip api_model_event_carries_tier api_chat_with_model_pin` | feature, refactoring: sonnet high |
| R4 | Mac app: model line, composer pin menu (item 13) | `macos/HeronMac/**` | R3 | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && test "$(swift test list --package-path macos/HeronMac \| grep -cE '/test_(modelEventDecodesTierAndPinned\|modelLineShowsNameAndReason\|composerMenuListsPresentTiersOnly\|pinMenuCallsSetModelPin\|newChatSendsHeldPin)$')" -eq 5` | feature, refactoring: sonnet high |
| R5 | Web UI Usage page (item 14) | `assets/**` | R2 | `grep -q 'routing_available' assets/index.html && ! grep -q 'fast_model === u.main_model' assets/index.html` | feature, refactoring: sonnet high |
| L | The live run passes the gate; on failure, stop and report (§5.6) | (none: reads only) | R2, and the committed live rows of `labels.json` | `python3 -I eval/router/score.py --gate --live` | judgment and prose: opus high |
| V | Verify the whole branch | (none: reads only) | R3, R4, R5, L | `cargo clippy --locked --all-targets -- -D warnings && cargo test --locked && swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && python3 -I -m unittest discover -s eval/tests -t eval && genesis audit-evaluator eval/genesis/router.yaml --suite json && python3 -I eval/router/score.py --gate && python3 -I eval/router/score.py --gate --live && testteeth run --base main --fail-under 70 -q` | judgment and prose: opus high |

--------------------------------------------------------------------------------

*Drafted with EGM*
