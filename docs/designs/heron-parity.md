# Heron parity: named agents, live voice, and the rest of what Muse, Grok Bot, Dots and Duet do

*   **Author:** Heron design session (EGM), for the owner
*   **Status:** Draft, waiting for owner approval. Nothing is built.
*   **Date:** 2026-11-23
*   **Facts this rests on:** [research/heron-parity-competitors.md](research/heron-parity-competitors.md) (what the four products do) and [research/heron-parity-provider-facts.md](research/heron-parity-provider-facts.md) (third-party requirements and prices, with sources).

--------------------------------------------------------------------------------

## Overview

The owner wants Heron to do everything that Meta's Muse, xAI's Grok Bot, OpenAI's Dots and Anthropic's Claude Duet do. Nothing is dropped as out of scope. A feature that must wait gets a later phase and an owning doc.

This doc does three things:

1.  **It designs the two top-priority features in full, ready to build:**
    *   **Named agents.** The owner creates several agents, each with a name, its own instructions, tools, permission scope and memory scope, and calls any of them by name: in chat, by voice, and later from messaging channels.
    *   **Live voice.** A spoken conversation with Heron or with a named agent that answers in about a second and can be interrupted mid-sentence.
    *   It also designs **ask rules**: per-action "always ask" rules on the existing rule matcher. Named agents need them for their permission scope, and they are the first half of gap 7. Recipient and amount rules follow in P2.
2.  **It designs the remaining gaps at feature level, ranked by impact for a personal agent on a Mac.** These are messaging channels, meetings, deliverables, outbound calls, a mobile app and approved checkout. The Muse and Grok Bot extras are covered too: goals with plans, tools the agent writes, the shared computer, a small-business pack, an Instagram connector, and shared bots in Slack, Teams and meetings. For each it says what the user sees, how it fits Heron's local-first design, which macOS or third-party pieces it needs, its safety rules, its running cost and its phase.
3.  **It lists the owner decisions** this needs, including eight places where it reverses a decision in an approved doc.

Only Phase 1 (named agents, ask rules, live voice) has a Detailed Implementation Plan and Work units. Every later phase needs a short design delta of its own before it is built, which is how Harness2 handled its Phase 2.

## The Problem

Heron is a local, private agent with a careful safety layer. The four products it is measured against are cloud agents that live where their users already talk: in WhatsApp, Slack, phone calls and meetings. Each has features Heron lacks. The owner uses two of them every day in Muse: calling on several named agents, and live voice.

**Where Heron stands today**, from the code on `main` (commit `c9dd12c`) and the feature branches:

*   **One agent, one identity.** There is one persona file (`src/persona.rs`) and one name (`cfg.agent.name`, `src/config.rs:160`). Persona, memory and skills reach the prompt only for the top-level agent (`src/agent.rs:357-444`, all inside `if self.depth == 0`).
*   **`delegate` is a helper, not an agent.** It is a clone of the calling agent with half the step budget. It has the same tools, policy, approver, memory and session id (`src/agent.rs:712-742`). Its output comes back as plain text, not marked untrusted. Open PR #75 fixes that.
*   **Memory is one pool.** Memory has no scope. Its only split is confirmed versus unconfirmed (`src/memory.rs:395`).
*   **No per-agent tools.** The tool list sent to the model is the whole registry plus `delegate`, and nothing filters it per caller (`src/agent.rs:309-315`, `:487`).
*   **Voice is two pieces, and neither is live.** `feat/v1-voice` (feature F7, unmerged) adds:
    *   a button that starts macOS dictation;
    *   spoken replies through `AVSpeechSynthesizer`, which stop on a key press or click.

    It has no microphone capture and no speech recognition of its own. Barge-in, latency figures and a way to cancel the run by voice are missing too (`Features/Voice/*.swift` on that branch).
*   **Telegram is the only channel.** `src/channel.rs` has a `ChannelAdapter` trait and `ChannelKind` values for Slack and WhatsApp, but nothing implements the trait. Telegram uses the channel store directly, and pairing lives in `src/telegram.rs:331-411`.
*   **No calls, meetings, payments, deliverables or mobile app.**

**The parity matrix.** One row per competitor feature, checked against every line of the owner's scope message. "Phase" is when it lands, and "Owner" is the doc that designs it in detail.

| # | Competitor feature (who has it) | Heron today | Phase | Owner doc |
|---|---|---|---|---|
| 1 | Your own named agents, several, called by name (Muse, Grok Bot) | One persona | **P1** | this doc, [§1](#1-named-agents) |
| 1a | Named agents in chat and by voice (Muse) | none | **P1** | this doc |
| 1b | Named agents in messaging channels, incl. WhatsApp (Muse) | none | P2 (Slack, Messages), P3 (WhatsApp, Teams) | this doc §4.1, plus a channel delta |
| 1c | Create, share and import bots (Grok Bot) | none | **P1** (export and import), P3 (bots others can talk to) | this doc |
| 1e | One agent passing work to another by name | `delegate` (unnamed helper only) | P2 | goals delta |
| 1d | Bots in Slack, Teams and meetings that other people talk to (Grok Bot) | none | P3 | this doc §4.1, §4.2 |
| 2 | Live, interruptible voice, about 1 s to first audio (Muse, Grok, GPT-Live) | dictation and spoken replies, unmerged | **P1** | this doc, [§3](#3-live-voice) |
| 2a | Talk to a named agent by voice, in its own voice | none | **P1** | this doc |
| 2b | Animated avatar while talking (Muse) | `AvatarStateMachine` and `AvatarView` in the Mac app (idle, thinking, awaiting approval, speaking, error) | **P1** (listening states), P4 (a character) | this doc; later doc |
| 3 | Outbound calls to businesses, live transcript, Take over, End call (Muse, Grok Bot) | none | P3 | this doc §4.4, plus a calls delta |
| 4 | Messages, WhatsApp, Slack, Teams with one shared memory (Dots, Grok, Muse) | Telegram | P2, P3 | this doc §4.1 |
| 5 | Join Meet, Zoom and Teams, take notes, answer in the meeting chat (Grok Bot, Claude Meetings) | none | P3 | this doc §4.2 |
| 6 | Approved checkout with a one-time card and Face ID (Dots, Duet, Grok, Muse) | none | P2 (hand-off), P4 (agent card) | this doc §4.6 |
| 7 | Per-action allow, ask, block rules (Dots Custom Rules) | modes, allow rules and "never" rules, matched on `tool:summary` | **P1** (ask rules, per-agent rules), P2 (recipient, domain and amount facts) | this doc, [§2](#2-action-rules) |
| 7a | Checks before important actions such as sending email or buying (Muse) | yes, by mode and taint | **P1** (rules), P2 (sending) | this doc |
| 8 | Editable documents, slides, dashboards (Duet Make, ChatGPT Space, Muse Spaces) | none | P2 | this doc §4.3 |
| 9 | Mobile app (all four) | none | P4 | `heron-mobile.md` (later) |
| 10 | Goals with action plans, progress, ideas (Muse) | tasks and schedules | P2 | this doc §5.1; proactive design |
| 11 | Builds its own tools when a task needs one (Muse) | `save_skill`; Harness2 manifests (approved, unbuilt) | P2 | this doc §5.2; Harness2 §7 |
| 12 | Shared computer with a full browser that the user can take over (Muse) | `browser_*` tools | per the router and computer-use design | that design |
| 13 | Small-business pack of skills and connectors (Muse) | none | P3 | this doc §5.3 |
| 14 | Email and calendar connectors (Muse) | Google, Microsoft, Mac Mail and Calendar | done; many accounts in PR #83 | PR #83 |
| 15 | Instagram connector (Muse) | none | P3 | this doc §5.3 |

**Ranking of gaps 3 to 9.** The brief asks for these by impact for one person on a Mac:

1.  **Messaging channels (gap 4).** This is daily use. The phone is where the owner is when away from the Mac, and Telegram already proves the pattern. Messages and Slack need no server.
2.  **Action rules (gap 7).** Every outward feature below depends on them. The cheap half, ask rules on the existing matcher, moves into Phase 1, and recipient and amount rules come with the first send tool in P2.
3.  **Meetings (gap 5).** This is weekly, high-value work. Local notes on the Mac cost nothing.
4.  **Deliverables (gap 8).** These turn chat answers into files the owner keeps. They are local and cost only model tokens.
5.  **Outbound calls (gap 3).** These are useful but occasional. They need a paid phone number and carry legal and safety weight.
6.  **Mobile app (gap 9).** Channels cover most of the phone use first, and the app needs the relay and an Apple Developer account.
7.  **Approved checkout (gap 6).** This is the highest risk and the least frequent. The hand-off form, where the owner pays, comes early. An agent card comes last.

## Definitions

| Term | Meaning |
|---|---|
| Named agent | A saved agent profile: name, call names, instructions, tools, permission ceiling, memory scope, voice. The row in the `agents` table ([§1.1](#11-the-agent-profile)). |
| Default agent | The agent with id `heron`. It is today's Heron, with today's persona and full scope. |
| Call name | A name the agent answers to. Its display name always counts, and up to 4 extra names may be added. |
| Addressing | Choosing which agent a message is for, by a fixed parser in the core, never by the model ([§1.3](#13-addressing-how-a-message-finds-its-agent)). |
| Scope | What an agent may do: tool set, mode ceiling, extra rules, memory scope, surfaces. |
| Effective scope | What a turn may actually do: the narrowest of the global settings, the agent's scope and the surface's ceiling. A `delegate` child keeps its parent's effective scope. |
| Ceiling | The highest permission mode a turn may run in. Modes order as `readonly < ask < auto < yolo`. |
| Surface | Where a message comes from. It is derived from the session's `Origin` by one function, `surface_of(origin)`: `User` is `app`, the new `Voice` is `voice`, and `Telegram` is `telegram`. Later deltas add their own origins. |
| Owner sender | A message on a channel from the paired owner account. |
| Guest sender | A message on a channel from anyone else, for example a colleague in a Slack channel. |
| Live voice | A spoken session with turn-taking, barge-in and on-device speech recognition and speech ([§3](#3-live-voice)). |
| Barge-in | The owner speaks while Heron is talking. Heron stops speaking and stops generating. |
| Soft cancel | A new cooperative stop. The run ends at the next step boundary, and a tool already running finishes ([§3.5](#35-barge-in-and-soft-cancel)). |
| Action rule | An allow, ask or block rule. In P1 it is a glob on `tool:summary`, the matcher the allow and deny rules already use, plus a new `ask` verdict. P2 adds matches on recipient, domain and amount ([§2](#2-action-rules)). |
| Relay | A small store-and-forward service in the owner's own Cloudflare account. It takes the webhooks that need a public address and passes them to the Mac, which pulls them ([§6](#6-the-relay)). |
| P1 to P4 | The phases in [§10](#10-phases-and-owning-docs). |

## Goals

*   **G1. Named agents.** The owner can create, edit, archive, export and import named agents. Each agent has its own instructions, tool set, mode ceiling, memory scope and voice.
*   **G2. Call by name.**
    *   In chat, a message that starts with `@Name`, or one sent with the agent chosen in the composer, runs as that agent.
    *   In live voice, an utterance that starts with "Name," or "Hey Name" runs as that agent and is answered in its voice.
    *   Channels use the same parser from P2. In P1, Telegram stays with the default agent.
*   **G3. Scope never widens.**
    *   A turn's effective scope is never wider than the global settings.
    *   A `delegate` child is never wider than its parent.
    *   A tool outside the agent's set is refused when it is called, not only hidden from the model.
*   **G4. Memory scope holds for reads and writes.**
    *   An agent with `own` or `private` scope never receives another agent's own or private memories, whether in its prompt or from `recall`.
    *   No agent's `remember`, post-turn learning, dedupe, supersede or `forget` changes a memory in a pool it can't read.
    *   Tests with planted memories prove both.
*   **G5. Live voice is fast and interruptible.**
    *   With an HTTP streaming model provider, the time from the end of the owner's speech to the first audio is p50 ≤ 1.0 s and p90 ≤ 1.5 s, over the 20-utterance gate set on the owner's Mac.
    *   When the owner speaks over a reply, the reply stops within 150 ms.
    *   Heron logs these times and shows them.
*   **G6. Audio stays on the Mac in P1.** Speech recognition and speech output run on the device. Only the recognised text goes to the model provider, as typed text does.
*   **G7. Ask rules.**
    *   The owner can add rules that make one action always ask, in every mode, matched by the same `tool:summary` glob the allow and never rules use. "Ask: `shell` when the command contains `git push`" is the glob `shell:*git push*`.
    *   A never rule beats everything, and an ask rule beats modes and allow rules.
    *   No rule weakens the taint check.
    *   Rules matched on recipients, domains and amounts come in P2 with the first send tool.
*   **G8. Every outward action is approved.** Sending a message, email or chat reply, placing a call and paying each need the owner's approval:
    *   for each item, unless an action rule scoped by recipient allows it;
    *   never by a tool-wide "always";
    *   payments never by voice and never from a channel.
*   **G9. Nothing is dropped.** Every feature in the parity matrix has a phase and an owning doc.

## Non-Goals

*   **Building P2 to P4 from this doc.** Each later phase gets its own short design delta and owner approval first.
*   **Redesigning what other designs own:**
    *   proactive suggestions and interactive onboarding (the "Proactive Heron + interactive onboarding" design session);
    *   the auto model router, Mac computer use, and the shared computer the owner can take over (the "Heron auto model router + Mac computer use" design session);
    *   many Google and Microsoft accounts (PR #83);
    *   the effect journal, loop guard and egress allowlist (PR #84).
*   **Agents that talk to each other on their own.** An agent calls another only through `delegate`, inside one owner turn.
*   **A wake word in P1.** Live voice starts from a button or a hotkey. Always listening is P4.
*   **Voice cloning of real people.** An agent's voice is a macOS system voice, or the owner's own Personal Voice with Apple's consent flow.
*   **Linux and Windows versions** of the new features. Those builds keep the Slint app, which shows none of this.

## Owner decisions this doc needs

### Reversals of approved decisions

Full parity means these approved decisions will change. Approving this doc decides only the first group. The second group is the direction this doc proposes. Each item there is decided in the delta that owns it, and nothing in this doc builds it. The owner sees the whole list now so that no later delta surprises them.

**Decided by approving this doc**

*   **R4. Recording audio.** Reverses `heron-v1-features.md:148`. **Proposed:** live voice (P1) captures the microphone. Audio is processed on the device and never written to disk. Transcripts are kept as text, like typed messages.
*   **R1. A native iPhone app.** Reverses the non-goal at `heron-v1-features.md:145`. **Proposed:** it is in parity scope as P4, designed in its own doc, `heron-mobile.md`. Nothing in this doc builds it.

**Direction, decided in the owning delta**
*   **R2. Heron sending messages and email itself.** Reverses `heron-v1-features.md:146`. **Proposed:**
    *   From P2, Heron may send a message or email it composed.
    *   Each send asks, showing the recipient and full text.
    *   "Always" exists only as an action rule scoped to one recipient or domain, never tool-wide. This follows the Muse failure where an "always allow" messaging grant let the agent decide what to say to strangers ([muse-ux.md](research/muse-ux.md), pitfall 2).
*   **R3. Reading Messages.** Reverses `heron-v1-features.md:147` and the Refused store `~/Library/Messages` in `heron-v1.md:342`. **Proposed:** a narrow exception for the Messages channel in P2.
    *   The Mac app, not the core, reads only the one self-chat thread the owner pairs, with one fixed query.
    *   No tool can read Messages: tools still get `Effect::Refused` for that path.
    *   The app needs Full Disk Access to read that one thread.
*   **R5. Slack.** Reverses Alternative A at `heron-v1-features.md:799`, which rejected Slack and cloud connectors. **Proposed:** Slack becomes a channel (P2) through Socket Mode, which needs no public address.
*   **R6. Connectors Heron writes for itself at runtime.** This changes Harness2 in two ways:
    *   Harness2 loads connector manifests only from the app bundle (§7).
    *   It lists "Heron offering connectors on its own" as a non-goal (`heron-harness2.md:107`).

    **Proposed:** from P2, an untainted turn may write a draft manifest into `~/.heron/connectors/`. The same fixed executor loads it as untrusted: every call asks until the owner turns on Trust ([§5.2](#52-tools-the-agent-builds)). Harness2's PR path stays for manifests that should ship in the app.
*   **R7. Cloud pieces.** Reverses "cloud or always-on execution" at `heron-v1-features.md:145`. **Proposed:** three cloud pieces are allowed, each switched off until the owner sets it up:
    *   the relay (P3);
    *   a hosted voice agent for phone calls (P3);
    *   a meeting-bot service (P3).

    None of them runs Heron's tools. Each is listed with its cost in [§9](#9-running-cost).
*   **R8. Telegram's mode.** Narrows the accepted risk C35-8 (`docs/channel-gateway.md`), under which a phone turn runs in the desktop's mode. **Proposed:** in the P2 channels delta, every channel gets a mode ceiling, defaulting to `ask`, and Telegram gets the same ceiling. The owner can raise any channel's ceiling in Settings. P1 leaves Telegram as it is: the default agent only, with no `@Name` routing.
*   **R4, extended (P3).** Meeting notes capture the meeting app's audio as well as the microphone, and are decided in the meetings delta.

### Open Questions

Each has a proposed default. The doc is written to the defaults.

**Named agents and voice (P1)**

*   **PQ1. Which model live voice uses.** Sub-second replies need a streaming HTTP model ([§3.4](#34-the-model-path-what-decides-latency)). The owner uses CLI providers, which start a process per call, and has said they want zero charges. **Proposed:** Settings › Voice offers three choices.
    1.  The main model. This is free beyond the owner's plan, and replies start in a few seconds with a CLI.
    2.  A local model through Ollama or LM Studio. This is free, a 7–8B model is fast on Apple Silicon, and it is weaker at tools.
    3.  A paid fast model through an API key.

    The default is 1. Heron suggests 2 when it finds Ollama or LM Studio running. Heron never turns on 3 without the owner's choice.
*   **PQ2. Approving by voice.** **Proposed:** off by default. When on, saying "approve" works only for an ask that is not high risk, not in a tainted session, and not a payment, a send or a call, and only while the card is on screen. Clicking stays the only way for the rest.
*   **PQ3. A private agent in someone else's thread.** **Proposed:** calling an agent whose memory scope is `private` from a thread that another agent has spoken in opens a new thread for it. Otherwise, what the first agent said would sit in the private agent's context.
*   **PQ4. Rules an agent may carry.** **Proposed:** an agent's own rules may be never or ask rules, not allow rules. Allow rules are global only, so an agent profile can never loosen anything.
*   **PQ5. Who may edit agents.** **Proposed:** only the owner may create or change an agent, in the Agents screen. The `propose_agent` tool lets the default agent draft one in chat. Its effect is `Privileged`, so it asks every time and the owner sees the whole profile. Other agents don't get it unless the owner adds it.
*   **PQ6. Imported agents.** **Proposed:** an imported `.heronagent` file arrives in state `review`, with no tools, mode ceiling `readonly` and memory scope `private`. The owner reads it in a review sheet and clicks **Use this agent**, then grants tools one by one.

**Later phases**

*   **PQ7. Guests on channels and in meetings.** **Proposed:** an agent answers a guest only if the owner turned on "Answer other people" for that agent on that channel. Each reply asks, unless an action rule allows replies in that one channel. A guest-facing turn runs with memory scope `none` and read-only tools.
*   **PQ8. The Messages channel design** (R3). **Proposed:** the owner texts their own number or address (the self-chat), and Heron replies in that thread. No second Apple ID is needed.
*   **PQ9. Calls.** A hosted voice-agent platform, or Heron's own voice pipeline over the relay? **Proposed:** hosted for P3, because phone audio needs a public real-time endpoint and telephony-grade turn-taking. The vendor is chosen in the calls delta after a price check ([§4.4](#44-outbound-calls)).
*   **PQ10. The relay host.** **Proposed:** a Cloudflare Worker in the owner's own account, on the free tier where it fits. It is preferred to a Cloudflare Tunnel, which would expose the Mac's server to the internet.
*   **PQ11. Meetings.** **Proposed:** local notes first (P3a), at no cost. A meeting bot comes later (P3b). Each meeting the bot joins asks, and each chat message it posts asks.
*   **PQ12. Checkout.** **Proposed:** a hand-off form first. Heron fills the cart and stops at the payment page, and the owner pays with Apple Pay or their saved card. An agent card from a provider comes in P4, after a provider check.
*   **PQ13. The mobile app.** **Proposed:** a separate doc after P3, because it needs the relay.

**Accounts the owner must create, by phase.** Heron never creates them.

| Phase | Account | Paid? |
|---|---|---|
| P1 | none. Optional: Ollama or LM Studio (free) for PQ1 choice 2; a model API key for choice 3 | optional |
| P2 | a Slack app in the owner's workspace (Heron supplies its manifest) | free |
| P2 | Full Disk Access and Messages automation grants for the Messages channel | free |
| P3 | Cloudflare account for the relay | free tier, see §9 |
| P3 | Meta Business account, WhatsApp Business number (a number not already on WhatsApp), Meta developer app | per-message, see §9 |
| P3 | Azure account with a bot registration, and a Microsoft 365 tenant that allows custom Teams apps | see §9 |
| P3 | Telephony number (Twilio) and a voice-agent platform account | per-minute, see §9 |
| P3 | Meeting-bot service account (P3b only) | per-hour, see §9 |
| P3 | Meta developer app with Instagram API access, and an Instagram Business or Creator account | free API; app review |
| P4 | Payment provider for agent cards; Apple Developer Program for the mobile app and push | see §9 |

## Critical User Journeys (CUJs)

### CUJ P-1: The owner makes two agents and calls one by name

*   **Given:** Heron P1 is installed, and the owner is in Ask mode.
*   **When:** the owner opens Agents › New and creates:
    *   **Lux:** "my research assistant: cite sources, be brief", tools `web` and `memory`, memory scope `own`, a British English voice;
    *   **Mango:** "keeps my home admin: bills, appointments", tools `tasks`, `schedules`, `files` and `memory`, memory scope `own`.

    Then, in chat, the owner types `@Lux what changed in the EU AI Act this month?`
*   **Then:**
    1.  The message runs as Lux, and the reply shows Lux's avatar and name.
    2.  Lux's prompt has Lux's instructions and shared plus Lux-own memories. Mango's own memories are absent.
    3.  If the model asks for `shell`, the call is refused: "shell is not one of Lux's tools". The refusal is audited with `agent=lux`.
    4.  The owner's next message without `@` goes to Lux too, because the thread is now Lux's.
    5.  `@Mango when is the dentist?` hands the thread to Mango. A divider says "Mango joined".

### CUJ P-2: Live voice with barge-in

*   **Given:** the owner picked a local or paid fast voice model (PQ1).
*   **When:** the owner presses ⌥Space twice to start live voice and says "Lux, give me three headlines on solar prices." Lux starts speaking the first headline, and the owner says "skip that, just the second one".
*   **Then:**
    1.  The first audio starts about a second after the owner stops speaking, and the time is logged.
    2.  When the owner starts talking, Lux's speech stops within 150 ms, and the run soft-cancels.
    3.  The stored reply holds only what was spoken, plus `[interrupted]`.
    4.  The new utterance runs as Lux, who answers with the second headline.
    5.  The Voice panel shows the live transcript and the state: listening, thinking, speaking.

### CUJ P-3: An action that asks, by voice

*   **Given:** Mango also has the `shell` tool, and the global mode is Auto.
*   **When:** in live voice the owner says "Mango, zip my receipts folder onto the desktop."
*   **Then:**
    1.  Mango proposes a `shell` command. In Auto mode a typed request would run it without asking, but the voice surface's ceiling is `ask`.
    2.  The approval card appears on screen: "Mango wants to run `zip -r ~/Desktop/receipts.zip ~/Documents/receipts`".
    3.  Mango says "It's on screen for you to approve."
    4.  Saying "approve" does nothing, because voice approval is off by default (PQ2). The owner clicks **Allow once**.

### CUJ P-4: An ask rule

*   **When:** in Settings › Rules the owner adds two rules:
    *   "Ask: `shell:*git push*`";
    *   "Never: `web_fetch:*competitor.com*`".
*   **Then:**
    *   In Auto mode, a `git push` asks.
    *   A fetch of `https://competitor.com/pricing` is refused in every mode, and the refusal names the rule.
    *   A chained command such as `git push && rm -rf build` is not covered by any allow rule, as today (`allow_rule_covers`, `permissions.rs:449-457`), so it asks too.

### CUJ P-5 (P2): Slack DM with an agent

*   **Given:** the Slack channel is paired.
*   **When:** the owner DMs the Heron bot in Slack: "Lux, summarise this thread" with a link to a thread.
*   **Then:**
    1.  The DM is an owner message (Trusted).
    2.  The thread content is a guest read, so it is Outside and taints.
    3.  Lux replies in the DM, and the turn runs at the Slack ceiling, `ask`.

### CUJ P-6 (P3): A call to a business

*   **When:** "Call Luigi's and order a large margherita for pickup at 7."
*   **Then:**
    1.  A call brief card shows the number with where it came from, the goal, what may be shared (first name, callback number), the limits (at most $30, give no card details) and the agent.
    2.  The owner approves, and the call starts.
    3.  The live transcript streams with **Take over** and **End call** buttons. The callee hears "I'm an AI assistant calling for Sam…" first.
    4.  A summary arrives in the Inbox.

## Technical Plan (Architecture)

### 0. Shape

```text
            ┌──────────────── Mac app (SwiftUI) ─────────────────────────────┐
            │ Agents screen   Composer (@, picker)   Voice panel  Rules pane  │
            │      │               │                     │                  │
            │      │        ┌──────┴───────┐   LiveVoiceSession (P1)        │
            │      │        │ ChatStore    │   mic → AEC → VAD → on-device  │
            │      │        └──────┬───────┘   STT → text ─┐   TTS ◀─ text  │
            └──────┼───────────────┼────────────────────────┼───────────────┘
                   │ HTTP + SSE (127.0.0.1, bearer token)   │
            ┌──────▼───────────────▼────────────────────────▼───────────────┐
            │ Rust core  (heron-cli serve)                                  │
            │  agents::address ─▶ effective scope ─▶ Agent::run_turn        │
            │       (fixed parser)   (global ∩ agent ∩ surface)             │
            │  Policy::decide + ask rules ──▶  Approver ─▶ tool ─▶ audit   │
            │  memory (scoped)   channel.rs adapters (P2+)   relay pull (P3)│
            └──────────────────────────────▲────────────────────────────────┘
                                           │ outbound HTTPS only (P3)
                                  ┌────────┴────────┐
                                  │ relay (owner's  │◀─ WhatsApp, Teams,
                                  │ Cloudflare acct)│   call and meeting webhooks
                                  └─────────────────┘
```

The core keeps the two rules every earlier design relies on. Every tool call goes through `Policy::decide`, and anything from outside the owner taints the session. Named agents, rules and channels only narrow what a turn may do. They never add a path around the decision function.

### 1. Named agents

#### 1.1 The agent profile

A new table, `agents`, is added in the next free migration step of `src/store.rs` (`STEPS` ends at `to_v3` on `main`, and PR #83 or Harness2 may take `to_v4` first). The rule stays: add a step, never edit one.

| Column | Type and rule | Meaning |
|---|---|---|
| `id` | `TEXT PRIMARY KEY`, `^[a-z][a-z0-9_-]{1,31}$`; `heron` is reserved | stable key |
| `name` | 1–32 characters, letters, digits, spaces, `.`, `'` and `-`; unique ignoring case | display name and first call name |
| `call_names` | JSON array, at most 4 items, each following the `name` rule, unique across all agents | extra names it answers to |
| `instructions` | at most 8,000 characters; `redact_secrets` on save | role, tone, what to do |
| `avatar` | an emoji or an SF Symbol name, at most 32 characters | shown in chat and in the voice panel |
| `voice` | an `AVSpeechSynthesisVoice` identifier, or empty for the system default | live voice and spoken replies |
| `tools` | JSON `{groups: [..], tools: [..], skills: [..], mcp: [..], connectors: [..]}` | the tool set ([§1.2](#12-effective-scope)) |
| `mode_ceiling` | `readonly`, `ask`, `auto` or `none` | highest mode its turns run in; `none` means no ceiling |
| `rules` | JSON list of `tool:summary` globs, each marked `never` or `ask` (PQ4) | its own extra checks |
| `memory_scope` | `shared`, `own`, `private` or `none` | [§1.4](#14-memory-scope) |
| `surfaces` | JSON list; P1 values are `app` and `voice` | where it may be called from; later deltas add channel values |
| `provenance` | `owner` or `imported` | where the profile came from; the pack delta adds `pack:<name>` |
| `state` | `active`, `review` or `archived` | only `active` agents can be called |
| `created_at`, `updated_at` | integers | |

The same step adds nullable columns:

*   `memories.agent TEXT`: null means the shared pool;
*   `sessions.agent TEXT`: the agent the thread belongs to now.

Each assistant message's stored JSON gains `"agent": "<id>"`.

**The default agent.** On first start after the migration, Heron writes row `heron`:

*   `name` is `cfg.agent.name`;
*   `instructions` is empty, because its character stays in `persona.md` and `update_persona` keeps working;
*   tools: every tool in the registry, including ones added later (`tools = {"all": true}`);
*   `mode_ceiling`: `none`;
*   `memory_scope`: `shared`;
*   `surfaces`: all;
*   `provenance`: `owner`.

Heron behaves exactly as today until the owner makes a second agent. All existing memories have `agent = NULL`, so they are shared.

**Tool groups.** These are fixed in `src/agents/groups.rs`, so a profile names groups, not a list that falls behind as tools are added:

| Group | Tools |
|---|---|
| `files` | `read_file`, `write_file`, `edit_file`, `list_dir`, `search_files`, `find_files` |
| `shell` | `shell` |
| `web` | `web_fetch`, `web_search` |
| `memory` | `remember`, `recall`, `recall_unconfirmed`, `forget`, `search_knowledge` |
| `tasks` | `task_add`, `task_list`, `task_update`, `recent_activity` |
| `schedules` | `schedule_add`, `schedule_list`, `schedule_remove` |
| `email` | F8's `draft_email` once `feat/v1-email_drafts` merges; empty before then |
| `calendar` | `upcoming_events`, and later calendar tools as they merge |
| `browser` | `browser_*` |
| `skills` | `use_skill`, limited to the profile's `skills` list |
| `delegate` | `delegate` |
| `admin` | `save_automation`, `update_persona`, `save_skill`, `propose_agent`, `run_shortcut` |

MCP servers and connectors are named one by one in `mcp` and `connectors`. A new agent made in the Agents screen starts with `web` and `memory`, `mode_ceiling = ask` and `memory_scope = own`.

#### 1.2 Effective scope

For every turn, `agents::effective_scope(global_mode, global_rules, agent, surface) -> Scope` computes one `Scope`. It is a pure function in `src/agents/scope.rs`, so it can be tested without a model.

*   **Tools:** the agent's set, intersected with the registry. Global "nevers" still deny at step 2 of `decide`, unchanged.
*   **Mode:** the lowest of three: the global mode, the agent's ceiling, and the surface ceiling. In P1 the surface ceilings are `voice` = `ask` and `app` = none. Channel ceilings come in the P2 delta.
*   **Rules:** the global rules plus the agent's own never and ask rules.
*   **Memory:** the agent's memory scope.

**A `delegate` child keeps its parent's `Scope`.** The child is a clone of the parent agent, so its `Policy`, tool filter and memory handle are the parent's. No P1 code path builds a wider scope for a child.

**Tools are enforced at two points, so hiding them is not the only guard:**

1.  `Agent::tool_schemas` sends the model only the tools in scope.
2.  `Policy::decide` gains a step 2b, right after the deny rules. A tool not in `scope.tools` returns `Verdict::Deny("<tool> is not one of <name>'s tools")`. This catches a model that names a tool it wasn't offered, and a workflow `tool` step.

Mode is enforced by building the turn's `Policy` with the effective mode, the way jobs (`app.rs:1544-1551`) and proactive work (`app.rs:416-420`) already swap the policy for one run.

#### 1.3 Addressing: how a message finds its agent

`agents::address(text, surface, agents) -> Addressed { agent_id, text }` is a fixed parser in `src/agents/address.rs`. The model never chooses the agent.

*   **Chat.** The message matches if it starts with `@` and a call name, after optional white space.
    *   Matching ignores case, and the name must end at a word boundary: a space, `,`, `:` or the end of the text.
    *   The prefix is removed from the text.
    *   A mention later in the message doesn't route, so "ask @Lux later" stays with the current agent.
    *   P1 applies this to the app only. Telegram turns keep running as the default agent until the P2 channels delta.
*   **Voice.** The transcript matches if it starts with a call name followed by `,` or `:`, or with "hey" plus a call name. The speech recogniser's punctuation supplies the comma. "Mango smoothies are good" doesn't route, because no comma follows the name.
*   **Explicit choice.** `POST /api/chat {agent}`, sent by the composer picker, beats the text rule.
*   **Unaddressed messages** go to the thread's `sessions.agent`, or else to `heron`.
*   **Switching.** When a message addresses a different agent:
    *   the thread's agent changes, and the app shows a divider: "Lux joined";
    *   if the new agent is `private` and another agent has already spoken in the thread, a new thread opens instead (PQ3).
*   **Refusals.** If the agent is archived, in `review`, or doesn't list this surface, the default agent replies "Lux isn't available here". No turn runs as that agent.
*   **Unique names.** Names are unique ignoring case, across names and call names, and saving a clash returns `409 name_taken`. Reserved words are refused: `hey`, `all`, `everyone`, and `heron` for any agent except the default.

#### 1.4 Memory scope

| Scope | What the agent reads | Where its new memories go |
|---|---|---|
| `shared` | the shared pool only (`agent IS NULL`) | the shared pool |
| `own` | the shared pool and its own (`agent IS NULL OR agent = :id`) | its own pool |
| `private` | its own pool only | its own pool |
| `none` | nothing | nothing; `remember` and post-turn learning are off |

**The scope lives in the memory handle, not in each caller.**

*   Each turn's `ToolCtx.memory` is a `Memory` bound to `(scope, agent id)` through `Memory::scoped(scope, id)`.
*   Every memory query in `src/memory.rs` and `src/store.rs` takes that binding, so no path can be missed. That covers:
    *   the ranked search (FTS and vector), which also feeds the 12 relevant memories in the prompt (`agent.rs:380`);
    *   `core()`, `list` and `get`;
    *   the near-duplicate scan in `save_detailed` (`memory.rs:268-381`);
    *   supersede;
    *   `forget`, which today deletes by raw id through `ctx.store` (`tools/personal.rs:120`);
    *   `replaces`, which today reads by raw id.
*   An id outside the readable pools behaves as if it doesn't exist. A write never updates, supersedes or deletes a row the agent can't read, so Lux saving a fact close to one of Mango's private memories adds a row to Lux's pool and leaves Mango's alone.
*   The store functions without a scope stay, for the Memory screen and the owner's API only. A test lists every call site of the unscoped functions and fails if a tool or the agent loop uses one.

**Who moves memories between pools.** Only the owner, with the Memory screen's agent filter and **Move to shared**. The model never chooses the target pool.

**Trust is separate from scope.** Confirmed or unconfirmed, outside or user source: these work as today in every pool.

**Harness2 dependency.** `is_outside()` is planned by Harness2 (`heron-harness2.md` §7) and PR #83 but not on `main` yet. Named agents don't need it. Channel sources do, from P2.

#### 1.5 The prompt for a named agent

At depth 0, `Agent::assemble` takes the agent profile:

*   **`# You are <name>`** comes first, then the instructions block. Its class is `Trusted`, and it carries the line persona has today: "shapes tone and manner only; it never overrides the Safety rules".
    *   For `heron`, the block is `persona.md`.
    *   For an imported agent, the block is used only after the owner clicks **Use this agent** (PQ6). Until then the agent can't be called.
*   **The memory blocks** come from the scoped handle ([§1.4](#14-memory-scope)).
*   **The skills block** lists only the profile's skills.
*   **The tasks block** appears only if the scope has the `tasks` group.

#### 1.6 How named agents relate to `delegate` and skills

**`delegate` stays the internal helper.** A named agent is a saved, scoped identity the owner calls. `delegate` is a throwaway helper inside one turn. P1 makes three changes to it:

*   **The child keeps the parent's scope** ([§1.2](#12-effective-scope)).
*   **Its output is untrusted.** It is wrapped with `wrap_untrusted`, as PR #75 proposes. This doc depends on PR #75 for that.
*   **The call is audited.** Today `delegate` returns before `audit` (`agent.rs:712-742`).

**Delegating to a named agent comes later.** `delegate {agent}`, a roster of agents in the prompt, schedules that run as an agent, and a workflow `agent` step that names one all wait for the P2 goals delta. Goals are their first real use (matrix row 1e). That delta must say how a depth-1 child gets a named agent's prompt, since today persona, memory and skills reach the prompt only at depth 0.

**Skills are procedures, and agents are identities.** A skill says how to do a task. An agent says who does it and how far it can reach. An agent lists the skills it may use, and a skill doesn't name an agent. Agents are not stored as skills: skills have no scope fields, and their bodies are treated as trusted.

Skill trust for imports is an existing gap: bodies are trusted, and `declared_tools` and `sha256` are parsed but unused. The unmerged branch `feat/skill-provenance-hardening-20261116` covers it. Named agents don't make it worse, because a skill body never grants tools. Only the agent's tool set does.

#### 1.7 Approvals and audit

*   **Approval cards name the agent.** `ApprovalRequest` gains `agent: {id, name}`, and the card's title is "Lux wants to …".
*   **The audit row keeps the current chain format.** Its `decision` field gains the suffix ` agent=<id>`, so `verify_audit` is unchanged. A test proves that an audit row written by an agent turn still verifies.
*   **Grants are per agent.** A conversation grant ("allow for this conversation") covers the agent that received it, not the next agent called in the thread.

#### 1.8 Sharing (Grok-style bots)

*   **Export.** Writes a `.heronagent` file: TOML with `name`, `call_names`, `instructions`, `avatar`, `voice`, `tools`, `mode_ceiling`, `memory_scope` and `rules`. It never includes memories, keys, connector secrets or the `id`.
*   **Import.** Validates the file with the same rules and creates an agent with `provenance = imported` and `state = review`. It starts with no tools, ceiling `readonly` and scope `private` (PQ6).
*   **The review sheet** shows the instructions in full, and the tools the file asked for as unticked boxes.
*   **Limits.** The file may be at most 32 KB. Unknown keys are refused, not ignored.
*   **Bots other people talk to,** in Slack, Teams or a meeting, are P3 ([§4.1](#41-messaging-channels), [§4.2](#42-meetings)).

#### 1.9 What the owner sees (Mac)

*   **The Agents screen.** A new `Destination.agents`, added after Runs the way F3 added Runs. It has:
    *   a list of agent cards, each showing avatar, name, role line, surfaces and state;
    *   **New**, **Import** and **Export**;
    *   **Talk**, which starts live voice with that agent.
*   **The editor sheet** has fields for name, call names, avatar, voice (with a **Preview** button), instructions, tools (group checkboxes plus MCP and connector pickers), mode ceiling, memory scope, surfaces and rules.
*   **The composer** gains an agent chip that opens a picker, and `@` autocomplete over call names. Each assistant message shows its agent's avatar and name.
*   **Settings › Heron** edits the default agent, as today, with a link to Agents.

**API** (bearer auth, as every route):

*   `GET /api/agents` and `POST /api/agents`;
*   `GET`, `PUT` and `DELETE /api/agents/{id}`; `DELETE` archives the agent;
*   `POST /api/agents/import`, with the file text as its body;
*   `POST /api/agents/{id}/activate`;
*   `GET /api/agents/{id}/export`.

`POST /api/chat` gains an optional `agent`. Errors are `400 bad_agent`, `404` and `409 name_taken`.

### 2. Action rules

Today the owner has modes, allow rules and "never" rules (deny rules, `permissions.rs:286-323`). Both kinds match a glob on `tool:summary`. Allow and deny live in `config.toml` (`permissions.allow`, `permissions.deny`), and deny also has a kv mirror (`app.rs:748-760`).

**P1 adds one verdict, `ask`, to the same store and the same matcher.** It adds no second rule system.

*   **Storage.** `config.toml` gains `permissions.ask`, with a kv mirror kept the same way as the deny rules.
*   **Matching.** It uses the existing glob on `tool:summary`. "Ask: `shell` when the command contains `git push`" is the rule `shell:*git push*`.
*   **Where ask rules sit in `decide`:**
    *   A new step after step 4 (Refused) forces `Ask` in every mode, Yolo included, when an ask rule matches.
    *   Never rules stay at step 2 and beat everything.
    *   Allow rules stay at step 8, with their existing guard: a rule for one shell command never covers chained, piped or substituted commands (`allow_rule_covers`, `permissions.rs:449-457`).
    *   An allow rule never beats taint (step 6), secrets (step 5) or a high-risk effect.
*   **Agent rules.** An agent's own rules are never and ask globs (PQ4), added to its `Scope.rules` for its turns only.
*   **Settings.** The existing "Never" section (`NeverViews.swift`, `SettingsView.swift:267`) becomes **Rules**, with three lists: Never, Always ask, Always allow. Each entry can name one agent or all.

**What waits for P2.** Matching on recipient, domain, host and amount needs typed facts from each tool (`Tool::facts(args) -> RuleFacts`). It comes with the first send tool (R2), where it matters, and the amount fact comes with payments. A glob over summary text would break whenever a tool's summary wording changed, so these facts are not faked with globs.

### 3. Live voice

#### 3.1 Decision: build on `feat/v1-voice`

**Yes.** F7 is finished and tested (18 tests in `VoiceTests.swift`). P1 needs it merged first. Live voice reuses these pieces:

*   `SentenceSplitter`, which speaks while the reply streams;
*   the `SpeechSynthesizing` protocol and the speech queue;
*   the duplicate-event handling and the event wiring from `ChatStore`;
*   `AVSpeechSpeaker`'s immediate stop.

It does not reuse F7's dictation path, which hands the microphone to macOS dictation and can't do turn-taking. Live voice is a new feature folder, `Features/LiveVoice/`, beside `Features/Voice/`.

#### 3.2 What the owner sees

*   **Starting.** A **Talk** button in the chat header, on each agent card, and in the menu bar. ⌥Space pressed twice starts live voice with the current thread's agent. This is F1's hotkey path, which needs no permission.
*   **The Voice panel** has three layouts, after Muse:
    *   a small floating pill;
    *   docked beside the chat;
    *   full window.

    It shows the agent's avatar, the live transcript of both sides, an **End** button and a mute button. The avatar is the existing `AvatarView`, driven by `AvatarStateMachine` (`Sources/HeronUI/Avatar/`). Live voice adds two states to `AvatarState`, `listening` and `hearing` (the owner is speaking), and feeds it voice events. One state machine then drives the sidebar avatar and the voice panel.
*   **Ending.** Esc, **End**, or saying "goodbye" or "stop listening" ends the session. While the session is on, the menu bar shows a **Listening** pill, and macOS shows its own microphone dot.
*   **Turns.** Each spoken turn is also a chat message in the thread, so the conversation is searchable and can be continued by typing.
*   **Calling another agent.** Saying "Mango, …" moves to Mango, and the reply comes in Mango's voice.

#### 3.3 The audio pipeline (all on the Mac)

```text
 mic ─▶ AVAudioEngine input (voice processing ON = echo cancellation)
      ─▶ VoiceActivity (energy + 500 ms end-of-speech hangover)
      ─▶ Transcriber (streaming, on-device): SFSpeechRecognizer, requiresOnDeviceRecognition = true
      ─▶ final text ─▶ POST /api/chat {agent, origin: "voice", message}
 SSE text chunks ─▶ SentenceSplitter (F7) ─▶ AVSpeechSynthesizer (agent's voice) ─▶ speaker
```

*   **Echo cancellation.** Voice processing on the input node (`setVoiceProcessingEnabled(true)`, macOS 10.15 and later) removes Heron's own speech from the microphone signal. Apple's page doesn't say "echo cancellation" in so many words, so the V1 build checks it with a test recording. That is what makes barge-in work without headphones. If it can't be enabled, or the test shows Heron hearing itself, live voice falls back to half-duplex: barge-in only by key or click, as F7 does. The panel says so.
*   **Which recogniser.** P1 uses `SFSpeechRecognizer` with `requiresOnDeviceRecognition = true`, which Apple documents as on-device (macOS 10.15 and later), on every macOS version. G6 holds.
    *   It sits behind a small `Transcriber` protocol whose only other implementation is the file-backed test source.
    *   `SpeechAnalyzer` (macOS 26) streams and downloads its model to the Mac, but the pages read don't say recognition stays on the device (UNCONFIRMED). It waits for a later voice delta that confirms this.
*   **End of speech.** 500 ms of silence by default, adjustable from 300 to 1,200 ms in Settings › Voice. This is the largest fixed cost in the latency budget.
*   **Partial transcripts** show in the panel as the owner speaks. Only the final text is sent.
*   **Permissions.** The app's `Info.plist` gains `NSMicrophoneUsageDescription` and `NSSpeechRecognitionUsageDescription` (R4). If either is refused, the panel says which one and links to System Settings.
*   **Nothing is recorded.** Audio buffers are dropped once transcribed.

#### 3.4 The model path: what decides latency

The audio side costs about 0.6 s: about 0.5 s of end-of-speech wait, about 0.1 s to finalise the transcript, and under 0.1 s for local speech to start. So the model's time to first sentence decides whether G5 holds.

| Provider path | Fits G5? | Why |
|---|---|---|
| HTTP streaming (`llm.rs` `HttpLlm`, any OpenAI-compatible endpoint: a local Ollama or LM Studio, or a paid API) | yes, if first token ≤ 0.4 s | streams text as `Event::Text` today |
| CLI provider (`cli_llm.rs`, 15 providers) | not expected; to be measured | a new CLI process per model call, each starting the official CLI with its own start-up and sign-in checks. Text does stream: the decoded `text` of the JSON envelope is passed on as it arrives (`envelope_text_prefix`, `cli_llm.rs:985-995`). The process start comes before the first token on every turn |

So live voice gets its own model setting, `voice.provider` and `voice.model` in `config.toml`, with the three choices in PQ1. When the main provider is a CLI and no voice model is set, live voice still works, but the panel says "Replies start in a few seconds with <CLI name>. Choose a faster voice model in Settings › Voice." The measured times are shown, never claimed.

**First-sentence shaping.** A voice turn adds one line to the system prompt: "You are speaking aloud. Answer in short sentences, the first one under 12 words. No Markdown, lists or code." This makes the first sentence end quickly, so `SentenceSplitter` can hand it to speech early.

**Measuring.**

*   The Mac app holds every timestamp, since it sees the end of speech, the first SSE text and the first audio. For each voice turn it appends one line to `voice-timing.jsonl` in its Application Support folder, keeping the last 50 lines.
    *   Each line has: `speech_end`, `transcript_final`, `first_token`, `first_sentence` and `first_audio`, as offsets in ms, plus the agent, the model and whether echo cancellation was on.
    *   No core route or kv key is added.
*   Settings › Voice shows p50 and p90 over the last 20 lines.
*   The gate G-V ([Verification Plan](#verification-plan)) runs 20 recorded utterances through the pipeline on the owner's Mac.

#### 3.5 Barge-in and soft cancel

Today a run stops only when its tokio task is aborted (`server.rs:645-692`). Aborting in the middle of a tool can leave a half-done action. P1 adds a soft cancel:

*   **The token.** `CancelToken` is an `Arc<AtomicBool>` per run.
    *   It is stored with the run's task handle in `ServerState.running` (`server.rs:49`) and passed to `run_turn`. Tools never see it.
    *   `POST /api/cancel {session_id, soft: true}` sets it.
    *   Without `soft`, the Stop button keeps today's abort behaviour.
*   **Where the loop checks it.** `run_turn` checks the token in three places:
    *   before each model call;
    *   between streamed text chunks, where it drops the stream;
    *   before each tool call.

    A tool that is already running finishes, and its result is recorded. The run then ends with `Event::Cancelled`. PR #84's effect journal still covers a hard abort.
*   **What triggers it.** The voice panel's `VoiceActivity` sees at least 200 ms of speech while Heron is speaking. Speech stops at once (`stopSpeaking(at: .immediate)`, target under 150 ms), and the soft cancel is sent.
*   **What gets stored.** The assistant message is cut to the sentences already spoken, plus `[interrupted]`, so the model knows where it was cut off.

#### 3.6 Voice and safety

*   **Content entry.** A new `ContentEntry::VoiceUtterance` (the enum from `heron-v1-features.md`'s B2, on `feat/v1-features-base`) has class **Trusted**: the owner is present and speaking. The microphone can also pick up a television or another person, so voice turns get the surface ceiling `ask` ([§1.2](#12-effective-scope)). In Auto mode a spoken request still asks before writing files or running commands.
*   **Approvals while in voice.** The card appears on screen, and the agent says one short line: "It's on screen for you to approve." Approving by speech is off by default (PQ2). A send, a call or a payment is never approved by voice.
*   **Taint.** A voice turn that reads outside content taints the thread, as a typed turn would. Tainted asks are never approvable by voice, even with PQ2 on.

#### 3.7 Later voice work

*   **A later voice delta:** `SpeechAnalyzer`, once confirmed on-device.
*   **P4:**
    *   a wake word;
    *   a character avatar built on `AvatarView` ([muse-ux.md](research/muse-ux.md) warns against a childlike mascot);
    *   voice in channels: Telegram and WhatsApp voice notes.
*   **The calls phase** reuses the turn-taking settings but not the audio path ([§4.4](#44-outbound-calls)).

### 4. The remaining gaps, ranked (direction)

**This section and §5 and §6 set direction, not approved design.** Each decision here belongs to the delta named at the end of its part, which may change it. They are written out so that the owner can see the whole plan and so that P1 doesn't block any of them.

Each part says what the owner sees, how it fits a local-first Mac app, the pieces it needs, its safety rules, its running cost ([§9](#9-running-cost)) and its phase.

#### 4.1 Messaging channels

*   **What the owner sees.** Settings › Channels lists Telegram, Slack, Messages, WhatsApp and Teams, each with a **Pair** button and a mode ceiling. The owner talks to Heron, or to any agent by name, from any of them. Every channel shares Heron's memory under the agent's scope, because they use the same store. A fact learned on WhatsApp is recalled in Slack.
*   **Decision, as the brief requires: extend `src/channel.rs`.** It already has the right pieces: an authenticated identity, a dedup journal, a durable outbox with leases, and `ChannelKind` values for Slack and WhatsApp. P2:
    *   implements `ChannelAdapter` for real;
    *   moves Telegram's pairing (code, lockout after 5 wrong codes) from `telegram.rs` into `channel.rs`, so every adapter shares it;
    *   adds `ChannelKind::Messages` and `ChannelKind::Teams`;
    *   adds one `Origin::Channel` with a `sessions.channel` column, rather than a new `Origin` variant per channel.

    Telegram moves onto the trait in the same delta.
*   **Sender classes:**

    | Sender | Content entry | Class | What it may do |
    |---|---|---|---|
    | the paired owner account | `ChannelOwnerMessage` | Trusted | call any agent whose `surfaces` include the channel, up to the channel ceiling |
    | anyone else (a group, a shared Slack channel, a forwarded message) | `ChannelGuestMessage` | Outside, taints | nothing unless PQ7; then a reply that asks, memory scope `none`, read-only tools |

    `accept_inbound` keeps refusing unknown senders for direct chats. For a shared channel the owner adds, guest messages are accepted only when an agent on that channel has `guest_replies` set.
*   **Channel ceilings (R8):** every channel's mode ceiling defaults to `ask`, Telegram included.
*   **The channels:**

| Channel | How it connects | Needs a public address? | Phase | Pieces |
|---|---|---|---|---|
| Slack | an app in the owner's workspace, Socket Mode (a websocket the Mac opens) | no | P2 | Slack app manifest; app-level and bot tokens in the Keychain |
| Messages (iMessage) | the owner's self-chat. The Mac app reads that one thread from `~/Library/Messages/chat.db` with one fixed query and sends with Messages automation (R3, PQ8) | no | P2 | Full Disk Access; Automation permission for Messages |
| WhatsApp | WhatsApp Business Platform Cloud API, on a dedicated number | yes (webhook) | P3 | the relay; Meta Business account; number |
| Teams | a bot registered in Azure; the owner's tenant allows the custom app | yes (messaging endpoint) | P3 | the relay; Azure bot registration |
| Telegram | today's long-poll bot | no | done | moves onto the trait in P2 |

*   **Owning doc.** A P2 channels delta covers Slack, Messages and the pairing move. A P3 delta covers WhatsApp and Teams with the relay.

#### 4.2 Meetings

*   **P3a, local notes (no bot, no cost).**
    *   **What it does.** When Zoom, Teams or a browser meeting tab is active, Heron offers **Take notes**. It captures the meeting app's audio through a Core Audio process tap (macOS 14.2 and later) and the microphone, and transcribes both on the device. The two streams are labelled "You" and "Others".
    *   **After the meeting.** The agent writes notes and action items. The action items become tasks the owner confirms.
    *   **Trust.** The transcript is `MeetingTranscript`, class Outside, because other people's words are untrusted.
    *   **Consent.** Each time, the panel reminds the owner to tell others the meeting is being transcribed. Some US states require every party's consent.
*   **P3b, a meeting bot.**
    *   **Joining.** A meeting-bot service joins Meet, Zoom or Teams by link, under the name "<agent> (notes for <owner>)". The service is chosen in the meetings delta from [provider facts](research/heron-parity-provider-facts.md). Joining asks for each meeting, and the card shows the calendar event.
    *   **Answering in chat.** The bot can answer questions put to it in the meeting chat. That is a guest-facing reply (PQ7): each message asks unless an action rule allows that meeting.
    *   **Data path.** The service's webhooks reach the Mac through the relay.
*   **Owning doc:** a P3 meetings delta.

#### 4.3 Deliverables: documents, slides, dashboards

*   **What the owner sees.** "Make me a one-page brief", "Turn this into slides" or "A dashboard of my spending" produce a file in `~/Heron/Made/<slug>/`. It opens in an in-app viewer with **Edit**, **Ask for changes**, **Export** and **Show in Finder**.
*   **Formats, all made locally:**
    *   documents in Markdown, exported to `.docx` with the built-in `textutil` and to PDF with WebKit;
    *   slides as one HTML file per deck, exported to PDF. `.pptx` export goes through Keynote's scripting when Keynote is installed, otherwise PDF only;
    *   dashboards as HTML with SVG charts the core draws, so they need no JavaScript, plus a JSON data file. A schedule can refresh the data, as Muse Spaces refresh.
*   **Safety.** The viewer is a `WKWebView` with JavaScript off and a content rule list that blocks every network load. A model-written page can't send data anywhere. Writing the files is `WorkspaceWrite` inside `~/Heron/Made/`.
*   **Cost:** model tokens only.
*   **Phase:** P2, in a deliverables delta.

#### 4.4 Outbound calls

*   **What the owner sees:** CUJ P-6. A call brief card holds:
    *   the number and where it came from (a number found on the web is tainted and must be confirmed);
    *   the goal;
    *   what may be shared;
    *   the limits: maximum spend, no card details, a time limit (default 10 minutes);
    *   the agent.

    After approval: a live transcript, **Take over**, which bridges the call to the owner's phone number, and **End call**. A summary goes to the Inbox, and the transcript to the thread.
*   **Pieces.** A phone number and per-minute telephony, from Twilio or the voice platform's own numbers, plus a hosted voice-agent platform (PQ9). The platform gets only the brief as its prompt and two tools: `end_call` and `ask_owner`.
    *   `ask_owner` reaches Heron through the relay and shows a live card, for example "Luigi's asks: thin crust or regular?". The owner answers by click or voice.
    *   The platform has no access to Heron's memory or tools.
*   **Why hosted.** Twilio's Media Streams need "a WebSocket server that accepts secure WebSocket connections (TCP port 443)", which means a public endpoint. Routing call audio through the relay to the Mac and back adds delay and a single point of failure. The hosted platforms found (ElevenLabs Agents, Vapi, Retell) all place outbound calls over a Twilio number and can transfer to a human number, which is what **Take over** needs. Vapi and Retell send live transcript events; ElevenLabs' webhook is post-call only, so its live transcript would need its websocket ([provider facts](research/heron-parity-provider-facts.md) §1). A later delta may move calls onto Heron's own pipeline once live voice is measured.
*   **Safety:**
    *   every call asks, and is never allowed by a rule;
    *   emergency, premium-rate and international numbers are blocked unless the owner adds a rule for one number;
    *   the agent says it is an AI calling for the owner, and that the call is transcribed;
    *   calls happen only between 08:00 and 21:00 in the callee's time zone;
    *   calling the same number again within 24 hours asks with a warning.
*   **Law.** The FCC's declaratory ruling FCC 24-17 (released 2024-02-08) says the TCPA's "artificial or prerecorded voice" rules cover AI-generated voices. Washington and Florida require every party's consent to record a call. California is widely reported to as well (UNCONFIRMED here). Heron transcribes, so the opening line asks for consent, and the calls delta must check the law for calls placed to businesses at the owner's request ([provider facts](research/heron-parity-provider-facts.md) §1).
*   **Phase:** P3, in a calls delta.

#### 4.5 Mobile app

*   **What the owner sees:** an iPhone app with chat, approvals with Face ID, the Inbox, and live voice with any agent.
*   **Fit.** The Mac stays the brain. The phone talks to it end-to-end encrypted through the relay, and pairs by QR code. When the Mac is asleep or off, the phone queues messages and says so.
*   **Pieces:** the relay (P3), APNs push (needs the Apple Developer Program), and TestFlight.
*   **Until then,** Messages, Telegram and WhatsApp are the phone surface.
*   **Phase:** P4. **Owning doc:** `heron-mobile.md` (R1, PQ13).

#### 4.6 Approved checkout

*   **P2, hand-off.**
    1.  The agent fills a cart in the browser: today's `browser_*` tools, or the computer-use design's browser once it lands.
    2.  It stops on the payment page, and the owner is shown a card: merchant, items, total.
    3.  The owner pays themselves with Apple Pay or their saved card.
    4.  Heron never types card data. A new rule blocks `browser_type` into fields whose autocomplete type is `cc-*`.
*   **P4, agent card.**
    *   **The card.** A provider issues a single-use or merchant-locked card for the exact amount the owner approved. Approval needs Touch ID on the Mac (LocalAuthentication) or Face ID on the phone.
    *   **Card data.** The card number goes from the provider to the browser form through a fixed filler in the core. It never enters the model's context, the transcript or the audit row; only the last four digits are kept.
    *   **A new effect.** `Effect::Payment` is high risk. It is never granted for a conversation or "always", never approved by voice, and never from a channel. It needs a design delta for the security core.
    *   **Provider choice:** in the checkout delta. The candidates found on 2026-11-23 ([provider facts](research/heron-parity-provider-facts.md) §5) are:
        *   Privacy.com's API, from its $5-a-month plan;
        *   Stripe's Link Agent Wallet, US and Canada, in preview;
        *   Shopify's Shop MCP, where the buyer confirms the total and payment method, for Shopify stores;
        *   Visa Intelligent Commerce, which needs onboarding to Visa's program.

        Stripe Issuing is for businesses, so it doesn't fit one person.
*   **Cost:** the provider's fees ([§9](#9-running-cost)).

### 5. The Muse and Grok Bot extras (direction)

#### 5.1 Goals with action plans

*   **What the owner sees.** "Goal: run a half marathon in April" leads to a plan the agent proposes: steps, dates and a check-in schedule. The owner edits and approves it. Approved steps become tasks, and check-ins become schedules that run as the goal's agent.
*   **The Goals screen** shows progress, and the agent comes back with ideas. That part is the proactive design's job: proactive suggestions and interactive onboarding.
*   **Data:** a `goals` table (`id`, `title`, `why`, `agent`, `state`, `created_at`); `tasks.goal` and `schedules.goal` columns.
*   **Safety.** A goal made in a tainted session starts in state `proposed`, and its check-ins are Outside until the owner approves the plan.
*   **Phase:** P2, in a goals delta, written with the proactive design.

#### 5.2 Tools the agent builds

When a task needs a tool that doesn't exist, the agent picks the first of these that works:

1.  **A skill**, a written procedure, through `save_skill`. This exists today, and its effect is `Privileged`, so it asks.
2.  **A Shortcut** the owner already has, through F5's `run_shortcut`, once the owner ticks it.
3.  **A connector manifest**, the format from Harness2 §7, run by Harness2's fixed executor in `src/ext/`.
    *   **The change (R6).** An untainted turn may call `draft_connector`, whose effect is `Privileged`, to write `~/.heron/connectors/<name>.toml` and its test pairs. The executor loads it at runtime as **untrusted**, so every call is `ExternalWrite` and asks, until the owner turns on **Trust** on its card.
    *   **Hosts.** The manifest's hosts must also be on PR #84's egress allowlist, which the owner edits.
    *   **A tainted turn can't draft.** Instead it files a connector request through Harness2's report path with provenance `pasted`.
4.  **Code tools** (a script in the shell sandbox) stay out. The `shell` tool already covers this case, under its own checks.

This reuses Harness2's manifest format and executor rather than adding a second way to call APIs. It depends on Harness2 graph 1b (units H8 to H8t) being built. **Phase:** P2.

#### 5.3 Small-business pack and Instagram

*   **The pack format.** A folder with `pack.toml` (name, version, description) and three subfolders:
    *   `agents/*.heronagent`
    *   `skills/*.md`
    *   `connectors/*.toml`
*   **Installing.**
    1.  The owner gets one review sheet that lists every piece.
    2.  Agents arrive in `review` (PQ6), connectors arrive untrusted, and skills arrive with provenance `pack:<name>`.
    3.  **Remove pack** removes all three.
*   **Pack v1 contents:**
    *   a **Front desk** agent that answers customer messages on WhatsApp or Instagram as a guest-facing agent (PQ7);
    *   a **Bookkeeper** agent with read-only connectors for Stripe, Square and QuickBooks;
    *   a **Social** agent, for drafting and scheduling Instagram posts;
    *   a **Bookings** skill over the calendar.
*   **Instagram.** The Instagram API with Instagram Login needs a professional account (Business or Creator) and a Meta app. An app used only for a business the owner manages gets Standard Access, which needs no App Review. A Tech Provider app serving many businesses needs review. Messaging works only after a user has messaged the account first, inside a 24-hour window ([provider facts](research/heron-parity-provider-facts.md) §6). The connector is a manifest with OAuth auth. Harness2 says OAuth manifests are never auto-eligible, so it is written in a PR, not at runtime.
*   **Phase:** P3, in a pack delta.

#### 5.4 The shared computer the owner can take over

*   **Owned elsewhere.** Muse's isolated VM with a full browser, which the user and agent share, is designed by the router and computer-use session ("Heron auto model router + Mac computer use").
*   **What this doc adds:** the `computer` tool group, so the owner chooses which agents may use the computer, and approval cards that name the agent.
*   **Assumed interface (to confirm with that design):**
    *   computer actions are tools in the registry;
    *   they go through `decide`;
    *   their output is untrusted;
    *   take-over pauses the agent.

### 6. The relay (direction)

WhatsApp, Teams, call platforms, meeting bots and the mobile app all need a public HTTPS address. The Mac has none and should not get one.

*   **What it is.** A Cloudflare Worker that the owner deploys from `relay/` with one command (PQ10). It has three jobs:
    1.  It checks each provider's webhook signature on the raw body, and drops anything that fails.
    2.  It encrypts the body to the Mac's public key, a sealed box whose key lives in the Mac's Keychain, and stores only that ciphertext for at most 7 days.
    3.  It answers the Mac's long-poll (`GET /pull`, authenticated with a per-install token), and deletes a message when the Mac acknowledges it.
*   **What it never does:**
    *   run Heron's tools;
    *   hold a model key;
    *   keep plaintext after the signature check;
    *   accept a message without a valid provider signature.
*   **Outbound sends** (a WhatsApp reply, a Teams message) go straight from the Mac to the provider's API, not through the relay.
*   **Content entry:** `RelayEvent` has class Outside, and its sender class is decided by the channel ([§4.1](#41-messaging-channels)).
*   **Phase:** P3, in its own delta, because it is the first network-facing code Heron ships.

### 7. New content entries

These extend the trust table in `heron-v1-features.md` (Content entries, B2), which has one variant per way content enters a turn. Output from a connector Heron drafts at runtime ([§5.2](#52-tools-the-agent-builds)) reuses Harness2's existing `ext:` source class, so it needs no row here.

| Variant | Where it comes from | Class | Phase |
|---|---|---|---|
| `VoiceUtterance` | final transcript of the owner's speech in live voice | Trusted (surface ceiling `ask`) | P1 |
| `AgentInstructions` | an agent profile's instructions | Trusted once the owner made or reviewed it; an agent in `review` can't run | P1 |
| `DelegateResult` | a named or unnamed `delegate` child's output | Outside (PR #75) | P1 |
| `ChannelOwnerMessage` | a message from the paired owner on any channel | Trusted (channel ceiling) | P2 |
| `ChannelGuestMessage` | a message from anyone else, or a forward | Outside | P2 |
| `GoalCheckIn` | a check-in prompt of an approved goal | Trusted; Outside while the goal is `proposed` | P2 |
| `MeetingTranscript` | meeting audio transcribed, or a bot's transcript | Outside | P3 |
| `CallTranscript` | a call's transcript and `ask_owner` questions | Outside | P3 |
| `RelayEvent` | anything pulled from the relay | Outside unless the channel marks it an owner message | P3 |

### 8. Outward actions: one table

| Action | Phase | Approval | Never |
|---|---|---|---|
| Send a message on a channel to the owner (a reply) | P1/P2 | none (it is the reply) | — |
| Send a message or email to anyone else | P2 | each send shows recipient and full text; "always" only as a rule scoped to recipient or domain | tool-wide "always"; from a tainted turn without asking |
| Reply to a guest in a shared channel or meeting chat | P3 | each reply, unless a rule allows that channel (PQ7) | memory recall in that turn |
| Place a phone call | P3 | each call, with the brief | rules; emergency, premium or international numbers |
| Join a meeting as a bot | P3 | each meeting | joining a meeting not on the owner's calendar without the link being typed by the owner |
| Pay (hand-off) | P2 | the owner pays | Heron typing card fields |
| Pay (agent card) | P4 | Touch ID or Face ID each time | voice; channels; conversation grants; rules |
| Publish to Instagram | P3 | each post shows the final media and caption | "always" |

### 9. Running cost

These are monthly estimates for one owner with moderate use. Prices are from [provider facts](research/heron-parity-provider-facts.md), checked 2026-11-23. Where a price couldn't be confirmed, the note says UNCONFIRMED, and the delta for that phase rechecks it before build.

| Piece | Phase | Price (source: facts note) | Rough monthly for one owner |
|---|---|---|---|
| Named agents, ask rules | P1 | nothing | $0 |
| Live voice, audio side | P1 | nothing (on-device) | $0 |
| Live voice, model, choice 1 or 2 | P1 | the owner's plan, or a local model | $0 |
| Live voice, model, choice 3 | P1 | a Haiku-class API model: Claude Haiku 5.5 is $0.10 per million input tokens and $0.50 per million output | under $1 for 60 voice minutes, assuming about 3k input and 150 output tokens a turn, 4 turns a minute, with caching |
| For comparison: a realtime speech API (Alternative D) | — | OpenAI GPT-Live $0.05 per active minute; gpt-realtime-2.1 audio $32/$64 per million tokens | $3 for 60 minutes of GPT-Live |
| Slack, Messages, Telegram | P2 | Slack free plan allows up to 10 custom apps; Socket Mode needs no server | $0 |
| Deliverables, goals, own tools | P2 | model tokens | within the plan |
| Relay | P3 | Cloudflare Workers Free: 100,000 requests a day | $0 |
| WhatsApp | P3 | per-message pricing since 2025-07-01; replies within 24 h of the owner's message: 1,000 free per number a month since 2026-10-01, then charged (rate UNCONFIRMED) | $0 at personal volume |
| Teams | P3 | Azure AI Bot Service: Teams is a standard channel, unlimited on the Free tier | $0 (plus the Microsoft 365 tenant the owner already has) |
| Phone calls | P3 | Twilio US number $1.15 a month; outbound $0.014 a minute; voice platform $0.05–0.08 a minute (Vapi example, ElevenLabs) or $0.07–0.31 (Retell), plus its model | about $2 for 10 calls of 5 minutes, plus the number |
| Meeting notes, local | P3a | nothing | $0 |
| Meeting bot | P3b | Recall.ai $0.50 an hour, plus $0.15 an hour for its transcription | about $5 for 8 one-hour meetings |
| Agent card | P4 | Privacy.com API starts on its $5-a-month Plus plan; Stripe Link Agent Wallet is US and Canada, preview | $5 or provider fees |
| Mobile app | P4 | Apple Developer Program | $99 a year |

### 10. Phases and owning docs

| Phase | What lands | Before it can start | Design |
|---|---|---|---|
| P0 (preconditions) | the same list as the [Preconditions](#preconditions) below: `feat/v1-features-base`, F1, F7, PR #75 and PR #84 | owner merges | existing |
| **P1** | named agents, ask rules, live voice (button and hotkey), export and import | P0 | **this doc** |
| P2 | Slack, Messages, Telegram on the trait, channel ceilings (R8), `@Name` on channels; deliverables; goals, with delegating to a named agent and schedules that run as one; runtime draft connectors; hand-off checkout; sending messages and email (R2), with recipient, domain and amount rules | P1; Harness2 graph 1b for draft connectors | four short deltas |
| P3 | relay; WhatsApp, Teams; meetings (local, then bot); calls; small-business pack; Instagram; guest-facing agents | P2; owner accounts | five short deltas |
| P4 | agent card and `Effect::Payment`; mobile app; wake word; avatar; voice in channels | P3 | deltas plus `heron-mobile.md` |

The G9 owner trial runs until 2026-12-05. Nothing here merges code before it ends.

## Alternatives Considered

### Alternative A: Named agents as skills with a persona section

*   **Pros:** no new table, and skills already load from disk.
*   **Cons:** skills have no scope fields. Their bodies are trusted, and their `declared_tools` is unused. A skill can't hold a memory scope or a mode ceiling.
*   **Reason for rejection:** an agent's main job is to limit reach. Skills can't express limits.

### Alternative B: Named agents as separate Heron processes, one `HERON_HOME` each

*   **Pros:** hard isolation of memory and tools.
*   **Cons:**
    *   no shared memory, which the owner wants;
    *   N background workers competing for the background-work lock;
    *   approvals spread over N processes;
    *   N times the start cost.
*   **Reason for rejection:** isolation through scope filters in one process meets G3 and G4 and keeps one approval surface.

### Alternative C: Let the model pick the agent

*   **Pros:** natural phrasing like "ask the research one".
*   **Cons:** a prompt injection in outside content could re-route a turn to an agent with wider tools.
*   **Reason for rejection:** the agent decides the scope, so a fixed parser must pick it. The model can still suggest "Lux could do this", and the owner calls Lux.

### Alternative D: Speech-to-speech API (a realtime voice model) for live voice

*   **Pros:** about 0.5–0.8 s round trip, and turn-taking built in.
*   **Cons:**
    *   audio leaves the Mac (against G6);
    *   it costs per minute;
    *   its tool calls must still be bridged into `decide`;
    *   it bypasses the agent's own model and instructions path.
*   **Reason for rejection:** the cascaded on-device pipeline with a fast streaming model meets G5 and keeps audio local. It may return as an opt-in choice 4 in a later voice delta, if the gate shows the cascaded path misses G5.

### Alternative E: Keep F7's dictation path for live voice

*   **Pros:** no microphone permission.
*   **Cons:** macOS dictation gives no end-of-speech signal, no barge-in and no streaming hook. It also can't be started reliably from a button (marked UNVERIFIED on the branch).
*   **Reason for rejection:** it can't meet G5.

### Alternative F: A Cloudflare Tunnel to the Mac's server instead of a relay

*   **Pros:** less code.
*   **Cons:** it puts the core's HTTP server on the internet, behind only a bearer token.
*   **Reason for rejection:** a store-and-forward relay that checks signatures and never reaches the Mac keeps the Mac with no inbound path.

### Alternative G: New channels as separate modules beside `channel.rs`

*   **Pros:** each adapter stays independent.
*   **Cons:** the dedup, outbox and pairing logic would be duplicated per channel, and `channel.rs` was written to be shared.
*   **Reason for rejection:** extending `channel.rs` reuses tested code.

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| A short or common agent name ("Mango") routes speech by accident | voice needs a comma or "hey" after the name; reserved words are refused; the panel shows which agent took the turn |
| The owner gives an agent the `admin` group and it edits other agents | `propose_agent` stays `Privileged` (asks every time); the editor warns when `admin` is ticked |
| A television says "Heron, delete my files" in live voice | the voice surface ceiling is `ask`; outward and destructive actions show a card; voice approval is off by default |
| CLI providers can't meet the latency goal | the voice model setting (PQ1); the panel shows measured times; G5 is stated only for HTTP providers |
| Echo cancellation unavailable on some setups | half-duplex fallback; the panel says so |
| A memory read or write path skips the scope | the scope is bound into the `Memory` handle and every store memory query takes it; `unscoped_memory_fns_not_used_by_tools` fails if a tool or the agent loop calls an unscoped function; tests plant memories in three pools and check reads, dedupe, supersede and `forget` |
| The Messages exception grows into general Messages reading | only the Mac app reads, with one fixed query; tools keep `Refused`; a test greps that no Rust code opens `chat.db` |
| A guest in Slack injects instructions | guest messages are Outside and taint; guest-facing turns have memory scope `none` and read-only tools |
| Telephony and AI-call law | calls ask each time, disclose, keep hours, block risky numbers; the calls delta rechecks law |
| Running costs grow unseen | every paid piece is off until the owner sets it up; the Usage tab gains rows per paid piece in its delta |

## Non-Functional Requirements

*   **Security and privacy:**
    *   no new path around `Policy::decide`;
    *   audio stays on the Mac in P1;
    *   imported agents start with nothing;
    *   the relay holds only ciphertext;
    *   every new input has a content entry ([§7](#7-new-content-entries)).
*   **Performance:**
    *   G5's latency goal for live voice;
    *   the scope computation is a pure function, run once per turn;
    *   memory filters add one indexed `WHERE` clause, with a new index on `memories(agent)`.
*   **Rollout and rollback:**
    *   P1 ships behind `[agents] enabled` and `[voice] live` in `config.toml`, both off in the G9 trial build;
    *   with agents off, the default agent is the only one and the composer has no picker;
    *   the migration adds a table and nullable columns only, so an older build ignores them.

### Additional Resources

*   [research/heron-parity-competitors.md](research/heron-parity-competitors.md): what the four products do.
*   [research/heron-parity-provider-facts.md](research/heron-parity-provider-facts.md): third-party requirements and prices.
*   [research/muse-ux.md](research/muse-ux.md): Muse's UI and trust patterns, and its failures.
*   [heron-v1.md](heron-v1.md): the decision function (§1), API contract (§3) and Mac app (§4).
*   [heron-v1-features.md](heron-v1-features.md): content entries (B2), F1 quick entry, F5 shortcuts, F7 voice.
*   [heron-harness2.md](heron-harness2.md): connector manifests and the executor (§7).
*   [../channel-gateway.md](../channel-gateway.md): the channel transport and C35-8.
*   PR #75 (delegated tasks untrusted), PR #83 (multi-account), PR #84 (effect journal, loop guard, egress allowlist).
*   Design sessions in flight: "Proactive Heron + interactive onboarding" and "Heron auto model router + Mac computer use".

### Future Improvements

*   **Agents talking to each other on a schedule** (a "team" that plans together), once single-agent goals have been used for a while.
*   **A per-agent usage view** in the Usage tab.
*   **Bring-your-own voice platform for calls,** once Heron's own pipeline is measured.

--------------------------------------------------------------------------------

## Detailed Implementation Plan

This covers **Phase 1 only**: named agents, ask rules and live voice. Later phases have no file list here, because each needs its own delta.

### Preconditions

These are the same items as P0 in [§10](#10-phases-and-owning-docs).

1.  **Merged:**
    *   `feat/v1-features-base`, which supplies `ContentEntry`;
    *   F1 (`feat/v1-quick_entry`), which supplies the hotkey for ⌥Space;
    *   F7 (`feat/v1-voice`), which supplies `SentenceSplitter`, `SpeechSynthesizing` and `SpokenReplies`.
2.  **Merged:** PR #75, so that `delegate` output is wrapped untrusted. If it hasn't merged, unit A3 adds the wrap itself, under the same test name.
3.  **Built:** PR #84's effect journal. A soft cancel never cuts a tool short, so P1 doesn't strictly need it. A hard abort does.
4.  **Confirmed when the build starts:** the next free migration step on `main`. This doc calls it `to_vN`.
5.  **Both config flags exist and are off:** `[agents] enabled = false` and `[voice] live = false`.

### Target Files to Create/Modify

#### Unit A1: `src/agents/` (Create)

1.  **`src/agents/mod.rs`**
    *   `AgentProfile`, with the fields of [§1.1](#11-the-agent-profile).
    *   `validate(&AgentProfile, &[AgentProfile]) -> Result<()>`, which checks the name rule, uniqueness ignoring case across names and call names, reserved words, length limits and JSON shapes.
    *   `default_agent(cfg) -> AgentProfile`.
2.  **`src/agents/groups.rs`**
    *   The group table of [§1.1](#11-the-agent-profile).
    *   `expand(tools_json, registry_names) -> BTreeSet<String>`. `{"all": true}` expands to the whole registry.
3.  **`src/agents/scope.rs`**
    *   `Scope {tools, mode, rules, memory}`.
    *   `effective_scope(global_mode, global_rules, agent, surface) -> Scope`.
    *   `surface_of(Origin) -> Surface`.
    *   The `Mode` order is `readonly < ask < auto < yolo`. Ceiling `none` means no cap.
4.  **`src/agents/address.rs`**
    *   `address(text, surface, agents) -> Addressed`, following the rules of [§1.3](#13-addressing-how-a-message-finds-its-agent).
5.  **`src/agents/share.rs`**
    *   `export(&AgentProfile) -> String`, as TOML.
    *   `import(text) -> Result<AgentProfile>`. It refuses unknown keys and files over 32 KB, and sets provenance, state, tools, ceiling and scope as in PQ6.
6.  **Tests, inline.** Each name is listed in the unit's check:
    *   `scope_never_widens_past_global`
    *   `voice_surface_caps_at_ask`
    *   `ceiling_none_keeps_global_mode`
    *   `agent_rules_only_never_or_ask`
    *   `telegram_origin_is_default_agent_only`
    *   `address_needs_leading_at_in_chat`
    *   `address_ignores_mid_text_mention`
    *   `address_voice_needs_comma_or_hey`
    *   `explicit_agent_beats_text`
    *   `names_unique_ignoring_case_across_call_names`
    *   `reserved_names_refused`
    *   `import_refuses_unknown_keys`
    *   `import_starts_in_review_with_no_tools`
    *   `export_has_no_memories_or_id`
    *   `groups_all_expands_to_registry`

#### Unit A3: wiring in `src/` (Modify)

1.  **`src/store.rs`**
    *   Step `to_vN` creates `agents`, adds the nullable columns `memories.agent` and `sessions.agent`, and adds an index on `memories(agent)`.
    *   Agent CRUD.
    *   Every memory query gains a `MemoryScope` argument: search, FTS, vectors, `core`, `list`, `get`, `delete`, the near-duplicate scan and supersede. The unscoped versions stay, renamed `*_unscoped`, for the Memory screen and the owner's routes.
2.  **`src/memory.rs`**
    *   `Memory::scoped(scope, agent_id)` returns a handle that passes its scope to every store call.
    *   `save_detailed`'s near-duplicate scan, update and supersede run inside the handle's readable pools. New rows go to the handle's write pool.
3.  **`src/tools/personal.rs`**
    *   `forget` and `replaces` go through `ctx.memory`, the scoped handle, instead of `ctx.store`.
    *   Adds `propose_agent` (`Privileged`), which returns the draft profile for the owner's card.
4.  **`src/permissions.rs`**
    *   `Policy` gains `scope: Option<Scope>` and an `ask` rule list.
    *   `decide` gains two steps: step 2b refuses a tool that isn't in scope, and a new step after step 4 asks when an ask rule matches.
    *   `ApprovalRequest` gains `agent`.
5.  **`src/config.rs`**
    *   `permissions.ask`, with a kv mirror kept the same way as `deny`.
    *   `[agents] enabled`.
    *   `[voice] live`, `provider`, `model` and `end_of_speech_ms`.
6.  **`src/agent.rs`**
    *   `Agent` carries an `AgentProfile` and a `Scope`.
    *   `assemble` follows [§1.5](#15-the-prompt-for-a-named-agent), and `tool_schemas` filters by scope.
    *   `Origin::Voice`.
    *   `delegate`: the child keeps the scope, its output is untrusted, and the call is audited.
    *   `run_turn` takes a `CancelToken` and checks it before each model call, between text chunks and before each tool call.
    *   Assistant message JSON gains `agent`.
    *   The audit `decision` gains ` agent=<id>`.
    *   The voice turn gets its system-prompt line.
7.  **`src/llm.rs`:** builds the voice provider from `[voice] provider` and `model` when set, using the existing `HttpLlm`.
8.  **`src/app.rs`:** creates the default agent row at start, and looks up the agent for each turn.
9.  **`src/server.rs`**
    *   The agents routes of [§1.9](#19-what-the-owner-sees-mac).
    *   `POST /api/chat {agent?, origin?}`. `origin` accepts only `voice`.
    *   `POST /api/cancel {soft}`, with the token kept in `ServerState.running`.
    *   `GET /api/rules` and `PUT /api/rules`, covering never, ask and allow.
10. **Tests, inline in each file:**
    *   `tool_outside_scope_denied_at_call`
    *   `schemas_filtered_by_scope`
    *   `delegate_child_keeps_parent_scope`
    *   `delegate_output_is_untrusted`
    *   `delegate_call_is_audited`
    *   `audit_row_with_agent_verifies`
    *   `planted_private_memory_absent_from_other_agent`
    *   `dedupe_never_touches_other_pool`
    *   `supersede_stays_in_pool`
    *   `forget_refuses_other_pool_id`
    *   `unscoped_memory_fns_not_used_by_tools`
    *   `soft_cancel_finishes_running_tool`
    *   `soft_cancel_stops_before_next_model_call`
    *   `ask_rule_beats_yolo`
    *   `ask_rule_beats_allow_rule`
    *   `allow_rule_never_beats_taint`
    *   `allow_rule_still_skips_compound_shell`
    *   `voice_turn_in_auto_asks_for_shell`
    *   `default_agent_created_once`
    *   `migration_adds_only_nullable_columns`
    *   `agents_routes_need_token`
    *   `chat_agent_field_beats_text`
    *   `grant_does_not_carry_to_next_agent`

#### Unit A3t: API fixtures in `tests/` (Modify)

*   `tests/integration.rs` gains `api_agents_crud_and_import_review` and `api_rules_round_trip`.
*   The fixtures in `tests/fixtures/api/` are re-recorded, and `agents.json` and `rules.json` are added.

#### Unit A4: Mac app, agents and rules (Modify, in `macos/HeronMac/`)

1.  **`Sources/HeronCore/`:**
    *   `AgentsClient` and `RulesClient`;
    *   `CoreClient.chat` gains `agent` and `origin`;
    *   `cancel(soft:)`.
2.  **`Sources/HeronUI/Features/Agents/`** (new):
    *   `AgentsStore`, `AgentsView`, `AgentEditorSheet`, `AgentReviewSheet`, `AgentPicker`, `MentionCompleter`.
3.  **`Sources/HeronUI/Stores/AppModel.swift`:** `Destination.agents`, shown only while `[agents] enabled`.
4.  **`Sources/HeronUI/Views/RootView.swift`:** the destination route.
5.  **`ChatView.swift`:**
    *   the agent chip;
    *   `@` completion;
    *   avatar and name on each message.
6.  **`Sources/HeronUI/Views/NeverViews.swift` and `SettingsView.swift`:** the Never section becomes Rules, with three lists.
7.  **Tests,** in `Tests/HeronUITests/Features/AgentsTests.swift` and `RulesTests.swift`. Names use `test_lowerCamel`, so the grep checks match:
    *   `test_mentionCompletesCallNames`
    *   `test_pickerSetsAgentOnSend`
    *   `test_importOpensReviewSheet`
    *   `test_reviewShowsRequestedToolsUnticked`
    *   `test_messageShowsAgentName`
    *   `test_destinationHiddenWhenDisabled`
    *   `test_rulesSectionShowsThreeLists`
    *   `test_adminGroupShowsWarning`

#### Unit V1: Mac app, live voice (Create, in `macos/HeronMac/Sources/HeronUI/Features/LiveVoice/`)

1.  **`AudioSource.swift`:** a protocol, with two implementations:
    *   an `AVAudioEngine` source with voice processing on;
    *   a file-backed source, for tests and the gate.
2.  **`VoiceActivity.swift`:** an energy VAD with start and end hangovers, plus the barge-in detector (≥ 200 ms).
3.  **`Transcriber.swift`:** a protocol, with `SFOnDeviceTranscriber` (`requiresOnDeviceRecognition = true`) and a fake for tests.
4.  **`LiveVoiceSession.swift`:** the session logic.
    *   It drives `AvatarStateMachine` with the new `listening` and `hearing` states.
    *   Barge-in stops speech at once and sends a soft cancel.
    *   The end phrases are "goodbye" and "stop listening".
    *   It appends a line to `voice-timing.jsonl`.
5.  **`LiveVoiceView.swift`:**
    *   the pill, docked and full layouts, using `AvatarView`;
    *   the transcript;
    *   End and Mute.
6.  **`Sources/HeronUI/Avatar/AvatarStateMachine.swift`** (Modify): the `listening` and `hearing` cases, with their labels.
7.  **`Sources/HeronUI/Settings/VoiceSettingsView.swift`** (Create, or extend F7's settings rows):
    *   the voice model choice (PQ1);
    *   end-of-speech;
    *   p50 and p90, read from `voice-timing.jsonl`.
8.  **Tests,** in `Tests/HeronUITests/Features/LiveVoiceTests.swift`:
    *   `test_bargeInStopsSpeechWithin150ms` (fake clock, file source)
    *   `test_bargeInSendsSoftCancel`
    *   `test_endOfSpeechAfterSilence`
    *   `test_interruptedReplyStoresSpokenPart`
    *   `test_halfDuplexWhenNoEchoCancel`
    *   `test_goodbyeEndsSession`
    *   `test_timingLineWrittenPerTurn`
    *   `test_agentVoiceUsedForReply`
    *   `test_avatarShowsListeningAndHearing`

#### Unit V1p: usage strings in `packaging/macos/` (Modify)

*   The `Info.plist` that `build-app.sh` writes gains `NSMicrophoneUsageDescription` and `NSSpeechRecognitionUsageDescription`.

#### Unit V2: the voice gate in `eval/voice/` (Create)

1.  **`eval/voice/utterances/`**
    *   20 short WAV files of requests in the owner's style, made with `say -o`, so they carry no personal voice.
    *   `utterances.md`, which lists each file with the agent it should reach.
2.  **`eval/voice/run.sh`**
    *   Runs the app's file-backed source through the pipeline, using a test-target flag and the configured voice model.
    *   Then runs `report.py` on the `voice-timing.jsonl` it produced.
    *   Exits 0 when G5 holds, 1 when it doesn't, and 2 on setup errors.
    *   `--self-test` checks the script and the files without a model.
3.  **`eval/voice/report.py`:** reads a `voice-timing.jsonl`, computes p50 and p90 of `first_audio - speech_end`, checks them against G5, and writes `eval/voice/last-run.json`.
4.  **`eval/voice/genesis.yaml`:** follows `eval/genesis.yaml`, so that `genesis audit-evaluator` can test `report.py`'s verdict.
5.  **`eval/tests/test_voice_report.py`:** unit tests for the percentile maths, the G5 bounds and the exit codes.

#### Unit A7: docs (Modify and Create)

*   `docs/engineering/SECURITY_THREAT_MODEL.md` gains `## Named agents and live voice`, covering:
    *   scope and the memory handle;
    *   addressing;
    *   the voice ceiling;
    *   imported agents;
    *   ask rules.
*   It creates `docs/features/agents.md` and `docs/features/live_voice.md`, with the new `docs/features/` folder.

### Verification Plan

*   **Build:** `cargo build --locked --features desktop --bins` and `swift build --package-path macos/HeronMac`.
*   **Unit tests:** `cargo test --locked` and `swift test --package-path macos/HeronMac`.
    *   Run them outside the Bash sandbox. About 27 cargo tests need local ports and Seatbelt.
*   **Static analysis:** `cargo clippy --locked --all-targets -- -D warnings`.
*   **Test quality, testteeth:** `testteeth run --base main --fail-under 70 -q` on the branch.
    *   Every surviving mutant in `src/agents/`, the `decide` changes and the scoped memory handle is killed or justified in the PR.
*   **Evaluator audit, genesis:** `genesis audit-evaluator eval/voice/genesis.yaml --suite code-py` must exit 0 (TRUSTED) before G-V's verdict counts.
    *   `report.py` is P1's only grader. No LLM judge is added.
*   **Isolation proof:** G3 and G4 are proved by these tests:
    *   `tool_outside_scope_denied_at_call`;
    *   `planted_private_memory_absent_from_other_agent`;
    *   `dedupe_never_touches_other_pool`;
    *   `forget_refuses_other_pool_id`.
*   **Gate G-V, run by the owner:** `bash eval/voice/run.sh` on the owner's Mac with the chosen voice model.
    *   Its output goes in the PR.
    *   It can't run in CI, which has no audio hardware and no model.
*   **End-to-end, by hand:** CUJs P-1 to P-4, recorded in the PR.

## Work units

Phase 1 only. In the check column, `\|` stands for a shell `|`. Units whose scopes share a prefix run in the order "depends on" gives.

| id | what to build | scope | depends on | check | model role |
|---|---|---|---|---|---|
| A1 | `src/agents/`: profile, groups, scope, addressing, share (items A1.1–6) | `src/agents/**` | none | `cargo test --locked --lib agents:: && cargo clippy --locked --all-targets -- -D warnings && bash eval/require-tests.sh --lib -- scope_never_widens_past_global voice_surface_caps_at_ask ceiling_none_keeps_global_mode agent_rules_only_never_or_ask telegram_origin_is_default_agent_only address_needs_leading_at_in_chat address_ignores_mid_text_mention address_voice_needs_comma_or_hey explicit_agent_beats_text names_unique_ignoring_case_across_call_names reserved_names_refused import_refuses_unknown_keys import_starts_in_review_with_no_tools export_has_no_memories_or_id groups_all_expands_to_registry` | hardest tasks: opus high |
| A3 | Wiring: store step and scoped memory queries, memory handle, `forget`, `decide` and ask rules, config, agent loop, delegate, soft cancel, voice provider, routes (items A3.1–10) | `src/**` | A1 | `cargo test --locked --lib --bins && cargo clippy --locked --lib --bins -- -D warnings && bash eval/require-tests.sh --lib -- tool_outside_scope_denied_at_call schemas_filtered_by_scope delegate_child_keeps_parent_scope delegate_output_is_untrusted delegate_call_is_audited audit_row_with_agent_verifies planted_private_memory_absent_from_other_agent dedupe_never_touches_other_pool supersede_stays_in_pool forget_refuses_other_pool_id unscoped_memory_fns_not_used_by_tools soft_cancel_finishes_running_tool soft_cancel_stops_before_next_model_call ask_rule_beats_yolo ask_rule_beats_allow_rule allow_rule_never_beats_taint allow_rule_still_skips_compound_shell voice_turn_in_auto_asks_for_shell default_agent_created_once migration_adds_only_nullable_columns agents_routes_need_token chat_agent_field_beats_text grant_does_not_carry_to_next_agent` | hardest tasks: opus high |
| A3t | API fixtures and integration tests (unit A3t) | `tests/**` | A3 | `cargo test --locked --test integration && bash eval/require-tests.sh --test integration -- api_agents_crud_and_import_review api_rules_round_trip && test -s tests/fixtures/api/agents.json && test -s tests/fixtures/api/rules.json` | feature, refactoring: sonnet high |
| A4 | Mac app: Agents screen, editor, review sheet, picker, `@` completion, Rules section (items A4.1–7) | `macos/**` | A3t | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && test "$(swift test list --package-path macos/HeronMac \| grep -cE 'AgentsTests\|RulesTests')" -eq 8` | feature, refactoring: sonnet high |
| V1 | Mac app: live voice, avatar states, voice settings (items V1.1–8) | `macos/**` | A4 | `swift build --package-path macos/HeronMac && swift test --package-path macos/HeronMac && test "$(swift test list --package-path macos/HeronMac \| grep -c 'LiveVoiceTests')" -eq 9` | hardest tasks: opus high |
| V1p | Info.plist usage strings (unit V1p) | `packaging/macos/**` | none | `bash -n packaging/macos/build-app.sh && grep -rq NSMicrophoneUsageDescription packaging/macos && grep -rq NSSpeechRecognitionUsageDescription packaging/macos` | feature, refactoring: sonnet high |
| V2 | Voice gate: utterances, runner, report, genesis config, tests (unit V2) | `eval/**` | none | `python3 -I -m unittest discover -s eval/tests -t eval -p 'test_voice_report.py' && test "$(ls eval/voice/utterances/*.wav \| wc -l)" -eq 20 && bash eval/voice/run.sh --self-test && genesis audit-evaluator eval/voice/genesis.yaml --suite code-py` | feature, refactoring: sonnet high |
| A7 | Docs (unit A7) | `docs/**` | A3 | `grep -qx '## Named agents and live voice' docs/engineering/SECURITY_THREAT_MODEL.md && test -s docs/features/agents.md && test -s docs/features/live_voice.md` | judgment and prose: opus high |
| verify-p1 | Suite, required tests, test quality, evaluator audit | `scripts/gates/parity-p1` (check only) | A1, A3, A3t, A4, V1, V1p, V2, A7 | `cargo test --locked && cargo clippy --locked --all-targets -- -D warnings && swift test --package-path macos/HeronMac && genesis audit-evaluator eval/voice/genesis.yaml --suite code-py && testteeth run --base main --fail-under 70 -q` | feature, refactoring: sonnet high |

**Run notes:**

*   **Sandboxing.** The skein supervisor runs every check that calls `cargo`, `swift`, `testteeth` or `genesis` outside the sandbox.
    *   Inside a node's sandbox, `swift build` fails, and about 27 cargo tests fail for sandbox reasons.
    *   Expect a compile-fix round after A4 and V1.
*   **Steps run by hand:** gate G-V (`bash eval/voice/run.sh` with the owner's voice model) and CUJs P-1 to P-4. Their output goes in the PR.
*   **Timeouts:** `verify-p1` runs with `--verify-timeout 3600`.

**Later phases, not approved by this doc.** Each gets a design delta with its own units first.

| Phase | Planned units (scope) |
|---|---|
| P2 | channel trait and pairing (`src/channel/**`); Slack adapter (`src/channel/slack/**`); Messages reader (`macos/**`); deliverables (`src/make/**`); goals and delegating to a named agent (`src/goals/**`); draft connectors (`src/ext/**`, after Harness2 1b); recipient and domain rules (`src/permissions.rs`) |
| P3 | relay (`relay/**`); WhatsApp and Teams adapters (`src/channel/**`); meetings (`macos/**`, `src/meetings/**`); calls (`src/calls/**`); pack (`packs/**`) |
| P4 | payment effect and filler (`src/pay/**`); mobile (`ios/**`) |

--------------------------------------------------------------------------------

*Drafted with EGM*
