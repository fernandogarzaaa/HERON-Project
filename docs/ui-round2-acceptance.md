# Native Slint UI — Round 2 acceptance checklist

This document covers the UI-only changes in PR #27, stacked on the Round 1 shell PR #24. It does **not** qualify or include OAuth/backend PR #26.

## Automated checks (run against the final head commit)

```sh
cargo fmt --all -- --check
cargo test --locked --features desktop --bin heron-desktop window_layout_tests::main_window_lays_out_and_renders_without_a_binding_loop -- --nocapture
cargo test --locked --features desktop
cargo build --locked --features desktop --bin heron-desktop
```

The headless resize regression checks that the native window draws a frame after successive size changes (1100×700, 800×600, 640×480, 1440×900, 1100×700). It does **not** assert whether text is clipped, whether widgets remain visible, or whether focus order is usable. The native app enforces an 880×560 minimum window size; the smaller headless case is an additional stress check.

## Manual desktop acceptance

Use a macOS or Windows machine with a real desktop window. Inspect at 880×560 (minimum), 1100×700 (default), and 1440×900 (large). At each size, confirm all controls remain visible and reachable with no overlapping or cut-off panels.

| Area | Acceptance action | Expected result |
| --- | --- | --- |
| App startup | Launch with working backend | Window opens and status updates without blank/frozen frame |
| Error | Start with unavailable backend | Clear error is visible, navigation/settings remain reachable |
| Sidebar | Open 0, 2, and 20+ sessions | Recent sessions scroll independently; connection status stays visible |
| Navigation | Tab/Shift+Tab through navigation | Focus is visible, no trapped focus, correct view opens |
| Composer | Type text and press Enter | Sends one message when idle |
| Composer | Press Shift+Enter | Inserts a newline without sending |
| Composer | Press Enter repeatedly while busy | No duplicate send; draft is not cleared |
| Composer | Press Escape while busy | Stop/cancel callback is invoked |
| Action buttons | Tab to Send/Stop/Copy/Newest; press Enter and Space | Each performs its normal click behavior once |
| Empty chat | Tab to a starter prompt, press Enter/Space | Starter action is invoked |
| Tool steps | Tab to a tool step, press Enter/Space | Expanded/collapsed state and preview update |
| Thread follow | Scroll up while reply streams | Scroll position remains user-controlled and Newest appears |
| Thread follow | Click/activate Newest, then stream another delta | Scroll returns to bottom and follows new content |
| Modes | Select read-only/ask/auto/unrestricted | Correct selected state; unrestricted still asks for confirmation |
| Accessibility | VoiceOver or Narrator inspect action buttons/cards/steps | Role and accessible label are announced; default action works |
| Loading | Start a slow response | Stop action and thinking indicator are visible; no double-send |
| Motion | Test reduced-motion setting when wired | No unwanted transition animation |

## Evidence before merging

- [ ] Successful compiler, test, and desktop build checks on the exact final SHA
- [ ] Keyboard and accessibility inspection on macOS and Windows
- [ ] Screenshots at minimum, default, and large window dimensions
- [ ] Long-session-list screenshot with the sidebar footer in view
- [ ] Error, loading, idle, and in-progress response screenshots
- [ ] No new OAuth/backend files or integration scope in the diff
- [ ] All confirmed P0/P1 interaction and layout failures resolved

## Scope and known limitations

This PR is a focused functional/accessibility and layout hardening pass, **not a complete replacement of every HERON view**. It does not add a full platform screenshot comparison harness, cross-platform screen-reader automation, or an OS reduced-motion preference bridge. UI compilation and real-device behavior must be verified before this draft is marked ready.
