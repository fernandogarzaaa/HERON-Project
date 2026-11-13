# Model providers: subscriptions through the official CLIs

Heron's default model path is an API key against an OpenAI-compatible
endpoint (`[llm] base_url`). This page covers the other path: run on a
subscription you already pay for, through that subscription's official CLI.
Heron never reads, copies, or prints a CLI token. Login happens in the CLI
itself, on your own machine, before Heron ever spawns it.

Check what is installed and logged in:

```sh
heron providers
heron doctor        # also exercises the selected provider with one real call
```

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
Despite the name, it speaks native `stream-json` directly and does not
require the Python Agent SDK package. Heron's Claude provider copies that
shape in the simpler `claude -p --output-format stream-json` form: one turn
(`--max-turns 1`), tools off (`--tools ''`), no session persistence,
`dontAsk` permission mode, no setting sources, slash commands disabled.
What Heron does not copy is Hermes' loopback admission relay, which exists
to hold the CLI to exactly one upstream request and to keep prompt-cache
prefixes stable. If a CLI version ever retries a turn on its own, the extra
turn still runs in the same throwaway directory with no tools, so the blast
radius stays small.

**T3 Code (`pingdotgg/t3code`).** A control surface over the CLIs you
already pay for: Claude Code, Codex, Cursor, Grok Build, OpenCode,
Antigravity, Kiro, Pi, Muse Code, fx, and ACP Registry agents. Three rules
from T3 Code shape this module:

1. Never read a harness credential. Resolve the binary, point the CLI at a
   config directory with an environment variable, and ask the CLI who it is
   logged in as. Claude is probed with `claude --version` (plus an SDK
   probe in T3; `claude auth status` here, which reads the local credential
   store and sends no model request). Codex is probed with
   `codex app-server` `account/read` (here: `codex login status`, which
   reads the cached login token). OpenCode is probed with
   `opencode --version` plus the server's provider inventory.
2. Per-instance isolation. Two accounts using the same driver do not share
   state. In Heron that is `[llm] cli_config_dir`, which sets the
   provider's config-dir variable (for example `CLAUDE_CONFIG_DIR`) for the
   child only.
3. Setup never happens as a health-check side effect. Probing a provider
   does not start a session, open a login browser, or create an account.

## Safety rules

1. Heron never reads, copies, or prints a CLI token. Login is yours, in
   the CLI, on your machine.
2. The CLI runs in a fresh, empty, private temporary directory under
   `~/.heron/tmp/cli-*`, never in your workspace. Even a CLI that ignores
   the no-tools instruction can only touch throwaway files there.
3. No provider is ever spawned with an auto-approve flag (`--yolo`,
   `--force`, `--dangerously-skip-permissions`, `--allow-all-tools`).
   The CLI is a text model for Heron, not a second agent. Heron owns the
   tool loop, permissions, sandbox, audit log and taint tracking, exactly
   as it does for the HTTP path.
4. Subscription billing is never silently flipped to metered API billing.
   For subscription providers the child environment has the API-key
   variables removed, so a stray `OPENAI_API_KEY` cannot turn a ChatGPT
   subscription turn into a metered API call. Set
   `cli_allow_api_key = true` only if you mean it.
5. Cline and Goose run their own tool loop in headless mode and cannot be
   used as a plain text model. They refuse to run unless you set
   `cli_allow_agent_tools = true` explicitly. That flag is a deliberate,
   visible choice, and the CLI still runs in a throwaway directory.
6. Background work (reflections, memory extraction, scheduled jobs) uses
   the same provider as chat, so it draws on the same subscription usage.
   Keep that in mind before turning on proactive features with a
   subscription provider.
7. Cancellation and timeout kill the whole provider process tree, not
   just the CLI process, so a wrapper cannot leave a model call running
   after you cancelled it. On Unix the CLI runs in its own process
   group; on Windows only the direct child is killed. A CLI that
   daemonizes will have its daemons killed when the request ends.

## Providers

| Provider id | What it is | Login | Cost |
| --- | --- | --- | --- |
| `claude-cli` | Claude Code | `claude auth login` | Claude Pro/Max usage window |
| `codex-cli` | Codex CLI | `codex login` | ChatGPT Plus/Pro/Team plan |
| `cursor-cli` | Cursor agent CLI | `agent login` | Cursor plan usage pool |
| `grok-cli` | Grok Build | `grok login` | Grok plan |
| `opencode-cli` | OpenCode | `opencode auth login` or `opencode.json` | Free with a free/local model, else upstream price |
| `antigravity-cli` | Antigravity CLI (`agy`) | Sign in with Google | Google pricing for the method |
| `cline-cli` | Cline CLI | `cline auth <provider>` | Free with a free/local model. Needs `cli_allow_agent_tools = true` |
| `copilot-cli` | GitHub Copilot CLI | `copilot`, then `/login` | Copilot plan credit pool |
| `qwen-cli` | Qwen Code | `qwen`, then `/auth` | Paid key or local Ollama/vLLM. Free Qwen OAuth ended 2026-04-15 |
| `gemini-cli` | Gemini CLI | Pick Gemini API Key | API-key free tier. Google OAuth free tier ended 2026-06-18, use Antigravity for Google login |
| `pi-cli` | Pi | `pi`, then sign in | Whatever Pi is configured with |
| `fx-cli` | fx | `fx login` | Whatever fx is configured with |
| `kiro-cli` | Kiro CLI | `kiro-cli`, then sign in | Whatever Kiro is configured with |
| `muse-cli` | Muse Code | `muse`, then sign in | Whatever Muse is configured with |
| `goose-cli` | Goose | `goose configure` | Free with a local model. Needs `cli_allow_agent_tools = true` |
| `custom-cli` | Anything else | The CLI's own docs | The CLI's own pricing |

Providers marked experimental (`grok-cli`, `pi-cli`, `fx-cli`, `kiro-cli`,
`muse-cli`, `goose-cli`) have flags that are best effort and not yet verified
against a live binary. Check `binary --help` before relying on one, and
report what you find so the registry can be corrected.

The generic `custom-cli` covers what is not listed: Crush, Amp, Kimi,
Droid, Simon Willison's `llm` (a clean text-in, text-out CLI with no tool
loop, which is the safest shape for this use), and anything else with a
print mode. Set `cli_command` to the binary and `cli_args` to its argv,
with `{prompt}` and `{model}` placeholders. If no arg contains
`{prompt}`, the prompt is appended as the last argument.

## Configuration

```toml
[llm]
provider = "claude-cli"
cli_model = "sonnet"   # sonnet | haiku | opus | fable, or a full model id; empty = CLI default
# cli_command = "/opt/homebrew/bin/claude"   # only if the binary is not on PATH
# cli_config_dir = "~/.claude_personal"      # a second account, isolated from the first
```

Or per run:

```sh
HERON_PROVIDER=codex-cli heron
HERON_PROVIDER=opencode-cli HERON_CLI_MODEL=anthropic/claude-sonnet-5.5 heron
```

In the web UI, Settings has a Provider dropdown plus CLI command and CLI
model fields. Switching provider there hot-swaps the model client, the
same as changing the endpoint does.

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
the tool-call arguments never reach the terminal or the web chat.
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

- `... is not installed` — install the CLI with the hint in the error, or
  set `cli_command` to its path. `heron providers` shows install status.
- `... is not logged in` — run the login command from the table above, as
  the same user Heron runs as, then try again. Heron never logs in for you.
- `... took longer than Ns and was killed` — the CLI may be waiting on a
  login or approval, or the prompt may be too long. Run the login command
  and `heron doctor` to check.
- `... refused the request` with a usage or limit message — the
  subscription's usage window is exhausted. Wait, or check the plan.
- A CLI that exits non-zero after giving a complete answer still returns
  that answer, mirroring Hermes' handling of `error_max_turns`.
- A CLI that fails after only a partial reply is an error, not a saved
  answer. The reply was incomplete, so Heron refuses to store it as if
  the turn had succeeded.
- Terms: subscription logins in third-party tools sit in a grey area of
  Anthropic's terms, and each vendor's terms differ. This is your account
  on your machine, through the vendor's own unmodified CLI, with no
  credentials copied anywhere. Read the vendor's terms before relying on
  it, and disable extra usage in the account if you do not want overage
  billing.
