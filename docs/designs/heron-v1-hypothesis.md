# Heron v1 improvement loop: anchored hypothesis

Date: 2026-11-18. Status: input for the EGM brainstorm Goldfish review. Not a design doc.

## Hypothesis

Heron becomes a production-ready personal agent for its owner and a few friends who accept the risk, on macOS only for now. It gets a native SwiftUI Mac app on top of the existing Rust core. The look takes inspiration from Meta's Muse personal agent (named agent, custom avatar, themes, animation) without copying it. The existing Slint app stays for now, with no new UI work.

Models connect by driving each user's logged-in official CLIs (Claude Code, Codex, and others such as Cursor, Grok Build, OpenCode). This follows the "control plane" approach of T3 Code and Hermes Agent's Claude-subscription integration: Heron does not resell tokens and is not a model provider. It detects installed, authenticated CLIs and inherits their session, whether a subscription login or an API key. Each CLI runs with its built-in tools switched off, and Heron serves its own tools to the CLI over MCP. Heron's permission checks, untrusted-content marking ("taint") and hash-chained audit log therefore stay in charge of every action.

## Order of work

1. Fix the two shared root causes from the v1.0 qualification:
   - Taint enters a conversation only through tool outputs marked untrusted, so other paths (for example text placed in prompts) bypass it.
   - The secret-read gate is a denylist of names.
2. New features. The owner wants more features, not fewer, and nothing cut.
3. The SwiftUI Mac app.
4. The rest of the hardening: the remaining P0 and P1 findings from the qualification.
5. A signed, notarized production build.

## Provisional success measure (owner is unsure; the review should sharpen it)

- A qualification rerun gives a READY verdict, with 0 P0 and 0 P1.
- EVE task success of 85% or more on about 10 core tasks.
- The owner uses Heron daily for two weeks instead of their current tool.

## Context and constraints

- Rust core (Rust 1.99, SQLite via rusqlite, a local axum HTTP API, tokio). The current UI is Slint 1.18. See `README.md` for the feature list.
- Local-first: no cloud deployment, no telemetry, one SQLite file in `~/.heron`.
- Qualification reports: on branch `qualification/v1.0-rc1`, under `qualification/reports/`, starting with `HERON-V1-RELEASE-QUALIFICATION.md` and `02-architecture-audit.md`.
- Owner decisions already made: phone (Telegram) turns inherit the desktop permission mode, as a documented accepted risk (C35-8). Calendar text stays out of system prompts and is looked up on demand through a path that taints the conversation (O26-1).
- Autonomy: each loop iteration opens a PR and merges it to main once CI, Codex review and mutation testing (testteeth) pass. Security and product-policy calls go to the owner.
- Usability is measured with EVE (experience-validation-engine) through `eve chat` against Heron's HTTP chat API.
