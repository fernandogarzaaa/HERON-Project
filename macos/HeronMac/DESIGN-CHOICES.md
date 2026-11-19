# Design choices for the Heron Mac app (U4a)

`docs/designs/heron-mac-visual.md` is the owner-approved visual spec and wins on appearance; `heron-v1.md` §4 governs behaviour and the API. This file records (1) the owner's answers D1 to D8, copied from the visual doc, and (2) each choice the visual doc leaves open, with one line of reason. The owner reviews these in the second pull request (Q7); any change is a follow-up, not a blocker. U4b appends to the second list for anything it adds.

## The owner's answers (heron-mac-visual.md, Decisions)

| # | Choice | Answer, as built |
|---|---|---|
| D1 | Overall direction | Calm, native, serif only for display; Dusk is the default theme. |
| D2 | Inbox scope | One Inbox: parked approvals, suggestions and delivered results (`InboxStore`, `InboxView`). |
| D3 | Themes | Shore, Dusk, Graphite; Dusk is the default. |
| D4 | Default accent | `system`, with `slate` as the fallback when the system accent is graphite, gold or purple. |
| D5 | Default avatar | `ripple`. |
| D6 | "Prep my day" prompt | Brief plus drafts; the prompt text is `TodayStore.prepPrompt`, copied from the visual doc. |
| D7 | Menu bar extra | The full panel (status, waiting items, next event, actions, footer). |
| D8 | Sandbox on first run | A status card above the risk text (`SandboxStatusCard`). |

## Choices the visual doc leaves open

### Avatar and events

*   **`text` speaks again after a tool call.** §4 says "first `text` of a run → `speaking`" and `tool_start` → `thinking`; the machine speaks on any `text` that follows a tool call, because otherwise a reply streamed after a tool would show "thinking" while words appear.
*   **After an approval is answered the avatar returns to `thinking` (or `idle` if no run is active).** §4 only says the wait ends on `approval_resolved` or `approval_parked`; the run is still going, so `thinking` is the honest state.
*   **The machine tracks runs and pending approvals by id.** Two concurrent runs (a schedule and a chat) must not send each other to `idle`, and a cancelled run must not leave a stale "waiting".
*   **Error timing takes an injected clock.** `AvatarStateMachine` is a value type driven by `handle(_:at:)` and `tick(at:)`, so tests never wait 5 seconds and U4b can replay recorded streams into it.
*   **Badges are always on for waiting and error.** The doc says colour and badge carry the state under Reduce Motion; showing them at all times keeps one design instead of two.
*   **Disc styles (`heron`, `monogram`) carry the loops on a halo ring around the disc.** The doc describes the loops only for `ripple`; a halo keeps the disc steady and still shows state.
*   **The `heron` mark is a plain one-stroke head and neck.** The doc asks for "a heron's head and neck in one stroke" without a drawing; this is a first version that can be refined without touching any API.

### Approvals

*   **The `approval` event carries `tainted`, but no outside source (U4b update).** The core's `approval` event has a `tainted` flag (`src/server.rs`, recorded in `events-approval.jsonl`), so `CoreClient` decodes it. It sends no name for the outside source, so `outsideSource` stays nil on a live approval and the sheet uses its generic wording. Showing the source needs a core change, which U4b may not make.
*   **"High risk" is the core's `high()` set plus `external` and `egress`.** The visual doc says high risk means "sends, deletes, spends, can't be undone"; the core's `high()` does not include external writes or network egress, but a sent email can't be undone.
*   **The primary button on a high-risk sheet is named from the tool ("Send", "Delete", "Pay") and falls back to "Do it".** The doc forbids a bare "Allow" there and names no fallback.
*   **Impact lines come from the effect label.** The doc shows examples only; the lines in `ApprovalCopy.impact` are plain-word summaries per label, and say "you can undo it" only for writes, where the core offers `/api/undo`.
*   **Esc on a normal sheet means "Don't allow"; on a high-risk sheet Return does.** Closing a sheet without an answer would leave the run waiting, and Return must never approve a high-risk action.
*   **An approval for the open conversation shows as a sheet only while Chat is the visible destination.** Otherwise it goes to the Inbox feed and the sidebar count, so a sheet never opens over a screen the user isn't looking at.

### Screens

*   **A single `Window` scene, not a `WindowGroup`.** One window per core; reopening from the menu bar must not create a second window with its own state.
*   **Chat shows at most 8 recent conversations in the sidebar.** The doc says "recent conversations" without a number.
*   **Today's greeting uses the first word of the Mac's full name, or none.** The API has no user name (the persona name is the agent's); a missing name reads "Good morning." rather than a placeholder.
*   **Starter chips on the first conversation are "Prep my day", "Tidy my Downloads folder" and "Draft a reply to my latest email".** Copied from the doc's empty-state table.
*   **"Edit…" on a memory is a forget plus an add.** The core has no memory update route; an edit therefore resets the use count.
*   **"New schedule" opens a new conversation with a starter sentence in the composer.** The core creates schedules from a described request ("when" and "prompt"), and `CoreClientProtocol` has no create-schedule call; the user edits the sentence and sends it.
*   **"Done this week" counts tasks completed in the last 7 days, and tasks with no completion time.** The core's task list carries no completion time yet.
*   **"Learn more" on the another-Heron banner opens a short popover.** There is no documentation URL to open, and inventing one would be wrong.
*   **"Use an API key instead" skips to the last first-run step and opens Settings › Model after it.** The key itself is entered in Settings, not on the first-run screen, so a key never appears in the onboarding flow.
*   **Working folder and launch at login are shown but disabled in Settings › General.** There is no route to change the folder yet, and §4 defers the login item until unattended work through a subscription login is allowed (Q6).
*   **"Suggestions" in the Inbox use the buttons "Do it" and "Not now".** "Do it" accepts the suggestion and starts a conversation with its prompt.
*   **Parked approvals show "Approve and send" when the action is a send, else "Approve".** The doc names the button after the action; "Approve" is the neutral form for the rest.

### Colour, type and accessibility

*   **Increase Contrast draws the hairline as `ink` at 40% and gives cards and sheets a 1 pt `ink` border.** The doc says "twice the opacity" of a hairline that is an opaque colour, so the new value is the nearest meaningful reading.
*   **`system` falls back to `slate` when the live accent's saturation is under 0.15 (graphite), or its hue is 35 to 70 degrees (gold) or 250 to 310 degrees (purple).** The doc names the three colours without numbers.
*   **Text size scales the reading font explicitly.** Verified on macOS 27 (an `ImageRenderer` check at `.xSmall`, `.large` and `.xxxLarge`): `.dynamicTypeSize` leaves text styles at the same width, so the Settings control and ⌘+ and ⌘− scale the `reading` font (chat replies and the user's messages) by 0.92, 1.0, 1.12 and 1.26, and set `.dynamicTypeSize` on the window for whatever macOS honours. Other text styles stay at the system size; widening the control to them is a follow-up.
*   **Reduce Transparency replaces the sidebar material with the solid `sidebar` colour; otherwise the `sidebar` colour is drawn at 88% over the material.** The doc says "tint, drawn over the system sidebar material" without an opacity.
*   **Checkboxes, radio rows and buttons are drawn in SwiftUI, not native controls.** They look the same in every theme and render in snapshot tests, which `ImageRenderer` can't do for AppKit-backed controls.

### Menu bar

*   **The menu bar icon is a template image drawn with `NSBezierPath`: two rings and a core, plus a knocked-out dot while something waits.** The doc asks for a template `ripple` with a dot; the knock-out ring keeps the dot readable at 18 pt.
*   **"Review" in the menu bar opens the main window on that item in the Inbox.** A live approval shows the same sheet inline in the Inbox detail.

### Tests

*   **Snapshots render at 1x with scroll views drawn flat, avatars still and the default blue accent.** `ImageRenderer` can't draw a `ScrollView`, has no clock, and `Color.accentColor` follows the user's system setting, which would make the references machine-dependent.
*   **References are named `<name>.macos<major>.png`.** A missing reference is recorded and fails once; a reference for another macOS major version skips the comparison, as item 41 says.
*   **Which screens have a snapshot:** the approval sheet (clean with all scopes, tainted with none, high risk) and the risk notice (sandbox on and off), in light and dark Dusk. Other screens get none, as item 41 says.

## Choices U4b made (the core API client)

### Where U4a's protocol or models had to change

*   **`CalendarEvent.start` is optional, and the event has a `whenText`.** `GET /api/today` serves each event as one text line, `"Title — When: tomorrow 10:00"` (`sources::upcoming`), not as fields. A date is made only when the text is an ISO 8601 time; otherwise the served words are shown (`Fmt.time(of:)`). The alternative, inventing a date from free text, would show a wrong time. Three view lines changed (Today, the menu bar's "Next"), and nothing else.
*   **`showsAnotherHeronBanner` waits for `hasBootstrapped`.** `client.endpoint` isn't observable, and for the runtime client it is only the real endpoint once the core has answered. Without the wait the banner could be decided from the placeholder and never re-read.

### Wire shapes that differ from the models

*   **Usage rows are the per-model month totals, labelled "This month".** `GET /api/usage` serves per-day token totals and per-model month totals as arrays of arrays. The Usage tab shows tokens in and out per row; only the model rows have both.
*   **Chat history is folded.** `GET /api/sessions/{id}` returns the model's raw transcript (assistant messages with `tool_calls`, then `tool` messages). Tool results become the steps of the assistant message that called them (failed when the result starts with "Error"), and an assistant turn split by tool calls shows as one message. The step's sentence is the tool name in words plus its first argument ("write file · plan.md"), since the transcript has no summary. The core serves no per-step duration or untrusted flag, so those stay empty.
*   **The core serves no `tainted` for a session, no row id for audit entries, and no progress or completion time for tasks.** `SessionDetail.tainted` is false unless the core starts sending it, audit entries are numbered by position, and the "14 of 22 receipts" line and "Done this week" use what U4a already does without them. A held schedule has no served reason, so `heldReason` is nil.
*   **Provider rows join two routes.** `GET /api/providers` has the installed state and versions; the login command, the auth kind and which provider is in use come from `GET /api/settings`. A null `in_range` (no tested range yet) reads as not in range. A null `logged_in` (the core can't check) reads as signed in, because the core reports a sign-in problem itself when a run starts, and "signed out" would tell the user to log in to a CLI that may be fine. The tested range reads "2.0.0 to below 3.0.0".
*   **Settings join four routes.** `connections` come from `GET /api/sources`, `awareness` from `/api/awareness` and `phoneConnected` from `/api/telegram`. If one of those fails the page still loads, with an empty list or false.

### Gaps in the core, surfaced rather than hidden

*   **A single schedule can't be paused or resumed.** `POST /api/schedules/{id}` accepts only `may`. `setScheduleEnabled` throws a plain-words error instead of sending a request the core answers with 400. Pause and Resume on a schedule row therefore do nothing visible until the core grows the route (the store ignores the error, as U4a wrote it). A follow-up for the core, not this unit.
*   **`/api/chat` has no title field.** `sendMessage(title:)` is accepted and not sent, so "Prep my day" is titled by the core from the message, not "Prep · Tue 18 Nov".

### The core process and the runtime client

*   **One client that follows the core.** `ManagedCoreClient` forwards to a `CoreClient` for the current endpoint. A restart gives the core a new port and token, and the stores were built before either was known. A call made while the core starts waits for the endpoint (failing instead would make `bootstrap()` read a failed `state()` as "onboarded" and skip first run); once the core has stopped for good, calls fail with the core's last output.
*   **A restart shows as `resync`.** The new core numbers events from 1 again, so the old position is ahead of its bus and the bus answers `resync`; the app reloads. After a `resync` the stream reconnects with no `since`, because asking again with the old one would be answered with `resync` for ever.
*   **Two exits within 60 seconds stop the core; exits further apart restart it each time.** The spec says "restarts once, stops after a second exit within 60 seconds"; a sliding window is the reading under which a core that runs for a week and crashes isn't treated as a crash loop. "Restart Heron's core" on the card clears the history.
*   **A core that prints no endpoint within 20 seconds is terminated** and counts as an exit, so a hung start ends at the "core stopped" card.
*   **Stdout and stderr are read to the end for as long as the core runs.** A pipe nobody reads fills and blocks the core. The last lines are kept and shown on the card.
*   **`HERON_CLI_PATH` points a development build at another core binary, and `HERON_PREVIEW=1` runs on `PreviewCoreClient`.** `swift run Heron` has no `Contents/MacOS/heron-cli`; without these a development build could only show the "core stopped" card. Neither is read in a release build's normal path.

### Tests

*   **Decoding tests assert values, and the fixture folder is enumerated.** A new recording with no decoder in `ModelDecodingTests` fails the suite.
*   **Routes with no recording (memories, tasks, schedules, inbox, suggestions, usage, sources) are tested against JSON copied from the core's serde structs.** They are unverified against a live core until Ut records them.
*   **No test opens a port.** `EventStream` takes an injected connection, `CoreClient` is tested through a `URLProtocol` stub, and `CoreProcess` through a fake core script with an injected clock.
