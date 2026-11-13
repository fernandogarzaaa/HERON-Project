# Heron next to five personal AI agents

Compared: Hark Pro (Hark), Muse (Meta), Grok Bot (xAI), Dots (OpenAI) and Claude Duet (Anthropic).
Written 13 November 2026.

**This is a paper comparison from public sources, not a measured benchmark.** None of the five
products was run. Their columns come from launch posts, documentation and reviews (listed at the
end); some official pages could not be read, so a few cells rest on third-party write-ups and are
marked "(reported)". Heron's column comes from its own README and changelog, including the
"Known limits" section. Nothing here says one product is better overall; it reports dimensions.

## At a glance

| Area | Heron | Hark Pro | Muse | Grok Bot | Dots | Claude Duet |
|---|---|---|---|---|---|---|
| Status and price | v0.1.0 beta, open source, free; you pay your model provider; no real users yet | Launched; free, $20 and $100 tiers | Launched (US adults); free, $20, $100 | Beta; needs a paid plan (about $200/month reported, sources conflict) | Launched; ChatGPT Pro ($100+) or Business Premium; not EEA/UK/CH | Waitlist preview for US Pro/Max plans |
| Platforms | macOS, Windows, Linux, command line | Web, iOS, Android | Web, iOS, Android, WhatsApp | macOS, Windows, iOS (others disputed) | ChatGPT desktop, web, mobile; Slack, Teams | iOS, Android, claude.ai |
| Where it runs | On your computer, only while the app or daemon is running | Cloud machine per task | Dedicated cloud machine per user | One cloud machine per account | Cloud computer per dot; optional local access | Cloud machine |
| Models | Any OpenAI-compatible model, including local ones | Its own computer-use model | Muse Spark 1.3 | Grok 4.5 family, no choice | GPT-6 Astra | Opus 5.5, no choice |
| Where your data lives | One local database file; no account, no telemetry | Vendor cloud; says no training, no ads | Vendor cloud; Meta can access machine data; training opt-out | Vendor cloud; storage mandatory | Vendor cloud; training depends on a setting | Vendor cloud |
| Memory | Local, versioned, unconfirmed-fact queue, forgettable | Inspectable, editable profile | Editable memory files | Per bot; editing not described | Cannot view or delete individual memories (reported) | Carries over Claude memory |
| Acts without being asked | Four-level dial, off by default, daily token cap | Suggested-action feed | Yes, with reduce/increase/disable | Yes | Read-only "proactive research" | Morning brief, checks every few hours |
| Awareness of what you are doing | Partial: local file names, git branch, hours | Connected accounts only | Connected accounts only | Connected accounts; "Teach a task" recording | Connected apps; optional local computer | Connected accounts |
| Connected sources | Partial: notes, read-only calendar link, read-only IMAP (no Outlook), MCP | Google, Outlook, cards, files | Built-in connectors, custom keys, browser fallback | Plugins, browser sign-in | "Over 4,000 apps" | 6,000+; no Microsoft 365 |
| Browser and computer control | Partial: local browser, text only, no vision; untested on the public web | Yes, live window with handoff | Yes, watch and take over | Yes, browser and terminal | Yes, "Take over" | Yes, but you cannot watch or take over |
| Schedules and triggers | Schedules, natural language, file and email triggers, fixed-step automations | Scheduled tasks (reported) | Scheduled and event-driven | Routines on schedule or event | Scheduled tasks | Brief and periodic checks |
| Voice | No | Input only (reported) | Real-time (reported) | Voice chat | You can call it | Voice messages |
| Mobile | Partial: Telegram bot, tested only against a stand-in | Yes | Yes | iOS | Yes | Yes |
| Several agents | Sub-agents only | Unknown | Parallel sub-agents (reported) | Group chats of 2–6 bots | One dot; teams "over time" | Unknown |
| Extensibility | MCP client and server, Markdown skills | MCP mentioned | Skills | Skills, plugins | Plugins, local skills | Skills, connectors |
| Approvals | Four modes, risk classes, rules, parked "Waiting for you" | Approve/decline cards | Separate reviewer, five grant scopes, dialog outside the chat | Auto review, cards, "Ask first" rules | Auto review, four rule behaviours | Before send, pay, sign, delete |
| Prompt-injection defence | Partial: a fixed rule per conversation; OS sandbox on Linux and macOS only, network open | Unknown | Tagging, classifiers, tainted egress, container | Outside text is not permission | Stated as not eliminating the risk | Classifier, penetration test, bounty (claimed) |
| Undo and audit | File checkpoints; tamper-evident log | Unknown | Activity log, machine restore; no undo | Run history; audit view "coming" | Activity view; no export | Unknown |
| Credentials and payments | Keys in the OS credential store; no payments | Encrypted vault | Surrogate tokens, single-use card | Vault, secret card | Private sign-in form | Never sees passwords; virtual card |
| Usage and cost control | Usage page: used, left, projected; own limits; economy mode; routes simple requests to a cheaper model | Plan tiers | Plan tiers | Routines consume usage even when idle (reported) | Plan limits | Background checks count against plan limits |

## Where Heron is behind

- **Always on.** All five run on a cloud machine and keep working when your laptop is closed.
  Heron works only while its app or daemon is running.
- **Reach.** Duet lists 6,000+ connectors and Dots 4,000+. Heron reads mail over IMAP (no
  Outlook) and a calendar link, and cannot send mail or write to a calendar.
- **Browser and computer use.** The others drive a full browser on a cloud computer; Hark, Muse
  and Dots show it live and Muse and Dots let you take over. Heron reads pages as text, lists at
  most 80 controls and has not been tried against real sign-in walls.
- **Voice and mobile.** Heron has neither; its Telegram gateway has never touched the real service.
- **Credentials and payments.** Vaults, surrogate tokens and single-use cards are standard
  elsewhere. Heron has none of that.
- **Depth of injection defence.** Muse layers classifiers, per-process tagging and network-level
  checks. Heron's rule is per conversation, its sandbox leaves the network open, and there is no
  sandbox on Windows.
- **Maturity.** Heron has no real users, has been run against two free models on a handful of
  tasks, and ships unsigned installers.

## Roughly level (in design, not in proof)

- Schedules and event triggers.
- Controls over proactivity.
- The principle that outside content is not permission to act.
- Extensibility through MCP and skills.

## Where Heron is ahead on a specific dimension

- **Data location and model choice.** It is the only one that runs locally, needs no account and
  works with any model, including one on your own machine.
- **Memory you can audit.** Version history, an unconfirmed queue, and forgetting a source when
  it is disconnected.
- **Audit and undo.** A tamper-evident log and file undo.
- **Cost control.** Automations that use no tokens unless a step needs the model, your own
  limits with a projection, and economy behaviour as the allowance runs down.

## Ideas worth taking, in order

| # | Idea | From | Size | Status in Heron |
|---|---|---|---|---|
| 1 | Grant scopes on the approval card (once, this conversation, this task, for a time, always) and a page to review and revoke them | Muse, Grok Bot | Medium | Not yet |
| 2 | One Activity view: in progress, scheduled, completed; answer waiting items in place | Dots, Muse | Medium | Not yet |
| 3 | Morning brief pushed to desktop or phone, only when something needs attention | Duet | Small | Partly (Today summary exists; not pushed) |
| 4 | Suggestion cards with "Do it", "Not now", "Never suggest this" | Hark | Small–medium | Partly (no "never") |
| 5 | Live view of the browser with "Take over" and "Return control", and a sign-in handoff | Hark, Dots, Muse | Medium | Not yet |
| 6 | Four rule behaviours: without asking, when you say so, ask before, hand off to you | Dots | Medium | Not yet |
| 7 | Teach a task by recording it | Grok Bot | Large | Not yet |
| 8 | Routine menu: pause, resume, test, edit; stale approvals expire | Grok Bot | Small | Partly (run now, cancel) |
| 9 | Usage meter and limits | Duet, Muse | Small | **Done** (Usage page, limits, projection, economy mode, routing) |
| 10 | Pin an automation's latest result as a tile | Hark | Medium | Not yet |

Needs infrastructure Heron does not have: always-on cloud execution, OAuth connectors and
writing to mail or calendars, a native mobile app with push, real-time voice, virtual payment
cards, classifier ensembles and a bug bounty.

## Interface patterns worth copying

1. Approval shown by the app itself, outside the message stream, so injected content cannot imitate it (Muse).
2. Grant-scope chips on the approval card, with "once" preselected (Muse, Grok Bot).
3. A one-sentence status line saying what the agent is doing, visible everywhere (Muse).
4. Tabs for computers, scheduled and activity, with inline answers for waiting items (Dots).
5. A small live window while browsing, with "Take over", "Return control" and "Done" (Hark, Dots).
6. One persistent thread plus side chats, with tasks queued mid-run (Muse, Duet).
7. A masked secret field outside the transcript (Grok Bot, Dots).
8. A menu on each routine: pause, resume, test, edit, delete (Grok Bot).

## Sources

- Hark Pro: https://techcrunch.com/2026/10/06/hark-releases-an-ai-personal-assistant-with-a-focus-on-privacy/ · https://www.layer3labs.io/guides/hark-pro-explained
- Muse: https://techcrunch.com/2026/09/08/meta-debuts-its-muse-ai-agent-will-consumers-trust-it/ · https://muse.ai/blog/security-and-safety-for-ai-agents-our-approach-with-muse · https://www.eesel.ai/blog/meta-muse-agent · https://www.testingcatalog.com/meta-introduces-muse-as-a-proactive-personal-agent.md
- Grok Bot: https://x.ai/news/introducing-grok-bot · https://www.eesel.ai/blog/grok-bot-review · https://flaviocopes.com/grok-bot.md
- Dots: https://techcrunch.com/2026/09/29/openai-launches-dots-its-bubbly-agentic-avatar/ · https://www.eesel.ai/blog/openai-dots · https://flaviocopes.com/openai-dots/
- Claude Duet: https://claude.com/blog/claude-duet · https://www.vktr.com/ai-platforms/claude-duet-anthropic-previews-an-ai-agent-that-messages-you-first/ · https://aibusiness.com/agentic-ai/anthropic-rolls-out-claude-duet-enterprises-aren-t-ready
- Could not be read: OpenAI's Dots launch post (403), two xAI documentation pages (404), a Meta help page (truncated)
- Interface references used for the UI pass: https://github.com/vercel-labs/web-interface-guidelines · https://github.com/ancoleman/ai-design-components (building-ai-chat)
