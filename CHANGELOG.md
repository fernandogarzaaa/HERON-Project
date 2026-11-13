# Changelog

## Unreleased

Permission system (behaviour changes):

- Every tool call is now classified as one of ten effects (reads, changes to Heron's own notes, file changes, secret reads, running programs, reading the web, sending something out, changing something elsewhere, destructive, forbidden) and decided in one place.
- **Read-only mode is read-only for everything.** It used to stop file changes and commands but still let Heron save memories, add tasks and schedule jobs. It now refuses all of those.
- **"Unrestricted" no longer switches off the outside-content rule.** After a web page, email or connected app's output is in a conversation, changes and anything Heron sends out need your approval in every mode.
- **Web searches are no longer exempt.** After outside content, a search asks too. "Allow searches for this conversation" lets research continue without a prompt per query.
- **Only addresses you typed count as yours.** An address that arrived inside a page or a tool's result is asked about before it is opened. Expect more prompts in research tasks than before.
- Reading a file that may hold secrets always asks, even in unrestricted mode.
- Pressing buttons and typing in the browser, and unknown connected-app tools, ask in auto mode as well as ask mode.
- Other apps calling Heron over MCP go through the same rules; they used to bypass them.
- Approving a waiting request from two places at once can no longer carry it out twice, and a request interrupted by a restart is shown as "outcome unknown" instead of being repeated.
- Engineering documents (threat model, remediation matrix, decisions, qualification) are in `docs/engineering/`.

Durability and isolation:

- **Undo will not overwrite your edits.** If a file has been changed, replaced or removed since Heron edited it, undo says so and touches nothing. You can restore anyway; the version that was there is saved beside the file first.
- File changes are written in one step, so a crash or a full disk cannot leave half a file.
- A scheduled job can no longer be started twice for the same time, and a job that was running when Heron stopped is reported in the inbox as "not known whether it finished" instead of disappearing. It is not repeated automatically.
- **Commands no longer see your whole environment.** They get paths, language settings and toolchain locations; tokens and cloud keys are withheld. If a command needs one, add its name to `shell_env` under `[agent]`.

Found by the first runs against a real AI model (a free reasoning model through OpenRouter):

- Models that think before answering no longer come back empty: every request leaves room for the thinking as well as the answer, and a reply that is all thinking is reported as such instead of being retried.
- When nobody is present to approve a change, Heron now tells the model the real reason (outside content is in the conversation, or the action is risky), so the advice it passes on is right. It used to suggest switching mode, which would not have helped.
- Reminders understand days of the week: "weekdays 9:00", "every weekday at 9am", "every Friday at 4pm", "weekends 10:00".
- Setting a reminder no longer also saves a memory saying that a reminder was set.
- Releases no longer include the interface file as a download.

Token awareness:

- A Usage page (and `heron usage`): tokens used today and this month, what is left of your own daily and monthly limits, a projection for the month, a 30-day chart, a breakdown by model, and remaining OpenRouter credit where that applies. Every model call is recorded; where a provider reports no counts they are estimated.
- Economy behaviour as the allowance runs down: fewer steps and shorter answers at a quarter left; the cheapest model and no self-started work near the end. Heron always still answers you.
- Model routing: with a cheaper model configured, short simple requests go to it, and a request that turns out to need real work moves to the main model after two steps. The conversation shows which model is answering and why.
- Approval prompts have a middle choice, "Allow for this conversation", between once and always. It is never offered for risky actions or after outside content has been read, and it ends with the conversation.
- A written comparison with Hark Pro, Muse, Grok Bot, Dots and Claude Duet (docs/COMPARISON.md), from public sources.

Interface, checked against the Web Interface Guidelines and a review of chat-interface practice:

- While Heron works you can see it: a "thinking" indicator between steps, a cursor on the reply as it is written, and no stray symbols from half-arrived formatting.
- The page no longer pulls you to the bottom while you are reading earlier text; a "Newest message" button takes you back.
- Finished replies have a Copy button. Reminders read "Every weekday at 9:00 AM" instead of a cron pattern. The folder line shows the folder's name, not its whole location.
- Smaller things: new messages ease in, buttons respond to a press, text wraps without orphans, the composer clears the phone's home bar, the browser's toolbar colour matches the page, and the phone layout no longer truncates the title.

## 0.1.0 — first beta

The first public beta of Heron, a personal AI agent that runs on your own computer.

**What is in it**

- Desktop app for macOS, Windows and Linux, plus a command-line interface.
- Works with any OpenAI-compatible model service (OpenRouter, OpenAI) or a model running locally (Ollama, LM Studio). A built-in demo mode needs no key.
- Onboarding that gets to know you: tell Heron about yourself, let it look around your computer, and connect notes, a calendar (ICS link) and email (read-only).
- Long-term memory that keeps history when facts change, and holds facts picked up from outside sources as unconfirmed until you confirm them.
- Background awareness of what you are working on, and a proactivity dial: only when asked, suggest, prepare ahead, or take over within limits.
- Tasks, reminders and recurring jobs with per-job permissions.
- Use it from your phone through Telegram, paired to a single chat.
- Safety: four permission modes, approval for anything that changes state once outside content is in a conversation, undo for file changes, an activity log, keys in the operating system's credential store, and a daily cap on self-started work.
- Extensible through MCP connectors and skills, including skills Heron writes itself with your approval.
- An editable personality, changed in Settings or just by asking.
- "Waiting for you": steps that needed approval while you were away are kept, not lost, and can be run later with one click.
- Jobs that react to events (a file changing, an email arriving) as well as times; watches that speak up only when needed.
- A Today summary and one-click day prep; per-ability permission switches; a pause-everything switch.
- A tamper-evident, append-only activity log; picking up a reply that was cut short.
- Commands run inside an operating-system sandbox on Linux (bubblewrap) and macOS (Seatbelt).
- Automations: recurring jobs made of fixed steps (tool, ask, agent, if, for_each, set, notify, stop) that run without the AI model unless a step needs it, with templates, per-step retry, a step-by-step run record and "Run now". Heron builds them on request and you approve the outline.
- Optional neural memory search (build feature `neural`): finds facts by meaning, fused with the typo-tolerant built-in method. In the built-in test the right fact is in the top three for 33 of 34 questions (30 of 34 without it), and ranked first for 31 (28 without it).
- Memory takes a fixed share of the prompt however large it grows; sub-agents get a smaller step allowance; a provider's Retry-After is respected; model calls can be recorded and replayed for tests.
- Browser control: Heron can open pages in a real browser on your computer (Chrome, Edge, Chromium or Brave), follow links, fill in forms and press buttons, using its own separate profile. Typing and pressing buttons ask first.
- Hardening from code review before release:
  - After outside content is read, fetching any web address that was not already in the conversation needs approval (not only addresses with a query), as do connected apps' tools and scheduling a job; this now applies in read-only mode too. Facts saved in such a conversation are unconfirmed, and automatic learning uses only your own words.
  - Web requests refuse internal addresses at the moment of connecting, which closes DNS rebinding and redirects to internal names; NAT64, 6to4 and other IPv4-in-IPv6 addresses are recognised.
  - "Read-only" commands are checked by their arguments as well as their name (`sort -o`, `rg --pre`, `git branch NAME`, `/proc/.../environ`, variables that may hold secrets), and a standing "always allow" for one command no longer covers anything chained after it. "Always" is not offered for risky actions.
  - Command output is capped while it is read, so a command that prints without end cannot exhaust memory.
  - Telegram pairing uses a longer code, locks after five wrong guesses until you look at the new code in the app, and `/stop` halts a reply that is waiting on a yes or no. Reconnecting or replacing the bot token restarts the single gateway.
  - The fallback secrets file is written atomically, never overwritten when damaged, and saves are serialised; a failed removal from the credential store is reported.
  - The model client reads non-streamed replies, bounds line length and tool-call positions, rejects replies cut off before their end, and no longer switches to a fallback model after part of a reply has been shown.
  - Work Heron starts on its own re-checks the daily allowance before each piece, stops when it is used up, and takes fewer steps.
  - Calendar repeats keep their local time across clock changes and honour COUNT and EXDATE; cron times in the repeated autumn hour are always in the future.
  - Smaller fixes: MCP tool fingerprints use SHA-256; `heron chat --continue` skips empty conversations; links open in the browser on Windows; installers replace running files safely and keep the Windows uninstaller.

**Known limits of this beta**

- Not yet tested with real users. It was built and tested with automated tests and stand-ins for model, mail, calendar and Telegram services; the first runs against a real model came after this release (see Unreleased).
- The apps are not code-signed, so macOS and Windows show a warning on first launch.
- No command sandbox on Windows; no voice, mobile app, email sending or calendar writing. Browser control reads pages as text, without screenshots.
- Outlook.com and Microsoft 365 mail cannot be connected.

See the README for the full list.
