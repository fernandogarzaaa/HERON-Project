# Personal AI agents, late 2026: research for the Heron v1 feature list

Collected on 2026-11-18 for [heron-v1-features.md](../heron-v1-features.md). It adds to [COMPARISON.md](../../COMPARISON.md) (13 November 2026), which covers Hark Pro, Muse, Grok Bot, Dots and Claude Duet.

**How much to trust this.** It comes from web search result summaries only. No official page was fetched, and no product was run. Most links are third-party write-ups, so each row is weak evidence. Anything marked UNVERIFIED could not be confirmed at all. Use this to choose features, never as a claim in user-facing text.

## Local-first and self-hosted

| Product | What a Mac user would notice | Channels | Safety, as reported |
|---|---|---|---|
| OpenClaw (formerly Clawdbot, Moltbot; MIT) | A local gateway driven from 20+ messaging apps, including iMessage. Skills are `SKILL.md` folders, with the ClawHub registry. | WhatsApp, Telegram, Slack, Signal, iMessage and more | CVE-2026-25253. Koi Security found 341 malicious skills among 2,857 audited on ClawHub. Mitigations: `allowedPaths`, DM pairing, Docker, approvals. |
| goose (Rust, Apache 2.0, Linux Foundation AAIF) | Shareable YAML "recipes", 70+ MCP extensions, a cron scheduler (`goose schedule add`). ACP mode can hand work to Claude Code or Codex. | Desktop, CLI | UNVERIFIED |
| AnythingLLM | Document search, a no-code agent builder and scheduled tasks. | UNVERIFIED | Telemetry reportedly on by default |
| Khoj | Self-hosted document search and agents. The hosted service closed in April 2026. | Web, phone, Obsidian, Emacs | UNVERIFIED |
| Jan | Offline chat, agent features only through MCP. | none | n/a |

## Desktop assistants from the large vendors

| Product | What a Mac user would notice |
|---|---|
| ChatGPT desktop | One app for Chat, Work and Codex since July 2026. Atlas closed on 9 August. "ChatGPT agent" was removed in early August in favour of Work. Pulse closed in June and became scheduled tasks. Memory has "memory sources" and a project-only setting. |
| Claude desktop and Cowork | Cowork runs in the cloud, on web and mobile since 7 July. Memory spans chat and Cowork. Scheduled tasks run hourly, daily, weekly or by hand, and can't see local MCP connectors. A guide says local-only tasks went away in October (UNVERIFIED). |
| Gemini for Mac | "Task" automates files in folders you choose, with access you can revoke, plus a Scheduled view. Antigravity is built in since 13 October. Needs AI Ultra, US only. Phone control of Mac tasks is announced, not shipped. |
| Siri and Apple Intelligence (macOS 27) | Personal context across mail, messages, calendar and photos; on-screen awareness; multi-step tasks across apps. Simple requests run on the device, the rest on Private Cloud Compute. |
| Microsoft Copilot for Mac | Not researched. |

## CLI and coding agents used as personal agents

| Product | What matters for Heron |
|---|---|
| Claude Code | Channels (preview, March 2026) connects a live session to Telegram, Discord or iMessage, with sender allowlists and pairing codes. Remote Control (February 2026) drives a session from the Claude mobile app. Approvals can be relayed to the phone (early). |
| Codex CLI and app | Scheduled tasks, which can run on a background worktree. Sandbox modes: read-only, workspace-write, full. Automatic approval review ("Guardian"). |
| Gemini CLI | A free tier with a Google login (1,000 requests a day). One source says it is being retired in favour of Antigravity CLI (UNVERIFIED). heron-v1.md records that the Google OAuth free tier ended on 2026-06-18, which conflicts with the free-tier figure. |
| Warp (Oz) | Cloud agents with cron and Slack, GitHub and Linear triggers, which can launch Claude Code or Codex. |
| Cursor | Parallel cloud agents in their own worktrees, with no access to local tools. |

## Memory and automation assistants

| Product | Status |
|---|---|
| Rewind and Limitless | Closed. Meta bought Limitless on 5 December 2025, and Rewind stopped recording on Macs on 19 December 2025. |
| Raycast AI | Mac launcher with AI extensions, MCP and dictation. Prices conflict across sources. |
| Lindy | Cloud assistant in Slack: routines, meeting notes, inbox, phone calls. |
| Manus, Comet | Cloud agents; details thin. |

## Abilities that recur in the leading products

| Ability | Seen in |
|---|---|
| Scheduled and recurring tasks | ChatGPT, Cowork, Gemini, Codex, goose, AnythingLLM, Warp, Lindy |
| A phone or messaging companion | OpenClaw, Claude Code, Cowork, ChatGPT, Siri, Lindy |
| File automation or search in local folders | Gemini, AnythingLLM, Khoj, Cowork |
| Multi-step actions across apps | Siri, Gemini, ChatGPT Work, Lindy, OpenClaw |
| Memory across sessions | ChatGPT, Cowork, Siri, Khoj |
| Shareable skills or plugins | OpenClaw, Claude, goose, Gemini |
| Background runs | Codex, Cursor, Warp, Claude Code |
| Voice input | Siri, Raycast, Lindy (phone calls) |
| Screen context | Siri |
| Folder-scoped access you can revoke | Gemini, OpenClaw |

## Gaps that few products fill for one private Mac user

1. **Injection safety with real approvals.** No surveyed product documents a rule like Heron's taint. OpenClaw's record is poor.
2. **A local, readable action log and undo.** None was found advertising either (absence UNVERIFIED). Codex and Cursor worktrees give a partial undo for code only.
3. **Personal-assistant work through the user's existing subscription CLI.** Warp and goose hand coding work to CLIs; none routes personal tasks through them.
4. **Local scheduling that sees local tools.** Cloud schedulers (Cowork, Codex cloud) can't reach local MCP tools or local folders.
5. **Local memory of what you did.** Rewind and Limitless are gone.

## Sources

- https://en.wikipedia.org/wiki/OpenClaw
- https://presenc.ai/research/openclaw-ecosystem-2026
- https://help.openai.com/en/articles/6825453-chatgpt-release-notes
- https://www.lilachbullock.com/chatgpt-pulse/
- https://support.claude.com/en/articles/12138966-release-notes
- https://claude.com/docs/cowork/changelog
- https://oktopeak.com/blog/claude-cowork-scheduled-tasks-local-mcp-connector/
- https://9to5google.com/2026/06/30/gemini-spark-mac-app/
- https://9to5google.com/2026/10/13/gemini-app-macos-antigravity/
- https://easternherald.com/2026/09/13/ios-27-siri-google-gemini-apple-launch/
- https://www.linuxfoundation.org/press/linux-foundation-announces-the-formation-of-the-agentic-ai-foundation
- https://blog.marcnuri.com/goose-on-machine-ai-agent-cli-introduction
- https://www.vellum.ai/blog/best-local-ai-assistants
- https://developers.openai.com/codex/changelog?type=codex-app
- https://agentscamp.com/guides/configuration/codex-sandbox-and-approvals
- https://www.developersdigest.tech/blog/claude-code-channels
- https://jwatte.com/blog/claude-code-remote-control-channels/
- https://www.morphllm.com/comparisons/gemini-cli-vs-cursor
- https://www.augmentcode.com/tools/warp-vs-cursor
- https://remyndai.com/blog/what-happened-to-rewind-ai/
- https://dev.to/jangwook_kim_e31e7291ad98/raycast-review-2026-the-mac-launcher-that-replaced-4-apps-mcp-setup-guide-3ag2
- https://www.aitools-directory.com/blog/manus-ai-vs-lindy-vs-autogpt-business-automation/
