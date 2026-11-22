# Model providers: subscriptions through the official CLIs

In the v1 research preview, the main way to connect a model is a CLI you are
already logged in to, on your own Mac, with your own subscription. Heron can
also use an API key against an OpenAI-compatible endpoint (`[llm] base_url`).
Heron never reads, copies, or prints a CLI token. You log in inside the CLI
itself, before Heron ever starts it.

Check what is installed and logged in:

```sh
heron providers
heron doctor        # also exercises the selected provider with one real call
```

For a first install on a Mac, follow [Getting started on a Mac](getting-started-mac.md).

## Tier 1 CLIs

A Tier 1 CLI passed Heron's containment check on a real install (see
[Containment](#containment)) and the release's task gate. Tier 1 CLIs are the
ones the release gates are run on. Every CLI that passed the containment check
has a pinned version range. Every provider that isn't Tier 1 is labelled
experimental.

Recorded on the owner's Mac on 2026-11-18 (`eval/containment/*.json`;
`python3 -I eval/containment/check.py --list-tier1` prints the list):

| Provider id | CLI | Passed on | Supported versions | Login | Where the login is kept |
| --- | --- | --- | --- | --- | --- |
| `claude-cli` | Claude Code | 2.1.327 | `>=2.1.327, <3.0.0` | `claude auth login` | macOS login Keychain |
| `codex-cli` | Codex CLI | 0.169.0 | `>=0.169.0, <1.0.0` | `codex login` | `~/.codex` |

The lower bound is the version that passed the containment check. The upper
bound is the next major version. The same values are in the registry
(`src/cli_llm.rs`, field `supported`).

**OpenCode is not configured or tested yet in v1.** `opencode-cli` passed the
containment check on 2.1.4, and Heron keeps its version range
(`>=2.1.4, <3.0.0`), because the way Heron turns its own tools off was checked
on 2.x only. Its task gate has not been run, so the owner left it out of v1
Tier 1 on 2026-11-21 and it is labelled experimental. Don't rely on it yet.

The other candidates did not reach Tier 1:

| Provider id | Result | Why |
| --- | --- | --- |
| `antigravity-cli` | not installed | Not installed on the owner's Mac when the check ran, so the Google slot is empty. |
| `cline-cli` | not installed | Not installed when the check ran. Its one-shot mode also runs its own tools, so it needs `cli_allow_agent_tools = true`. |
| `gemini-cli` | unavailable | A personal Google login no longer works (below). |

**gemini-cli and Google logins.** Gemini CLI v0.68.0 refused the owner's
personal Google login with: "This client is no longer supported for Gemini Code
Assist for individuals. To continue using Gemini, please migrate to the
Antigravity suite of products". Gemini CLI now works only with a Gemini API key
(or Vertex AI), and it stays experimental. If you want to use a Google login,
use `antigravity-cli` (`agy`) instead.

### Version checks

Heron reads the CLI's version (the first `x.y.z` in its `--version` output) at
startup, when you switch provider, and after a failed turn. If the version is
outside the supported range, or can't be read, Heron stops before the turn and
says so, for example:

> codex-cli reports version 1.2.0, which Heron has not been tested with.
> Continue with this version (POST /api/providers/codex-cli/accept-version), or
> install a tested one.

The Mac app offers the same choice as a prompt. Nothing runs through the
untested version until you accept it. Heron remembers the exact
version you accepted, and asks again when it changes. Experimental providers
have no range, so they show no version prompt.

## Agent loop topology

Heron owns the agent loop for every CLI. A CLI only returns text plus the tool
calls it wants. Heron runs each call through its own permission check and
executes it itself. Two ways of talking to the CLI were measured:

- **envelope**: Heron puts the conversation and the tool list into one prompt,
  and the CLI answers with one JSON object (see [How the model answers](#how-the-model-answers));
- **hybrid**: the CLI sees Heron's tools as native tools over MCP, is held to
  one turn, and never runs them.

The measurement spike (`eval/spike/topology.md`) ran the 10 core tasks 5 times
per arm on claude-cli:

| Arm | Passed | Median time to first token | Median tokens per task |
| --- | --- | --- | --- |
| envelope | 50/50 | 5615.5 ms | 134.5 |
| hybrid | 50/50 | 5581 ms | 136.5 |

Hybrid needed to win by at least 4 runs of 50 to become the default. It tied,
so the result is inconclusive and the chosen default is `topology=envelope` for
every CLI. The hybrid code has been removed, so there is no topology setting.

## Containment

Every CLI runs under the same containment on macOS:

1. **Built-in tools off.** Heron starts the CLI with its own shell, edit and
   fetch tools turned off. For the Tier 1 CLIs the containment check proves
   it with a canary: the CLI is asked to write a file with its own tools, and
   the check passes only if the file never appears. The exception is
   `cline-cli` and `goose-cli`, which run their own tool loop in one-shot mode
   and can't turn it off. They refuse to run unless you set
   `cli_allow_agent_tools = true`, a deliberate choice; they still run in the
   sandbox below.
2. **A sandbox.** The CLI runs under its own Seatbelt profile. It can write
   only to its CLI run folder (`$TMPDIR/heron-cli-<run>`, removed after the
   turn), `/dev`, and its own config folders. It can read the system and temp
   folders, the folder its binary (and the binary's interpreter) is installed
   in, and its own config folders. It can't read the rest of your home folder,
   Heron's own folder, or another CLI's folders.
3. **An allowlisted environment.** The CLI gets only `PATH`, `HOME`, `LANG`,
   `USER`, `TMPDIR`, `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` and
   `SSL_CERT_FILE` (those that are set), its config-folder variable
   (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`) when set, and, for claude-cli,
   `DISABLE_AUTOUPDATER=1` and `DISABLE_FEEDBACK_COMMAND=1`, which keep it
   from updating itself past its tested version. API-key variables are passed
   only to providers listed in `cli_allow_api_key`.
4. **No sandbox, no CLI.** If the sandbox is off (`agent.sandbox = "off"`,
   `HERON_SANDBOX=off`, or `sandbox-exec` missing), Heron refuses to start a
   CLI: "the sandbox is off, so Heron won't run <CLI>".

No provider is ever started with an auto-approve flag (`--yolo`, `--force`,
`--dangerously-skip-permissions`, `--allow-all-tools`). Cancellation and
timeouts kill the whole process tree of the CLI.

### CLI config folders

Each CLI's config folders are Refused everywhere else: `read_file` denies them
in every mode, no allow rule can open them, and the shell sandbox can't read
them. Only that CLI's own sandbox can read and write them.

| Provider id | Config folders |
| --- | --- |
| `claude-cli` | `~/.claude`, `~/.claude.json` (login in the Keychain; its sandbox may also reach `~/Library/Keychains`) |
| `codex-cli` | `~/.codex` |
| `opencode-cli` | `~/.cache/opencode`, `~/.config/opencode`, `~/.local/share/opencode`, `~/.local/state/opencode` |

The Tier 1 folders were recorded by logging each CLI in under an empty home
folder (`bash eval/containment/discover-dirs.sh <id>`). Shared parents such as
`~/.config` or `~/.local/share` are never counted as a CLI's folder. The
folders for experimental providers come from their documentation and are not
yet confirmed.

### Toolchain folders

The shell sandbox (for the agent's `shell` tool, not for CLIs) denies reads
under your home folder by default. So that builds, package managers and git
still work, it can read these folders. The list is kept in step with
`TOOLCHAIN_DIRS` in `src/sandbox.rs`:

- `~/.gitconfig` (the file), `~/.config/git`
- `~/.cargo`, `~/.rustup`
- `~/.npm`, `~/.nvm`
- `~/.local/bin`, `~/.local/share/uv`
- `~/.cache/uv`, `~/.cache/pip`
- `~/Library/Caches/Homebrew`, `~/Library/Caches/pip`

A folder is added only when the build check
(`build_and_git_still_work_under_deny_default_reads`) needs it. Commands that
need the Keychain, such as git's `osxkeychain` credential helper, fail inside
the shell sandbox.

## Subscriptions and cost

Every CLI provider counts as a subscription unless you say otherwise. That
matters for work nobody is watching:

- **Unattended work waits for you.** While the active provider is a
  subscription and `allow_unattended_subscription` is `false` (the default),
  schedules, automations, watches, proactive sessions and reflection never call
  the model. Each held run is recorded as `held-subscription` and leaves an
  inbox item, "Waiting for you: <name> needs the model". "Run now" on a
  schedule or automation is attended, so it isn't held. Telegram turns are
  attended too.
- **A CLI that isn't a subscription.** If a CLI routes to an API key or a
  local model, set `cli_subscription` to `false` for it (in Settings, or in
  `config.toml`). Heron never guesses this from the CLI's own config.
- **No silent switch to metered billing.** A provider not listed in
  `cli_allow_api_key` never sees `ANTHROPIC_API_KEY`, `OPENAI_API_KEY` and
  similar variables, so a stray key can't turn a subscription turn into a
  metered API call.

What each Tier 1 CLI costs:

- **claude-cli:** draws on the Claude Pro/Max plan's usage window.
- **codex-cli:** draws on the ChatGPT Plus/Pro/Team plan.

Running the full task gate before a release (10 tasks and one probe, 3 runs
each) uses 33 runs of each Tier 1 CLI's allowance.

```toml
[llm]
provider = "claude-cli"
cli_model = "sonnet"                  # sonnet | haiku | opus | fable, or a full model id; empty = CLI default
allow_unattended_subscription = false # true lets schedules and other unattended work use the subscription
cli_allow_api_key = false             # or a list of provider ids that may receive API-key variables, e.g. ["gemini-cli"]
# cli_command = "/opt/homebrew/bin/claude"   # only if the binary is not on PATH
# cli_config_dir = "~/.claude_personal"      # a second account, isolated from the first

# [llm.cli_subscription]
# opencode-cli = false                # only if this OpenCode routes to an API key or a local model
```

## Terms decisions

Using a consumer subscription through a third-party app is a terms-of-service
question for each vendor. The owner reads each vendor's current terms and
records a decision here before builds go to friends. Until a line is filled in,
that provider's terms are unreviewed.

- `claude-cli` (Anthropic): Terms decision: _not yet recorded_
- `codex-cli` (OpenAI): Terms decision: _not yet recorded_
- `opencode-cli` (OpenCode, plus whichever upstream it routes to): Terms decision: _not yet recorded_
- `antigravity-cli` (Google): Terms decision: _not yet recorded_
- `cline-cli` (Cline, plus whichever upstream it routes to): Terms decision: _not yet recorded_
- `gemini-cli` (Google): Terms decision: _not yet recorded_
- `cursor-cli` (Cursor): Terms decision: _not yet recorded_
- `grok-cli` (xAI): Terms decision: _not yet recorded_
- `copilot-cli` (GitHub): Terms decision: _not yet recorded_
- `qwen-cli` (Alibaba Cloud, or the configured upstream): Terms decision: _not yet recorded_
- `goose-cli` (the configured upstream): Terms decision: _not yet recorded_
- `pi-cli` (the configured upstream): Terms decision: _not yet recorded_
- `fx-cli` (the configured upstream): Terms decision: _not yet recorded_
- `kiro-cli` (Kiro, or the configured upstream): Terms decision: _not yet recorded_
- `muse-cli` (the configured upstream): Terms decision: _not yet recorded_
- `custom-cli` (whichever CLI you configure): Terms decision: _not yet recorded_

## All providers

| Provider id | What it is | Login | Cost | Status |
| --- | --- | --- | --- | --- |
| `claude-cli` | Claude Code | `claude auth login` | Claude Pro/Max usage window | Tier 1 |
| `codex-cli` | Codex CLI | `codex login` | ChatGPT Plus/Pro/Team plan | Tier 1 |
| `opencode-cli` | OpenCode | `opencode auth login` or `opencode.json` | Free with a free/local model, else upstream price | experimental: not configured or tested yet in v1 |
| `antigravity-cli` | Antigravity CLI (`agy`) | Sign in with Google | Google pricing for the method | experimental |
| `cline-cli` | Cline CLI | `cline auth <provider>` | Free with a free/local model. Needs `cli_allow_agent_tools = true` | experimental |
| `gemini-cli` | Gemini CLI | Pick Gemini API Key | API-key free tier. Google login no longer works; use Antigravity | experimental |
| `cursor-cli` | Cursor agent CLI | `agent login` | Cursor plan usage pool | experimental |
| `grok-cli` | Grok Build | `grok login` | Grok plan | experimental |
| `copilot-cli` | GitHub Copilot CLI | `copilot`, then `/login` | Copilot plan credit pool | experimental |
| `qwen-cli` | Qwen Code | `qwen`, then `/auth` | Paid key or local Ollama/vLLM. Free Qwen OAuth ended 2026-04-15 | experimental |
| `pi-cli` | Pi | `pi`, then sign in | Whatever Pi is configured with | experimental |
| `fx-cli` | fx | `fx login` | Whatever fx is configured with | experimental |
| `kiro-cli` | Kiro CLI | `kiro-cli`, then sign in | Whatever Kiro is configured with | experimental |
| `muse-cli` | Muse Code | `muse`, then sign in | Whatever Muse is configured with | experimental |
| `goose-cli` | Goose | `goose configure` | Free with a local model. Needs `cli_allow_agent_tools = true` | experimental |
| `custom-cli` | Anything else | The CLI's own docs | The CLI's own pricing | experimental |

Experimental providers run under the same containment, but they have no pinned
version range, don't count toward the release gates, and some of their flags
are not verified against a live binary. Check `binary --help` before relying on
one, and report what you find so the registry can be corrected.

The generic `custom-cli` covers what is not listed: Crush, Amp, Kimi,
Droid, Simon Willison's `llm` (a clean text-in, text-out CLI with no tool
loop, which is the safest shape for this use), and anything else with a
print mode. Set `cli_command` to the binary and `cli_args` to its argv,
with `{prompt}` and `{model}` placeholders. If no arg contains
`{prompt}`, the prompt is appended as the last argument.

## Configuration

A relative `cli_config_dir` resolves against Heron's working directory, and a
leading `~/` expands to the home directory, so the login probe and the
requests always inspect the same account.

Or per run:

```sh
HERON_PROVIDER=codex-cli heron
```

In the Mac app, Settings also shows whether the sandbox is on.

## Where the design comes from

Two shipped projects do this well, and Heron copies them rather than
inventing a third way.

**Hermes, `claude-subscription-directsdk`.** The official Nous Research
plugin (`NousResearch/hermes-plugin-claude-subscription-directsdk`) drives
the unmodified official `claude` binary as a request-scoped model client.
Hermes keeps its own agent loop, tools, approvals and compaction. The plugin
holds no credentials, never opens, copies, refreshes or prints the CLI's
credential files, rejects inherited API-key or custom-endpoint overrides
before spawning, runs each request in a private temporary directory with
native tools disabled, and kills the whole process tree on cancellation.
Heron's Claude provider copies that shape in the simpler
`claude -p --output-format stream-json` form: one turn (`--max-turns 1`),
tools off (`--tools ''`), no session persistence, `dontAsk` permission mode,
no setting sources, slash commands disabled.

**T3 Code (`pingdotgg/t3code`).** A control surface over the CLIs you
already pay for. Three rules from T3 Code shape this module:

1. Never read a harness credential. Resolve the binary, point the CLI at a
   config directory with an environment variable, and ask the CLI who it is
   logged in as (`claude auth status`, `codex login status`,
   `opencode --version`).
2. Per-instance isolation. Two accounts using the same driver do not share
   state. In Heron that is `[llm] cli_config_dir`, which sets the
   provider's config-dir variable (for example `CLAUDE_CONFIG_DIR`) for the
   child only.
3. Setup never happens as a health-check side effect. Probing a provider
   does not start a session, open a login browser, or create an account.

## How the model answers

The CLI gets the full conversation plus the tool inventory, and is told to
answer with one JSON object and to use none of its own tools:

```json
{"text": "I found two files.", "tool_calls": [{"name": "list_dir", "arguments": {"path": "."}}]}
```

Plain text with no envelope is a reply with no tool calls. Tool names not
in the request are dropped, so a CLI that invents a tool cannot reach one.
Usage tokens are read from the CLI's own JSON when it reports them;
providers that do not report usage show zero for that turn. While the
answer streams, only the reply text is shown: the envelope framing and
the tool-call arguments never reach the screen.
An explicitly selected CLI provider wins over `base_url = "demo"`, so
picking a provider never silently returns canned demo answers.

## Claude model names

Aliases follow the Hermes catalog: `sonnet` is Claude Sonnet 5.5,
`haiku` is Claude Haiku 5.5, `opus` is Claude Opus 5.5, `fable` is Claude
Fable 5.1. Those routes get the `[1m]` suffix, which selects the 1M
context window explicitly; the CLI defaults to 200K otherwise. Haiku 4.5
stays at 200K. An unrecognised model id is passed through bare, which
gives a working 200K session rather than a broken 1M one.

## Troubleshooting

- `... is not installed`: install the CLI with the hint in the error, or
  set `cli_command` to its path. `heron providers` shows install status.
- `<CLI> is logged out: run <login command>`, for example
  "Claude Code (subscription) is logged out: run `claude auth login`": run
  that command in Terminal, as the same user Heron runs as, then try again.
  Heron never logs in for you.
- `... reports version x.y.z, which Heron has not been tested with`: see
  [Version checks](#version-checks).
- `the sandbox is off, so Heron won't run <CLI>`: turn the sandbox back on
  (remove `HERON_SANDBOX=off`, or set `[agent] sandbox` back to `"auto"`).
- `... took longer than Ns and was killed`: the CLI may be waiting on a
  login or approval, or the prompt may be too long. Run the login command
  and `heron doctor` to check.
- `... refused the request` with a usage or limit message: the
  subscription's usage window is exhausted. Wait, or check the plan.
- A CLI that exits non-zero after giving a complete answer still returns
  that answer, mirroring Hermes' handling of `error_max_turns`.
- A CLI that fails after only a partial reply is an error, not a saved
  answer. The reply was incomplete, so Heron refuses to store it as if
  the turn had succeeded.
