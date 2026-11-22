# Research: official CLIs as the model backend, and a SwiftUI shell over the Rust core

Researched 2026-11-18 from public docs and repos. Nothing was installed or run. Every claim carries a source URL. Anything I could not confirm is marked UNCONFIRMED.

---

## A. Official agent CLIs as the model backend

### A1. How T3 Code and Hermes do it

**T3 Code** (https://github.com/pingdotgg/t3code)
- It calls itself "the open-source control plane for coding agents" and says "T3 Code does not resell tokens. It drives the agents you are already signed in to." It works with Claude Code, Codex, Cursor, OpenCode, Grok Build, Antigravity and others. Source: https://raw.githubusercontent.com/pingdotgg/t3code/main/README.md
- It is not a harness. Its FAQ says Claude Code is "an interface ... plus a harness underneath (the Anthropic Agent SDK)", Codex is "the app plus the Codex App Server", and "T3 Code uses those harnesses." Source: https://raw.githubusercontent.com/pingdotgg/t3code/main/docs/user/faq.md
- Login is the CLI's own: "T3 Code uses Claude Code's login and configuration." Multiple accounts are done with separate `CLAUDE_CONFIG_DIR` directories. Source: https://raw.githubusercontent.com/pingdotgg/t3code/main/docs/user/providers-claude.md
- Important difference from your design: T3 lets the CLI run its own tools (it is a coding-agent UI). It exposes its own functions to outside agents as an MCP server. Source: https://raw.githubusercontent.com/pingdotgg/t3code/main/docs/user/outside-agents.md. It does not strip the CLI's tools, so it is a model for the control-plane shape but not for "host owns every action".
- Its FAQ claims "T3 Code is within the TOS of Claude Code, Codex, and Google Antigravity". That is the vendor's own claim, not Anthropic's. Source: the FAQ above.
- It reports that Claude's 5-hour meter used to charge Agent SDK usage about 1.7x more than Claude Code, fixed from v0.1.7. Same FAQ.

**Hermes Agent, "Claude Subscription DirectSDK (Experimental)"** (marketplace page: https://hermes-agent.nousresearch.com/marketplace/claude-subscription-directsdk; source: https://github.com/NousResearch/hermes-official-plugins/tree/main/claude-subscription-directsdk). This is the closest match to your plan, because Hermes keeps its own agent loop and uses the CLI as a model client. Details from its README (https://raw.githubusercontent.com/NousResearch/hermes-official-plugins/main/claude-subscription-directsdk/README.md):
- It "drives the unmodified official Claude Code executable as a request-scoped model client." Hermes "keeps its normal agent loop, tools, approvals and compaction." It speaks stream-json directly and does not need the Python SDK.
- The plugin has no credentials. It never opens, copies, refreshes or prints the CLI's credential files. It drops `ANTHROPIC_API_KEY` and similar variables from the child's environment so the subscription is used.
- Each request starts a fresh process in a private temp directory. "Native tools, skills and setting sources are disabled. MCP advertises only the current Hermes tool inventory, has inert callbacks, and is denied execution by native `dontAsk`." The system prompt and tool schemas go through private files to avoid per-argument limits. So Hermes does not let the CLI run tools. It advertises tool schemas through an MCP helper, lets the model emit tool calls, then executes them itself. This is the pattern to copy.
- A local loopback relay forwards only the first Messages request and rejects later ones, because the CLI "can attempt extra generations despite `--max-turns 1`" (tested on 2.1.263). This is a workaround that needs `ANTHROPIC_BASE_URL`, which is invasive. See risks.
- Tool-less calls skip the MCP helper because "the helper's interpreter start (~0.6 s)" is a fixed per-call cost.
- History is replayed on each request using version-sensitive stream-json input behaviour. The README says these are "version-sensitive interfaces, not a public arbitrary-history SDK guarantee". Qualified on Claude Code 2.1.263 at the time of writing.
- Metering: Claude Code names its caller on each request, and `claude -p` defaults to `sdk-cli`, which draws the 5-hour window about 2x faster per token than interactive use (see https://github.com/anthropics/claude-code/issues/97074, cited by the README). The plugin sets its own client name. Anthropic has not documented the weighting.
- It is labelled "a review build, not a full-parity or production-readiness claim."
- Secondary report (not primary): a Hermes feature request says that from 2026-06-15 Agent SDK and `claude -p` usage is decoupled from interactive limits and given a separate monthly Agent SDK credit. Source: https://github.com/NousResearch/hermes-agent/issues/25267 (UNCONFIRMED; I did not read it directly, only a search summary).

### A2. Claude Code (headless `claude -p` and the Agent SDK)

Primary docs: https://code.claude.com/docs/en/headless and https://code.claude.com/docs/en/cli-reference

| Need | How | Source |
|---|---|---|
| Stream output | `--output-format stream-json --verbose --include-partial-messages`. Newline-delimited JSON; token deltas arrive as `stream_event` with `delta.type == "text_delta"`; the last line is a `result` message. `--include-partial-messages` requires `--print` and `stream-json`. | headless page, cli-reference |
| Streaming input | `--input-format stream-json` (options `text`, `stream-json`). | cli-reference |
| Turn off all built-in tools | `--tools ""` ("Use `""` to disable all"). It does not affect MCP tools. To also deny MCP tools use `--disallowedTools "mcp__*"`. `--disallowedTools "*"` removes every tool. | cli-reference |
| Attach only your MCP server | `--strict-mcp-config --mcp-config ./mcp.json`. The strict flag skips servers from `.mcp.json`, `~/.claude.json` and plugins. With `-p`, the CLI waits for pending servers up to `MCP_TIMEOUT` (30 s default). Check `mcp_servers` and `mcp_server_errors` in the `system/init` event, because a bad entry is skipped silently. | cli-reference, headless page |
| Custom system prompt | `--system-prompt` or `--system-prompt-file` replaces the whole default prompt. `--append-system-prompt[-file]` adds to it. Replacing it also drops the git status snapshot. | headless page, cli-reference |
| Token usage and cost | `--output-format json` returns session ID, usage and `total_cost_usd` with a per-model breakdown. Both are "client-side estimates". The stream-json `result` message carries cost and session metadata. | headless page |
| Resume | `--resume <session-id>` (or a transcript `.jsonl` path), `--continue` for the latest, `--session-id <uuid>` to choose your own ID. | headless page, cli-reference |
| Structured output | `--json-schema` with `--output-format json` fills `structured_output`. | headless page |
| Permissions | `--permission-mode dontAsk` denies anything that would prompt. `--permission-prompt-tool mcp__<server>__<tool>` delegates approvals to an MCP tool. `--permission-prompts none` for unattended runs (v2.1.259+). | headless page, cli-reference |
| Skip project/user config | `--bare`. Warning: "In bare mode, Claude Code never reads OAuth credentials or the system keychain", so `--bare` is for API-key use and will not work with a subscription login. | headless page |
| Narrow settings | `--setting-sources user,project,local` picks which settings files load. | cli-reference |

Agent SDK: the same engine as a Python/TypeScript package (https://code.claude.com/docs/en/agent-sdk/overview). I did not read the SDK pages in this pass, so SDK-only features such as in-process custom tools are UNCONFIRMED here. For a Rust core, the CLI's stream-json over a child process is the practical route.

Design consequence for your tool layer: with `--tools ""` plus `--strict-mcp-config` pointing at your own MCP server (HTTP or stdio into your axum process), the model can only call your tools. The permission, taint and audit logic then sits in your MCP handler. One gap: the CLI will still execute MCP tool calls itself (it is the MCP client), so your handler is the only gate. Hermes goes further and denies native execution so that Hermes alone runs the tool. Whether `--permission-mode dontAsk` combined with a stub MCP server cleanly gives that behaviour is Hermes's reported approach (README above) but I have not verified it independently.

### A3. Codex (`codex exec`, app-server)

Primary docs: https://learn.chatgpt.com/docs/non-interactive-mode.md (the old developers.openai.com/codex/noninteractive URL redirects there).
- Stream: `codex exec --json` makes stdout a JSONL stream. Event types are `thread.started`, `turn.started`, `turn.completed`, `turn.failed`, `item.*` and `error`. Without `--json`, progress goes to stderr and only the final message goes to stdout.
- Token usage: `turn.completed` carries `usage` with `input_tokens`, `cached_input_tokens`, `output_tokens`, `reasoning_output_tokens` (sample in the same page).
- Resume: `codex exec resume --last "..."` or `codex exec resume <SESSION_ID>`. `codex exec fork <SESSION_ID>` copies a session (https://learn.chatgpt.com/docs/developer-commands.md). `--ephemeral` skips saving rollout files.
- Structured output: `--output-schema ./schema.json` with `-o file`.
- Sandbox: default is a read-only sandbox; `--sandbox workspace-write` or `danger-full-access`. `--full-auto` "is no longer supported". Flags are changing, which supports the version-drift risk below.
- Config: `--ignore-user-config` skips `$CODEX_HOME/config.toml`; `--ignore-rules` skips execpolicy rules. Project-defined MCP servers load only for trusted projects. An MCP server with `required = true` that fails to start makes `codex exec` exit with an error.
- Auth: "`codex exec` reuses saved CLI authentication by default." ChatGPT-managed auth in CI is described as an advanced path using `~/.codex/auth.json`, which "contains access tokens". The doc says not to use that workflow for public or open-source repositories.
- App-server: a JSON-RPC server with `thread/start`, `thread/resume`, `turn/start` style methods and experimental APIs (https://raw.githubusercontent.com/openai/codex/main/codex-rs/app-server/README.md). T3 uses it per its FAQ. I did not find, in the parts I read, a documented way to carry a host-defined tool set or a base-instructions override. Those are UNCONFIRMED: check the app-server README for `dynamicTools` or instruction fields before committing.
- UNCONFIRMED: a single documented switch that turns off every Codex built-in tool (shell, apply_patch, web search). The flag tables on the docs site render client-side and I could not read them. Config keys such as `features.*` and `mcp_servers` exist but I could not verify names. Treat "Codex with no built-in tools" as unproven. A read-only sandbox plus `--ignore-user-config` plus a `-c mcp_servers...` override is a fallback to test.

### A4. Other CLIs

- **Cursor CLI** (`agent`): official flags page https://cursor.com/docs/cli/reference/parameters. `-p` prints and has "access to all tools"; `--output-format text|json|stream-json`; `--stream-partial-output` with stream-json; `--force` allows commands unless denied; `--approve-mcps` and `--trust` for headless. It reads the same `mcp.json` as the IDE. Summary via search: https://devhints.io/cursor-cli. Thinking events are not emitted in print mode. UNCONFIRMED: how to disable built-in tools, how to pass a custom system prompt, and token usage fields. Not a good fit for "no built-in tools".
- **Grok Build** (`grok`): https://docs.x.ai/build/overview. Headless `grok -p` with `--output-format streaming-json` (NDJSON). `grok agent stdio` exposes ACP (JSON-RPC over stdio). MCP via `grok mcp add` or `~/.grok/config.toml`. Per a secondary source, headless setups use API-key auth rather than browser OAuth (https://www.scriptbyai.com/grok-build-coding-cli/; UNCONFIRMED against primary docs). UNCONFIRMED: tool disabling, system prompt override, usage fields.
- **OpenCode**: runs as a local HTTP server, `opencode serve`, with an OpenAPI spec, `GET /event` SSE stream, `POST /session`, `POST /session/{id}/prompt_async`. Source (docs mirror, not the official site): https://open-code.ai/en/docs/server. A GitHub issue reports no SSE on serve (https://github.com/anomalyco/opencode/issues/13416), which looks like a misunderstanding of the separate `/event` stream. UNCONFIRMED: tool disabling and system prompt override. OpenCode is a good "API-shaped" backend, but note its own agent loop would run tools unless configured off.
- **Google Antigravity** (`agy`): headless `agy -p` with `--output-format json|stream-json` (https://antigravity.google/docs/cli/headless/); `agy acp` serves ACP over stdio (https://antigravity.google/docs/cli/acp/); flags at https://antigravity.google/docs/cli/reference. Headless runs are stateless by default. UNCONFIRMED: tool disabling, system prompt override, token usage fields, and whether consumer Google plans allow third-party control. I did not read the primary pages, only a search summary.
- **ACP (Agent Client Protocol)** is a common JSON-RPC-over-stdio protocol that Grok Build and Antigravity both expose, and T3 says "everything else uses official ACP bindings" (T3 FAQ above). One ACP client in Rust would cover several CLIs. The ACP spec itself: UNCONFIRMED (not read here).

### A5. Terms of service position

**Claude (Anthropic).** The official page is https://code.claude.com/docs/en/legal-and-compliance. Key wording:
- "OAuth authentication is intended exclusively for purchasers of Claude Free, Pro, Max, Ultra, Team, and Enterprise subscription plans and is designed to support ordinary use of Claude Code and other native Anthropic applications."
- "Developers building products or services that interact with Claude's capabilities, including those using the Agent SDK, should use API key authentication through Claude Console or a supported cloud provider. Anthropic does not permit third-party developers to offer Claude.ai login into their own applications, or to route requests through Free, Pro, Max, or Ultra plan credentials on behalf of their users. Moreover, developers may not collect, store, or intermediate Claude.ai credentials or session tokens — sign-in to a Claude account must complete through Anthropic's own flow."
- Carve-out: "Nor does it prevent an end user from signing in to the unmodified Claude Code binary with their own Claude subscription, including where a platform hosts Claude Code as described under *Can customers offer Claude Code in their products?* above."
- Conditions for running Claude Code in your product: "The Claude Code binary must not be modified" and customers "may not pay for, resell, or intermediate Claude usage on their end users' behalf. Each end user must authenticate with their own Anthropic API key, Claude subscription plan credentials, or 3P inference provider credential."
- "Advertised usage limits for Pro, Max, and Ultra plans assume ordinary, individual usage of Claude Code and the Agent SDK." Anthropic "may [enforce] without prior notice."

My reading (not legal advice): a local app that spawns the unmodified `claude` binary, never touches the OAuth token, and uses only the user's own login on the user's own machine fits the carve-out and the T3/Hermes pattern. The risk is the ambiguity between "developers building products ... using the Agent SDK should use API keys" and "end user signing in to the unmodified binary". The page does not say which wins for a third-party app that drives the CLI as a model backend. The conservative stance is to treat subscription use as supported-but-grey, ship API-key and other providers as first-class alternatives, and ask Anthropic (contact link on that page) before any commercial release. Also, a strict reading of "unmodified" argues against patching the CLI or injecting an `ANTHROPIC_BASE_URL` relay as Hermes does; I would avoid the relay. Whether `--tools ""`, `--system-prompt` and `--strict-mcp-config` count as "modifying" is UNCONFIRMED but they are documented flags, not binary changes.

**Consumer Terms** (https://www.anthropic.com/legal/consumer-terms): referenced by the page above as governing Free, Pro, Max, Ultra. I did not read the full text; any clause on automated access is UNCONFIRMED.

**Codex / ChatGPT (OpenAI).** The non-interactive doc says `codex exec` reuses saved CLI auth and describes ChatGPT-managed auth in CI as advanced, with an explicit warning against public repos (https://learn.chatgpt.com/docs/non-interactive-mode.md). OpenAI also launched "Sign in with ChatGPT", an official route for third-party apps to spend a user's ChatGPT plan usage, in a limited preview with partner apps including Hermes Agent, T3 and OpenCode (secondary coverage: https://thenewstack.io/sign-in-with-chatgpt/; UNCONFIRMED: I could not reach OpenAI's own help-center page). Per that coverage, the app sees name, email and picture but not conversations, and the user can cap each app weekly. This is an official, sanctioned alternative to scraping the CLI login, but it appears invite-based. OpenAI's terms text on driving the Codex CLI from another app: UNCONFIRMED.

**Others.** T3 says it worked with SpaceXAI on Grok Build and Cursor integrations (T3 FAQ). Cursor, xAI and Google terms on third-party control: UNCONFIRMED.

### A6. Practical risks

1. **Per-turn startup latency.** Hermes starts a new process per request and skips its MCP helper for tool-less calls because the helper costs about 0.6 s (README above). Claude Code with `--mcp-config` waits for MCP servers up to 30 s before the first turn (headless page). I found no published CLI cold-start numbers: UNCONFIRMED. Mitigations: a persistent process using `--input-format stream-json` (multi-turn over stdin) instead of one process per turn; a long-lived HTTP/stdio MCP server in your core so the CLI connects to something already running; measure it yourself before deciding.
2. **Flags drift.** Evidence: Claude's `--bare` is slated to become the default for `-p` and changed behaviour before v2.1.286 (headless page); `--permission-prompts` needs v2.1.259+; Codex dropped `--full-auto` (non-interactive page); Hermes pins models to minimum CLI versions and qualified against 2.1.263 (README). Mitigations: detect the version on startup, read `system/init` (it has a `capabilities` array "to feature-detect instead of comparing version strings"), pin and test a version matrix, fail closed when a required flag is missing, and keep each CLI behind one adapter trait.
3. **The CLI's own system prompt and tokens.** `--system-prompt` replaces the default prompt; `--append-system-prompt` keeps it. Even with a replacement, tool schemas for your MCP tools and any reminders still add tokens. `--bare` removes system reminders but is API-key only. Hermes measured about 0.6x the tokens of native Claude Code on identical tasks with its own prompt, and measured the subscription meter charging `claude -p` roughly 1.7 to 2x the interactive rate (README). The size of the default prompt: UNCONFIRMED, measure it from the `result` usage on an empty turn.
4. **Safety of the host boundary.** The CLI is the MCP client, so your MCP handler must be the only enforcement point, and CLI config discovery must be blocked (`--strict-mcp-config`, `--setting-sources`, a private working directory like Hermes). Without `--bare`, `-p` runs hooks from project settings and connects project MCP servers (headless page). Run it in an empty temp dir.
5. **Taint and context control.** If the CLI keeps the session, history lives in its transcript, not your SQLite. Hermes replays canonical history each request to keep ownership. Decide which side owns history.
6. **Rate-limit behaviour.** T3 notes Claude Code "holds the turn until that window reopens" when the subscription limit is hit (providers-claude.md). Your UI needs a visible "waiting for limit" state.
7. **Account and policy risk.** Anthropic can enforce without notice (A5). Keep non-subscription providers available.

### A recommendation

Build a provider-adapter layer with Claude Code as the first CLI backend: spawn the unmodified `claude` in an empty temp dir with `-p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --tools "" --strict-mcp-config --mcp-config <your axum MCP endpoint> --system-prompt-file`, keep your own history and audit log, and treat subscription use as grey-area, optional and not the only path (API key and a direct HTTP provider alongside it); do not copy Hermes's relay and do not use `--bare`.

---

## B. Native SwiftUI macOS app over the Rust core

### B1. Integration: local HTTP API vs FFI

**Option 1: talk to the existing axum API over localhost (SSE or WebSocket).**
- Latency: loopback HTTP adds sub-millisecond to low-millisecond overhead per request, negligible next to model latency (general knowledge; no source, treat as UNCONFIRMED as a measured number). Token streaming over SSE is fine. Swift side: `URLSession.bytes(for:)` gives an async line stream you can parse as SSE; WebSocket via `URLSessionWebSocketTask`. Apple docs: https://developer.apple.com/documentation/foundation/urlsession (not fetched; UNCONFIRMED detail).
- Packaging: the Rust binary ships as a separate helper that the app starts. Options seen in practice: a LaunchAgent registered with `SMAppService` and a plist in `Contents/Library/LaunchAgents` (JerisLink, https://github.com/JerisHQ/jerislink/pull/1), or a helper in `Contents/MacOS` or `Contents/Helpers` (https://developer.apple.com/forums/thread/129544?page=2, rig-doctor https://github.com/stitts-dev/rig-doctor/pull/67). A LaunchAgent shows in System Settings > Login Items (https://gist.github.com/Matejkob/f8b1f6a7606f30777552372bab36c338). Benefit: the agent can keep running (channels, schedules) after the window closes, and the CLI and a future non-Mac client share one API.
- Signing and notarization: "Every Mach-O binary inside the app bundle, including the helper, has to be signed separately with your Developer ID and the hardened runtime turned on", then you notarize the whole app; a bare command-line tool cannot be notarized on its own, but one embedded in an app is covered (https://scriptingosx.com/2019/09/notarize-a-command-line-tool/, plus the forum thread above). Sign inside-out, then `codesign --verify` and `spctl -a -vvv`. Cargo-built binaries need signing with `-o runtime` after build.
- Local API security (relevant to your taint model): any local process can reach a localhost port. Bind to loopback, use a per-install bearer token or a Unix domain socket with file permissions. Unix socket support in `URLSession`: UNCONFIRMED (not natively supported in plain URLSession as far as I know; check). A token in the macOS Keychain shared via app group is the common alternative.
- Update flow: updating the app replaces the helper in the same bundle, so one signed artifact. Sparkle (https://sparkle-project.org) is the usual updater for non-App-Store apps; I did not verify its current docs (UNCONFIRMED). The helper must be restarted after an update (re-register the SMAppService agent or kill and relaunch).
- Sandbox: a Mac App Store sandboxed app cannot easily spawn arbitrary helpers or run the user's `claude` CLI. A Developer ID, non-sandboxed build avoids this. For your design (spawning the user's CLIs, reading their login) a non-sandboxed Developer ID build is effectively required. UNCONFIRMED against Apple docs here, but strongly implied by the CLI-spawning need.

**Option 2: FFI through UniFFI or swift-bridge.**
- UniFFI (Mozilla): official Swift bindings, library mode, `uniffi-bindgen-swift` generates XCFramework-compatible modulemaps (https://mozilla.github.io/uniffi-rs/latest/swift/overview.html, https://mozilla.github.io/uniffi-rs/next/swift/uniffi-bindgen-swift.html). Async Rust becomes Swift `async throws`. Caveats reported by real projects: Swift async still does not cancel Rust futures in 0.32.2 (https://github.com/oddurs/xel/pull/22); a Rust panic is only caught for functions returning `Result`; partial Swift 6 support, and the docs say async code is known not to conform (https://mozilla.github.io/uniffi-rs/latest/swift/overview.html and the same PR). It also generates Kotlin, useful if you ever add Android.
- swift-bridge: generates a typed bridge from a macro module, supports async and Swift 6.0+ output, Swift only (https://lib.rs/crates/swift-bridge). Smaller community. I found no direct head-to-head article; one project ran a spike comparing UniFFI, swift-bridge and hand-written C ABI and chose UniFFI (https://github.com/oddurs/xel/pull/22).
- Latency: in-process calls, microseconds, no sockets or serialization of HTTP. For chat tokens this gain is irrelevant; it matters only for high-frequency calls.
- Packaging: static library or XCFramework linked into the app, so there is one binary, one signature, no helper and no Login Item. Simplest notarization.
- Update flow: single bundle, simplest.
- Costs: the Rust core's tokio runtime and its HTTP server and channel listeners then live inside the GUI process, so closing the app stops the agent unless you also keep a separate daemon. That undermines a local-first assistant that should run in the background and receive Telegram or scheduled work. A crash or panic in Rust takes down the UI. You would maintain two surfaces (FFI API and the existing HTTP API). Bindings must be regenerated and the XCFramework rebuilt for arm64 and x86_64 on every API change.

**Comparison.**

| | Localhost HTTP + SSE | UniFFI / swift-bridge |
|---|---|---|
| Per-call latency | ms-scale, irrelevant for chat | microseconds |
| Streaming | SSE or WebSocket, trivial | callbacks or async streams, more binding work |
| Agent outlives the window | yes (LaunchAgent) | no, unless you also run a daemon |
| Reuse by other clients | yes (same API) | no |
| Signing | helper signed separately, whole app notarized | one binary |
| Update | replace bundle, restart helper | replace bundle |
| Crash isolation | good | poor |
| Security surface | local port needs auth | none |
| Swift 6 / cancellation | plain URLSession | known gaps reported |

**B1 recommendation:** keep the HTTP API as the contract (the daemon is the product), run it as an `SMAppService` LaunchAgent bundled in the signed app, and authenticate it with a per-install token; consider UniFFI later only for a narrow, hot, synchronous surface (for example local search) if profiling shows a need.

### B2. Open-source SwiftUI AI chat and agent apps to study

I read only the search summaries and repo titles, not the code. Strengths below are what the repo descriptions claim; verify before relying on them.
- **Warden** (https://github.com/SidhuK/WardenApp): a fully native macOS chat client, Apache 2.0, about 347 stars. Good reference for multi-provider settings and native feel.
- **ayna** (https://github.com/sozercan/ayna): agentic AI chat for macOS, iOS and watchOS, SwiftUI, MIT, needs macOS 26+, multi-model comparison and Apple Intelligence. The closest to "agent" and a cross-platform SwiftUI structure.
- **QuickChat** (https://github.com/SYJun404/QuickChat): menu-bar app with a global hotkey and floating panel; says it works with "your ChatGPT subscription". Useful for the quick-capture UI pattern. The subscription path it uses needs checking against the ToS points in A5.
- **iChat** (https://github.com/iChochy/iChat): SwiftUI plus SwiftData, MIT, many providers, macOS 14+. A simple data-model reference.
- **OmniChat** (https://github.com/bowenyu066/OmniChat): SwiftUI, SwiftData, unifies ChatGPT, Claude, Gemini APIs.
- **AskKit** (https://github.com/gregyuzik/AskKit): a library for an in-app Ask screen with Apple's Foundation Models.
- Not found in my searches, so UNCONFIRMED as current open-source options: Enchanted, Ollamac, MacPaw/OpenAI. Search them directly if you want mature streaming-markdown rendering examples.
- Also worth reading for architecture, not SwiftUI: T3 Code's server-plus-thin-clients design (README above).

### B3. Animated avatar options in native SwiftUI

Facts below on Rive and Lottie are from the sources shown; SpriteKit, RealityKit and SwiftUI-native notes are general knowledge, so mark them UNCONFIRMED until checked against Apple docs (https://developer.apple.com/documentation/spritekit, https://developer.apple.com/documentation/realitykit, https://developer.apple.com/documentation/swiftui/animations).

| Option | What it is | Strengths | Trade-offs |
|---|---|---|---|
| SwiftUI native (shapes, `TimelineView`, `Canvas`, `phaseAnimator`, `KeyframeAnimator`) | Draw the avatar in code | No dependency, tiny, theme and accessibility aware, easy to bind to agent state (idle, thinking, speaking) | Everything is hand-built; complex character animation is laborious; no artist workflow |
| Rive | State-machine animation runtime, designer authors `.riv` files | Official runtime for Apple platforms: "UIKit, AppKit and SwiftUI", macOS 13.1+ per a DeepWiki summary (https://deepwiki.com/rive-app/rive-ios). State machine inputs (Boolean, Number, Trigger) map well to agent state: `riveViewModel.setInput(...)` (https://rive.app/docs/runtimes/apple/state-machines). GPU vector rendering, small files | A dependency with a C++ core; docs show a newer async API (`Worker`, `File`, `Rive`) alongside the older `RiveViewModel`, so expect API churn; a past SwiftUI state-machine crash was reported (https://github.com/rive-app/rive-ios/issues/330, fixed in 2024 per the issue); you need a Rive-authored asset or a designer |
| Lottie | Plays After Effects animations exported as JSON/dotLottie | Official SwiftUI `LottieView` since 4.3.0 (https://github.com/airbnb/lottie-ios/discussions/2189); use the small `lottie-spm` package, not the 300 MB repo (https://github.com/airbnb/lottie-spm); can play frame ranges and loop modes | Playback-oriented rather than state-driven: you sequence clips yourself; macOS support through the iOS code path, details UNCONFIRMED; limited interactivity |
| SpriteKit (`SpriteView`) | 2D sprite and particle engine | Built in, good for frame-by-frame sprites, particles, simple physics | Sprite-sheet asset pipeline; feels game-like inside a SwiftUI UI; less resolution-independent than vector |
| RealityKit (`RealityView`) | 3D engine | A 3D or stylised character, lighting, USDZ assets, eye tracking-like effects | Heaviest: a 3D art pipeline, GPU and battery cost, macOS version requirements UNCONFIRMED; overkill for a chat avatar |

**B3 recommendation:** start with a SwiftUI-native avatar (a few states driven by the agent's event stream), and adopt Rive if you hire or commission a designer, because its state machine matches an agent-state model; skip RealityKit until you need 3D.

---

## Summary of what I could not confirm
- Whether Codex (and Cursor, Grok Build, OpenCode, Antigravity) can disable all built-in tools or accept a host tool set and custom system prompt.
- Token-usage fields for Cursor, Grok Build, OpenCode, Antigravity.
- The default Claude Code system-prompt size and cold-start latency.
- Anthropic's position on Agent SDK or CLI driving by third-party apps beyond the legal page, and any Consumer Terms clause on automated access.
- OpenAI's and Google's own wording on third-party apps using the Codex or Antigravity CLI login (only vendor-partner claims and secondary coverage found).
- The Hermes "June 15 2026 Agent SDK credit" claim (secondary).
- Apple-doc and Sparkle details in B1 and B3 (general knowledge, not fetched).
