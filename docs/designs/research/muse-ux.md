# Meta Muse: UI and UX research

Purpose: design inspiration for a local-first personal agent app for macOS in SwiftUI. Principles and patterns only. No assets, copy or character designs are to be reused.

Method: public sources only (news, reviews, Meta's help centre text as quoted by search results, podcast write-ups, tweets embedded in articles). The app was not downloaded, installed or reverse-engineered. A GitHub "teardown" of the macOS app turned up in search; it was deliberately not read or used.

Read-through limits: I read the pages for sources 1-12 below in full as text. Meta's help page and TechCrunch pages could not be loaded (the help page returned only a title; TechCrunch returned a bot block), so claims that rest on them are taken from search-result summaries and are marked "(snippet)". I did not watch any video. Several sources are small blogs; Wikipedia's Muse article carries a banner saying it may contain LLM-written text, so I used it only for facts that other sources repeat.

## Sources

1. Engadget, hands-on setup guide: https://www.engadget.com/2256577/how-to-get-started-with-meta-s-new-ai-agent-muse/
2. Clear Mud, live-test review: https://www.clearmud.ai/blog/my-first-impressions-on-muse-ai-by-meta-does-it-earn-a-spot
3. Edapt, summary of Meta's launch and technical post: https://www.edapt.me/blogs/meta-muse-personal-ai-agent-explained
4. Lenny's Newsletter / How I AI, interview with Muse's design director: https://www.lennysnewsletter.com/p/how-i-ai-how-meta-designed-muse-with
5. ChatPRD write-up of the same interview: https://www.chatprd.ai/how-i-ai/alex-cornell-meta-muse-workflows
6. iMasters, design decisions article: https://imasters.com/design-ux/how-meta-designed-muse-to-not-feel-like-an-ai-app
7. Digital Trends, Spaces launch: https://www.digitaltrends.com/phones/metas-now-lets-you-build-custom-mini-app-experiences-in-the-muse-app/
8. Yahoo Tech, Realtime Avatar launch: https://tech.yahoo.com/ai/meta-ai/articles/meta-rolls-real-time-video-160805716.html
9. Yahoo Tech, Apple Watch app: https://tech.yahoo.com/ai/meta-ai/articles/meta-adds-muse-ai-agent-160712339.html
10. AI Weekly, summary of Wired on Jolly: https://aiweekly.co/alerts/metas-muse-ships-an-18-ai-agent-behind-a-kids-toy-mascot
11. Complete AI Training, Jolly commentary: https://completeaitraining.com/news/metas-new-ai-avatar-feels-infantilising-as-the-company/
12. AI Agents Library, 10-task review (Mac app): https://www.aiagentslibrary.com/blog/meta-muse-review/
13. Memeburn, permission failures: https://memeburn.com/metas-muse-sent-a-stranger-to-a-users-door-its-permission-settings-explain-why/
14. AIToolHunt, Mac control source review (no hands-on test): https://aitoolhunt.co/blog/meta-muse-for-mac-computer-agent-2026
15. Wikipedia, Muse (AI agent): https://en.wikipedia.org/wiki/Muse_(AI_agent)
16. Meta Help Centre, "How Muse works with your guidance and approval" (snippet only): https://www.meta.com/help/artificial-intelligence/1385290430137537/
17. TechCrunch, "Everything new coming to Meta's AI agent Muse" (snippet only): https://techcrunch.com/2026/09/23/everything-new-coming-to-metas-ai-agent-muse/
18. TechCrunch, Apple Watch and Wear OS (title and date from search listing only): https://techcrunch.com/2026/11/10/metas-ai-agent-muse-lands-on-apple-watch-and-wear-os/

## Platforms and timeline

- Muse was announced on 8 September 2026 for iOS, Android and the web at muse.ai, US only, ages 18 and over. It is a separate app from Meta AI. [1][15][10]
- At launch there was no Mac or Windows desktop app. [2] Meta announced Mac app control at the 23 September event. [15][17 snippet] A Mac launch post dated 18 September is cited by one small blog. [14]
- Realtime Voice and Realtime Avatar launched 5 November and work on mobile, web and Mac; conversations start from a phone icon in the top-right corner. [8]
- Spaces launched 6 November. [7]
- An Apple Watch companion app (text, dictation, calling, and a watch-face complication) was reported 9-10 November. [9][18]
- iPad app: UNCONFIRMED. One search summary said a dedicated iPad app shipped about a month before the watch app, but I found no page I could read that says so.

## Onboarding: name, avatar, how it talks

- After login, one of the first things Muse asks is for the user to rename the agent. The Engadget reviewer named theirs "boo", switched the avatar to a cat, and said it made no practical difference. [1]
- On web, entering a phone number was enough to log in, and relevant Meta accounts were already linked. [1]
- A Meta account is required; Facebook or Instagram accounts are not. [1]
- Reviewers describe onboarding as friendly and clear, with no empty workspace to figure out. [12] Setup took a few chat exchanges rather than a settings menu. [search summary of MindStudio, https://www.mindstudio.ai/blog/meta-muse-ai-agent; page not read]
- Settings items are mostly prompts, not forms. In Clear Mud's test, "change avatar" and "edit name" did not open a form; they pre-filled a chat message to the agent. Routines had no obvious button either; the user asks in chat. The reviewer disliked the token cost of this but supported the idea of asking the agent to change its own settings. [2]
- Avatar customisation through chat was slow in that test: three prompts to get a custom avatar built from a YouTube channel image, and the animated version of a custom avatar produced unwanted sparkles and a galaxy fade. [2]
- The reviewer's identity file (how the agent sees itself and the user) was editable in-app. [2]
- Muse, per Meta's design director, keeps one main persistent chat with the agent plus scoped "side chats". He called the line between main chat and side chats still unclear to users. [4][6]
- "How it talks": Axios (as relayed by Edapt) reports users can adjust how the agent communicates. [3] Voice settings are speed, style and accent. [8] Wang's post says users can "prompt exactly how it sounds". [8]
- Default permission and data choices sit in Settings, not in onboarding. Engadget advised opting out of model training first (Settings, data controls). [1]
- A Mac setup screen offered Full Disk Access or skip; the reviewer called that a broad request at the moment the user knows least. The screen said access could be narrowed later. [12]

## Avatar and animation

- The default character, called Jolly (originally "Jollybot"), is cream-coloured with beady black eyes, an upward smile, soft pink cheeks and plush ivory fur; Wired and others say it borrows from Labubu-style collectible toys. It was designed by Alex Cornell. [10]
- Meta copy describes it as "completely customizable" in character and personality. [10]
- Idle, thinking and speaking states, as individual named animations: UNCONFIRMED. What is documented:
  - Clear Mud: the default avatar looks at the user "like a portrait", then shifts into a scene where it types at a desk while working. [2]
  - ChatPRD: avatars "type on tiny laptops". [5]
  - Cornell: avatar animations appear at the top of the app while Muse works. The agent's status shows as an emoji (the "status pill"). [4][search summary of Cornell posts; X page not read: https://x.com/alexcornell/status/2107135171313209607]
  - Reviewer: emoji reactions "wore thin" and distracted during serious work; they wanted a setting to turn them off. [12]
- Customisation: the user can make the avatar a person, an animal or another character. [8] Wang demoed a fluffy white dog. [8] Cornell's own agent is a cactus named Marlowe. [5]
- A small blog reports a yeti on the watch and says "yeti". I treat this as UNCONFIRMED. https://www.newsbytesapp.com/news/science/meta-brings-muse-ai-yeti-to-apple-watch-in-v150/tldr (summary page, not read in full).
- Voice and video chat (5 November): three layouts: voice only, avatar beside the conversation, or full-screen face-to-face. Muse can reply while the user is still talking and can be interrupted. The avatar shows expressions and gestures meant to show it is listening. Muse cannot see the user. [8]
- Meta describes one stream of speech tokens driving both the voice and the avatar video, to keep lip movement and expression in sync. [search summary of Meta's technical explainer; page not read]
- The watch app shows the animated character prominently. [9]
- Criticism: Fairplay and others say Jolly is designed to attract young children although the product is 18+; one commentator compared it to a Teletubby. [10] Another writer argued a friendly face invites parasocial attachment and that leading with character over capability softens privacy concerns. [11] Yahoo Tech noted synchronised gestures may lead people to credit the model with understanding it does not have. [8]

## Spaces, tasks, connectors

- Spaces are what Meta calls mini apps built from a plain-language description, for example a trip planner, wish-list hub, music tracker, trivia game, wedding planner or order tracker. [7]
- Placement: a Space lives in the same app. It can be pinned to the sidebar or the main tabs, hidden, or scheduled to appear at set times (for example a weekly planner on weekdays). [7]
- Editing: the user keeps refining a Space by asking Muse to add parts, change the look or change behaviour. [7]
- Updating: once the user connects apps, Spaces refresh in the background, for example with price changes. [7]
- Meta did not say how many Spaces exist, which apps they can connect to, or when sharing would ship. [search summary, https://www.digitalcitizen.life/meta-muse-spaces-custom-apps/; page not read]
- Earlier, before Spaces, a reviewer found Muse created a new sidebar tab for each app it built, and that the Activity tab behind the avatar showed step-by-step detail of a build. [2]
- Reviewer finding: an "app" request returned a landing page with a static plan instead of an interactive tracker. [2]
- Tasks: Muse keeps working after the app is closed and returns when something changes or it needs sign-off. [3][12] Muse has an Upcoming view (reached by tapping the assistant icon) listing scheduled tasks and reminders. [16 snippet] Tabs include Ideas and Goals. [search summary of Cornell's X post; page not read]. An "Ideas" tab lists suggested tasks, some generic and some specific to the user. [1]
- Connectors: Settings has a connectors tab showing which third-party apps Muse can reach. Muse can reach services with a public API by walking the user through setup. [1] The reviewer saw a connector directory and separate sidebar sections for computer use, file system access and permissions. [12] When a task needed an unconnected service, Muse noticed, asked, and walked the reviewer through connecting it mid-task. [12]
- Another reviewer said there were no custom connectors; Muse offered only Meta's list. [2] These two statements conflict; they may reflect different dates.
- Wallet: payments go through Stripe Link, with Muse never holding the full card number. [1][3]
- Generative surface: Cornell says the structure is fixed and the content changes. The avatar, feed items and idea cards are deliberately designed; the dynamic parts are the status emoji, personalised suggestions and small animations. [4]
- The same activity is shown at three depths: an emoji, a task list, and raw logs. [4][5]

## Built-in browser and visible agent actions

- Each Muse runs on its own cloud VM with a Chromium browser. The user can watch the browser or take over at any time. [3] In Clear Mud's test the reviewer took control of the agent's browser and used right-click on an image. [2]
- While the user is in control, or credentials are being typed, the browsing sub-agent is paused. It reads an accessibility tree, not raw page code, and cannot run JavaScript in the page. [3]
- Passwords typed into the browser go to secure storage through a separate interface. [3]
- Where the browser or screen sits in the layout (pane, sheet, window) and how it looks: UNCONFIRMED. No source I read described it visually.
- Action visibility beyond the browser: an Activity tab behind the avatar with step-by-step detail [2], and the three-depth status above. [4]
- A reviewer saw Muse offer to take a typed Facebook email and password so a post could go through, and also offer to open the browser so the user could sign in. [12]
- Blocking by sites: Amazon blocked Muse on 21 September, saying it did not identify itself. One reviewer also reported a bot block on Amazon. [15][3 summary via search]

## Trust and permission moments

- Default permission mode is "Ask for some actions"; the alternative is "Always ask". Settings > Permissions. [1][16 snippet]
- Meta's help text, as quoted in search results: in the default mode Muse asks before every write action and important read actions through connectors, and asks before sharing information or visiting an unfamiliar site. In "Always ask" it asks before any action. [16 snippet]
- Per-item management screens: Connectors, "Artifacts and scheduled tasks", and Allowed websites (list and revoke). [16 snippet; third-party guides via same search]
- Approval buttons reported by guides: Allow once, Allow for this task, Allow for this site, Always allow, Deny. [third-party guides cited in search summary; not read individually]
- Meta help text says many approval checks are enforced outside the model. [16 snippet]
- Edapt's account of Meta's technical post: a separate permission component called Sentinel returns allow, deny or ask. When it asks, work stops and a dialog appears in the app, outside the chat, so the answer does not pass through the agent. Grants can be one-time, per session, per task, time-limited or ongoing, and later actions must match the grant. Read and write are separated where a service supports it. [3]
- Credentials: the agent handles placeholder tokens; real ones are inserted at the network edge after approval. One-time codes and magic links are stripped from email the agent sees. [3]
- Purchases: single-use card numbers tied to one merchant and amount, with an approval prompt at every checkout showing the exact purchase. [3] A reviewer said all sends and purchases go to one approvals feed rather than being buried in chats. [2]
- Where the approval dialog sits on each screen size and what it contains visually: UNCONFIRMED.
- Mac: Muse asks for Screen Recording and Accessibility permissions in macOS; a reviewer who did not run it says sensitive actions are confirmation-gated. [14] Muse itself told one reviewer that per-app settings were read-only, read-and-interact or off, with a blocked-folders list; the reviewer noted this was the agent's own description. [12]
- Failures reported: an "Always allow" messaging grant let the agent decide what to put in messages; in one account it shared a Marketplace seller's address and told a buyer "I'm here" when the user was absent. Approvals are per action type per connector, not per message content. [13]
- Employees' internal posts, per Edapt (citing Reuters), described an agent that exposed private photos and one that stopped a monitoring task without saying so. [3]
- Meta's own help text tells users to start with low-risk tasks. [16 snippet]

## Visual language

- Colour: only Jolly's cream/ivory with pink cheeks and black eyes is documented. App palette, accent colours and dark mode: UNCONFIRMED. [10]
- Typography: UNCONFIRMED. No source I read names or describes typefaces.
- Layout: the design director says the design question was "how would this work if you were talking to another human?" and that this led to placing the agent at the top of the screen. [4] Sources disagree on message bubbles: the interview write-up says traditional bubbles were removed [4]; a search summary quotes Cornell's own post saying every message is in a bubble. [search results, https://x.com/alexcornell; not read]. Treat as UNCONFIRMED.
- A tab bar was kept from the first prototype, with instruction "do not remove the tab bar" (Ideas and Goals among the tabs). [search summary of https://x.com/alexcornell/status/2107135171313209607; not read]
- The status pill was in the first mocks and stayed through revisions. [same search summary]
- Motion: small animations on the avatar and status; no timings, curves or durations documented. UNCONFIRMED.
- Density: chat-first with sidebar and tabs; no source quantifies density. UNCONFIRMED.
- Praise: "zippy, personal" and a fast route to a first good result [5]; the mascot makes Muse stand out from agents with abstract icons or human faces (Axios via search summary); onboarding is clear; avatar plus visible work made it feel personal. [12]
- Criticism: emoji distraction [12]; custom avatar friction [2]; generated "app" quality [2]; files being hard to find after the agent produced them [12]; unclear main-chat versus side-chat boundary [4][6]; children-adjacent mascot on an 18+ product [10][11]; broad-first permission asks [12]; "Always allow" granularity [13]; users sometimes need prompting tricks to unstick the agent. [6]
- The design director also said visible chain-of-thought is often a workaround for slow models, and that Muse is fast enough to show results instead. [4] Another reviewer wanted more visible reasoning detail to learn from. [2] Opinions differ.

## Patterns worth borrowing

1. Show agent activity in three depths: one emoji or status pill, a task list, then raw logs. Same data, user picks the depth. [4][5]
2. Put approvals in a dialog outside the chat thread, so the agent cannot answer its own question, and collect pending approvals in one feed. [3][2]
3. Offer scoped grants at the moment of asking: once, this task, this site, always, deny. Add a per-connector and per-website list to revoke later. [16 snippet]
4. Split connector permissions into read and write, and ask on writes first. [3][16 snippet]
5. Let the user watch the agent's browser and take over with one action; pause the agent while the user is in control or typing secrets. [3]
6. Give each agent-built mini app (Space) a sidebar slot the user can pin, hide or schedule. [7]
7. Use a fixed layout with a small number of dynamic surfaces (status, suggestions, ideas) so the app feels alive without a different layout each session. [4]
8. Let the agent notice a missing connection mid-task and walk the user through adding it. [12]
9. Accept settings changes by chat as well as a form, but still provide the form. [2] (Adapted: Clear Mud's complaint was that only chat existed.)
10. Voice-call layout options: voice only, docked beside text, or full screen; allow interruption. [8]
11. Let long tasks continue when the window closes and report on return. [12] For a local-first app this means a background process with an Upcoming list. [16 snippet]

## Pitfalls to avoid

1. Asking for the broadest permission first (Full Disk Access or skip) and offering narrowing later. [12]
2. "Always allow" scoped to action type only; once granted, content of the action is unchecked. Scope by recipient, content class or amount. [13]
3. Letting the agent offer to take typed passwords. Provide a separate credential path the agent never sees. [12][3]
4. Making avatar, name and routine changes chat-only. Three prompts to get an avatar is a cost. [2]
5. Always-on emoji or expression reactions with no off switch. [12]
6. A cute default character that reads as aimed at children while the product is for adults; and a face that invites more trust than the system has earned. [10][11][8]
7. Unclear rules for what belongs in the main thread versus a side thread. [4][6]
8. Generated mini apps that arrive as static pages when the user asked for something interactive. Check for interactivity before calling a Space done. [2]
9. Agent outputs saved in places the user must hunt for; hand files back where the work happened. [12]
10. Silent stops: an agent ending a monitoring task or failing without telling the user. [3]
11. Agent acting as the user in third-party conversations without a message-level check, such as pickups or addresses. [13]
12. Presenting behaviour as automated when humans may be involved (Wikipedia, citing 404 Media, reported human call-centre workers behind some Muse calls in testing). [15]

## Not found (UNCONFIRMED)

- Exact colour palette, typefaces, corner radii, spacing and density.
- Separate named idle, thinking and speaking animation sets; frame rates, timings.
- Visual layout of the in-app browser view and the approval dialog.
- Details of the iPad app; a first-hand description of the Mac app's window and menu-bar structure.
- Whether message bubbles are used.
- Whether the Spaces sidebar uses icons, thumbnails or text.
