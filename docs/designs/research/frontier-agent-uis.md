# Frontier personal-AI and agent desktop apps on macOS: UI and UX research

Date of research: 2026-11-18. Public sources only. Nothing was installed or run. No app was used by the author, so every statement describes what a document says, not what was seen on screen.

## How to read this file

- Each claim has a source URL in the same bullet or just after it.
- UNCONFIRMED means no source in this pass stated it, or sources conflict. It does not mean false.
- Source quality varies. Where a reviewer sells a competing product, that is flagged as "(competitor)". Vendor docs and changelogs are treated as the best evidence for what a feature does. Third-party reviews are the only evidence for how it feels.
- A limit of this research: OpenAI help-center and openai.com pages returned a bot challenge, so ChatGPT claims lean on OpenAI's learn.chatgpt.com docs, press coverage and third-party summaries. No screenshots were examined for any app.

## Scope note: which products are covered

- ChatGPT for Mac (new desktop app with Chat, Work, Codex; old app is now "ChatGPT Classic").
- Claude desktop (Chat plus Cowork, Code tab, computer use, quick entry).
- Perplexity (new Mac app with Personal Computer; Comet browser).
- Raycast (v2 with AI Chat, Quick AI, Projects, Automations).
- Poke (messaging-only; no Mac app found).
- Pi (no native Mac app found).
- Codex app (folded into ChatGPT desktop in July 2026, covered under ChatGPT).
- Others noted in passing: notch and menu-bar approval monitors for Claude Code (a niche, listed under "Other").

---

## 1. ChatGPT for Mac (new desktop app, July 2026)

### Product shape
- On 9 July 2026 OpenAI shipped ChatGPT Work and merged the Codex app into the desktop app. ChatGPT for Mac now has three modes: Chat, Work, Codex. The old native app is now called ChatGPT Classic. Source: https://9to5mac.com/2026/07/09/openai-announcing-the-next-chapter-for-chatgpt-today-watch-here/ and https://aitoolsreview.co.uk/insights/chatgpt-work
- The mode switcher puts Chat and Work side by side as the two primary modes, with Codex as the third. Source: https://aitoolsreview.co.uk/insights/chatgpt-work
- A 16 July 2026 refresh added Projects to the app and a single "Recents" list that mixes Chat and Work conversations, plus cross-device sync of cloud Work sessions. Source: https://aitoolsreview.co.uk/insights/chatgpt-work
- Codex mode shows more technical detail that Work hides. Source: https://daringfireball.net/linked/2026/07/09/todays-the-day-openai-fucked-up-the-chatgpt-mac-app (quoting 9to5Mac).

### Agent actions in progress and approval
- OpenAI states users decide "what it can access, when it should check in, and when it needs your approval before taking action." Source: https://www.engadget.com/2211869/openai-releases-chatgpt-work-tool-macos-windows-web-plans/
- Work plans an approach, pauses for approval on consequential actions, and lets the user redirect it mid-task. Source: https://aitoolsreview.co.uk/insights/chatgpt-work (a summary of OpenAI's help-center text; the review itself warns some claims could not be independently confirmed).
- Computer Use is invoked by typing `@Computer` for desktop apps or `@Chrome` for browser tasks. Source: https://learn.chatgpt.com/docs/computer-use (as relayed by search summary; direct page text did not render in this pass, so treat the exact `@` names as UNCONFIRMED against the page itself).
- Computer Use is per-app. The user is asked before an app is used, and can choose "Always allow" so the app is used "in the future without asking again." Persistent choices are stored in `config.toml` under an `always_allowed_app_ids` list; revoking removes the app in Settings. Source: https://greenlitbooks.com/field-notes/is-openai-chatgpt-work-computer-use-safe (quotes OpenAI docs; the page says it was read 29 Sep 2026).
- macOS system permissions (Screen Recording, Accessibility) are separate from in-app app approvals. Source: same Greenlit page, quoting OpenAI docs.
- File reads, edits and shell commands "still follow the sandbox and approval settings for the task." Source: same.
- OpenAI says the app "may also ask for permission before taking sensitive or disruptive actions", which the Greenlit author calls soft wording. Source: same.
- The user can stop the task or take over the computer at any time. "Locked Use" (use after the Mac locks) is off until enabled and installs an Apple authorization plug-in in the unlock flow. Source: same.
- Admins can restrict which apps Computer Use may reach and whether approvals can be saved. Source: same.
- Visual form of the in-progress display (progress rows, step list, live screen view): UNCONFIRMED. No source in this pass described it.
- Codex mode, from the earlier Codex macOS app: threads grouped by project in a sidebar, per-thread git worktrees, in-thread diff review with comments, plan mode, and automations that deliver results to a review queue. Sources: https://dev.to/sivarampg/openais-new-codex-mac-app-is-an-agent-command-center-and-its-lighting-up-the-devtool-wars-3aek and https://www.ctodiscovery.com/codex-app (reviews; Threads post by a developer quoted in the search result https://www.threads.com/@dimillian/post/DUTZunbir7W/).

### Memory and personalization
- Settings path reported as Settings > Personalization > Memory > Manage, with two toggles: "Reference saved memories" and "Reference chat history". Turning off saved memories also turns off chat history. Source: https://chatgptmemory.com/saved-memories-vs-reference-chat-history/ and https://openai.com/index/memory-and-new-controls-for-chatgpt/ (third-party guides; the OpenAI page itself is old and describes viewing and deleting, not editing).
- After a save, a "Memory updated" notice links to "Manage memories." Source: same search results; the exact UI text is from third-party guides, so mark as partly UNCONFIRMED.
- Chat history has no editable list, only a toggle. Temporary Chat ignores both. Source: https://chatgptmemory.com/saved-memories-vs-reference-chat-history/
- Custom-instructions character limit was raised to 5,000 on 15 July 2026. Source: https://aitoolsreview.co.uk/insights/chatgpt-work
- Whether Work uses the same memory as Chat: UNCONFIRMED.

### Home and start screens
- A unified Recents list and Projects inside the app (see above). A built-in browser and a "Sites" feature that turns output into hosted pages were added. Source: https://www.engadget.com/2211869/openai-releases-chatgpt-work-tool-macos-windows-web-plans/
- The prompt box takes `@app` mentions to name a plugin to use or pull context from, and the app recommends apps during conversations. Source: same Engadget page.
- Scheduled tasks can run once, repeat, trigger on an event, or monitor for a change. Source: https://aitoolsreview.co.uk/insights/chatgpt-work
- Layout of an empty home screen (suggested prompts, cards): UNCONFIRMED.

### Keyboard and menu bar
- The Chat Bar opens with Option+Space by default, can be rebound in Settings, and can also be opened from the menu bar icon. From it you can ask, upload files, take a screenshot or a photo. Source: https://help.openai.com/en/articles/9295241-how-to-launch-the-chat-bar (shown in search result; page itself blocked here).
- The 2024 companion window stays in front of other apps and can be expanded to the full window. Sources: https://x.com/OpenAI/status/1820914089612439622 and https://9to5mac.com/2024/08/06/multitasking-with-chatgpt-on-the-mac-just-got-a-lot-better/
- Whether Option+Space still opens the Chat Bar in the new app: UNCONFIRMED. A third-party shortcut site said it shows "pet controls" in the new app; no OpenAI source confirmed it (https://fastshortcuts.com/shortcuts/chatgpt/).
- The Voice mode in the Mac app was retired 15 Jan 2026. Source: search summary of OpenAI release notes (page blocked); UNCONFIRMED against primary text.

### Native Mac conventions
- The old ChatGPT Mac app is a native Swift app (OpenAI engineer statement quoted in a search summary; UNCONFIRMED against the original post). The new desktop app is Electron and about 1.5 GB versus 159 MB for Classic. Source: https://daringfireball.net/linked/2026/07/09/todays-the-day-openai-fucked-up-the-chatgpt-mac-app
- Codex team member on Hacker News: the app was built in Electron to support Windows and Linux. A teardown poster listed `electron-liquid-glass` for macOS vibrancy. Source: https://news.ycombinator.com/item?id=46859054
- 9to5Mac's editor wrote that Classic "looks more native Mac-like." Source: Daring Fireball page above.

### Onboarding
- Free users can try Work through the desktop app on Mac and Windows. Source: https://www.engadget.com/2211869/openai-releases-chatgpt-work-tool-macos-windows-web-plans/
- Computer Use setup: switch to Work or Codex, open Settings > Computer use, install the Computer Use plugin, grant Screen Recording and Accessibility. Source: search summary of https://learn.chatgpt.com/docs/computer-use (UNCONFIRMED against page text).
- First-run flow of the app itself: UNCONFIRMED.

### What reviewers say
- Praise: PCWorld's tester asked Work to play chess in the macOS Chess app; it moved pieces without trouble and the verdict was that clunky computer use "is pretty much over." Source: https://www.pcworld.com/article/3190769/chatgpt-can-now-control-your-whole-desktop-i-tested-it-with-chess.html (as quoted in search summary).
- Criticism: Daring Fireball's headline calls the Electron switch a mistake; size and non-native look are the stated complaints. Source: https://daringfireball.net/linked/2026/07/09/todays-the-day-openai-fucked-up-the-chatgpt-mac-app
- Criticism: three installable apps (Classic, new ChatGPT, Codex) cause confusion. Source: same.
- Usage works on a credit system shared with Codex, so a long session draws more of the plan's allowance. Source: https://aitoolsreview.co.uk/insights/chatgpt-work

---

## 2. Claude desktop (Chat + Cowork + Code tab)

### Product shape
- The Claude desktop app has Chat, Cowork and a Code tab. Cowork handles Dispatch and longer agentic work. Source: https://code.claude.com/docs/en/desktop
- Chat and Cowork are merging into one home with one message box; a toggle selects "Cowork" or "Chat" in the older layout. Source: https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork
- Cowork sessions now run in the cloud on Pro, Max and Ultra and follow the account across desktop, web and mobile. A cloud session can touch local folders only while the desktop app is open. Source: https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork (and the search summary of https://support.claude.com/en/articles/15520349-use-claude-cowork-on-web-desktop-and-mobile).
- Note for comparison: Cowork's cloud move is the opposite of a local-first design. Source: same.

### Agent actions in progress and approval (Code tab, the best documented)
- Each conversation is a session; the sidebar lists sessions and runs several in parallel. Source: https://code.claude.com/docs/en/desktop
- The row under the prompt box holds environment, project folder, permission mode and model selectors, plus a usage ring. Source: same.
- Permission modes (Manual, Accept edits, Plan, Auto, Bypass) can change mid-session through a selector; Cmd+Shift+M opens the mode menu. A mode chosen in a local session is remembered per folder. Source: same.
- The user can click stop to interrupt, or type a correction and press Enter; Claude reads it after the current action completes. Source: same.
- A tasks pane shows background work in the current session: subagents, background shell commands and workflows. Source: same.
- The app sends an OS notification when a session finishes while the user is not viewing it. Source: same.
- A side chat (Cmd+;) asks a question using the session's context without adding to the main thread. Source: same.
- Diffs can be reviewed and commented on, then the PR monitored through CI. Source: same.
- Browser actions on external sites: a permission card appears at first use with Allow once, Always allow or Deny. Safety classifiers check write actions on external pages in every mode, and a prompt appears if one flags. Source: same.

### Computer use (research preview)
- Off by default, turned on in Settings. macOS needs Accessibility and Screen Recording; the Settings page shows a status badge for each and links to System Settings. Source: https://code.claude.com/docs/en/desktop
- The first time Claude needs an app, a prompt appears in the session: "Allow for this session" or "Deny." Approval lasts the session, or 30 minutes in Dispatch-spawned sessions. Source: same.
- Access is tiered by app category and the tiers cannot be changed: View only (browsers, trading platforms), Click only (terminals, IDEs), Full control (everything else). Apps with broad reach such as terminals, Finder and System Settings show an extra warning. Source: same.
- Settings has a Denied apps list and "Unhide apps when Claude finishes." While working in the foreground Claude hides the user's other windows so only the approved app is touched. Source: same.
- Not available on Team or Enterprise plans. Source: same.

### Memory and personalization
- Memory is stored as individual topics as the user chats, not as a summary after a chat ends. The user can say "remember this." Source: https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context
- Where to see it: Settings > Your Claude > "Manage" next to "Your memory"; a modal lists what Claude remembers. Edits are made by typing into a "Tell Claude what to change or remove" box or by asking in chat. Sources: same help article and the search summary of third-party pages (https://dalimss.news/articles/claude-memory-settings-review-pause-reset-delete is a third-party page; its topic-list claims with edit and Delete icons are UNCONFIRMED against official text).
- Pause memory and reset memory exist (third-party; UNCONFIRMED in official text).
- Per-project memory: each project has its own, with a toggle "Use account memory" on the project page. Source: official help article above.
- Per-conversation off switch: "+" menu > Memory off; set when the task starts and cannot be changed later. Source: https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork
- Incognito chat via a ghost icon at the top right. Source: https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context
- Searching past chats appears in the conversation as a visible tool call. Source: same.
- Memory is shared between Chat and cloud Cowork; locally run Cowork sessions do not use memory. Source: same.

### Home and start screens
- Chat and Cowork share one home with one message box. Sources: https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork
- A "Scheduled" item in the Cowork left sidebar lists scheduled tasks; `/schedule` creates one. Source: search summary of https://support.claude.com/en/articles/15520349-use-claude-cowork-on-web-desktop-and-mobile (UNCONFIRMED against page text).
- A "Tasks" list supports delete per task through a "⋮" menu. Source: https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork
- A "Today"-style home is not described anywhere found. UNCONFIRMED.

### Keyboard and menu bar
- Quick entry: double-tap Option to open Claude from any app, a text box opens, a "New chat" click shows the five most recent conversations, screenshots and window sharing are available, and Caps Lock starts dictation. Source: https://support.claude.com/en/articles/12626668-use-quick-entry-with-claude-desktop (fetched; the voice shortcut text is from the same page).
- Setup is a first-run prompt with a "Turn on shortcut" checkbox; later managed under Settings. Source: same.
- Code tab shortcuts: Cmd+N new session, Ctrl+Tab session switch, Cmd+Shift+M mode menu, Cmd+Shift+I model menu, Cmd+Shift+E effort menu, number keys select in an open menu. Source: https://code.claude.com/docs/en/desktop
- A menu-bar item: UNCONFIRMED.

### Native Mac conventions
- One summary says Claude Desktop stays on Electron. Source: https://codenote.net/en/posts/famous-electron-apps-2026-research/ (third-party list, not verified against Anthropic).
- Panes are rearrangeable by drag; split view uses Option-click on a session in the sidebar; Cmd-click opens a session in its own window. Source: https://code.claude.com/docs/en/desktop
- Settings window layout, materials, SF Symbols: UNCONFIRMED.

### Onboarding
- Quick entry is offered through the first-run prompt after update. Source: quick-entry help page above.
- Computer use is opt-in with an explicit "Turn on" step and badges for each macOS permission. Source: https://code.claude.com/docs/en/desktop
- Dispatch pairing between phone and desktop: see https://support.claude.com/en/articles/13947068-assign-tasks-from-anywhere-in-claude-cowork. The search summary says Dispatch is being phased out; UNCONFIRMED.

### What reviewers say
- Computer use runs on the real desktop unlike the sandboxed shell tool; reviewers flag that as the main risk. Source: https://aireiter.com/blog/claude-cowork-computer-use-review (search summary only).
- Coverage disagrees on how local the work is: Cowork now runs in the cloud, so earlier articles describe it wrongly. Source: search summary of https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork
- Approval re-prompts after 30 minutes in Dispatch sessions can interrupt long tasks. Source: https://code.claude.com/docs/en/desktop

---

## 3. Perplexity (new Mac app with Personal Computer, and Comet)

### Mac app with Personal Computer
- A new Mac app launched 7 May 2026 with "Personal Computer," a hybrid local and cloud agent; it opened to Pro and Enterprise from Max-only. Sources: https://forums.macrumors.com/threads/perplexitys-new-mac-app-brings-personal-computer-to-pro-users.2482072/ and https://9to5mac.com/2026/05/07/perplexity-ai-app-introduces-all-new-native-mac-experience-for-personal-computer/
- Pressing both Command keys opens it; 9to5Mac calls the result a Command Bar. It takes text or voice and shows quick actions. Sources: https://www.macrumors.com/2026/04/16/perplexity-personal-computer-for-mac/ and search summary of the MacRumors May article.
- Files are created in a sandbox; MacRumors says its actions "can be audited and reversed" (a Perplexity claim, not tested). Source: MacRumors forum link above.
- On Mac the setup asks for Accessibility, Screen Recording and Full Disk Access, then the user picks folders. Perplexity says "every sensitive action requires explicit approval," so steps are approved mid-run. Source: https://elephas.app/resources/perplexity-computer-review (competitor: Elephas sells a rival assistant; the quoted Perplexity statement is relayed, not verified).
- Cost is shown in credits (100 credits = $1; one task 100 to 9,800 credits; default spend cap $200 on Max, adjustable). A reviewer sets a cap early to stop runaway cost. Source: same Elephas page (third-party figures; the author says they have no account).
- A forum user reported burning $20 of credits in two simple tasks. Source: https://forums.macrumors.com/threads/perplexitys-new-mac-app-brings-personal-computer-to-pro-users.2482072/page-2
- Whether the Mac app is native or Electron: the toolchase review calls the older desktop apps Electron wrappers (https://toolchase.com/blog/perplexity-ai-review/); 9to5Mac's headline says "all-new native Mac experience". Conflict: UNCONFIRMED for the new app.
- Memory UI, home screen layout, Settings window: UNCONFIRMED.

### Comet browser (sidebar assistant)
- Chromium-based browser with an assistant sidebar that sees the current page and other open tabs. Source: https://supasidebar.com/blog/comet-browser-mac-review-2026 (author sells a competing sidebar app).
- The assistant asks approval before acting in the browser and remembers the choice; options are one-time approve, always allow, deny. Sources: https://www.perplexity.ai/changelog/what-we-shipped-november-14th (changelog, fetched) and https://www.therundown.ai/tools/comet (the three-option detail).
- A "Privacy Snapshot" widget on the homepage consolidates assistant controls: which actions are allowed, blocked sites, ad blocking. Sources: same changelog and https://www.testingcatalog.com/comet-update-centralizes-data-controls-with-privacy-snapshot/
- Opening a link from an answer keeps the original thread in the sidebar. Source: same changelog.
- Reviewers split on autonomy: one calls Comet autonomy-first with review at the end; another says a visual confirmation layer exists. Source: https://www.softwareseni.com/perplexity-comet-what-an-ai-native-browser-actually-does/ (read via search summary; UNCONFIRMED which is current).
- Criticism: conventional tab strip with no Spaces; autonomous mode is hit or miss; the assistant sees only Comet tabs; CPU and memory spikes reported by PCMag via a search summary. Sources: https://supasidebar.com/blog/comet-browser-mac-review-2026 and https://www.eesel.ai/blog/perplexity-comet-reviews
- Security: researchers showed prompt-injection attacks ("CometJacking"); Perplexity added the approval model afterwards. Source: https://aionx.co/ai-comparisons/perplexity-comet-review/ (secondary; UNCONFIRMED ordering).

---

## 4. Raycast (v2 with AI Chat, Quick AI, Projects, Automations)

### Architecture and native conventions
- v2 is a rewrite. The macOS host is Swift with AppKit; it handles windows, global hotkeys and the menu bar, and loads one React and TypeScript frontend in WKWebView. Separate entry points exist per window (Launcher, AI Chat, Notes, Settings). Source: https://www.raycast.com/blog/a-technical-deep-dive-into-the-new-raycast
- On macOS Tahoe it adopted the Liquid Glass material. Source: same.
- Raycast states its needs: global hotkeys, accessibility APIs, panels that float above other apps without taking focus, translucency in inner panels. Source: same.
- macOS requirement is Tahoe and Apple Silicon (third-party). Source: search summary of https://devtoolpicks.com/blog/raycast-2-public-beta-windows-indie-hackers-2026 (UNCONFIRMED).

### Agent actions in progress and approval
- Since v2.3 (11 Sep 2026) AI Chat "works out a plan, uses your extensions, writes and runs code, checks the results, and keeps going until it's finished"; when it needs a decision it stops, asks, and resumes. Source: https://www.raycast.com/changelog/macos/2-3
- A built-in "ask user question" shows a short multiple-choice question in the conversation. Source: https://manual.raycast.com/ai/ai-chat
- Tool approval prompts "clearly explain the requested action and its impact." Tool confirmations default to Auto, and "Auto-review rules" tune Auto mode. Source: https://www.raycast.com/changelog/macos/2-3
- Pending confirmations pause a run even when follow-ups are queued or steered. While a response streams, the user can Queue a follow-up or Steer to inject it into the current run. Source: https://manual.raycast.com/ai/ai-chat
- Destructive tools carry a confirmation step before anything is replaced (example: Clipboard History rewrite). Source: https://www.raycast.com/changelog/macos/2-3
- Background notifications are off by default; a one-time alert offers to enable and reports why the chat stopped (done, needs confirmation, failed). Clicking returns to that chat. Source: https://manual.raycast.com/ai/ai-chat
- Raycast stops the computer sleeping while AI runs. Source: https://www.raycast.com/changelog/macos/2-3
- Layout of the plan and step display on screen: UNCONFIRMED.

### Memory and personalization
- Settings > AI > Personalization > Memory. The model reads and writes durable facts as the user chats. Source: https://manual.raycast.com/ai/personalization (via search summary of the manual; UNCONFIRMED against page text).
- Every few hours Raycast sends recent chats and the current memory to a model that rewrites a summary. Source: same.
- Controls: Show Memory (review and edit), Import, Add to Memory, Generate or Regenerate, Reset Memory, and a master off switch. Source: same.
- Each Project has its own memory with a "Show Memory" button, plus an optional working directory such as an Obsidian vault. Sources: https://www.raycast.com/changelog/macos/2-3 and the manual page above.
- Memory is stored locally and syncs encrypted through Cloud Sync when enabled. Source: manual page via search summary.
- A separate "Profile" is text the user writes about role and style (Settings > AI > Profile). Source: same.
- Automations example in the changelog: "update Raycast's memory from their meeting notes every evening." Source: https://www.raycast.com/changelog/macos/2-3

### Home and start screens
- Root Search is the start screen. Typing then Tab sends the question to Quick AI in place; Cmd+J moves it to AI Chat with history. Sources: https://manual.raycast.com/ai/quick-ai and https://manual.raycast.com/ai/ai-chat
- AI Chat has a history sidebar with search, a Recently Deleted area (60 days) and Cmd+F in-chat find. Source: https://www.raycast.com/changelog/macos/2-3
- Send to AI commands push a focused window, selected text, a screen area or the screen into the active chat. Source: https://manual.raycast.com/ai/ai-chat
- The `@` mention menu orders by frecency; the `/` menu picks models. Source: https://www.raycast.com/changelog/macos/2-3
- Automations: Once, Hourly or Daily prompts that post into a new or existing chat; created manually, with AI, or via `@automations`; AI drafts then asks the user to confirm before saving. Source: https://manual.raycast.com/ai/automations

### Keyboard and menu bar
- Raycast is a global-hotkey launcher; commands take their own hotkeys (v2.11 suggests Control+A for Dictate to AI). Source: https://www.raycast.com/changelog/macos/2-11
- AI Chat opens in its own window with "Always on Top." Source: https://manual.raycast.com/ai/ai-chat
- A menu-bar command shows AI usage (Settings > AI > Show AI Usage in Menu Bar). Source: https://www.raycast.com/changelog/macos/2-11
- Chat keys: Return sends, Shift+Return new line, Up edits the last message, Cmd+R regenerates, Cmd+N new chat, Ctrl+M dictation. Source: https://manual.raycast.com/ai/ai-chat
- A pinned Notes window stays visible across Spaces. Source: https://www.raycast.com/changelog/macos/2-11

### Onboarding
- Existing users can import their v1 setup during onboarding. Source: search summary of third-party page https://devtoolpicks.com/blog/raycast-2-public-beta-windows-indie-hackers-2026 (UNCONFIRMED).
- First-run steps for AI: UNCONFIRMED.

### Cost and models
- Plans moved from rate-limited to usage-based on 11 Sep 2026; a Max plan was added. Users can connect a Claude or ChatGPT subscription or their own keys. Settings > AI > Models & Providers. Source: https://www.raycast.com/changelog/macos/2-3

### What reviewers say
- No independent review of the v2 AI Chat was found in this pass. UNCONFIRMED. The Raycast sources above are all first party.
- A third-party piece says Raycast "doesn't want to be just a launcher anymore". Source: https://kurtis-redux.medium.com/raycast-doesnt-want-to-be-just-a-launcher-anymore-904da0663986 (not read in full).

---

## 5. Poke (Interaction Company; now part of Cognition)

- No Mac app was found. Poke lives in Messages, WhatsApp and Telegram. Source: https://poke.com/
- Product claims: integrations (Notion, Gmail, Oura and others), "Recipes" for setting up integrations and automations, reminders by text or voice, proactive use of integrations and memory. Source: https://poke.com/
- Pricing: Free, Pro $19 a month; Saner.ai also lists an Ultra tier at $199. Sources: https://poke.com/ and https://blog.saner.ai/poke-reviews/ (competitor).

### Agent actions and approval
- A leaked system prompt (third-party, not official) says Poke must get user confirmation before sending, forwarding or replying to email and always show drafts first. Source: https://www.systemprompts.org/poke-system-prompt/ (UNCONFIRMED as current behavior).
- How in-progress work appears in the chat thread: UNCONFIRMED.

### Memory
- Long-term memory is conversational. Saner.ai says there is "no memory layer you can actually use" and no task list or searchable notes. Source: https://blog.saner.ai/poke-reviews/ (competitor: Saner sells a notes and task product).
- Product Hunt reviewers praise it keeping context. Source: https://www.producthunt.com/p/poke-by-interaction-co/a-week-with-poke-review-a-promising-start-for-a-proactive-ai-assistant
- Memory view or edit UI: UNCONFIRMED; no source found.

### Home screen
- None; the thread is the home. Proactive pushes arrive as texts, such as a 7 a.m. overview with calendar events and important emails. Source: https://www.notis.ai/blog/poke-ai-review/ (competitor) and https://www.productpep.com/blog/2025/11/16/its-finally-cool-to-poke
- Saner.ai: "Poke's morning message is a digest, not a plan." Source: https://blog.saner.ai/poke-reviews/

### Onboarding
- Sign-up starts by texting Poke. An earlier "bouncer" persona negotiated the price in onboarding, replaced by flat tiers. Sources: https://blog.saner.ai/poke-reviews/ and https://www.notis.ai/blog/poke-ai-review/
- Saner.ai scores setup at 4.8 of 5 (a phone number and one email, under two minutes) and onboarding experience 2.8 of 5. Source: https://blog.saner.ai/poke-reviews/
- A user reported abandoning the first attempt over data-access worry. Source: same.

### Reviewers
- Praise: texting UI, proactivity (Productpep graded the interface A+). Source: https://www.productpep.com/blog/2025/11/16/its-finally-cool-to-poke
- Criticism: date and time-zone mistakes, a reminder an hour late, slow third-party integrations, no Google Workspace. Source: https://blog.saner.ai/poke-reviews/ and https://www.producthunt.com/p/poke-by-interaction-co/a-week-with-poke-review-a-promising-start-for-a-proactive-ai-assistant
- Relevance to the Heron Telegram channel: Poke shows proactive messages work on a chat channel, and also what that channel lacks (a searchable memory and task list). Source: Saner.ai page above.

---

## 6. Pi (Inflection)

- No native Mac app was found. Reviews cover mobile and web. Source: https://www.theaidirectory.com.au/review-pi-inflection-australia-2026/ and https://delv.tools/agents/pi-ai (search summaries; the absence of a Mac app is UNCONFIRMED, not proven).
- 2026 additions are light practical tools (reminders, checklists, shareable plans) per App Store release notes, and "Pi Journeys" structured memory announced July 2026. Source: search summary of https://www.theaidirectory.com.au/review-pi-inflection-australia-2026/ (UNCONFIRMED against primary text).
- Memory: sources conflict. One says Pi has no long-term memory; another says it remembers across sessions. Source: https://delv.tools/agents/pi-ai versus https://toolcurrent.com/tools/pi
- G2 rates it 4.2 of 5 from 11 reviews, praising ease of use and empathy, with weakness in long-conversation context. Source: https://www.g2.com/products/pi/reviews
- One review says the app is in maintenance mode. Source: https://www.vgvstudio.com/2026/09/pi-ai-review-is-inflections-free-assistant-worth-it.html
- Little here is useful for a Mac agent UI beyond tone. Approval, home, keyboard and native sections: UNCONFIRMED / not applicable.

---

## 7. Other items noted

- Claude Code monitors in the notch or menu bar approve tool calls from outside the terminal and show usage limits (for example "Claude Pulse"). The source is a vendor list that ranks its own product; use only as evidence the pattern exists. Source: https://vibeisland.app/best-ai-agent-notch-apps/
- Local-first Mac agents (Dottie, OpenAGI, Jan, Ollama front ends) appear in roundups written by vendors of competing products; none was reviewed independently. Sources: https://www.dottie.ai/blog/best-ai-assistants-mac/ and https://blog.buildbetter.ai/best-personal-ai-agents-that-run-locally-on-your-mac-in-2026-2/ . Treat as leads only.
- A roundup lists "Siri AI built into macOS 27" as a free Apple option. Source: https://www.usecarly.com/blog/best-ai-assistant-for-mac/ (competitor; UNCONFIRMED). Worth a separate pass if Apple's system agent matters for the design.

---

## Cross-app comparison (confirmed items only)

| Topic | ChatGPT | Claude | Perplexity | Raycast | Poke |
|---|---|---|---|---|---|
| Per-app computer-use grant | Allow, Always allow, stored in config | Allow for session or Deny, fixed tiers | Folder and permission grants | Not applicable | Not applicable |
| Memory edit surface | Settings > Personalization > Manage (delete) | Settings > Your Claude > Manage, edit by instruction | UNCONFIRMED | Show Memory dialog, Generate, Reset | UNCONFIRMED |
| Per-project memory | Projects (memory scope UNCONFIRMED) | Yes, with toggle | UNCONFIRMED | Yes | No |
| Global entry | Option+Space (old app) | Double-tap Option | Both Command keys | Launcher hotkey | Text message |
| Scheduled work | Yes | Yes (cloud) | UNCONFIRMED | Once, Hourly, Daily | Recipes |
| Cost visibility | Credits, shared with Codex | Usage ring in Code tab | Credits and spend cap | Menu-bar usage item | Plan tiers |
| UI stack | Electron (new), Swift (Classic) | Electron (UNCONFIRMED) | UNCONFIRMED | Swift/AppKit shell plus WKWebView | Messages |

---

## Gaps to close if this matters

- No screenshots or videos were reviewed, so layout, spacing, material use, toolbar contents and Settings window structure are unconfirmed for every app.
- OpenAI help-center text was unreadable here; ChatGPT claims lean on secondary summaries.
- A hands-on pass on a Mac with each app (outside this research brief) would settle the UNCONFIRMED items.

---

## Patterns worth borrowing (ranked)

Ranking weighs fit with Heron's existing features (permissions, activity log, memory, Today, Telegram, automations, usage) and how strongly the sources support the pattern.

1. **Per-app, tiered approval with a visible scope and expiry** (Claude desktop). Fixed access tiers by app category (view, click, full), a stated expiry, extra warnings for broad-reach apps, a deny list, and an "Always allow" that stays revocable (ChatGPT stores it in a file and a Settings list). Maps directly to Heron's permission system. Sources: https://code.claude.com/docs/en/desktop and https://greenlitbooks.com/field-notes/is-openai-chatgpt-work-computer-use-safe
2. **Approval prompts that state the action and its impact, with a default of Auto and user-tunable rules** (Raycast). Pairs an explanatory prompt with "Auto-review rules" and a pause that survives queued messages. Source: https://www.raycast.com/changelog/macos/2-3 and https://manual.raycast.com/ai/ai-chat
3. **Memory as a reviewable, editable document with per-project scope and a per-conversation off switch** (Claude desktop and Raycast). Claude: topic-based memory, a "Manage" modal with a plain-language edit box, per-project memory, a "Memory off" toggle for one task, a visible tool call when memory is searched. Raycast: Show Memory, Generate, Reset, Import, and memory kept locally. Sources: https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context and https://manual.raycast.com/ai/personalization
4. **Global quick entry that grows into the full window** (Claude, Raycast, ChatGPT). Double-tap Option (Claude) with a recent-chats list, screenshot and window capture, dictation; Raycast's Tab to Quick AI then Cmd+J to promote to AI Chat; ChatGPT's Chat Bar from Option+Space or the menu bar. Sources: https://support.claude.com/en/articles/12626668-use-quick-entry-with-claude-desktop , https://manual.raycast.com/ai/quick-ai , https://help.openai.com/en/articles/9295241-how-to-launch-the-chat-bar
5. **Background run with a notification that says why it stopped** (Raycast, Claude). Raycast: off by default, a one-time opt-in alert, text of done, needs confirmation or failed, click returns to the chat; Claude sends an OS notification when an unviewed session finishes. Sources: https://manual.raycast.com/ai/ai-chat and https://code.claude.com/docs/en/desktop
6. **Automation created by plain language, then confirmed before saving** (Raycast). The AI drafts schedule, destination and prompt; the user confirms. Output lands in a normal chat, so history and approvals stay one system. Source: https://manual.raycast.com/ai/automations
7. **Ask-the-user multiple-choice question inline in the conversation** (Raycast). Source: https://manual.raycast.com/ai/ai-chat
8. **Steer or queue while a run is active** (Raycast); and **stop plus type-a-correction** (Claude). Source: https://manual.raycast.com/ai/ai-chat and https://code.claude.com/docs/en/desktop
9. **Side chat that reads the thread but writes nothing back** (Claude, Cmd+;). Useful for asking about an activity-log entry without derailing a task. Source: https://code.claude.com/docs/en/desktop
10. **Usage visible where work happens** (Claude's usage ring beside the prompt; Raycast's optional menu-bar usage item; Perplexity's credit cost and spend cap). Source: https://code.claude.com/docs/en/desktop , https://www.raycast.com/changelog/macos/2-11 , https://elephas.app/resources/perplexity-computer-review
11. **Privacy summary widget on the home page** (Comet's Privacy Snapshot): one place to see and change what the agent may do. Fits a Today screen or a Settings overview. Sources: https://www.perplexity.ai/changelog/what-we-shipped-november-14th and https://www.testingcatalog.com/comet-update-centralizes-data-controls-with-privacy-snapshot/
12. **Proactive messages as a first-class feature on the chat channel** (Poke): a morning overview that includes calendar and important email. Heron's Telegram channel and Today screen can reuse the same content. Counterpoint from reviewers: a digest is not a plan, and a thread has no searchable task list. Sources: https://www.notis.ai/blog/poke-ai-review/ and https://blog.saner.ai/poke-reviews/
13. **Native host with web content is a trade, not a default** (Raycast chose a Swift host for hotkeys, floating panels and Liquid Glass; OpenAI's switch to a 1.5 GB Electron app drew public criticism). For a SwiftUI app this supports staying native. Sources: https://www.raycast.com/blog/a-technical-deep-dive-into-the-new-raycast and https://daringfireball.net/linked/2026/07/09/todays-the-day-openai-fucked-up-the-chatgpt-mac-app

### Anti-patterns seen in the sources

- Three overlapping apps after a merge (ChatGPT Classic, ChatGPT, Codex). Source: https://daringfireball.net/linked/2026/07/09/todays-the-day-openai-fucked-up-the-chatgpt-mac-app
- Approvals that expire during long tasks and re-prompt (Claude Dispatch sessions, 30 minutes). Source: https://code.claude.com/docs/en/desktop
- Vague permission wording ("may also ask for permission"). Source: https://greenlitbooks.com/field-notes/is-openai-chatgpt-work-computer-use-safe
- A chat-only memory with no queryable task list (Poke). Source: https://blog.saner.ai/poke-reviews/
- Credit costs that surprise users (Perplexity forum report). Source: https://forums.macrumors.com/threads/perplexitys-new-mac-app-brings-personal-computer-to-pro-users.2482072/page-2
