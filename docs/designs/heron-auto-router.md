# Heron auto router: Haiku, Sonnet or Opus for each turn

*   **Author:** fernandogarzaaa (drafted with Claude)
*   **Status:** Draft
*   **Date:** 2026-11-23
*   **Read at:** `main` = `c9dd12c`. Line numbers below are from that commit.
*   **Related:** [heron-harness-safety.md](https://github.com/fernandogarzaaa/HERON-Project/pull/84) (PR #84, not merged) defines `RepeatGuard`, which this doc uses as an escalation signal. PR #72 (open) makes the Mac app show the active model in Settings.
*   *Drafted with EGM.*

--------------------------------------------------------------------------------

## Overview

The owner asked: "is a complicated task routed through Opus or just Sonnet? I think we should have an auto router." Today the answer is "just Sonnet". This doc adds a router that picks one of three tiers for every turn:

*   **Quick** (Haiku 5.5 on Claude): short replies and background chores such as memory learning and summaries.
*   **Standard** (Sonnet 5.5): the default.
*   **Deep** (Opus 5.5): multi-step work, planning, coding and hard reasoning.

The router also moves a turn up a tier partway through when the turn proves hard. It respects the token caps, shows the chosen model and the reason in the chat, lets the user pin a model, and works for OpenRouter as well as the Claude CLI. An eval proves it picks well before it ships on.

The router replaces the body of the existing `usage::route` (`src/usage.rs:203`). It does not add a second router.

**Terms used throughout:**

*   A **tier** is one of `quick`, `standard` or `deep`. A tier is not a model. Each provider maps the three tiers to its own model ids.
*   A **turn** is one call to `Agent::run_turn` (`src/agent.rs`). A **step** is one model call inside that turn. With the `claude-cli` provider, each step is a separate `claude -p` process (`src/cli_llm.rs:247-264`, `--max-turns 1`).
*   **Escalation** is a move to a higher tier between two steps of the same turn.
*   A **pin** is the user's choice of one tier for a chat session, which turns the router off for that session.

## The Problem

1.  **Nothing ever reaches Opus.** With `provider = "claude-cli"` and an empty `[llm] cli_model`, `argv` falls back to `"sonnet"` (`src/cli_llm.rs:1689-1695`). The owner's config has `cli_model` empty, so every turn runs on Sonnet 5.5, however hard it is.
2.  **The existing router only routes down, and only when configured.** `usage::route` (`src/usage.rs:203-221`) does nothing unless `fast_model` is set and differs from `model`. When it is set, it sends requests that `looks_simple` (280 characters or fewer, no code fence, no "heavy" word, `src/usage.rs:188-199`) to `fast_model`, and moves back to the main model after 2 steps (`src/agent.rs:522`). It has no tier above the main model.
3.  **Background chores use the wrong model id on the CLI.** Compaction, memory learning, reflection, suggestions, workflow `ask` steps and source learning all call `cfg.fast_model()` (`src/agent.rs:806`, `:831`, `src/app.rs:1864`, `:1987`, `:2240`, `src/sources.rs:498`). With `fast_model` empty that returns `llm.model`, an HTTP id such as `anthropic/claude-sonnet-5.5` (`src/config.rs:282`, `:818-819`). `claude_native_model` strips the prefix (`src/cli_llm.rs:428-450`), so the chores run on Sonnet, not on a cheap model.
4.  **The chat names the wrong model.** `Event::Model` names `cfg.llm.model` when the main model is chosen (`src/agent.rs:525`, `:527`), but the CLI runs `cli_model` or `"sonnet"`. The Mac app decodes the event and drops it (`macos/HeronMac/Sources/HeronCore/Wire.swift:419`, `Sources/HeronUI/Stores/ChatAndApprovalsStores.swift:328`).
5.  **The user can't choose a model for one chat.** Sessions have no model column (`src/store.rs:194-196`), `POST /api/chat` takes only `session_id` and `message` (`src/server.rs:521-524`), and the Mac Model tab picks a provider only (`macos/HeronMac/Sources/HeronUI/Views/SettingsView.swift:164-205`).

**Why not just set `cli_model = "opus"`?** It is the one-line fix, and Opus 5.5's list price is only twice Sonnet's ($4 / $20 against $2 / $10 per million tokens in and out, Anthropic's price list of 2026-11-03). Two things make it worse than a router:

*   Background chores would run on Opus too, unless `fast_model` is also set. Haiku 5.5 is $0.10 / $0.50, 40 times cheaper than Opus on input.
*   Opus turns are slower. A "thanks" or "what's on my calendar" reply waits for the largest model.

The eval keeps "Opus everywhere" and "Opus everywhere at low effort" as baselines (§5), so the owner can see whether the router earns its complexity. If either baseline wins on cost per passed task, the build stops and that baseline ships as a config change.

## Goals

*   **G1.** Every turn at depth 0 gets a tier from the router, unless the session is pinned. The router decides from the request and the session state, with no model call, in under 1 ms.
*   **G2.** Background chores run on the quick tier. Each call site names its tier in code.
*   **G3.** A turn moves up one tier when a fixed signal fires (§3). It moves up at most twice and never moves down.
*   **G4.** The deep tier is used only while the token budget level is Normal (`src/usage.rs:113-121`), unless the session is pinned to deep. At Minimal and below, the router keeps today's rule: quick, with no escalation.
*   **G5.** The chat shows the real model name and a one-line reason at the start of each turn and at each escalation. The Mac app shows it too.
*   **G6.** The user can pin a chat session to Auto, Quick, Standard or Deep from the Mac app. The pin survives a restart.
*   **G7.** The router works with any provider that has a tier map: `claude-cli` (built in), the HTTP provider (OpenRouter and other OpenAI-compatible endpoints) when the owner fills in the map, and `codex-cli` when the owner fills in the map. A provider with no tier map runs every turn on its one model, as today.
*   **G8.** On the eval in §5, the router's under-routing rate is 10% or less, and its cost per passed task is lower than "Opus everywhere". Its pass rate is at least Sonnet-everywhere's.

## Non-Goals

*   **A model call to classify requests.** A Haiku classifier adds a step and its tokens to every turn. It is Alternative B, and a Future Improvement if the eval shows the rules under-route.
*   **Routing between providers.** The router picks a tier inside the active provider. Moving a turn from Claude to OpenRouter is the job of `fallback_models` (`src/config.rs:88`).
*   **Moving a turn down a tier.** A turn that escalated stays up. Moving down mid-turn would lose the prompt cache twice and confuse the reason line.
*   **Fable 5.x.** `claude_native_model` knows `fable` (`src/cli_llm.rs:440`), but it is not a tier here. Its list price is twice Opus's.
*   **Dollar budgets.** Caps stay in tokens, as the owner set them. The Usage tab adds a dollar estimate for information only (§4.4).
*   **Per-message effort, thinking display, or the Anthropic Messages API.** Heron has no Anthropic API client (`src/llm.rs` is OpenAI-compatible only). The CLI's `--effort` flag is the one effort control used.

## Owner questions

Each question has a proposed default. The build uses the default unless the owner changes it.

*   **RQ1, router on by default for `claude-cli`. Proposed: yes, after the eval gate passes.** `[agent] routing` already defaults to `"auto"` (`src/config.rs:329`). With this change, `"auto"` on `claude-cli` starts sending hard turns to Opus without any config edit. The alternative is a new value, `routing = "tiers"`, that the owner turns on.
*   **RQ2, effort per tier. Proposed: quick = Haiku at `low`, standard = Sonnet with no `--effort` flag (the CLI's default), deep = Opus at `high`.** Opus 5.5 defaults to `medium` effort, one level below Opus 5. The eval measures deep at `medium` and at `high` and keeps the cheaper one if pass rates are within one case.
*   **RQ3, how the caps count Opus tokens. Proposed: as today, one token is one token on every tier.** The alternative weights each token by its tier's list price relative to Sonnet (Opus 2, Haiku 0.05), so a cap means the same spend whatever the mix. Weighting changes what the owner's existing caps mean, so it is not the default.
*   **RQ4, a floor for tainted sessions. Proposed: a tainted session never routes below standard.** A smaller model is more likely to follow instructions injected through web pages or email. This is a judgment, not a measured fact: no eval in this repository compares injection rates across tiers.
*   **RQ5, eval spend during the G9 trial (until 2026-12-05). Proposed: the labelling run (§5.2, about 360 turns) waits until after 2026-12-05, or until the owner says go.** The offline replay and the genesis audit spend nothing and run in CI.
*   **RQ6, where the pin lives. Proposed: per chat session, set from the composer, plus a global default ("Auto") in Settings › Model.** The alternative is one global pin.
*   **RQ7, OpenRouter tier ids. Proposed: no built-in defaults.** The owner fills `[llm.tiers]` for HTTP providers. OpenRouter's ids for the 5.5 models are not verified in this repository, and a wrong id fails every turn.

## Prototype checks

These run before the build (Pre-steps). Each records its output in the build PR.

*   **PT-R1.** `claude -p --model haiku --effort low` and `claude -p --model opus --effort high` both answer with Heron's fixed flags (`src/cli_llm.rs:247-264`) under the owner's subscription login. The installed CLI's `--help` lists `--effort <level>`; whether every alias accepts every level is unverified.
*   **PT-R2.** The CLI's `stream-json` result reports which model answered, so `Event::Model` can name the model that ran rather than the one requested. If it doesn't, the event names the requested model, as today.
*   **PT-R3.** Measure prompt-cache reuse across steps for one model, and the first step after a model switch. Each `claude -p` step starts a new process with the whole transcript flattened to text (`render_prompt`, `src/cli_llm.rs:878`). Heron reads only `input_tokens` and `output_tokens` from the CLI (`src/cli_llm.rs:1427-1450`), so cache reads are not visible today. PT-R3 reads the raw `usage` object to see whether caching happens at all.

## Critical User Journeys (CUJs)

### CUJ 1: A quick question

*   **Given:** `provider = "claude-cli"`, `routing = "auto"`, the session is not pinned or tainted, and the budget level is Normal.
*   **When:** the user types "what's on my calendar tomorrow?"
*   **Then:** the turn runs on Haiku 5.5. The chat shows "Haiku 5.5 · a short question". If the turn needs a tool error retry or a third step, it moves to Sonnet and the chat shows "Sonnet 5.5 · needed more than two steps".

### CUJ 2: A coding task

*   **Given:** the same setup.
*   **When:** the user asks "refactor the mail sync so retries back off, and add tests".
*   **Then:** the turn runs on Opus 5.5 at `high` effort. The chat shows "Opus 5.5 · asks to change code".

### CUJ 3: A standard turn that proves hard

*   **Given:** the same setup.
*   **When:** the user asks "why did last night's backup job fail?" The router picks standard. Over the next steps, two tool calls fail.
*   **Then:** before the next step, the turn moves to Opus. The chat shows "Opus 5.5 · two tool calls failed". The rest of the turn stays on Opus.

### CUJ 4: The budget runs low

*   **Given:** the daily cap is 25% or less left (level Careful).
*   **When:** the user asks a coding question.
*   **Then:** the router picks standard, not deep. The chat shows "Sonnet 5.5 · Opus is paused while the daily allowance is low". At Minimal, the router picks quick with no escalation, as it does today.

### CUJ 5: The user pins Opus

*   **Given:** a chat session.
*   **When:** the user opens the model menu in the composer and picks "Deep (Opus 5.5)".
*   **Then:** every turn in that session runs on Opus, including at Careful and Minimal, and the chat shows "Opus 5.5 · pinned". At Spent, turns stop as today. Other sessions stay on Auto.

### CUJ 6: OpenRouter

*   **Given:** `provider = "http"`, `base_url` set to OpenRouter, and `[llm.tiers]` filled with three model ids.
*   **When:** the user sends a coding task.
*   **Then:** the request goes out with the deep tier's id. With `[llm.tiers]` empty, the HTTP provider keeps today's behaviour: `fast_model` is the quick tier, `model` is standard, and there is no deep tier.

## Technical Plan (Architecture)

```text
 POST /api/chat, Telegram, schedule, workflow agent step, proactive takeover
        │
        ▼
 Agent::run_turn (src/agent.rs)
   Budget::now ──▶ level                               session.model_pin
        │                                                    │
        ▼                                                    ▼
   router::decide(Inputs { text, level, pin, tainted, last_tier, context_items }) ──▶ Decision { tier, reason }
        │
        ▼  each step
   router::escalate(&decision, &TurnSignals { step, tool_errors, repeat_nudged, writes }) ──▶ maybe a higher tier
        │                                    (Event::Model on change)
        ▼
   TierMap::model(tier) ──▶ ChatRequest { model: Some(id), effort: tier effort }
        │
        ├─▶ CliLlm: --model <alias> [--effort <level>]
        └─▶ HttpLlm: "model": <id>

 background call sites (compaction, learning, reflection, suggestions, workflow ask, sources)
        └─▶ TierMap::model(Tier::Quick)
```

### 1. Tiers and the tier map

A new module, `src/router/`, holds `enum Tier { Quick, Standard, Deep }` and `struct TierMap`. `TierMap::for_config(&Config) -> TierMap` resolves the three tiers for the active provider:

| Provider | Quick | Standard | Deep | Source |
|---|---|---|---|---|
| `claude-cli` | `haiku`, effort `low` | `cli_model`, or `sonnet` when empty | `opus`, effort `high` | built in, overridden by `[llm.tiers]` |
| `http` | `[llm.tiers].quick`, else `fast_model` | `[llm.tiers].standard`, else `model` | `[llm.tiers].deep`, else none | config |
| `codex-cli` and other CLIs | `[llm.tiers].quick` | `[llm.tiers].standard`, else `cli_model` | `[llm.tiers].deep` | config |

*   A tier with no model is **absent**. The router never picks an absent tier: it picks the nearest present tier below it, or above it when nothing is below.
*   A provider whose three tiers resolve to the same model has a single tier, and the router is inactive for it. `Event::Model` is then never sent, as today (`src/agent.rs:523`).
*   `[llm.tiers]` is a new optional table: `quick`, `standard`, `deep` (strings) and `quick_effort`, `standard_effort`, `deep_effort` (strings, one of `low`, `medium`, `high`, `xhigh`, `max`, or empty for no flag).
*   `TierMap::display(tier)` gives the chat name: "Haiku 5.5", "Sonnet 5.5", "Opus 5.5" for the built-in Claude ids, and the raw id for anything else.

`Config::fast_model()` (`src/config.rs:818`) is kept for existing callers outside this change, but every call site in Problem 3 moves to `TierMap::model(Tier::Quick)`. That fixes the HTTP-id-on-the-CLI gap.

### 2. Choosing the tier at the start of a turn

`router::decide(&Inputs) -> Decision` is a pure function. Rules apply in this order. The first match wins.

| # | Condition | Tier | Reason shown |
|---|---|---|---|
| 1 | The session is pinned | the pin | "pinned" |
| 2 | Budget level Spent | none: the turn stops, as today | (today's notice) |
| 3 | Budget level Minimal | quick, no escalation | "the token allowance is nearly used up" (today's text) |
| 4 | `routing = "off"`, or the tier map has one tier | standard | none: no event, as today |
| 5 | The text asks for deep work (deep words, below) | deep, or standard at Careful | "asks to change code", "asks for a plan", "a long, detailed request" or "a hard reasoning question"; at Careful, "Opus is paused while the allowance is low" |
| 6 | The previous turn in this session ended on deep, and this text is 280 characters or fewer | deep, or standard at Careful | "a follow-up to deep work" |
| 7 | `looks_simple(text)` (`src/usage.rs:188`), no context items, and the session is not tainted | quick | "a short question" |
| 8 | otherwise | standard | "an everyday request" |

**Deep work (rule 5)** is any of:

*   a code fence, or three or more file paths;
*   one of the deep words, matched case-insensitively on word boundaries: `refactor`, `implement`, `debug`, `migrate`, `architect`, `design doc`, `plan`, `step by step`, `prove`, `think hard`, `root cause`, `write tests`, `fix the bug`;
*   1,500 characters or more;
*   three or more numbered or bulleted requirements.

The word list lives in one `const` in `src/router/rules.rs`, next to `looks_simple`'s `HEAVY` list, which moves there too. The eval (§5) tunes both lists.

**What the router reads.** Only the text the user typed: the `input` argument of `run_turn`. Chat context items (the `context` of `POST /api/chat`, which heron-v1-features.md B2 classes as Outside `ContentEntry::ChatContext`; B2 is not built on `main`) count only as present or absent (rule 7). Their words never match deep words. A pasted web page that says "think hard" therefore can't buy an Opus turn.

**Tainted sessions (RQ4).** Rule 7 doesn't apply to a tainted session, so it never starts on quick. Escalation still applies, within the caps.

**Sub-agents.** A `delegate` child calls `run_turn` with its own task text (`src/agent.rs:712-735`) and is routed on that text. It inherits the parent's pin and its taint floor. Its `Event::Model` stays dropped (`src/agent.rs:732`).

### 3. Moving up mid-turn

Before each step after the first, the loop calls `router::escalate(&decision, &signals) -> Option<(Tier, &'static str)>`. `TurnSignals` is filled by the loop from data it already has.

| From | Signal | To | Reason shown |
|---|---|---|---|
| quick | step 2 reached (today's `escalate_after = 2`, or 4 at Careful) | standard | "needed more than two steps" |
| quick | a tool call returned an error | standard | "a tool call failed" |
| standard | two tool calls in this turn returned errors | deep | "two tool calls failed" |
| standard | the loop nudge fired: the existing repeat check (`src/agent.rs:558-566`), or `RepeatGuard`'s nudge once PR #84 lands | deep | "it was going in circles" |
| standard | step 8 reached and the turn has made a `write_file`, `edit_file` or `shell` call | deep | "a long change to files" |

*   A move happens at most once per tier boundary, so a turn changes model at most twice.
*   Escalation to deep is skipped unless the budget level is Normal (G4). A pinned session never escalates.
*   Each move sends `Event::Model` with the new model and reason (G5).
*   The prompt cache is per model. The first step on the new model pays full input price for the whole transcript (PT-R3 measures this).

**The step limit.** `budget.max_steps` stays per turn (`src/usage.rs:130-146`). An escalation does not reset the step count.

**The closing call.** When a turn hits its step limit, the closing call today uses the fast model only when there is no escalation (`src/agent.rs:580`). It now uses the turn's current tier.

### 4. Controls, visibility and usage

#### 4.1 The pin

*   **Store.** `sessions` gains a nullable column `model_pin TEXT` with values `quick`, `standard` or `deep`. Null means Auto.
*   **API.**
    *   `POST /api/sessions/{id}/model` with `{"pin": "auto"|"quick"|"standard"|"deep"}` sets it. `auto` writes null.
    *   `GET /api/sessions/{id}` returns `model_pin`, plus `tiers`: the display name of each present tier, so the menu shows only what this provider has.
*   **Mac app.** A menu in the composer: "Auto", "Quick (Haiku 5.5)", "Standard (Sonnet 5.5)", "Deep (Opus 5.5)". The label shows the current choice.
*   **Settings › Model** gains "Default model for new chats: Auto". This is `[agent] default_pin`, empty by default. New sessions copy it.
*   **Telegram and the REPL** are unchanged: they use the session's pin if it has one.

#### 4.2 The event

`Event::Model` gains two fields: `{ name, why, tier, pinned }`. Its JSON form becomes `{"type":"model","name","why","tier","pinned"}` (`src/events.rs:192`). Both new fields are additive, so the REPL, the Slint UI and the web UI keep working unchanged.

*   `name` is `TierMap::display(tier)`, the model that runs, not `cfg.llm.model` (fixes Problem 4). If PT-R2 finds the CLI reports the model it actually used, a mismatch is logged.
*   The event is sent at step 0 whenever the router is active, and at each escalation.
*   **The Mac app** shows the event as a small line above the reply, "Opus 5.5 · asks to change code", in the secondary text style. Today it drops the event.

#### 4.3 The caps

*   The daily and monthly caps and the budget levels stay as they are (`src/usage.rs:87-123`). Tokens count the same on every tier (RQ3).
*   Background chores keep their own ledger, `bg_tokens:YYYY-MM-DD` (`src/app.rs:1889-1910`), and still run only at Normal. Moving them to quick lowers what they spend from the same 150,000-token default allowance.
*   Proactive takeover keeps its per-run `token_limit` (`src/app.rs:2046`). A takeover turn is routed like any other turn.

#### 4.4 The Usage tab

*   `usage::Report.models` already lists each model's tokens this month (`src/usage.rs:229-230`). It gains a fourth number, an **estimate at API list price** in US dollars, from a fixed table in `src/router/prices.rs` covering the three built-in Claude ids. Other ids show no estimate.
*   The Mac Usage tab (`SettingsView.swift:315-340`) shows a "By model" group under the daily list, with each model's tokens in and out and the estimate.
*   With a CLI subscription login, the tab says "Estimate at API list prices. Your plan bills differently." With a CLI login, the real limits are the plan's quotas, which Heron can't see.

### 5. The eval

The router is a pure function of its inputs, so its decisions can be replayed offline at no cost. Only the labels need model runs.

#### 5.1 The cases

`eval/router/cases.md` holds 40 cases in the format of `eval/core-tasks.md` (a `## rNN: <title>` heading and one JSON block each), with machine-checkable pass rules (`eval/pass_rules.py`: `file_contains`, `file_present`, `file_absent`, `reply_matches`, `no_request`).

*   The ten core tasks `t01`-`t10`.
*   Ten quick cases: questions with one checkable answer in a fixture file (`reply_matches`).
*   Ten standard cases: one- or two-tool tasks on fixtures (read, summarise into a file, rename).
*   Ten deep cases: fixture repositories with a failing test to fix, a refactor with a test that must still pass, a plan written to a file with required sections.

Every fixture uses demo data only.

#### 5.2 The labels

`eval/router/label.py` runs each case on each of the three tiers, 3 runs per tier, with the router pinned to that tier, through `eval/run-core-tasks.sh --provider claude-cli`. That is 40 × 3 × 3 = 360 turns (RQ5). A case's **sufficient tier** is the lowest tier that passes at least 2 of its 3 runs. A case no tier passes is dropped from the routing metrics and listed in the report. Labels and per-run tokens, `ttft_ms` and wall time go to `eval/router/labels.json`, which is committed.

#### 5.3 The replay and the metrics

`eval/router/replay.py` calls `heron route --explain --json` (a new subcommand, §Detailed Implementation) on each case's text and compares the tier with the label.

| Metric | Definition | Gate |
|---|---|---|
| Under-routing rate | cases where the router's tier is below the sufficient tier | ≤ 10% |
| Over-routing to deep | cases routed deep whose sufficient tier is quick or standard | ≤ 30%, reported |
| Routed pass rate | mean pass rate of the router's tier for each case, from the label runs | ≥ Sonnet-everywhere's |
| Cost per passed task | tokens × list price (§4.4), summed, over passed runs | below Opus-everywhere's |
| Latency | median and 90th percentile of `ttft_ms` and wall time of the router's tier | reported |

Baselines are computed from the same label runs: Haiku, Sonnet and Opus everywhere. "Opus everywhere at `low` effort" needs one more set of 40 × 3 runs, included in the labelling run.

**Mid-turn escalation is not in the replay**, because the label runs are pinned. A second, optional run, `label.py --routed`, runs each case 3 times with the router on and reports how often escalation fired and the pass rate. It is not a gate.

#### 5.4 genesis

genesis audits the grader, not the router. `eval/genesis/router.yaml` follows `eval/genesis/limit-classifier.yaml`: the dataset is `eval/genesis/router-cases.jsonl`, one row per case with the sufficient tier as the reference. The verifier, `eval/genesis/router_verifier.py`, accepts only the strict answer `{"tier": "<tier>"}` that equals the label. The `json` suite probes that the verifier can't be passed by duplicate keys, prose around the JSON, extra fields or a schema-only check. Exit 0 (SOUND) is required.

#### 5.5 The cost and latency numbers

All figures below are **estimates, unverified**, computed from list prices and an assumed turn shape. The eval replaces them with measurements.

*   **Assumed shape.** A standard tool-using turn of 6 steps, each sending about 12,000 input tokens (the whole transcript, `render_prompt`) and returning 400. A quick reply is 1 step of 8,000 in and 150 out.
*   **Per turn, with no caching:**

    | Turn | Haiku 5.5 | Sonnet 5.5 | Opus 5.5 |
    |---|---|---|---|
    | Quick reply (8,000 in, 150 out) | $0.0009 | $0.018 | $0.035 |
    | 6-step tool turn (72,000 in, 2,400 out) | $0.008 | $0.168 | $0.336 |

*   **An escalation** from Sonnet to Opus at step 4 of 6 adds one uncached Opus step on top of the remaining Opus steps.
*   **Latency.** Smaller models usually return the first token sooner, but this repository has no per-model `ttft_ms` measurements yet. The baseline in `eval/baseline/summary.json` is Sonnet only.

## Alternatives Considered

### Alternative A: Set `cli_model = "opus"` (or Opus everywhere at low effort)

*   **Pros:** One config line. No code. Every hard turn gets Opus.
*   **Cons:** Every easy turn and every background chore pays Opus prices and Opus latency, unless `fast_model` is also set. With `fast_model = "haiku"`, today's router already routes short requests down, so this is the closest config-only option.
*   **Reason for rejection:** Not rejected outright. It is the eval's main baseline. If it beats the router on cost per passed task at the same pass rate, the owner gets the config change instead of this build.

### Alternative B: A Haiku classifier call before each turn

*   **Pros:** Reads meaning, not keywords. Handles requests the word lists miss.
*   **Cons:** One extra model call on every turn, which adds latency to quick replies, the case it is meant to speed up. Its decisions vary run to run, so the eval can't replay them for free. A classifier that reads pasted outside content can be steered by it.
*   **Reason for rejection:** The rules are free, deterministic and replayable. If the eval's under-routing rate stays above 10% after tuning the word lists, a classifier is the next step (Future Improvements).

### Alternative C: Let the model ask for a bigger model

*   **Pros:** The model knows when it is stuck.
*   **Cons:** An `escalate` tool is a lever outside content can pull: an injected "call escalate" in a web page spends Opus tokens. Models also call such tools unevenly.
*   **Reason for rejection:** Harness signals (tool errors, the loop nudge, step count) are facts the model can't fake, and they are already in the loop.

### Alternative D: Route by Claude Code's own `--fallback-model` or "opusplan" style modes

*   **Pros:** Uses the CLI's built-in behaviour.
*   **Cons:** Heron runs one step per process with tools off (`--tools ""`, `--max-turns 1`), so CLI-side planning modes never see a multi-step turn. Only the `claude-cli` provider has them, which breaks G7.
*   **Reason for rejection:** The tier decision belongs where the turn loop is: in Heron.

### Alternative E: A second router beside `usage::route`

*   **Pros:** No risk to today's downgrade behaviour.
*   **Cons:** Two routers can disagree, and both would send `Event::Model`.
*   **Reason for rejection:** The new rules include today's (rules 3 and 7 are today's Minimal and `looks_simple` rules), so one function replaces the other.

--------------------------------------------------------------------------------

## Non-Functional Requirements

*   **Security and privacy.** No new tool, process or network host. The `claude` child gains one argument shape, `--effort <level>`, from a fixed set. The router reads only the user's own message (§2), so outside content can't choose a tier. Tainted sessions get a floor (RQ4). Nothing new is logged beyond the tier and reason in `Event::Model`.
*   **Performance.** `router::decide` runs a fixed set of substring checks on one message: well under 1 ms. No extra model call.
*   **Rollout and rollback.** `routing = "off"` restores "always the main model". Emptying `[llm.tiers]` on the HTTP provider restores today's behaviour exactly. For `claude-cli`, RQ1 decides whether the router is on by default. The schema change adds one nullable column. An older build ignores it.

### The eight checks

| Check | Answer |
|---|---|
| Switch and default | Core key `[agent] routing` (`"auto"` default, `"off"`); RQ1 decides whether `"auto"` includes the deep tier on `claude-cli`. App setting "Default model for new chats" (Auto) |
| New tools | none |
| Decision-table rows | none: no new `Effect` |
| Prompt blocks | none. The budget guidance block is unchanged |
| EVE | New task: a quick question shows "Haiku 5.5" in the chat, a coding request shows "Opus 5.5", and pinning Sonnet in the composer holds for the next turn |
| Content entries | none new. The router reads the typed `input`, and only the count of chat context items |
| Approval surfaces | none |
| Processes | `claude` (existing, `src/cli_llm.rs:247`): adds `--effort <low\|medium\|high\|xhigh\|max>` after `--model`. Same binary path, same environment |
| TCC | none |
| Q7 | yes: `cli_llm.rs` (the `--effort` argument) |

--------------------------------------------------------------------------------

### Additional Resources

*   `src/usage.rs:183-221`: today's `Route`, `looks_simple` and `route`.
*   `src/agent.rs:486-580`: the turn loop, the step-0 event and the escalation step.
*   `src/cli_llm.rs:247-264`, `:428-450`, `:1689-1700`: the `claude -p` argv, `claude_native_model` and the `"sonnet"` fallback.
*   `eval/genesis/limit-classifier.yaml`: the genesis spec pattern §5.4 follows.
*   Anthropic's model price list as of 2026-11-03: Haiku 5.5 $0.10 / $0.50, Sonnet 5.5 $2 / $10, Opus 5.5 $4 / $20 per million tokens in / out.

### Future Improvements

*   **A classifier model call** (Alternative B), if the word lists can't reach the under-routing gate.
*   **Effort as a fourth dial**, for example standard at `low` for chat, once the eval measures it.
*   **Learning from the user's pins**: a session the user re-pins to deep after an Auto turn is a labelled example.

--------------------------------------------------------------------------------

## Detailed Implementation Plan

### Pre-steps (before `skein run`)

1.  Run PT-R1 to PT-R3 and paste their output into the build PR.
2.  Record the owner's answers to RQ1 to RQ7, or note that the defaults stand.
3.  PR #84 is not required. Without it, the "going in circles" signal uses the existing repeat check at `src/agent.rs:558-566`. When PR #84's W6a lands, it swaps in `RepeatGuard`'s nudge; that swap is a one-line change in its own PR.

### Target Files to Create/Modify

#### 1. `src/router/mod.rs`, `src/router/rules.rs`, `src/router/prices.rs` (Create, R1)

*   **`mod.rs`:** `enum Tier { Quick, Standard, Deep }` with `as_str`, `parse`; `struct TierMap { quick: Option<TierModel>, standard: TierModel, deep: Option<TierModel> }` where `TierModel { id: String, effort: Option<Effort> }`; `TierMap::for_config(&Config) -> TierMap`; `TierMap::model(Tier) -> &TierModel` (nearest present tier); `TierMap::display(Tier) -> String`; `TierMap::is_active() -> bool`; `struct Inputs<'a> { text: &'a str, level: Level, pin: Option<Tier>, tainted: bool, last_tier: Option<Tier>, context_items: usize, routing_off: bool }`; `struct Decision { tier: Option<Tier>, reason: &'static str, escalate: bool }` (`tier: None` means stop, Spent); `fn decide(&TierMap, &Inputs) -> Decision`; `struct TurnSignals { step: usize, tool_errors: usize, nudged: bool, wrote: bool, level: Level }`; `fn escalate(&TierMap, current: Tier, &Decision, &TurnSignals, moved: &mut [bool; 2]) -> Option<(Tier, &'static str)>`.
*   **`rules.rs`:** `looks_simple` and `HEAVY` moved from `src/usage.rs`, `DEEP_WORDS`, `fn asks_deep(text) -> Option<&'static str>` (the reason), `fn count_paths`, `fn count_requirements`. Word-boundary matching, case-insensitive.
*   **`prices.rs`:** `fn list_price(id: &str) -> Option<(f64, f64)>` for the three built-in Claude ids (after `claude_native_model` normalisation) and `fn estimate_usd(id, tokens_in, tokens_out) -> Option<f64>`.
*   **Tests (inline):** `decide_follows_rule_order` (one case per row of §2), `pinned_session_ignores_budget_except_spent`, `minimal_level_routes_quick_without_escalation`, `careful_level_caps_deep_at_standard`, `tainted_session_never_starts_quick`, `context_item_words_never_match_deep`, `follow_up_to_deep_stays_deep`, `absent_tier_falls_to_nearest`, `single_tier_map_is_inactive`, `escalate_quick_to_standard_at_step_two`, `escalate_quick_on_tool_error`, `escalate_standard_on_two_errors`, `escalate_standard_on_nudge`, `escalate_standard_on_long_write_turn`, `escalate_never_twice_per_boundary`, `escalate_skips_deep_below_normal`, `pinned_session_never_escalates`, `deep_words_match_on_word_boundaries`, `tier_map_claude_cli_defaults`, `tier_map_http_falls_back_to_fast_model`, `estimate_usd_matches_list_price`.

#### 2. `src/usage.rs` (Modify, R2)

*   `route()` becomes a thin wrapper that builds `Inputs` and calls `router::decide`, returning today's `Route` shape plus the tier, so callers outside `agent.rs` keep compiling. `looks_simple` re-exports `router::rules::looks_simple`.
*   `Report.models` rows gain `usd_estimate: Option<f64>`. `Report` gains `tiers: Vec<(String, String)>` (tier, display name) and replaces `routing: bool`'s meaning with "the tier map is active and routing is not off".

#### 3. `src/config.rs` (Modify, R2)

*   New optional `[llm.tiers]` table, `struct Tiers { quick, standard, deep, quick_effort, standard_effort, deep_effort: String }`, all empty by default, with the commented example in the config template near line 959.
*   New `[agent] default_pin: String`, empty by default.
*   Environment overrides `HERON_TIER_QUICK`, `HERON_TIER_STANDARD`, `HERON_TIER_DEEP`, beside `HERON_CLI_MODEL` (`:486`).
*   Test: `tiers_table_parses_and_defaults_empty`.

#### 4. `src/llm.rs` (Modify, R2)

*   `ChatRequest` gains `effort: Option<Effort>`. `HttpLlm` ignores it (no reasoning parameter is sent today, `:138-145`). `ScriptedLlm` records it so tests can assert it.

#### 5. `src/cli_llm.rs` (Modify, R2, Q7)

*   `CliSpec::base_args` takes `effort: Option<&str>`. For `claude-cli` only, it pushes `--effort <level>` right after `--model`. Other CLIs ignore it.
*   The `argv` fallback to `"sonnet"` (`:1689-1695`) stays for the no-router case.
*   Tests: `claude_cli_argv_has_effort_after_model`, `codex_argv_ignores_effort`, and the existing no-auto-approve argv test (`:4313`) extended with `--effort` present.

#### 6. `src/agent.rs` (Modify, R2)

*   At turn start (`:492`): build `TierMap` once per turn, read `session.model_pin`, `session.tainted`, `session.last_tier`, and call `router::decide`. On `tier: None`, keep today's Spent behaviour.
*   In the step loop (`:522-528`): replace the `escalate_after` filter with `router::escalate`, fed from counters the loop keeps: tool errors (from `execute_call` results), the repeat nudge (`:558-566`), and whether a `write_file`, `edit_file` or `shell` call ran. Send `Event::Model { name, why, tier, pinned }` at step 0 when `TierMap::is_active()`, and on every move.
*   The closing call (`:580`) uses the current tier's model.
*   At turn end, store the final tier in `session.last_tier` (in memory and in the `sessions` row).
*   Compaction (`:806`) and memory learning (`:831`) use `TierMap::model(Tier::Quick)`.
*   `Event::Model` gains `tier: String` and `pinned: bool`.
*   Tests: `turn_start_sends_model_event_with_real_name`, `escalation_changes_request_model_between_steps` (with `ScriptedLlm`, asserting `req.model` per step), `background_compaction_uses_quick_tier`, `delegate_child_inherits_pin`.

#### 7. `src/events.rs` (Modify, R2)

*   The `model` event's JSON adds `tier` and `pinned` (`:192`). Test: `model_event_json_has_tier_and_pinned`.

#### 8. `src/store.rs` (Modify, R2, Q7: schema)

*   The next schema step adds `sessions.model_pin TEXT` and `sessions.last_tier TEXT`, both nullable. `set_model_pin(id, Option<Tier>)`, `model_pin(id)`, and `set_last_tier(id, Tier)`.
*   Test: `schema_step_adds_nullable_pin_columns`, `model_pin_round_trips`.

#### 9. `src/app.rs`, `src/sources.rs` (Modify, R2)

*   Reflection (`app.rs:1864`), suggestions (`:1987`), workflow `ask` (`:2240`) and `learn_one` (`sources.rs:498`) use `TierMap::model(Tier::Quick)`.
*   Test: `background_calls_request_quick_tier` (one `ScriptedLlm` assertion per call site).

#### 10. `src/server.rs` (Modify, R2)

*   `POST /api/sessions/{id}/model` and `model_pin`, `tiers` in `GET /api/sessions/{id}`, behind the existing token check.
*   `GET /api/usage` carries the new `Report` fields. `POST /api/usage` keeps `routing` and `fast_model`.
*   Tests: `session_pin_route_needs_token`, `session_pin_rejects_unknown_tier`, `usage_report_has_estimate_per_model`.

#### 11. `src/main.rs` (Modify, R2)

*   New subcommand `heron route --explain [--json] [--level normal|careful|minimal] [--tainted] <text>`. It prints the tier, the model it maps to and the reason, using the current config. The eval uses it.
*   Test: `route_explain_prints_json`.

#### 12. `tests/integration.rs`, `tests/fixtures/api/` (Modify, R3)

*   `api_session_pin_round_trip`, `api_usage_reports_tiers`, and re-recorded `tests/fixtures/api/usage.json` and `sessions.json` fixtures with the new fields.

#### 13. Mac app (Modify, R4)

*   `macos/HeronMac/Sources/HeronCore/Wire.swift`: decode `tier` and `pinned` on the model event (`:419`), `model_pin` and `tiers` on sessions, `usd_estimate` on usage rows.
*   `macos/HeronMac/Sources/HeronCore/CoreClient.swift`: `setModelPin(session:pin:)` in the client protocol and all three clients.
*   `macos/HeronMac/Sources/HeronUI/Stores/ChatAndApprovalsStores.swift`: keep the event (`:328`) as the turn's model line instead of dropping it.
*   `macos/HeronMac/Sources/HeronUI/Views/ChatView.swift`: the model line above each reply, and the composer menu.
*   `macos/HeronMac/Sources/HeronUI/Views/SettingsView.swift`: "Default model for new chats" in the Model tab, and the "By model" group in `UsageTab` (`:315`).
*   Tests (`macos/HeronMac/Tests/`): `test_modelEventDecodesTierAndPinned`, `test_modelLineShowsNameAndReason`, `test_composerMenuListsPresentTiersOnly`, `test_pinMenuCallsSetModelPin`, `test_usageTabShowsEstimateNote`.

#### 14. `eval/router/`, `eval/genesis/` (Create, R5)

*   `eval/router/cases.md` (40 cases, §5.1), `eval/router/fixtures/` (demo data), `eval/router/label.py`, `eval/router/replay.py`, `eval/router/README.md`.
*   `eval/genesis/router.yaml`, `eval/genesis/router_verifier.py`, `eval/genesis/build_router_cases.py`, `eval/genesis/router-cases.jsonl` (generated from `labels.json`; before the labelling run, from a 12-case seed file `eval/router/seed-labels.json` written by hand, so the genesis audit can run in CI from day one).
*   `eval/tests/test_router_eval.py`: case-file structure, replay scoring against a fixed fake `heron route` output, metric arithmetic, and the verifier's known-good and known-bad answers.

#### 15. Documents (Modify, R2)

*   `docs/engineering/CONFIG.md` (or the config template comments if no such page exists): `[llm.tiers]`, `default_pin`, and what `routing = "auto"` now does.

### Verification Plan

*   **Build:** `cargo build --locked`, `cargo clippy --locked --all-targets -- -D warnings`, `swift build --package-path macos/HeronMac`.
*   **Unit and integration tests:** `cargo test --locked` (run outside the Bash sandbox: about 27 unrelated tests need local ports and nested Seatbelt), `swift test --package-path macos/HeronMac`.
*   **Test quality (testteeth):** `testteeth run --base main --fail-under 70` on the Rust changes, with `src/router/` expected near 100% because it is pure logic. testteeth has no Swift support here, so the Swift tests are checked by name (`swift test list | grep`) and by the owner's manual pass.
*   **Evaluator audit (genesis):** `genesis audit-evaluator eval/genesis/router.yaml --suite json` exits 0.
*   **The router eval:** `python3 -I eval/router/replay.py --labels eval/router/labels.json` meets the §5.3 gates. Until the labelling run (RQ5), it runs on the seed labels and reports, without gating.
*   **EVE:** the task in the eight-checks table.

## Work units

Units whose scopes share a prefix (`src/**` and `src/router/**`) are ordered by depends-on. In the check column, `\|` stands for a shell `|`. Cargo and Swift checks run in the skein supervisor, outside the node sandbox (earlier runs could not run them inside it).

| id | what to build | scope | depends on | check | model role |
|---|---|---|---|---|---|
| R1 | Router module: tiers, tier map, rules, escalation, prices (item 1) | `src/router/**` | none | `cargo test --locked --lib router:: && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib -- decide_follows_rule_order pinned_session_ignores_budget_except_spent minimal_level_routes_quick_without_escalation careful_level_caps_deep_at_standard tainted_session_never_starts_quick context_item_words_never_match_deep escalate_never_twice_per_boundary escalate_skips_deep_below_normal tier_map_claude_cli_defaults tier_map_http_falls_back_to_fast_model` | feature, refactoring: sonnet high |
| R2 | Wiring: config, llm, cli argv, agent loop, events, store, background call sites, server routes, `heron route` (items 2-11, 15) | `src/**` | R1 | `cargo test --locked --lib --bins && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib --bins -- claude_cli_argv_has_effort_after_model escalation_changes_request_model_between_steps background_compaction_uses_quick_tier background_calls_request_quick_tier model_event_json_has_tier_and_pinned schema_step_adds_nullable_pin_columns session_pin_route_needs_token route_explain_prints_json` | hardest tasks: opus high |
| R3 | Integration tests and API fixtures (item 12) | `tests/**` | R2 | `cargo test --locked --test integration && bash eval/require-tests.sh --test integration -- api_session_pin_round_trip api_usage_reports_tiers` | feature, refactoring: sonnet high |
| R4 | Mac app: model line, composer pin menu, default pin, usage by model (item 13) | `macos/HeronMac/**` | R2 | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && swift test list --package-path macos/HeronMac \| grep -q 'test_modelEventDecodesTierAndPinned' && swift test list --package-path macos/HeronMac \| grep -q 'test_composerMenuListsPresentTiersOnly'` | feature, refactoring: sonnet high |
| R5 | Eval: cases, fixtures, label and replay scripts, genesis spec and verifier, tests (item 14) | `eval/**` | none | `python3 -I -m unittest discover -s eval/tests -t eval -p 'test_router_eval.py' && test "$(grep -c '^## r[0-9][0-9]: ' eval/router/cases.md)" -eq 40 && genesis audit-evaluator eval/genesis/router.yaml --suite json` | feature, refactoring: sonnet high |
| V | Verify the whole branch | (none: reads only) | R1, R2, R3, R4, R5 | `cargo test --locked && swift test --package-path macos/HeronMac && python3 -I -m unittest discover -s eval/tests -t eval && genesis audit-evaluator eval/genesis/router.yaml --suite json && testteeth run --base main --fail-under 70 -q` | judgment and prose: opus high |
