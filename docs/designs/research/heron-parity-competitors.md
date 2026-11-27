# Competitor features for the Heron parity design (2026-11-23)

Purpose: the facts that `docs/designs/heron-parity.md` rests on. This note relays research done by the Heron main session on 2026-11-23, plus the owner's own report of using Muse daily. I did not re-fetch the pages, so each fact names its source as relayed. Where only the owner's account supports a fact, the line says so.

Earlier research that this note builds on: [muse-ux.md](muse-ux.md) (Muse UI and trust patterns, 2026-11) and [personal-agents-2026-11.md](personal-agents-2026-11.md).

## The four products

| Product | Vendor |
|---|---|
| Muse | Meta |
| Grok Bot (1.0) | xAI |
| Dots | OpenAI |
| Duet | Anthropic (Claude) |

## Facts by feature

### 1. Your own named agents

*   **Muse:** users name their own agent. Meta's Muse page (https://ai.meta.com/muse/, read 2026-11-23 by the main session) shows creators' agents named "Pumpkin", "Lux", "AunTEY" and "Mango". Users chat with it by message or by voice mode, in the Muse app on mobile and desktop, and directly in WhatsApp.
*   **Muse, several agents:** the owner reports using "call on your own agents" in Muse every day: several named agents, each called by name. The public sources found only one agent per user, so this rests on the owner's use.
*   **Grok Bot 1.0:** users create several Bots, share them, and bring them into Slack, Teams and meetings.

### 2. Live voice

*   Muse Voice: about 870 ms response, with an avatar. Muse can reply while the user is still talking and can be interrupted ([muse-ux.md](muse-ux.md), source 8).
*   Grok Voice 3.0: under 800 ms.
*   OpenAI GPT-Live: live, interruptible voice.

### 3. Outbound phone calls

*   Muse and Grok Bot phone businesses for the user, with a live transcript and "Take over" and "End call" controls.
*   Wikipedia, citing 404 Media, reported human call-centre workers behind some Muse calls in testing ([muse-ux.md](muse-ux.md), pitfall 12).

### 4. Messaging channels

*   Dots, Grok and Muse reach the user over iMessage or Messages, WhatsApp, Slack and Teams, with one shared memory across them.

### 5. Meetings

*   Grok Bot and Claude Meetings join Google Meet, Zoom and Teams, take notes, and answer in the meeting chat.

### 6. Approved checkout

*   Dots Wallet, Duet (through Link or Shop Pay) and Grok: a one-time or substitute card, and every purchase approved by the user with Face ID.
*   Muse: single-use card numbers tied to one merchant and amount, with an approval prompt at every checkout ([muse-ux.md](muse-ux.md), Trust section).

### 7. Action rules

*   Dots Custom Rules: per-action allow, require approval, or block.

### 8. Deliverables

*   Duet's Make tab and ChatGPT Space produce editable documents, slides and dashboards.
*   Muse Spaces: mini apps built from a plain-language description ([muse-ux.md](muse-ux.md), Spaces section).

### 9. Mobile app

*   All four products have one.

### 10. Muse page details (https://ai.meta.com/muse/, read 2026-11-23)

*   **Goals with action plans:** give it a goal, and it builds a plan, tracks progress, takes tasks off your plate on its own, and comes back with ideas, acting under your direction.
*   **Builds its own tools:** when a task needs a tool that doesn't exist, the agent writes one.
*   **Shared computer the user can take over:** a persistent isolated VM with a full browser that the user and agent share, so the user can step in at any point.
*   **Small-business pack:** a bundle of skills and connectors.
*   **Connectors:** email, calendar and Instagram.
*   **Approval:** it checks before important actions such as sending email or buying.
