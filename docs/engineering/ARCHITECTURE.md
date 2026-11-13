# Architecture

One Rust crate (`heron`), one process, one SQLite file. Binaries: `heron` (command line, local
server, daemon, MCP server) and `heron-desktop` (a window around the local server).

## Modules

| Module | Responsibility |
|---|---|
| `config` | Settings file, environment overrides, secrets (OS credential store or owner-only file) |
| `llm` | OpenAI-compatible streaming client, retries, fallback models, demo and scripted models, cassette record/replay, usage metering |
| `agent` | The conversation loop: prompt assembly, tool calls, approvals, sub-agents, compaction |
| `permissions` | `Effect` classification helpers and `Policy::decide_in`, the single decision point |
| `tools/*`, `browser`, `mcp` | The tools. Each declares the `Effect` of a concrete call from its arguments |
| `sandbox` | Wraps commands in bubblewrap (Linux) or Seatbelt (macOS) |
| `store` | SQLite: conversations, memories, tasks, schedules, runs, approvals, audit log, usage, key-value |
| `memory`, `embed`, `eval` | Hybrid retrieval, optional neural embeddings, the retrieval test |
| `schedule`, `workflow`, `app` | Time and event triggers, fixed-step automations, background loops |
| `usage` | Token ledger, limits, projection, effort level, model routing |
| `sources`, `mail`, `ambient` | Onboarding sources, read-only IMAP, folder awareness |
| `server`, `assets/index.html` | Local HTTP API (loopback, per-launch token) and the single-file interface |
| `telegram` | Phone gateway |

## How a tool call is decided

```
model output ──► Agent::execute_call ─┐
automation step ─► Agent::call_tool ──┤
approval given later ─► App::run_parked ─┤──► tool.risk(args, ctx) = Effect
another app ─► mcp::serve ────────────┘          │
                                                 ▼
                              Policy::decide_in(tool, summary, effect, tainted)
                                   │ Allow          │ Ask                 │ Deny
                                   ▼                ▼                     ▼
                               tool.call      Approver (app, terminal,  refusal text
                                              phone, or "park it")      to the model
```

`tainted` is a per-conversation flag set the first time a tool returns outside content. It is
stored with the conversation and shared with sub-agents.

## Data

Everything is in `~/.heron` (or `HERON_HOME`): `config.toml`, `heron.db`, `secrets.json` (only
where there is no OS credential store), `skills/`, `browser/` (the driven browser's own profile),
`models/` (optional neural model). The folder must be private to the user or Heron refuses to start.

## Trust boundaries

- **User ↔ Heron:** the app, the terminal, the paired Telegram chat.
- **Heron ↔ model provider:** everything in the conversation is sent to the chosen provider.
- **Heron ↔ outside content:** web pages, mail, documents, calendar entries, MCP outputs. Wrapped
  as untrusted, and their arrival taints the conversation.
- **Heron ↔ other local programs:** the local API (token) and the MCP server (same policy as the agent).
