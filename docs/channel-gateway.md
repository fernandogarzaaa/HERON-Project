# HERON channel gateway — reliability foundation

**Status: review candidate.** This PR improves the existing **Telegram** channel only. It does not add Discord, Slack, WhatsApp, a hosted relay, or an offline worker. See [issue #34](https://github.com/fernandogarzaaa/HERON-Project/issues/34) for the broader roadmap.

## Model

- A **channel** carries user-to-HERON messages (Telegram, Discord, Slack, WhatsApp).
- A **connector** gives HERON scoped access to an external service (OAuth, MCP, REST).
- Neither grants ambient permission to execute tools. All remote agent actions still pass HERON's existing permissions, taint isolation, and `Approver`.

`src/channel.rs` introduces an account + chat + sender identity, provider message IDs, bounded message sizes, an SQLite inbound idempotency journal, and a durable outbound retry spool. Provider adapters **must** verify platform webhook signatures or SDK-authenticated sender IDs before constructing `InboundMessage`. A client-provided `sender_id` must never be trusted.

## Telegram compatibility

The current Telegram bot remains the only Telegram adapter. On supported macOS/Windows vaults, existing single-owner pairing, stop/pause/resume, on-desktop dangerous-action approvals, and session semantics remain intact. Legacy Linux bot credential fallback is intentionally disabled until a verified encrypted vault exists.

- Owner updates are deduplicated by provider update ID and persisted before advancing Telegram's polling offset.
- Outgoing text is queued locally before delivery; retries use bounded exponential delay and ten-attempt cap. The queue is flushed in the polling loop and immediately after new replies are queued.
- Delivery is **at least once**, not exactly once. If Telegram accepts a reply immediately before HERON crashes without recording its acknowledgement, a rare duplicate may be delivered after restart.
- Each bot token is mapped to an opaque SHA-256 account key. Replacing or removing the bot disconnects the previous pairing and deletes pending messages for its account. Outbound sends recheck pairing before dispatch.
- Telegram message chunks respect Telegram's UTF-16 limit even when the text contains emojis.
- Telegram bot tokens are now **native-vault-only**: macOS Keychain and Windows Credential Manager must be functioning. Linux Telegram sign-in is disabled until HERON implements a verified Linux Secret Service/encrypted vault. Existing legacy plaintext-file credentials cannot launch the gateway and must be reconnected after a verified vault becomes available. Removing a bot also clears any legacy fallback.
- Spool is `~/.heron/channels.db` (actual path is `Config.home/channels.db`). It contains private message content; filesystem permissions on a newly created Unix file are owner-only. It is **not encrypted**. Protect the user's machine and backups accordingly.
- Failed sends are retained as `failed` after ten attempts for local diagnostics, rather than retried forever. There is no interactive resend UI in this PR.
- Accepted inbound updates are currently marked handled when the existing Telegram message handler returns; agent turns it starts continue asynchronously. Interrupted turns are not automatically replayed because re-running tool actions could duplicate side effects.

## Boundaries

This PR does **not** expose the local API or model to the public internet. Offline desktop means no Telegram agent processing; hosted/relay operation remains a separate opt-in with its own security and cost model. It neither uses another application's subscription credentials nor adds a generic login/password scraper.

## Release verification

1. `cargo test --locked --lib channel::tests`
2. `cargo test --locked --lib telegram::tests`
3. `cargo test --locked`
4. `cargo build --locked --features desktop --bins`
5. `cargo test --locked --features desktop --bin heron-desktop`
6. Verify the exact-head GitHub Actions platform matrix. Smoke-test pairing, token replacement, failed send/recovery, `/stop`, `/pause`, and no messages leaking to a previously paired chat.
7. Test 20+ consecutive messages, malformed update IDs and oversized/multilingual payloads with a disposable bot and account.

**Not yet qualified:** real Telegram outage/timeout reproducibility, hosted device-off operation, native UI for queue health, cross-process channel spooling on Windows/Linux, Discord/Slack/WhatsApp provider interoperability.

## Next PRs

1. Introduce `ChannelAdapter` async contract / mocked provider contract tests, with user identity paired through verified provider signatures.
2. Add Discord bot with official authentication, then Slack event/websocket adapter; keep pairing, retries, permission broker and attachments per-channel.
3. Add optional end-to-end encrypted outbound-only relay and separately an explicitly opted-in hosted worker. Do not imply these are active when only metadata is configured.
4. Add verified capability manifests, lifecycle status and approval controls to the Slint Connector/Channel settings.
5. Add skills and multi-agent workspaces with constrained execution and revocable permission manifests; implement memory distillation behind user visibility, corrections/deletion, and deterministic evals.

Both [Hermes Agent](https://github.com/NousResearch/hermes-agent) and [CowAgent](https://github.com/zhayujie/CowAgent) inspired this design. No upstream source code was copied; reimplementation is native Rust in the existing Apache-2.0 project.
