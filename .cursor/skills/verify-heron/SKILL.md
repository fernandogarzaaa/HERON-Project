---
name: verify-heron
description: Drive Heron's Slint desktop the way CI does, with the headless software renderer, and check chat, settings, and layout. Use when a change touches ui/ or src/bin/desktop and you need proof the window still draws.
---

# Verify Heron

The desktop is a native Slint window (`heron-desktop`). This environment has no display. CI proves the window with Slint's software renderer. Do that. Do not treat `cargo build` as proof that a screen looks right.

## Launch

Install the Linux libraries named in `.github/workflows/ci.yml` if `fontconfig` is missing.

```bash
mkdir -p /tmp/heron-ui
HERON_UI_SNAPSHOTS_DIR=/tmp/heron-ui cargo test --locked --features desktop --bin heron-desktop window_layout_tests::main_window_lays_out_and_renders_without_a_binding_loop -- --exact --nocapture
```

Ready means the test prints `test result: ok`. The test draws every view at 880×560, 1100×700, and 1440×900, then writes BMP frames, including `appearance-chat-*.bmp` and `appearance-settings-*.bmp`.

There is no server to keep. Each `cargo test` process is its own window. `HERON_HOME` is not required. The frames use fixture text, not a user's database.

## Doctor

```bash
test -s target/debug/deps/heron_desktop-* || cargo test --locked --features desktop --bin heron-desktop --no-run
pkg-config --exists fontconfig
```

`pkg-config` must exit 0. The binary name contains a hash, so check that `cargo test --no-run` exits 0.

## Drive

The layout test is the drive. It opens the window, resizes it, switches views, scrolls settings to the end, and draws a chat with an approval card. Selectors are the view ids the shell already uses: `chat`, `settings`, `memory`, `tasks`, `reminders`, `awareness`, `usage`, `activity`, `onboarding`.

To look at one frame:

```bash
python3 - << 'PY'
from PIL import Image
Image.open("/tmp/heron-ui/appearance-chat-dusk-dark.bmp").save("/tmp/heron-ui/appearance-chat-dusk-dark.png")
PY
```

## Evidence

Keep PNGs of `appearance-chat-dusk-dark`, `appearance-chat-dusk-light`, and `appearance-settings-dusk-dark`. A frame is proof only when the test passed and the PNG shows the control you changed (the approval card, the Appearance chips, the sidebar mark). A passing test with a blank or clipped frame is a failure. The test already rejects a uniform frame and a sidebar that ends below the window.

## Cleanup

The test process exits on its own. Delete `/tmp/heron-ui` only after you have copied the frames you will keep. Do not delete `target/`.

## What this does not cover

A real macOS or Windows window, VoiceOver, and a signed app are not run here. `docs/ui-round2-acceptance.md` lists those checks.
