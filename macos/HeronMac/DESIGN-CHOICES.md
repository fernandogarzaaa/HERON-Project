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

*   **The `approval` event as specified has no taint flag or outside source.** The visual doc's violet line needs both, so `ApprovalRequest` carries `tainted` and `outsideSource`, which the client supplies (the stub sets them). U4b should derive them from the session's taint state; this is a gap between §4's event table and the visual doc, not a divergence.
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
