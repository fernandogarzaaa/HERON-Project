# 06. Native Slint UI qualification

**Result: FAIL for release** (no real-window evidence; one P2 legibility defect). The automated
desktop checks pass.

## What was run

| Check | Result | Evidence |
|---|---|---|
| `cargo build --locked --features desktop --bins` on main and all PR trees | PASS | report 03 |
| `cargo test --locked --features desktop --bin heron-desktop` (31 tests: bridge, SSE parser limits, setup logic) | PASS on all trees | report 03 |
| Headless layout/render test at 1100x700, 800x600, 640x480, 1440x900 with no binding loop | PASS, BMPs captured | `target/qualification/<sha>/ui-snapshots/` |
| Static callback wiring: every `callback` on the root `MainWindow` has an `on_<name>` handler in `src/bin/desktop/` | PASS: 65/65 on `main`, 69/69 with PR #26 (adds `settings-connect-oauth`, `-disconnect-oauth`, `-refresh-oauth`, `-sync-oauth`) | command in §3 |
| Desktop app launched as a real window, clicked through | NOT_RUN | sandbox blocks launching GUI apps from this session; needs a person or a UI driver |
| Keyboard navigation, focus order, Escape, screen reader | NOT_RUN at runtime | Round 2 acceptance doc claims these; not re-verified |
| Fresh install, corrupted config, missing vault, crash recovery at startup | NOT_RUN in the GUI | the store and config layers have unit tests for damaged files (report 08) |
| Display scaling (1x/2x) | NOT_RUN | headless renderer only |

## Snapshot review (headless renderer, combined tree `97a418e4`)

Looked at by eye after converting to PNG:

- **M-UI-1 (P2, headless renderer only; not confirmed in a real window).** The selected sidebar item ("Conversation") and the selected permission-mode
  chip ("Ask first") draw blue text on a mid-grey fill. At 1440x900 both are close to illegible.
  That is the control telling the user which permission mode is active, so it matters for safety
  as well as looks.
- **P3.** The composer hint "Enter to send, Shift+Enter for a new line" overlaps the bottom
  border of the input.
- At 640x480 and 800x600 the mode chips, banner buttons and Send button are clipped. The window's
  minimum size is 880x560 (`ui/main.slint:72-73`), so a user cannot reach these sizes; the test
  forces them. Not a defect.
- "Heron works in ." shows an empty workspace name in the fixture; the binding is filled at
  runtime (`ui/views/chat.slint:226`). Not a defect.

A separate session is working on UI polish (the owner asked for a more beautiful, intuitive and
smooth interface). Its PRs are not part of this qualification.

## Static wiring check

```bash
rg -o "^\s*(in-out |in |out )?callback\s+([a-z0-9-]+)" -r '$2' ui/main.slint | sort -u > declared
rg -o "\.on_([a-z0-9_]+)\(" -r '$1' src/bin/desktop | sed 's/.*://' | tr _ - | sort -u > handled
comm -23 declared handled   # empty = every root callback is bound
```

A bound callback can still do nothing useful; this check only rules out controls with no handler
at all.
