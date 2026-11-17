Headless Slint snapshots for the HERON UI polish PR stack. Not for merging.
Produced by `HERON_UI_SNAPSHOTS_DIR=<dir> cargo test --locked --features desktop --bin heron-desktop window_layout_tests -- --nocapture`
(TZ=UTC, OPENROUTER_API_KEY unset), BMPs converted to PNG.
Each folder is the state after that PR in the stack; 0-before-main is main at 449231f.
`empty` is the first-run state; `populated` adds fixture sessions, demo and paused banners, and watching on.
