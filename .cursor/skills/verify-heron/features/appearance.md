# Appearance

## Sub-features

- Dusk, Shore, and Graphite
- Light and dark
- Accent chips
- Motion chips (System, Full, Reduced)

## How to get to it (user POV)

Open Settings from the sidebar. Appearance is the first section.

## Driving it with the headless layout test

The same Launch command writes `appearance-settings-dusk-dark.bmp` and `appearance-settings-dusk-light.bmp`. Dusk is the selected theme chip. System is selected for light/dark, accent, and motion, because the fixture does not load `ui.toml`.

Choosing a chip in a running app writes `ui.toml` under `HERON_HOME` (or `~/.heron`). The headless test does not click the chips. Clicking is covered by reading `UiPrefs` in `cargo test --lib ui_prefs::`.

## Gotchas

If `ui.toml` exists in the default home, the real app follows it. The headless test sets theme and dark itself and does not read that file.
