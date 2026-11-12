# Changelog

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
- Optional neural memory search (build feature `neural`): finds facts by meaning, fused with the typo-tolerant built-in method. In the built-in test the right fact is in the top three for 33 of 34 questions (30 of 34 without it), and ranked first for 31 (28 without it).
- Memory takes a fixed share of the prompt however large it grows; sub-agents get a smaller step allowance; a provider's Retry-After is respected; model calls can be recorded and replayed for tests.

**Known limits of this beta**

- Not yet tested with real users. It has been exercised with automated tests and stand-ins for model, mail, calendar and Telegram services.
- The apps are not code-signed, so macOS and Windows show a warning on first launch.
- No command sandbox on Windows; no browser automation, voice, mobile app, email sending or calendar writing.
- Outlook.com and Microsoft 365 mail cannot be connected.

See the README for the full list.
