# PR #35 security review (head 1c348ab)

No P0 found. The two most serious problems are C35-2 (nothing stops two gateways running at once) and C35-1 (a new bot token is never checked before it replaces the working one).

**Scope.** Line numbers come from `git show pr/35:<path>`. The PR's merge base is eae4cb3. Between it and main, only lib.rs changed among the touched files, and `git merge-tree` merges cleanly. I did not build or test the merged tree, only pr/35.

**Tests run.** On a `git archive pr/35` copy in $TMPDIR, with three of my own scratch tests appended to channel.rs:
- `channel::` passed 12/12, including the scratch tests.
- `telegram::` passed 5/5.
- The integration test did not run: the sandbox refused a local port bind (`Os { code: 1 … "Operation not permitted" }`).

## Findings

**C35-1 P2: A replacement token is not validated before it wipes the working pairing (CONFIRMED by trace)**
- Evidence: server.rs:1017 only checks the format. Lines 1029-1036 then store the token, stop the gateway, purge the spool, unpair and reset the offset. getUpdates failures are swallowed (telegram.rs:492, `Err(_) => { sleep; continue }`).
- Impact: a well-formed but wrong or revoked token silently unpairs the phone, and the gateway then fails forever. This is the owner's own 15:47 gate.
- Root cause: no liveness check before commit.
- Fix: call `getMe` with the candidate token before `store_vault_only_secret`, and show poll-error state in get_telegram.
- Regression test: mock getMe returning 401. Expect an error, with the old token, `telegram_chat` and spool rows unchanged.

**C35-2 P2: No single-gateway guard (mechanisms CONFIRMED by tests and trace; needs two processes)**
- Reachable: `serve` (server.rs:175) and `daemon` (main.rs:692) both call `spawn_if_configured`, whatever `own_background_work` (app.rs:670) returns.
- (a) Leases are not tied to the claimant. `delivered`/`retry` only check `status='leased'` (channel.rs:396-398, 408-411).
  - Scratch test: A claims, B reclaims after 60 s, then A's stale retry undoes B's ack. Output: `REV35 pending after B acked: 1`, so the message is sent a third time.
  - Live, this needs a stall of more than 60 s, since reqwest gives up at 40 s. Within one process `dispatch_lock` (telegram.rs:110) prevents it.
  - The thread asking for acknowledgements tied to the current claim remains open.
- (b) DEFERRED transactions read before they write (channel.rs:278, 345).
  - Scratch test with 2 connections and 1,200 operations: `REV35 busy errors: 35 first="enqueue after 1.006042ms: database is locked"`. The 5 s busy_timeout is bypassed.
  - When `send()` fails this way it only logs, and the reply is lost (telegram.rs:101-105).
- (c) Revocation only reaches one process. `stop()` aborts only the local task (telegram.rs:599-603).
  - After `serve` replaces or removes the token, the daemon keeps polling the old bot, and its running turn is not aborted.
  - The inbound gate checks `paired_chat`, not the epoch (507-508). Telegram user ids are the same across bots, so the owner's messages to the old bot still start turns there.
  - In those turns, auto-allowed tools run; replies and approvals are refused.
- Fix:
  - Take an exclusive `telegram.lock` in `run()`.
  - Use `TransactionBehavior::Immediate`.
  - Add a `lease_token` column that `delivered`/`retry` must match.
  - Gate inbound messages on `is_current_owner`.
- Regression tests: the scratch tests, asserting zero errors and that B's ack wins; and a second `run()` on the same home must refuse to start.

**C35-3 P2: Retry exhaustion silently drops a reply and releases its later chunks (CONFIRMED, test run)**
- Evidence: channel.rs:417 `if next >= MAX_ATTEMPTS { "failed" }` clears the text after about 766 s of backoff. The ordering guard (358) counts only `('pending','leased')` rows. Scratch output: `due returns ids=[2] (head=1)`.
- Also: `count_failed` is used only in tests. `retry_after` is ignored, and a 4xx is retried the same way as a 5xx (telegram.rs:154-160).
- Impact: if sendMessage fails for more than about 13 minutes while polling still works, the reply is lost or arrives tail-first. Nobody is told.
- Root cause: failure is decided per chunk, with no state for the whole message.
- Fix: give each logical message a key and fail all its chunks together. Write a local inbox notice. Honour `retry_after`. Fail permanent 4xx immediately.
- Regression test: a 2-chunk message whose head fails 10 times. Expect no chunk due and a notice present.

**C35-4 P3: Crash recovery misses in-flight agent turns (CONFIRMED by trace; the docs admit it)**
- Evidence: `complete_inbound` runs right after `handle_message` returns (telegram.rs:523-527), and `handle_message` only spawns the turn (405-428). That contradicts the comment at 476-477 ("An accepted turn left unfinished by a crash may have performed a tool action").
- Impact: after a crash in the middle of a tool call there is no "may have been interrupted" notice. Replay is still prevented by the persisted offset (565).
- Fix: complete the inbox row inside the spawned task after `run_turn`, and mark it interrupted on abort or `/stop`.
- Regression test: hold a turn open, drop the gateway, re-run, and expect the notice.

**C35-5 P3: A persistent spool error makes the poll loop spin (control flow CONFIRMED; needs SQLite to fail after startup)**
- Evidence: telegram.rs:533-534 `eprintln!(…); break;` leaves the offset where it was. A 25 s long poll returns the same update at once, and the loop only sleeps `if poll_secs == 0` (571).
- Impact: a tight loop against Telegram, risking 429s, and the owner gets no answer.
- Fix: exponential backoff and a visible health state.
- Regression test: after startup, hold an EXCLUSIVE lock from a second connection for more than 5 s. Expect getUpdates calls per second to stay bounded.

**C35-6 P3: Spool rows are never pruned (CONFIRMED by trace)**
- Evidence: rows are only removed by `purge_account` (channel.rs:444-458). The capacity check counts only `('pending','leased')` (299). Text is cleared, so this is metadata growth, not content.
- Fix: on open, prune sent/failed rows older than 7 days, and inbox rows older than 24 h plus a margin.
- Regression test: seed old rows, reopen, and expect them gone.

**C35-7 P3: The envelope sender check can never fail (CONFIRMED by trace)**
- Evidence: telegram.rs:510-512 builds `let identity = bot.identity(chat); … identity: identity.clone()`, and `accept_inbound(&identity, &event)` compares that value with itself. So channel.rs:209 can never fail. `sender_id` is the chat id (telegram.rs:70).
- The real gate at 505 (`from.id == chat`) is correct. The issue is that the channel contract gives future adapters false assurance.
- Fix: take `sender_id` from `from.id`, and take the expected owner from the stored pairing.
- Regression test: a message whose `from.id` differs from its chat makes `accept_inbound` return an Err.

**C35-8 P2, pre-existing: phone turns inherit the desktop permission mode (CONFIRMED by trace)**
- Evidence: `app.agent()` uses the global policy (app.rs:139-152).
  - Auto mode allows `WorkspaceWrite | ProcessExec` (permissions.rs:261); Yolo allows the rest (257).
  - `high()` excludes ProcessExec and ExternalWrite (95-100), so both can be approved from the phone (telegram.rs:207).
- Impact: a hijacked Telegram account gets a remote shell in Auto mode, and can post or send after a phone "yes". The PR did not introduce this.
- Fix: cap remote-channel turns at Ask, and allow ExternalWrite approval only at the computer.
- Regression test: in Auto mode a phone `shell` request prompts, and ExternalWrite is denied.

## Review threads at this head

**Still unresolved:**
- Lease tied to the current claim (C35-2a).
- The reviewer asked for a gateway test of lockout → unlock → pair with the replacement code. The fix at 553 is correct by trace but has no test.
- The owner's credential gate: no tests for set/remove_telegram storage failure, rotation or disconnect; no test that the token stays out of logs; no token validation (C35-1).

**Fixed in code (I read these; the integration test did not run):**
- Atomic enqueue (channel.rs:277-322).
- Ordering during backoff (352-359), apart from C35-3.
- The retry test (passes).
- Sender fixtures (integration.rs:404, 430-436).
- Approvals are no longer queued (telegram.rs:212-234, 470-475).
- Direct-prompt revocation (215, 225, 238).
- Abort on gateway drop (191-196, 486), within one process only (see C35-2c).

## What is live (item 5)

- **Live:** `ChannelStore` is used by the Telegram gateway for inbound dedup, interrupted marking, the ordered outbound retry spool, purge, and stale-prompt discard.
- **Contract only:** `ChannelAdapter`, `ChannelCapabilities`, the Discord/Slack/WhatsApp `ChannelKind` variants and `thread_id` have no callers outside channel.rs. `count_pending` and `count_failed` are only used in tests.

## Verified sound

- **Sender gate:** only private chats with `from.id == chat.id` and `message` updates are accepted. Other chats are ignored (504-505, 540). Dedup is keyed by (kind, sha256(token), update_id).
- **Approvals:** each prompt carries an 8-character challenge and a bare "yes" is refused. `/stop` takes priority. Approval is `Once` only. The epoch is re-checked before the prompt, before the send and after the answer. Tainted and high-risk requests are denied from the phone.
- **Taint:** persists across phone turns (agent.rs:57, 524-525), and sub-agents share the taint flag and the approver (549-554).
- **Token storage:**
  - Vault-only write, with read-back, rollback and sanitised errors (config.rs:446-462, credential_transaction.rs). It uses the native keyring API, so the token is not passed as a command-line argument.
  - The gateway needs the vault entry to start (telegram.rs:583). Removing the token also clears the legacy file.
  - No reqwest error, which would contain the token in its URL, is ever printed (154-160, 168, 229-230, 490-495).
- **Spool file:** rejects symlinks, is created with mode 0600, and uses `secure_delete` with a DELETE journal.

Draft copy: <scratch> The repo was not modified.
