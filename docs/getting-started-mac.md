# Getting started on a Mac

This guide takes you from a Mac without Heron to a first reply in which Heron
used one of its tools. Plan on up to 30 minutes, most of it installing and
logging in to a CLI. You never type a password or key into Heron itself.

Heron v1 is a **research preview**. Read the
[accepted risks](ACCEPTED_RISKS.md) before you start; Heron shows you the same
list and won't continue until you accept it.

## What you need

- A Mac with macOS 14 (Sonoma) or later.
- A subscription for one of the Tier 1 CLIs: Claude Pro or Max (Claude Code),
  ChatGPT Plus, Pro or Team (Codex), or an OpenCode setup. See
  [CLI providers](cli-providers.md) for the full list and what each costs.
- Node.js and npm, to install Claude Code or Codex. If `npm --version` prints
  nothing in Terminal, install Node first (for example `brew install node`).

## 1. Install a CLI and log in

This guide uses Claude Code. For Codex or OpenCode, use the install and login
commands from the [Tier 1 table](cli-providers.md#tier-1-clis) instead.

1. Open Terminal and install Claude Code:

   ```sh
   npm install -g @anthropic-ai/claude-code
   ```

2. Log in. This opens your browser; finish the login there.

   ```sh
   claude auth login
   ```

3. Check the version:

   ```sh
   claude --version
   ```

   Heron is tested with Claude Code `>=2.1.327, <3.0.0`. A version outside
   that range still works, but Heron asks you first (see
   [Messages you may see](#messages-you-may-see)).

## 2. Install Heron

1. Download `Heron-macos-universal.dmg` from the
   [releases page](https://github.com/fernandogarzaaa/HERON-Project/releases).
2. Open the DMG and drag **Heron** onto **Applications**.
3. Open Heron from Applications. The build is signed and notarized, so it
   opens with a normal double-click. You don't need the right-click "Open"
   workaround.

## 3. Accept the risk notice

On first launch Heron shows its research-preview risk notice. It lists each
accepted risk by its ID (C35-8 and R-1 in this version) and tells you whether
the sandbox is on. Read it, then accept it.

Heron runs nothing until you accept: no chat, no schedules, no phone messages.
If the notice changes in a later version, Heron shows it again. In a terminal,
`heron` shows the same notice and asks y/n.

## 4. Get a first reply

1. Heron finds the logged-in Claude Code on its own and checks its version. If
   it doesn't, pick **Claude Code** as the provider in Settings.
2. In the chat, type:

   > List the files in my working folder.

3. Heron uses its `list_dir` tool and replies with the list. The activity track
   beside the reply shows the tool call, the provider and the CLI version.

Your working folder is `~/Heron` unless you choose another one in Settings.
Heron reads and changes files there; it treats the rest of your Mac more
carefully, as the next section explains.

## What Heron asks before doing

Reading a file outside the working folder now asks you first.

That is new in v1: earlier versions let every mode read anywhere. In detail:

- **Inside the working folder,** Heron reads without asking. Writes and
  commands ask in Ask mode, the default.
- **Elsewhere in your home folder,** and on other users' folders or external
  drives, a read asks, in every mode except Yolo. When you approve, you choose
  **once**, **this conversation**, or **always for this folder**. You can
  revoke a folder in Settings.
- **Files that look like secrets,** such as `.env` files, keys and credentials,
  ask in every mode, the working folder included.
- **Some places are never read:** Heron's own folder (`~/.heron`), your CLIs'
  login folders, the Keychain, and your browser, Mail, Messages and Safari
  data. No approval opens them.
- **After outside content enters a conversation,** such as a web page, an email
  or a calendar entry, anything that changes something or sends data out asks
  first.

Shell commands run inside macOS's sandbox. They can read the working folder,
system folders, the toolchain folders builds need, and folders you granted, but
not the rest of your home folder. A command that tries gets an error that tells
the model to use `read_file`, which can ask you.

## macOS privacy prompts

macOS protects some folders, such as Desktop, Documents and Downloads, and
asks before an app reads them. Heron's CLI and the commands it runs are child
processes of Heron, so expect macOS to show these prompts as coming from
**Heron**, even when Heron's CLI or a shell command touched the folder.

- **Allow** if you asked Heron to work with files in that folder.
- **Don't Allow** if you didn't expect it. Heron then can't read that folder,
  and the read fails with an error.
- You can change your answer later in **System Settings → Privacy & Security →
  Files and Folders**.

Which prompts appear on a given Mac hasn't been recorded yet. Your answer to a
macOS prompt doesn't replace Heron's own approvals: Heron still asks before
reading outside the working folder.

## Messages you may see

| Message | What it means | What to do |
| --- | --- | --- |
| "Claude Code (subscription) is logged out: run `claude auth login`" | The CLI's login has expired or was removed. | Run the command in Terminal, finish the login in your browser, then send your message again. |
| "claude-cli reports version 3.0.0, which Heron has not been tested with…" | The CLI updated to a version Heron hasn't been tested with. Nothing has run through it. | Continue with this version from the prompt, or install a tested version. Heron asks again when the version changes. |
| "Claude Code (subscription) is not installed…" | Heron can't find the CLI. | Install it (step 1), or set its path in Settings. |
| "the sandbox is off, so Heron won't run Claude Code (subscription)" | The macOS sandbox is turned off, so Heron refuses to start CLIs and shell commands. | Turn the sandbox back on: remove `HERON_SANDBOX=off`, or set `[agent] sandbox = "auto"`. |
| "Another Heron is running your schedules and phone connection" | A `heron serve` you started yourself is already running. | Nothing. The app works normally; the other process keeps running your schedules. |
| "the working folder is inside …, which Heron never reads" | The working folder you chose is inside a folder Heron never reads. | Choose another working folder in Settings. |
| "Waiting for you: … needs the model" in the inbox | A schedule or other unattended job waited instead of using your subscription. | Run it now from its page, or allow unattended use (`allow_unattended_subscription = true` under `[llm]`). See [Subscriptions and cost](cli-providers.md#subscriptions-and-cost). |

## Next steps

- Name the agent, pick an avatar and a theme in Settings.
- Read [CLI providers](cli-providers.md) to switch to Codex or OpenCode, or to
  use a second account.
- Read the [accepted risks](ACCEPTED_RISKS.md) again before pairing Telegram.
