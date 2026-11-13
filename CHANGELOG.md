# Changelog

## Unreleased

Found by the first runs against a real AI model (a free reasoning model through OpenRouter):

- Models that think before answering no longer come back empty: every request leaves room for the thinking as well as the answer, and a reply that is all thinking is reported as such instead of being retried.
- When nobody is present to approve a change, Heron now tells the model the real reason (outside content is in the conversation, or the action is risky), so the advice it passes on is right. It used to suggest switching mode, which would not have helped.
- Reminders understand days of the week: "weekdays 9:00", "every weekday at 9am", "every Friday at 4pm", "weekends 10:00".
- Setting a reminder no longer also saves a memory saying that a reminder was set.
- Releases no longer include the interface file as a download.

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
