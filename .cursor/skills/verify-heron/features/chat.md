# Chat

## Sub-features

- Empty conversation and starter prompts
- A user message and an assistant reply
- A finished tool step
- An approval card, including one marked as outside content

## How to get to it (user POV)

Open Heron. The first screen is the conversation. A waiting approval sits in the thread, not in a separate dialog.

## Driving it with the headless layout test

Run the Launch command in `../SKILL.md`. The test ends by drawing `appearance-chat-dusk-dark.bmp` and `appearance-chat-dusk-light.bmp` at 1100×700. Those frames contain the fixture reply, a green tool step, and a violet approval card titled "Write notes/yesterday.md?".

## Gotchas

The composer is pinned over the thread. A frame that clips the Send button fails the geometry asserts in `window_layout_tests`. Reduced motion is on for these frames, so you will not see the message fade.
