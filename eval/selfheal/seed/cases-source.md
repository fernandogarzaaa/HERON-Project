# Replay case sources (supervisor seed for unit H3; deleted after the graph ships)

Selected by the doc's rule (item 20): the first 10 commits in `git log --no-merges 39dbdf5` whose subject starts with `fix`, whose non-test changes are all under `src/` or `macos/HeronMac/Sources/`, and whose diff has at most 200 changed lines. `base` is the fix commit's first parent.

## source case 1

- fix: `6adce3926db72926e7a6f1bd7072d5804b2bc893`
- base: `2b4036ac1220abddd2e98670de78cab1bfb7d602`
- subject: fix(sources): rank searches across email or calendar providers by relevance
- files: src/sources.rs src/store.rs 

Commit message body:

```text
A search over every email (or calendar) provider ran one query per provider,
sorted the results by date, and then cut them to the limit. A strong older
match could lose to newer, weaker ones. This was a Codex finding on #26.

The aliases now run as one bm25-ranked query (Store::search_knowledge_in).
The best matches are chosen first and only then shown newest first.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

```

File `src/sources.rs`: 716 lines at base, 731 at fix. Last 4 lines identical at base and fix: **no**.

Last 4 lines at base:
```rust
        assert!(upcoming_items[1].starts_with("Dentist —"), "legacy calendar stays unprefixed");
        assert_eq!(connected_summary(&app.store), "4 calendar events");
    }
}
```
Last 4 lines at fix:
```rust
        assert_eq!(search(&store, "invoice", Some("calendar"), 3).unwrap(), "nothing found in the connected sources");
    }

}
```

File `src/store.rs`: 2022 lines at base, 2037 at fix. Last 4 lines identical at base and fix: **yes**.

Last 4 lines at base:
```rust
        db.replace_knowledge_snapshot("google_email", &[]).unwrap();
        assert_eq!(db.knowledge_count("google_email"), 0);
    }
}
```

The fix (`git show -U12`):

```diff
diff --git a/src/sources.rs b/src/sources.rs
index e6c469c..e4d5fc9 100644
--- a/src/sources.rs
+++ b/src/sources.rs
@@ -554,33 +554,30 @@ pub async fn sync_due(app: &App) {
 /// Search connected sources. Returns formatted results for the agent.
 pub fn search(store: &Store, query: &str, source: Option<&str>, limit: usize) -> Result<String> {
     let tokens = tokenize(query);
     if tokens.is_empty() {
         bail!("give a few words to search for");
     }
     let expr = tokens.iter().map(|t| format!("\"{}\"", t.replace('"', ""))).collect::<Vec<_>>().join(" OR ");
     let aliases: &[&str] = match source {
         Some("email") => &["email", "google_email", "microsoft_email"],
         Some("calendar") => &["calendar", "google_calendar", "microsoft_calendar"],
         _ => &[],
     };
+    // The best matches are chosen by relevance across every alias at once; only then are they shown
+    // newest first. Picking the newest per source first would drop strong older matches.
     let mut hits = if aliases.is_empty() { store.search_knowledge(&expr, source, limit)? } else {
-        let mut combined = vec![];
-        for name in aliases {
-            combined.extend(store.search_knowledge(&expr, Some(name), limit)?);
-        }
-        combined
+        store.search_knowledge_in(&expr, aliases, limit)?
     };
     hits.sort_by(|a, b| b.ts.cmp(&a.ts));
-    hits.truncate(limit);
     if hits.is_empty() {
         return Ok("nothing found in the connected sources".into());
     }
     Ok(hits.iter().map(|k| format!("[{}] {} ({})\n{}", k.source, k.title, crate::util::fmt_ts(k.ts), truncate_middle(&k.body, 700))).collect::<Vec<_>>().join("\n---\n"))
 }
 
 /// One line for the prompt describing what is connected.
 pub fn connected_summary(store: &Store) -> String {
     let counts = [
         (store.knowledge_count("email") + store.knowledge_count("google_email") + store.knowledge_count("microsoft_email"), "emails"),
         (store.knowledge_count("calendar") + store.knowledge_count("google_calendar") + store.knowledge_count("microsoft_calendar"), "calendar events"),
         (store.knowledge_count("documents"), "documents"),
@@ -704,13 +701,31 @@ mod tests {
         app.store.upsert_knowledge("calendar", "a", "Dentist", "When: Friday 2026-11-13 09:00–09:30", now_ts() + 3600).unwrap();
         app.store.upsert_knowledge("calendar", "b", "Far away", "When: later", now_ts() + 30 * DAY).unwrap();
         assert_eq!(upcoming(&app.store, 72, 5), vec!["Dentist — Friday 2026-11-13 09:00–09:30"]);
         assert_eq!(connected_summary(&app.store), "2 calendar events");
         app.store.upsert_knowledge("google_calendar", "g", "Google visit", "When: Soon", now_ts() + 1800).unwrap();
         app.store.upsert_knowledge("microsoft_calendar", "m", "Microsoft visit", "When: Later", now_ts() + 7200).unwrap();
         let upcoming_items = upcoming(&app.store, 72, 2);
         assert_eq!(upcoming_items.len(), 2, "limit applies across all calendar providers");
         assert!(upcoming_items[0].starts_with("[Google Calendar] Google visit"));
         assert!(upcoming_items[1].starts_with("Dentist —"), "legacy calendar stays unprefixed");
         assert_eq!(connected_summary(&app.store), "4 calendar events");
     }
+
+    #[test]
+    fn searching_several_providers_returns_the_best_matches_not_the_newest() {
+        let store = Store::in_memory().unwrap();
+        // An older Google message matches both words; newer Microsoft and ICS entries match one.
+        store.upsert_knowledge("google_email", "g1", "Acme invoice overdue", "The Acme invoice is overdue", 100).unwrap();
+        store.upsert_knowledge("microsoft_email", "m1", "Lunch", "About the invoice for lunch", 300).unwrap();
+        store.upsert_knowledge("email", "e1", "Weekly note", "Nothing about invoices here, just an invoice", 200).unwrap();
+        let best = search(&store, "acme invoice", Some("email"), 1).unwrap();
+        assert!(best.contains("Acme invoice overdue"), "{best}");
+        // Shown newest first once chosen, across every provider of that kind.
+        let all = search(&store, "invoice", Some("email"), 3).unwrap();
+        let order: Vec<usize> = ["[microsoft_email]", "[email]", "[google_email]"].iter().map(|s| all.find(s).unwrap()).collect();
+        assert!(order.windows(2).all(|w| w[0] < w[1]), "{all}");
+        // Calendar aliases and other kinds stay separate.
+        assert_eq!(search(&store, "invoice", Some("calendar"), 3).unwrap(), "nothing found in the connected sources");
+    }
+
 }
diff --git a/src/store.rs b/src/store.rs
index 06371fe..bfc1bd4 100644
--- a/src/store.rs
+++ b/src/store.rs
@@ -1345,24 +1345,39 @@ impl Store {
             .collect::<rusqlite::Result<_>>()?;
         Ok(v)
     }
 
     pub fn search_knowledge(&self, match_expr: &str, source: Option<&str>, limit: usize) -> Result<Vec<KnowledgeItem>> {
         self.knowledge_rows(
             "SELECT k.id, k.source, k.ref, k.title, k.body, k.ts FROM knowledge_fts f JOIN knowledge k ON k.id=f.rowid
              WHERE knowledge_fts MATCH ?1 AND (?2 IS NULL OR k.source=?2) ORDER BY bm25(knowledge_fts) LIMIT ?3",
             &[&match_expr, &source, &(limit as i64)],
         )
     }
 
+    /// As `search_knowledge`, over several sources in one ranked query, so the best matches win
+    /// across all of them rather than the newest from each.
+    pub fn search_knowledge_in(&self, match_expr: &str, sources: &[&str], limit: usize) -> Result<Vec<KnowledgeItem>> {
+        let marks = vec!["?"; sources.len()].join(",");
+        let sql = format!(
+            "SELECT k.id, k.source, k.ref, k.title, k.body, k.ts FROM knowledge_fts f JOIN knowledge k ON k.id=f.rowid
+             WHERE knowledge_fts MATCH ? AND k.source IN ({marks}) ORDER BY bm25(knowledge_fts) LIMIT ?"
+        );
+        let limit = limit as i64;
+        let mut args: Vec<&dyn rusqlite::ToSql> = vec![&match_expr];
+        args.extend(sources.iter().map(|s| s as &dyn rusqlite::ToSql));
+        args.push(&limit);
+        self.knowledge_rows(&sql, &args)
+    }
+
     /// Items of a source, newest first. `undigested_only` = not yet learned from.
     pub fn knowledge_of(&self, source: &str, undigested_only: bool, limit: usize) -> Result<Vec<KnowledgeItem>> {
         self.knowledge_rows(
             "SELECT id, source, ref, title, body, ts FROM knowledge WHERE source=?1 AND (?2=0 OR digested=0) ORDER BY ts DESC LIMIT ?3",
             &[&source, &(undigested_only as i64), &(limit as i64)],
         )
     }
 
     /// Items of a source whose time falls in a window, soonest first (calendar lookups).
     pub fn knowledge_between(&self, source: &str, from: i64, to: i64, limit: usize) -> Result<Vec<KnowledgeItem>> {
         self.knowledge_rows(
             "SELECT id, source, ref, title, body, ts FROM knowledge WHERE source=?1 AND ts>=?2 AND ts<=?3 ORDER BY ts LIMIT ?4",
```

## source case 2

- fix: `1760164c5ec66b37a58b80c4c090466a1e581ae5`
- base: `0ee5b13e9045093ebbb872788ada6dd02845d3e2`
- subject: fix(channel): upgrade the spool schema in one transaction
- files: src/channel.rs 

Commit message body:

```text
Two processes opening a pre-upgrade spool could both see a column missing.
The second ALTER TABLE then failed with "duplicate column name", which the
busy retry does not cover. The table setup, column checks, ALTERs and
backfill now run in one IMMEDIATE transaction. Another opener waits, then
finds the upgrade done.

The two-process contention test now opens both stores before the load
starts, as Heron processes do at startup. Windows CI had starved the second
open past its 5 s budget while the first process hammered the spool.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

```

File `src/channel.rs`: 917 lines at base, 951 at fix. Last 4 lines identical at base and fix: **yes**.

Last 4 lines at base:
```rust
        assert!(store.accept_inbound(&owner(), &bad, 1).is_err());
        assert!(store.enqueue(&owner(), None, "ok", "", 1).is_err());
    }
}
```

The fix (`git show -U12`):

```diff
diff --git a/src/channel.rs b/src/channel.rs
index 5a2eac6..02a42b4 100644
--- a/src/channel.rs
+++ b/src/channel.rs
@@ -186,77 +186,82 @@ impl ChannelStore {
     }
 
     #[cfg(test)]
     fn in_memory() -> Result<Self> {
         Self::init(Connection::open_in_memory()?)
     }
 
     fn init(mut db: Connection) -> Result<Self> {
         db.busy_timeout(std::time::Duration::from_secs(5))?;
         db.execute_batch(
             "PRAGMA journal_mode=DELETE;
              PRAGMA synchronous=FULL;
-             PRAGMA secure_delete=ON;
-             CREATE TABLE IF NOT EXISTS channel_inbox(
+             PRAGMA secure_delete=ON;",
+        )?;
+        // One process sets up or upgrades the schema at a time. Another opening the spool waits,
+        // then finds the work done; checking first and adding a column later would let both see
+        // it missing, and the second ALTER would fail.
+        let tx = Self::write_tx(&mut db)?;
+        tx.execute_batch(
+            "CREATE TABLE IF NOT EXISTS channel_inbox(
                kind TEXT NOT NULL, account_id TEXT NOT NULL, chat_id TEXT NOT NULL,
                sender_id TEXT NOT NULL, provider_message_id TEXT NOT NULL,
                thread_id TEXT, text TEXT NOT NULL, received_at INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'accepted',
                PRIMARY KEY(kind, account_id, provider_message_id)
              );
              CREATE INDEX IF NOT EXISTS idx_channel_inbox_received ON channel_inbox(received_at);
              CREATE TABLE IF NOT EXISTS channel_outbox(
                id INTEGER PRIMARY KEY,
                kind TEXT NOT NULL, account_id TEXT NOT NULL, chat_id TEXT NOT NULL,
                sender_id TEXT NOT NULL, thread_id TEXT, text TEXT NOT NULL,
                idempotency_key TEXT NOT NULL, queued_at INTEGER NOT NULL,
                next_attempt INTEGER NOT NULL, attempts INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'pending',
                provider_message_id TEXT,
                lease_token TEXT,
                message_key TEXT,
                UNIQUE(kind, account_id, idempotency_key)
              );
              CREATE INDEX IF NOT EXISTS idx_channel_outbox_due
                ON channel_outbox(kind, account_id, status, next_attempt, id);",
         )?;
         for column in ["lease_token", "message_key"] {
-            let present: bool = db.query_row(
+            let present: bool = tx.query_row(
                 "SELECT COUNT(*) FROM pragma_table_info('channel_outbox') WHERE name=?1",
                 [column], |r| r.get::<_, i64>(0),
             )? > 0;
             if !present {
-                db.execute_batch(&format!("ALTER TABLE channel_outbox ADD COLUMN {column} TEXT"))?;
+                tx.execute_batch(&format!("ALTER TABLE channel_outbox ADD COLUMN {column} TEXT"))?;
             }
         }
-        Self::backfill_message_keys(&mut db)?;
+        Self::backfill_message_keys(&tx)?;
+        tx.commit()?;
         Ok(Self { db: Arc::new(Mutex::new(db)) })
     }
 
     /// Chunks queued before message_key existed carry it only inside their own key ("reply:0",
     /// "reply:1"). Without it, giving up a message would fail its head chunk and still send the tail.
-    fn backfill_message_keys(db: &mut Connection) -> Result<()> {
-        let tx = Self::write_tx(db)?;
+    fn backfill_message_keys(tx: &rusqlite::Transaction<'_>) -> Result<()> {
         let rows: Vec<(i64, String)> = tx
             .prepare("SELECT id, idempotency_key FROM channel_outbox WHERE message_key IS NULL")?
             .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
             .collect::<rusqlite::Result<_>>()?;
         for (id, key) in rows {
             if let Some((message, index)) = key.rsplit_once(':') {
                 if !message.is_empty() && !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) {
                     tx.execute("UPDATE channel_outbox SET message_key=?1 WHERE id=?2", params![message, id])?;
                 }
             }
         }
-        tx.commit()?;
         Ok(())
     }
 
     /// Writers take the database lock up front. A deferred transaction that reads first and
     /// upgrades later fails at once with "database is locked" when another process got there
     /// between the two, without waiting for busy_timeout.
     fn write_tx(db: &mut Connection) -> rusqlite::Result<rusqlite::Transaction<'_>> {
         db.transaction_with_behavior(TransactionBehavior::Immediate)
     }
 
     /// Fails every remaining chunk of the logical message `row` belongs to.
     fn fail_message(tx: &rusqlite::Transaction<'_>, row: i64) -> rusqlite::Result<()> {
@@ -636,31 +641,35 @@ mod tests {
         assert!(store.due(ChannelKind::Telegram, "bot-1", 319, 1).unwrap().is_empty());
         assert_eq!(store.due(ChannelKind::Telegram, "bot-1", 320, 1).unwrap().len(), 1);
     }
 
     #[test]
     fn two_processes_sharing_the_spool_wait_for_each_other_instead_of_failing_at_once() {
         let dir = tempfile::tempdir().unwrap();
         let path = dir.path().join("channels.db");
         ChannelStore::open(&path).unwrap();
         // Before the fix, about 35 of 1,200 operations failed after a millisecond. Under this much
         // contention a slow disk (Windows CI) can still starve a writer past busy_timeout; that is
         // the timeout working, so only a failure that did not wait it out counts.
+        // Both processes start up first, as Heron's do, then work at the same time.
         let early = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
+        let started = std::sync::Arc::new(std::sync::Barrier::new(2));
         let workers: Vec<_> = (0..2)
             .map(|w| {
-                let (path, early) = (path.clone(), early.clone());
+                let (path, early, started) = (path.clone(), early.clone(), started.clone());
                 std::thread::spawn(move || {
                     let start = std::time::Instant::now();
-                    let store = match ChannelStore::open(&path) {
+                    let opened = ChannelStore::open(&path);
+                    started.wait();
+                    let store = match opened {
                         Ok(store) => store,
                         Err(e) => {
                             early.lock().unwrap().push(format!("open: {e:#} after {:?}", start.elapsed()));
                             return;
                         }
                     };
                     let timed = |what: &str, op: &dyn Fn() -> Result<()>| {
                         let start = std::time::Instant::now();
                         if let Err(e) = op() {
                             if start.elapsed() < std::time::Duration::from_secs(4) {
                                 early.lock().unwrap().push(format!("{what}: {e:#} after {:?}", start.elapsed()));
                             }
@@ -671,24 +680,49 @@ mod tests {
                         timed("due", &|| store.due(ChannelKind::Telegram, "bot-1", 0, 1).map(|_| ()));
                     }
                 })
             })
             .collect();
         for w in workers {
             w.join().unwrap();
         }
         let early = early.lock().unwrap();
         assert!(early.is_empty(), "{} operations failed without waiting, first: {:?}", early.len(), early.first());
     }
 
+    #[test]
+    fn two_processes_upgrading_an_old_spool_at_once_both_open_it() {
+        for _ in 0..20 {
+            let dir = tempfile::tempdir().unwrap();
+            let path = dir.path().join("channels.db");
+            ChannelStore::open(&path).unwrap();
+            Connection::open(&path).unwrap()
+                .execute_batch("ALTER TABLE channel_outbox DROP COLUMN lease_token; ALTER TABLE channel_outbox DROP COLUMN message_key;")
+                .unwrap();
+            let go = std::sync::Arc::new(std::sync::Barrier::new(2));
+            let openers: Vec<_> = (0..2)
+                .map(|_| {
+                    let (path, go) = (path.clone(), go.clone());
+                    std::thread::spawn(move || {
+                        go.wait();
+                        ChannelStore::open(&path).map(|_| ()).map_err(|e| format!("{e:#}"))
+                    })
+                })
+                .collect();
+            for opener in openers {
+                opener.join().unwrap().expect("a second Heron process failed to open a spool being upgraded");
+            }
+        }
+    }
+
     #[test]
     fn opening_the_spool_waits_while_another_process_is_writing() {
         let dir = tempfile::tempdir().unwrap();
         let path = dir.path().join("channels.db");
         ChannelStore::open(&path).unwrap();
         // Another process in the middle of a commit: nobody may read or write until it finishes.
         let writer = Connection::open(&path).unwrap();
         writer.execute_batch("BEGIN EXCLUSIVE").unwrap();
         let release = std::thread::spawn(move || {
             std::thread::sleep(std::time::Duration::from_millis(300));
             writer.execute_batch("COMMIT").unwrap();
         });
```

## source case 3

- fix: `a2cf3ea12a9377817f7ee8b08fc8b17ffa6be1ce`
- base: `3d53be81b2a2d8bdcd42c2f146497d7cd7266f91`
- subject: fix(channel): retry opening the spool while another process is committing
- files: src/channel.rs 

Commit message body:

```text
Windows CI showed ChannelStore::open failing at once with "database is locked"
while a second process wrote to the spool, which would leave that process's
Telegram gateway disabled. open now retries busy errors for up to 5 s, the
same budget as busy_timeout.

The two-process test now records open failures with timing instead of
panicking, and a new test holds an exclusive lock while opening.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

```

File `src/channel.rs`: 857 lines at base, 898 at fix. Last 4 lines identical at base and fix: **yes**.

Last 4 lines at base:
```rust
        assert!(store.accept_inbound(&owner(), &bad, 1).is_err());
        assert!(store.enqueue(&owner(), None, "ok", "", 1).is_err());
    }
}
```

The fix (`git show -U12`):

```diff
diff --git a/src/channel.rs b/src/channel.rs
index 26edf97..5ea45f3 100644
--- a/src/channel.rs
+++ b/src/channel.rs
@@ -123,24 +123,31 @@ fn check_text(text: &str) -> Result<()> {
     }
     Ok(())
 }
 
 /// A shared database handle lets several asynchronous adapters atomically
 /// deduplicate event IDs and serialize delivery acknowledgements. SQLite is
 /// intentionally separate from HERON's agent DB migration lifecycle.
 #[derive(Clone)]
 pub struct ChannelStore {
     db: Arc<Mutex<Connection>>,
 }
 
+fn is_busy(err: &anyhow::Error) -> bool {
+    matches!(
+        err.downcast_ref::<rusqlite::Error>(),
+        Some(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::DatabaseBusy
+    )
+}
+
 impl ChannelStore {
     pub fn open(path: &Path) -> Result<Self> {
         if let Some(parent) = path.parent() {
             std::fs::create_dir_all(parent)?;
         }
         // Never follow a pre-existing symlink to an arbitrary database or
         // chmod another file through a symlinked channels.db path.
         match std::fs::symlink_metadata(path) {
             Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
                 bail!("The channel spool path must be a regular file, not a symlink");
             }
             Ok(_) => {}
@@ -155,26 +162,36 @@ impl ChannelStore {
             options.write(true).create_new(true).mode(0o600);
             match options.open(path) {
                 Ok(_) => {}
                 Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
                 Err(err) => return Err(err.into()),
             }
         }
         #[cfg(unix)]
         {
             use std::os::unix::fs::PermissionsExt;
             std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
         }
-        let db = Connection::open(path)?;
-        Self::init(db)
+        // On Windows, setting up the schema can fail with "database is locked" at once, without
+        // waiting on busy_timeout, while another Heron process is committing. Retry it the way
+        // busy_timeout would.
+        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
+        loop {
+            match Self::init(Connection::open(path)?) {
+                Err(err) if is_busy(&err) && std::time::Instant::now() < deadline => {
+                    std::thread::sleep(std::time::Duration::from_millis(25));
+                }
+                other => return other,
+            }
+        }
     }
 
     #[cfg(test)]
     fn in_memory() -> Result<Self> {
         Self::init(Connection::open_in_memory()?)
     }
 
     fn init(mut db: Connection) -> Result<Self> {
         db.busy_timeout(std::time::Duration::from_secs(5))?;
         db.execute_batch(
             "PRAGMA journal_mode=DELETE;
              PRAGMA synchronous=FULL;
@@ -624,47 +641,71 @@ mod tests {
     fn two_processes_sharing_the_spool_wait_for_each_other_instead_of_failing_at_once() {
         let dir = tempfile::tempdir().unwrap();
         let path = dir.path().join("channels.db");
         ChannelStore::open(&path).unwrap();
         // Before the fix, about 35 of 1,200 operations failed after a millisecond. Under this much
         // contention a slow disk (Windows CI) can still starve a writer past busy_timeout; that is
         // the timeout working, so only a failure that did not wait it out counts.
         let early = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
         let workers: Vec<_> = (0..2)
             .map(|w| {
                 let (path, early) = (path.clone(), early.clone());
                 std::thread::spawn(move || {
-                    let store = ChannelStore::open(&path).unwrap();
-                    let timed = |op: &dyn Fn() -> Result<()>| {
+                    let start = std::time::Instant::now();
+                    let store = match ChannelStore::open(&path) {
+                        Ok(store) => store,
+                        Err(e) => {
+                            early.lock().unwrap().push(format!("open: {e:#} after {:?}", start.elapsed()));
+                            return;
+                        }
+                    };
+                    let timed = |what: &str, op: &dyn Fn() -> Result<()>| {
                         let start = std::time::Instant::now();
                         if let Err(e) = op() {
                             if start.elapsed() < std::time::Duration::from_secs(4) {
-                                early.lock().unwrap().push(format!("{e:#} after {:?}", start.elapsed()));
+                                early.lock().unwrap().push(format!("{what}: {e:#} after {:?}", start.elapsed()));
                             }
                         }
                     };
                     for i in 0..300 {
-                        timed(&|| store.enqueue(&owner(), None, "hi", &format!("w{w}-{i}"), 0).map(|_| ()));
-                        timed(&|| store.due(ChannelKind::Telegram, "bot-1", 0, 1).map(|_| ()));
+                        timed("enqueue", &|| store.enqueue(&owner(), None, "hi", &format!("w{w}-{i}"), 0).map(|_| ()));
+                        timed("due", &|| store.due(ChannelKind::Telegram, "bot-1", 0, 1).map(|_| ()));
                     }
                 })
             })
             .collect();
         for w in workers {
             w.join().unwrap();
         }
         let early = early.lock().unwrap();
         assert!(early.is_empty(), "{} operations failed without waiting, first: {:?}", early.len(), early.first());
     }
 
+    #[test]
+    fn opening_the_spool_waits_while_another_process_is_writing() {
+        let dir = tempfile::tempdir().unwrap();
+        let path = dir.path().join("channels.db");
+        ChannelStore::open(&path).unwrap();
+        // Another process in the middle of a commit: nobody may read or write until it finishes.
+        let writer = Connection::open(&path).unwrap();
+        writer.execute_batch("BEGIN EXCLUSIVE").unwrap();
+        let release = std::thread::spawn(move || {
+            std::thread::sleep(std::time::Duration::from_millis(300));
+            writer.execute_batch("COMMIT").unwrap();
+        });
+        let opened = ChannelStore::open(&path);
+        release.join().unwrap();
+        opened.expect("a second Heron process could not open the spool while the first was writing");
+    }
+
     #[test]
     fn replies_queued_before_the_upgrade_are_still_given_up_whole() {
         let dir = tempfile::tempdir().unwrap();
         let path = dir.path().join("channels.db");
         ChannelStore::open(&path).unwrap().enqueue_parts(&owner(), None, &["head".into(), "tail".into()], "old-reply", 100).unwrap();
         // What an older build left behind: chunks with no logical message key.
         Connection::open(&path).unwrap().execute("UPDATE channel_outbox SET message_key=NULL", []).unwrap();
         let store = ChannelStore::open(&path).unwrap();
         let head = store.due(ChannelKind::Telegram, "bot-1", 100, 1).unwrap().remove(0);
         assert_eq!(head.text, "head");
         assert_eq!(store.fail(head.id, &owner(), &head.lease).unwrap(), RetryOutcome::GaveUp);
         assert!(store.due(ChannelKind::Telegram, "bot-1", 100, 5).unwrap().is_empty(), "the tail was sent without its head");
```

## source case 4

- fix: `2cdc0643e197a55d9fa560f7d0f7391acc4ae6cf`
- base: `01dee9e614ce631b33c59f6c87e7d16f6b8c42b2`
- subject: fix(oauth): restore the previous client ID when a reconnect fails to store its token
- files: src/oauth.rs 

Commit message body:

```text
A same-account reconnect with a different public client ID wrote the new
client ID before the refresh token. If the vault write then failed, the vault
restored the old refresh token, but it was left paired with the new client ID
and the working connection broke. The previous client ID is now put back.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

```

File `src/oauth.rs`: 563 lines at base, 596 at fix. Last 4 lines identical at base and fix: **yes**.

Last 4 lines at base:
```rust
        assert!(pairs.get("scope").unwrap().contains("Calendars.Read"));
        assert!(!pairs.get("scope").unwrap().contains("Mail.Send"));
    }
}
```

The fix (`git show -U12`):

```diff
diff --git a/src/oauth.rs b/src/oauth.rs
index 037f8f1..2ca8f13 100644
--- a/src/oauth.rs
+++ b/src/oauth.rs
@@ -217,34 +217,42 @@ fn install(cfg: &Config, provider: Provider, client_id: &str, token: &Value) ->
     let refresh = token.get("refresh_token").and_then(Value::as_str)
         .filter(|s| !s.is_empty())
         .ok_or_else(|| anyhow!("No offline access was granted. Remove the existing app consent and connect again."))?;
     let subject = account_subject(client_id, token)?;
     // Store the nonsecret client ID too so token refresh still works after an app restart.
     // The refresh token is not sent to the desktop frontend or an agent tool.
     // A different account needs an explicit Disconnect first. Holding
     // SOURCE_WRITES makes this credential installation atomic relative to
     // refresh-token rotation, source sync commits, and disconnect.
     let _guard = SOURCE_WRITES.lock().unwrap_or_else(|e| e.into_inner());
     let stored = cfg.secret(&provider.secret_key("account_subject"));
     let admitted = admit(provider.connected(cfg), stored.as_deref(), &subject)?;
-    cfg.store_secret(&provider.secret_key("client_id"), client_id)?;
+    let client_key = provider.secret_key("client_id");
+    let previous_client = cfg.secret(&client_key);
+    cfg.store_secret(&client_key, client_id)?;
     if admitted == Admit::Fresh {
         cfg.store_secret(&provider.secret_key("account_subject"), &subject)?;
         cfg.store_secret(&provider.secret_key("account_epoch"), &random_secret())?;
     }
     // The same account keeps its epoch, so its index and memories stay valid.
     // Publish the connection last, after its stable epoch is durable.
     // Unlike the legacy settings secret API this fails closed if Keychain or
     // Credential Manager is unavailable, and removes any old file fallback.
-    cfg.store_vault_only_secret(&provider.secret_key("refresh_token"), refresh)?;
+    if let Err(err) = cfg.store_vault_only_secret(&provider.secret_key("refresh_token"), refresh) {
+        // The vault put the previous refresh token back; it only works with the client ID it was issued to.
+        if let Err(undo) = cfg.store_secret(&client_key, previous_client.as_deref().unwrap_or("")) {
+            eprintln!("HERON OAuth: could not restore the previous {} client ID: {undo:#}", provider.name());
+        }
+        return Err(err);
+    }
     Ok(())
 }
 
 async fn access_token_with_generation(cfg: &Config, provider: Provider) -> Result<(String, String)> {
     let id = cfg.secret(&provider.secret_key("client_id"))
         .ok_or_else(|| anyhow!("This OAuth connection has no client ID. Please reconnect."))?;
     // Provider refresh tokens must never use Config::store_secret's legacy
     // owner-only plaintext-file fallback. Legacy installations must explicitly
     // disconnect and reconnect before their tokens may be used again.
     if !cfg.vault_only_secret_present(&provider.secret_key("refresh_token")) {
         bail!("OAuth credential is not in a verified native credential vault. Disconnect and reconnect this account.");
     }
@@ -458,24 +466,49 @@ mod tests {
         }
     }
 
     #[test]
     fn only_the_connected_account_may_sign_in_again_without_disconnecting() {
         assert_eq!(admit(false, None, "u1").unwrap(), Admit::Fresh);
         assert_eq!(admit(false, Some("old"), "u1").unwrap(), Admit::Fresh);
         assert_eq!(admit(true, Some("u1"), "u1").unwrap(), Admit::SameAccount);
         assert!(admit(true, Some("u2"), "u1").unwrap_err().to_string().contains("different account"));
         assert!(admit(true, None, "u1").unwrap_err().to_string().contains("Disconnect it"));
     }
 
+    /// Linux has no verified vault, so installing the refresh token always fails there: the same
+    /// failure a locked Keychain or Credential Manager gives on the desktop.
+    #[tokio::test]
+    #[cfg(not(any(target_os = "macos", windows)))]
+    async fn a_failed_reconnect_keeps_the_client_id_the_old_token_belongs_to() {
+        use crate::llm::ScriptedLlm;
+        use serde_json::json;
+        let dir = tempfile::tempdir().unwrap();
+        let mut cfg = Config::default();
+        cfg.home = dir.path().join("home");
+        cfg.ensure_home().unwrap();
+        std::fs::write(cfg.home.join("secrets.json"), json!({
+            "oauth_google_refresh_token": "working-refresh", "oauth_google_account_epoch": "e1",
+            "oauth_google_client_id": "old-cid", "oauth_google_account_subject": "user-1",
+        }).to_string()).unwrap();
+        let app = App::assemble(cfg, dir.path().join("ws"), crate::store::Store::in_memory().unwrap(), std::sync::Arc::new(ScriptedLlm::new(vec![]))).await.unwrap();
+        let p = Provider::Google;
+        let token = json!({"scope": p.scopes(), "refresh_token": "new-refresh", "id_token": id_token(json!({"aud": "new-cid", "sub": "user-1"}))});
+        assert!(install(&app.cfg(), p, "new-cid", &token).is_err());
+        let cfg = app.cfg();
+        assert_eq!(cfg.secret("oauth_google_client_id").as_deref(), Some("old-cid"));
+        assert_eq!(cfg.secret("oauth_google_refresh_token").as_deref(), Some("working-refresh"));
+        assert_eq!(p.account_epoch(&cfg).as_deref(), Some("e1"));
+    }
+
     /// Installing a credential needs the OS vault: run on an authorized desktop, and in CI's vault step.
     #[tokio::test]
     #[cfg(any(target_os = "macos", windows))]
     #[ignore = "requires an authorized, unlocked native credential vault"]
     async fn reconnecting_the_same_account_keeps_its_data_vault_live() {
         use crate::llm::ScriptedLlm;
         use serde_json::json;
         let dir = tempfile::tempdir().unwrap();
         let mut cfg = Config::default();
         cfg.home = dir.path().join("oauth-vault-qualification");
         let app = App::assemble(cfg, dir.path().join("ws"), crate::store::Store::in_memory().unwrap(), std::sync::Arc::new(ScriptedLlm::new(vec![]))).await.unwrap();
         let p = Provider::Google;
```

## source case 5

- fix: `3d53be81b2a2d8bdcd42c2f146497d7cd7266f91`
- base: `5ee7a8eb4671c4d6116546e007dcb49f7aaa89a1`
- subject: fix(channel): backfill message keys on upgrade, and time 429 waits from the response
- files: src/channel.rs src/telegram.rs 

Commit message body:

```text
- Chunks queued by an older build had no message_key, so giving up a reply
  failed only its head chunk and still sent the tail. Opening the spool now
  derives the key from the chunk's own key ("reply:0" -> "reply").
- retry_after now counts from when Telegram answered, not from before a
  request that may have taken many seconds.
- The two-process spool test now asserts what the fix guarantees: no busy
  error without waiting out busy_timeout. Windows CI's slow disk could starve
  one writer past the timeout under this contention. With deferred
  transactions it still fails (26 errors after about 1 ms).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

```

File `src/channel.rs`: 816 lines at base, 857 at fix. Last 4 lines identical at base and fix: **yes**.

Last 4 lines at base:
```rust
        assert!(store.accept_inbound(&owner(), &bad, 1).is_err());
        assert!(store.enqueue(&owner(), None, "ok", "", 1).is_err());
    }
}
```

File `src/telegram.rs`: 923 lines at base, 924 at fix. Last 4 lines identical at base and fix: **yes**.

Last 4 lines at base:
```rust
        assert!(!pairing_locked(&app));
        assert!(!wrong_code(&app), "and the count starts again");
    }
}
```

The fix (`git show -U12`):

```diff
diff --git a/src/channel.rs b/src/channel.rs
index 240a0f2..26edf97 100644
--- a/src/channel.rs
+++ b/src/channel.rs
@@ -164,25 +164,25 @@ impl ChannelStore {
             use std::os::unix::fs::PermissionsExt;
             std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
         }
         let db = Connection::open(path)?;
         Self::init(db)
     }
 
     #[cfg(test)]
     fn in_memory() -> Result<Self> {
         Self::init(Connection::open_in_memory()?)
     }
 
-    fn init(db: Connection) -> Result<Self> {
+    fn init(mut db: Connection) -> Result<Self> {
         db.busy_timeout(std::time::Duration::from_secs(5))?;
         db.execute_batch(
             "PRAGMA journal_mode=DELETE;
              PRAGMA synchronous=FULL;
              PRAGMA secure_delete=ON;
              CREATE TABLE IF NOT EXISTS channel_inbox(
                kind TEXT NOT NULL, account_id TEXT NOT NULL, chat_id TEXT NOT NULL,
                sender_id TEXT NOT NULL, provider_message_id TEXT NOT NULL,
                thread_id TEXT, text TEXT NOT NULL, received_at INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'accepted',
                PRIMARY KEY(kind, account_id, provider_message_id)
              );
@@ -202,27 +202,47 @@ impl ChannelStore {
              CREATE INDEX IF NOT EXISTS idx_channel_outbox_due
                ON channel_outbox(kind, account_id, status, next_attempt, id);",
         )?;
         for column in ["lease_token", "message_key"] {
             let present: bool = db.query_row(
                 "SELECT COUNT(*) FROM pragma_table_info('channel_outbox') WHERE name=?1",
                 [column], |r| r.get::<_, i64>(0),
             )? > 0;
             if !present {
                 db.execute_batch(&format!("ALTER TABLE channel_outbox ADD COLUMN {column} TEXT"))?;
             }
         }
+        Self::backfill_message_keys(&mut db)?;
         Ok(Self { db: Arc::new(Mutex::new(db)) })
     }
 
+    /// Chunks queued before message_key existed carry it only inside their own key ("reply:0",
+    /// "reply:1"). Without it, giving up a message would fail its head chunk and still send the tail.
+    fn backfill_message_keys(db: &mut Connection) -> Result<()> {
+        let tx = Self::write_tx(db)?;
+        let rows: Vec<(i64, String)> = tx
+            .prepare("SELECT id, idempotency_key FROM channel_outbox WHERE message_key IS NULL")?
+            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
+            .collect::<rusqlite::Result<_>>()?;
+        for (id, key) in rows {
+            if let Some((message, index)) = key.rsplit_once(':') {
+                if !message.is_empty() && !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) {
+                    tx.execute("UPDATE channel_outbox SET message_key=?1 WHERE id=?2", params![message, id])?;
+                }
+            }
+        }
+        tx.commit()?;
+        Ok(())
+    }
+
     /// Writers take the database lock up front. A deferred transaction that reads first and
     /// upgrades later fails at once with "database is locked" when another process got there
     /// between the two, without waiting for busy_timeout.
     fn write_tx(db: &mut Connection) -> rusqlite::Result<rusqlite::Transaction<'_>> {
         db.transaction_with_behavior(TransactionBehavior::Immediate)
     }
 
     /// Fails every remaining chunk of the logical message `row` belongs to.
     fn fail_message(tx: &rusqlite::Transaction<'_>, row: i64) -> rusqlite::Result<()> {
         tx.execute(
             "UPDATE channel_outbox SET status='failed',text='',lease_token=NULL
              WHERE status IN ('pending','leased') AND (kind,account_id,COALESCE(message_key,idempotency_key)) =
@@ -592,51 +612,72 @@ mod tests {
         let claim = store.due(ChannelKind::Telegram, "bot-1", 100, 1).unwrap().remove(0);
         assert_eq!(store.fail(claim.id, &owner(), &claim.lease).unwrap(), RetryOutcome::GaveUp);
         assert_eq!(store.count_failed().unwrap(), 2);
 
         store.enqueue(&owner(), None, "later", "limited", 200).unwrap();
         let claim = store.due(ChannelKind::Telegram, "bot-1", 200, 1).unwrap().remove(0);
         store.retry(claim.id, &owner(), &claim.lease, 200, Some(320)).unwrap();
         assert!(store.due(ChannelKind::Telegram, "bot-1", 319, 1).unwrap().is_empty());
         assert_eq!(store.due(ChannelKind::Telegram, "bot-1", 320, 1).unwrap().len(), 1);
     }
 
     #[test]
-    fn two_processes_sharing_the_spool_never_see_database_locked() {
+    fn two_processes_sharing_the_spool_wait_for_each_other_instead_of_failing_at_once() {
         let dir = tempfile::tempdir().unwrap();
         let path = dir.path().join("channels.db");
         ChannelStore::open(&path).unwrap();
-        let errors = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
+        // Before the fix, about 35 of 1,200 operations failed after a millisecond. Under this much
+        // contention a slow disk (Windows CI) can still starve a writer past busy_timeout; that is
+        // the timeout working, so only a failure that did not wait it out counts.
+        let early = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
         let workers: Vec<_> = (0..2)
             .map(|w| {
-                let (path, errors) = (path.clone(), errors.clone());
+                let (path, early) = (path.clone(), early.clone());
                 std::thread::spawn(move || {
                     let store = ChannelStore::open(&path).unwrap();
-                    for i in 0..300 {
-                        let step = store
-                            .enqueue(&owner(), None, "hi", &format!("w{w}-{i}"), 0)
-                            .and_then(|_| store.due(ChannelKind::Telegram, "bot-1", 0, 1))
-                            .map(|_| ());
-                        if let Err(e) = step {
-                            errors.lock().unwrap().push(e.to_string());
+                    let timed = |op: &dyn Fn() -> Result<()>| {
+                        let start = std::time::Instant::now();
+                        if let Err(e) = op() {
+                            if start.elapsed() < std::time::Duration::from_secs(4) {
+                                early.lock().unwrap().push(format!("{e:#} after {:?}", start.elapsed()));
+                            }
                         }
+                    };
+                    for i in 0..300 {
+                        timed(&|| store.enqueue(&owner(), None, "hi", &format!("w{w}-{i}"), 0).map(|_| ()));
+                        timed(&|| store.due(ChannelKind::Telegram, "bot-1", 0, 1).map(|_| ()));
                     }
                 })
             })
             .collect();
         for w in workers {
             w.join().unwrap();
         }
-        let errors = errors.lock().unwrap();
-        assert!(errors.is_empty(), "{} errors, first: {:?}", errors.len(), errors.first());
+        let early = early.lock().unwrap();
+        assert!(early.is_empty(), "{} operations failed without waiting, first: {:?}", early.len(), early.first());
+    }
+
+    #[test]
+    fn replies_queued_before_the_upgrade_are_still_given_up_whole() {
+        let dir = tempfile::tempdir().unwrap();
+        let path = dir.path().join("channels.db");
+        ChannelStore::open(&path).unwrap().enqueue_parts(&owner(), None, &["head".into(), "tail".into()], "old-reply", 100).unwrap();
+        // What an older build left behind: chunks with no logical message key.
+        Connection::open(&path).unwrap().execute("UPDATE channel_outbox SET message_key=NULL", []).unwrap();
+        let store = ChannelStore::open(&path).unwrap();
+        let head = store.due(ChannelKind::Telegram, "bot-1", 100, 1).unwrap().remove(0);
+        assert_eq!(head.text, "head");
+        assert_eq!(store.fail(head.id, &owner(), &head.lease).unwrap(), RetryOutcome::GaveUp);
+        assert!(store.due(ChannelKind::Telegram, "bot-1", 100, 5).unwrap().is_empty(), "the tail was sent without its head");
+        assert_eq!(store.count_failed().unwrap(), 2);
     }
 
     fn inbound(message_id: &str) -> InboundMessage {
         InboundMessage {
             identity: owner(),
             provider_message_id: message_id.into(),
             thread_id: None,
             text: "hello".into(),
         }
     }
 
     #[test]
diff --git a/src/telegram.rs b/src/telegram.rs
index c159c43..24de82e 100644
--- a/src/telegram.rs
+++ b/src/telegram.rs
@@ -158,36 +158,37 @@ impl Bot {
             let chat = match msg.identity.chat_id.parse::<i64>() {
                 Ok(chat) => chat,
                 Err(_) => {
                     let _ = self.spool.fail(msg.id, &msg.identity, &msg.lease);
                     continue;
                 }
             };
             if !self.is_current_owner(chat) {
                 // Revocation must not leak a delayed response to a former owner.
                 let _ = self.spool.purge_account(ChannelKind::Telegram, &self.account_id);
                 break;
             }
-            let now = crate::util::now_ts();
             // Provider errors may echo the bot URL, which embeds its token: never log them.
             let outcome = match self.send_message(chat, &msg.text).await {
                 Delivery::Sent(remote_id) => {
                     if let Err(err) = self.spool.delivered(msg.id, &msg.identity, &msg.lease, remote_id.as_deref()) {
                         eprintln!("HERON Telegram: reply acknowledgement failed: {err}");
                         break;
                     }
                     continue;
                 }
                 Delivery::Retry { after } => {
                     eprintln!("HERON Telegram: delivery failed; retry queued.");
+                    // Telegram's retry_after counts from its answer, which can come many seconds after the request.
+                    let now = crate::util::now_ts();
                     self.spool.retry(msg.id, &msg.identity, &msg.lease, now, after.map(|s| now.saturating_add(s)))
                 }
                 Delivery::Refused => {
                     eprintln!("HERON Telegram: Telegram refused a reply; it will not be retried.");
                     self.spool.fail(msg.id, &msg.identity, &msg.lease)
                 }
             };
             if matches!(outcome, Ok(RetryOutcome::GaveUp)) {
                 let _ = self.app.store.add_inbox(
                     UNDELIVERED_TITLE,
                     "Heron could not send a reply to your Telegram chat and stopped trying. Open the conversation in Heron to read it.",
                 );
```

## source case 6

- fix: `31dc11592b7ceb4e56a6599de1305161308e9948`
- base: `d20eadd3c959ec49673026a0683f604e8d230844`
- subject: fix(oauth): erase account data before its credentials on disconnect (O26-4)
- files: src/oauth.rs src/store.rs 

Commit message body:

```text
Disconnect removed the refresh token first. Any later failure hid the account
from Settings (and its Disconnect button) while its index and memories stayed.
Data now goes first, so a failure leaves the account listed and a retry works.

The refresh token is removed through delete_vault_only_secret, which checks it
is gone, plus the fallback file for tokens saved before the verified vault.
Disconnect is idempotent.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

```

File `src/oauth.rs`: 411 lines at base, 457 at fix. Last 4 lines identical at base and fix: **yes**.

Last 4 lines at base:
```rust
        assert!(pairs.get("scope").unwrap().contains("Calendars.Read"));
        assert!(!pairs.get("scope").unwrap().contains("Mail.Send"));
    }
}
```

File `src/store.rs`: 2017 lines at base, 2022 at fix. Last 4 lines identical at base and fix: **yes**.

Last 4 lines at base:
```rust
        db.replace_knowledge_snapshot("google_email", &[]).unwrap();
        assert_eq!(db.knowledge_count("google_email"), 0);
    }
}
```

The fix (`git show -U12`):

```diff
diff --git a/src/oauth.rs b/src/oauth.rs
index f5c3cc5..2b67693 100644
--- a/src/oauth.rs
+++ b/src/oauth.rs
@@ -227,33 +227,45 @@ async fn access_token_with_generation(cfg: &Config, provider: Provider) -> Resul
 }
 
 /// The credential generation is captured atomically with the access token.
 /// Callers syncing account data must validate that generation when committing
 /// fetched records, rather than sampling it after the token was issued.
 pub async fn access_token(cfg: &Config, provider: Provider) -> Result<String> {
     access_token_with_generation(cfg, provider).await.map(|(token, _)| token)
 }
 
 pub fn disconnect(app: &App, provider: Provider) -> Result<(usize, usize)> {
     let _guard = SOURCE_WRITES.lock().unwrap_or_else(|e| e.into_inner());
     let cfg = app.cfg();
-    cfg.store_secret(&provider.secret_key("refresh_token"), "")?;
-    cfg.store_secret(&provider.secret_key("account_epoch"), "")?;
-    cfg.store_secret(&provider.secret_key("client_id"), "")?;
+    // Data first. If anything fails, the account still shows as connected and Disconnect can be
+    // tried again; removing the credential first would hide that button and strand the data.
     let mut removed = 0;
     let mut memories = 0;
     for source in [provider.mail_source(), provider.calendar_source()] {
         removed += app.store.clear_knowledge(source)?;
         memories += app.store.delete_memories_by_source(source)?;
     }
+    let refresh = provider.secret_key("refresh_token");
+    cfg.delete_vault_only_secret(&refresh)?;
+    if cfg.secret(&refresh).is_some() {
+        // A token saved before the verified vault existed lives in the fallback file.
+        cfg.store_secret(&refresh, "")?;
+    }
+    // Neither is secret, and the next connection replaces both. With the token gone, a failure
+    // here leaves nothing that can sync.
+    for leftover in ["account_epoch", "client_id"] {
+        if let Err(err) = cfg.store_secret(&provider.secret_key(leftover), "") {
+            eprintln!("HERON OAuth: {} disconnected, but its {leftover} could not be cleared: {err:#}", provider.name());
+        }
+    }
     Ok((removed, memories))
 }
 
 async fn get_json(http: &reqwest::Client, token: &str, url: &str, params: &[(&str, &str)]) -> Result<Value> {
     let mut request = http.get(url).bearer_auth(token).query(params);
     if url.starts_with("https://graph.microsoft.com/") {
         // Graph DateTimeTimeZone fields are otherwise returned in account-local time.
         request = request.header("Prefer", "outlook.timezone=\"UTC\"");
     }
     let result = request.send().await?;
     if !result.status().is_success() {
         bail!("Read-only account sync failed (HTTP {}). The token may be expired or its scopes insufficient.", result.status().as_u16());
@@ -372,24 +384,58 @@ pub async fn sync(app: &App, provider: Provider) -> Result<(usize, usize)> {
     }
     let rows = |items: &[Record]| -> Vec<(String, String, String, i64)> {
         items.iter().map(|rec| (rec.id.clone(), rec.title.clone(), rec.body.clone(), rec.timestamp)).collect()
     };
     app.store.replace_knowledge_snapshot(provider.mail_source(), &rows(&email))?;
     app.store.replace_knowledge_snapshot(provider.calendar_source(), &rows(&calendar))?;
     Ok((email.len(), calendar.len()))
 }
 
 #[cfg(test)]
 mod tests {
     use super::*;
+
+    #[tokio::test]
+    async fn a_failed_disconnect_keeps_the_account_listed_so_it_can_be_retried() {
+        use crate::llm::ScriptedLlm;
+        let dir = tempfile::tempdir().unwrap();
+        let mut cfg = Config::default();
+        cfg.home = dir.path().join("home");
+        cfg.ensure_home().unwrap();
+        // A token from before the verified vault: the fallback file is the only copy.
+        std::fs::write(cfg.home.join("secrets.json"), serde_json::to_string(&serde_json::json!({
+            "oauth_google_refresh_token": "legacy-refresh", "oauth_google_account_epoch": "e1", "oauth_google_client_id": "cid",
+        })).unwrap()).unwrap();
+        let app = App::assemble(cfg, dir.path().join("ws"), crate::store::Store::in_memory().unwrap(), std::sync::Arc::new(ScriptedLlm::new(vec![]))).await.unwrap();
+        let p = Provider::Google;
+        assert!(p.connected(&app.cfg()));
+        app.store.replace_knowledge_snapshot(p.mail_source(), &[("m1".into(), "Invoice".into(), "Due Friday".into(), 1)]).unwrap();
+        app.memory.save("The invoice from Acme is due Friday", "fact", "", 0.5, p.mail_source()).await.unwrap();
+
+        app.store.execute_batch_for_test("CREATE TEMP TRIGGER stuck BEFORE DELETE ON knowledge BEGIN SELECT RAISE(ABORT, 'disk busy'); END;");
+        assert!(disconnect(&app, p).is_err());
+        assert!(p.connected(&app.cfg()), "the account vanished from Settings while its data stayed");
+        assert_eq!(app.store.knowledge_count(p.mail_source()), 1);
+
+        app.store.execute_batch_for_test("DROP TRIGGER stuck;");
+        assert_eq!(disconnect(&app, p).unwrap(), (1, 1));
+        let cfg = app.cfg();
+        assert!(!p.connected(&cfg));
+        assert_eq!(app.store.knowledge_count(p.mail_source()), 0);
+        assert!(cfg.secret("oauth_google_account_epoch").is_none() && cfg.secret("oauth_google_client_id").is_none());
+        assert!(!std::fs::read_to_string(cfg.home.join("secrets.json")).unwrap_or_default().contains("legacy-refresh"));
+        // Disconnecting again is harmless.
+        assert_eq!(disconnect(&app, p).unwrap(), (0, 0));
+    }
+
     #[test]
     fn incomplete_granular_google_consent_is_not_accepted_as_full_sync() {
         let required = Provider::Google.scopes();
         assert!(required.contains("gmail.readonly"));
         assert!(required.contains("calendar.readonly"));
     }
     #[test]
     fn pkce_is_sha256_base64url_without_padding() {
         assert_eq!(challenge("hello"), "LPJNul-wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ");
     }
     #[test]
     fn callbacks_are_only_local_and_bound_to_the_provider() {
diff --git a/src/store.rs b/src/store.rs
index a6357fd..06371fe 100644
--- a/src/store.rs
+++ b/src/store.rs
@@ -537,24 +537,29 @@ impl Store {
                 if Connection::open_with_flags(&copy, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).and_then(|c| c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))).map(|v| v >= SCHEMA_VERSION).unwrap_or(false) {
                     let _ = std::fs::remove_file(&copy);
                 }
                 migrate(&mut conn, STEPS).with_context(|| format!("upgrading {} failed and it was left as it was; a copy from before the attempt is at {}", path.display(), copy.display()))?;
                 prune_snapshots(path, "before-upgrade-", KEEP_UPGRADE_SNAPSHOTS);
             } else {
                 migrate(&mut conn, STEPS)?;
             }
         }
         Ok(Store { conn: Arc::new(Mutex::new(conn)), path: Some(path.to_path_buf()) })
     }
 
+    #[cfg(test)]
+    pub(crate) fn execute_batch_for_test(&self, sql: &str) {
+        self.db().execute_batch(sql).unwrap();
+    }
+
     pub fn in_memory() -> Result<Store> {
         let mut conn = Connection::open_in_memory()?;
         conn.busy_timeout(std::time::Duration::from_secs(5))?;
         conn.execute_batch(PRAGMAS)?;
         migrate(&mut conn, STEPS)?;
         Ok(Store { conn: Arc::new(Mutex::new(conn)), path: None })
     }
 
     /// The data format of the open database.
     pub fn schema_version(&self) -> Result<i64> {
         user_version(&self.db())
     }
```

## source case 7

- fix: `d20eadd3c959ec49673026a0683f604e8d230844`
- base: `c4251b1cdbd7d3ce1f7adc3bb7029de807a12a18`
- subject: fix(memory): keep per-account erasure from taking the user's facts or hiding older ones (O26-5)
- files: src/memory.rs src/store.rs 

Commit message body:

```text
- When the user (or agent) restates a fact first learned from an outside
  source, the fact is credited to them, so disconnecting that account no
  longer erases it.
- Erasing a source brings back the facts its rows had replaced. When a later
  fact from another source replaced that row, the later fact stays current and
  keeps the history link.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

```

File `src/memory.rs`: 501 lines at base, 539 at fix. Last 4 lines identical at base and fix: **yes**.

Last 4 lines at base:
```rust
        assert_eq!(learned.len(), 1);
        assert_eq!(m.core(5).unwrap()[0].kind, "profile");
    }
}
```

File `src/store.rs`: 1984 lines at base, 2017 at fix. Last 4 lines identical at base and fix: **yes**.

Last 4 lines at base:
```rust
        db.replace_knowledge_snapshot("google_email", &[]).unwrap();
        assert_eq!(db.knowledge_count("google_email"), 0);
    }
}
```

The fix (`git show -U12`):

```diff
diff --git a/src/memory.rs b/src/memory.rs
index 3c064a9..ac6e151 100644
--- a/src/memory.rs
+++ b/src/memory.rs
@@ -206,24 +206,27 @@ impl Memory {
         for (id, stored) in self.vectors(embedder, emb.len())? {
             let sim = cosine(&emb, &stored);
             if sim >= threshold && best.map(|b| sim > b.1).unwrap_or(true) {
                 best = Some((id, sim));
             }
         }
         let target = replaces.or(best.map(|b| b.0));
         if let Some(old) = target.and_then(|id| self.store.get_memory(id).ok().flatten()) {
             let merged_tags = merge_tags(&old.tags, tags);
             let same = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
             if same(&old.content) == same(&content) {
                 self.store.update_memory(old.id, &content, &merged_tags, old.importance.max(importance), Some(&to_blob(&emb)), old.confirmed || confirmed, embedder)?;
+                if confirmed && OUTSIDE_SOURCES.contains(&old.source.as_str()) {
+                    self.store.set_memory_source(old.id, source)?;
+                }
                 return Ok(SaveOutcome::Updated(old.id));
             }
             if old.confirmed && !confirmed {
                 // Outside material disagrees with something the user vouched for: keep both, retire nothing.
                 let id = self.store.insert_memory(kind, &content, tags, importance, source, Some(&to_blob(&emb)), false, embedder)?;
                 return Ok(SaveOutcome::Created(id));
             }
             let id = self.store.insert_memory(kind, &content, &merged_tags, old.importance.max(importance), source, Some(&to_blob(&emb)), confirmed, embedder)?;
             self.store.supersede_memory(old.id, id)?;
             return Ok(SaveOutcome::Updated(id));
         }
         let id = self.store.insert_memory(kind, &content, tags, importance, source, Some(&to_blob(&emb)), confirmed, embedder)?;
@@ -377,24 +380,59 @@ mod tests {
         m.save("The user is allergic to peanuts", "profile", "health", 0.9, "").await.unwrap();
         m.save("The user's sister Maria lives in Lisbon", "person", "family", 0.6, "").await.unwrap();
         m.save("Project Falcon ships its beta in March", "project", "work", 0.7, "").await.unwrap();
         let r = m.search("what food allergies should I know about? peanut", 3).await.unwrap();
         assert!(r[0].content.contains("peanuts"));
         let r = m.search("where does Maria live", 3).await.unwrap();
         assert!(r[0].content.contains("Lisbon"));
         // typo tolerance via character n-grams
         let r = m.search("projct falcn", 3).await.unwrap();
         assert!(r[0].content.contains("Falcon"));
     }
 
+    #[tokio::test]
+    async fn a_fact_the_user_also_states_survives_erasing_the_account_that_taught_it() {
+        let m = mem();
+        let fact = m.save("The dentist is Dr Lee on Main Street", "person", "", 0.5, "google_email").await.unwrap();
+        assert_eq!(m.save("The dentist is Dr Lee on Main Street.", "person", "", 0.5, "agent").await.unwrap(), SaveOutcome::Updated(fact.id()));
+        assert_eq!(m.store.delete_memories_by_source("google_email").unwrap(), 0);
+        let kept = m.store.get_memory(fact.id()).unwrap().expect("the user's own statement was erased");
+        assert_eq!((kept.source.as_str(), kept.confirmed), ("agent", true));
+        // Outside material restating an outside fact does not take it over.
+        let other = m.save("The gym opens at six", "fact", "", 0.5, "email").await.unwrap();
+        m.save("The gym opens at six", "fact", "", 0.5, "google_email").await.unwrap();
+        assert_eq!(m.store.get_memory(other.id()).unwrap().unwrap().source, "email");
+    }
+
+    #[tokio::test]
+    async fn erasing_an_account_brings_back_what_it_replaced_unless_something_newer_did() {
+        let m = mem();
+        let old = m.save("The office is on the third floor", "fact", "", 0.5, "email").await.unwrap().id();
+        let from_google = m.save_replacing("The office is on the fifth floor", "fact", "", 0.5, "google_email", Some(old)).await.unwrap().id();
+        assert!(m.store.get_memory(old).unwrap().is_none());
+        assert_eq!(m.store.delete_memories_by_source("google_email").unwrap(), 1);
+        assert!(m.store.get_memory(from_google).unwrap().is_none());
+        assert!(m.store.get_memory(old).unwrap().is_some(), "the fact Google replaced stayed hidden");
+
+        // email -> google -> user: the user's version stays current and the old one stays history.
+        let first = m.save("Parking is in lot B", "fact", "", 0.5, "email").await.unwrap().id();
+        let middle = m.save_replacing("Parking is in lot C", "fact", "", 0.5, "google_email", Some(first)).await.unwrap().id();
+        let last = m.save_replacing("Parking is in lot D", "fact", "", 0.5, "agent", Some(middle)).await.unwrap().id();
+        m.store.delete_memories_by_source("google_email").unwrap();
+        assert!(m.store.get_memory(first).unwrap().is_none(), "an outdated fact came back");
+        assert!(m.store.get_memory(last).unwrap().is_some());
+        let history = m.store.memory_history(last).unwrap();
+        assert_eq!(history, ["Parking is in lot B"], "the old version lost its link");
+    }
+
     #[tokio::test]
     async fn near_duplicates_update_instead_of_duplicating() {
         let m = mem();
         let a = m.save("The user prefers dark mode in all apps", "preference", "ui", 0.5, "").await.unwrap();
         let b = m.save("The user prefers dark mode in all apps.", "preference", "theme", 0.8, "").await.unwrap();
         assert!(matches!(a, SaveOutcome::Created(_)));
         assert_eq!(b, SaveOutcome::Updated(a.id()));
         let all = m.store.list_memories(None, 10).unwrap();
         assert_eq!(all.len(), 1);
         assert_eq!(all[0].tags, "theme,ui");
         assert!((all[0].importance - 0.8).abs() < 1e-9);
     }
diff --git a/src/store.rs b/src/store.rs
index 4dd18d9..a6357fd 100644
--- a/src/store.rs
+++ b/src/store.rs
@@ -1,20 +1,21 @@
 //! SQLite persistence: sessions, messages, memories (FTS5 + vectors), tasks,
 //! schedules, inbox, audit log and file checkpoints. One file, fully local.
 
 use crate::llm::Message;
 use crate::util::now_ts;
 use anyhow::{anyhow, bail, Context, Result};
 use rusqlite::{params, Connection, OptionalExtension};
 use serde::Serialize;
+use std::collections::HashMap;
 use std::path::{Path, PathBuf};
 use std::sync::{Arc, Mutex, MutexGuard};
 
 fn audit_hash(prev: &str, ts: i64, session: &str, tool: &str, args: &str, decision: &str, result: &str) -> String {
     use sha2::{Digest, Sha256};
     let mut h = Sha256::new();
     // Length-prefix each field so that no two different entries can produce the same input.
     for field in [prev, &ts.to_string(), session, tool, args, decision, result] {
         h.update((field.len() as u64).to_le_bytes());
         h.update(field.as_bytes());
     }
     h.finalize().iter().map(|b| format!("{b:02x}")).collect()
@@ -1370,26 +1371,58 @@ impl Store {
             let _ = db.execute("UPDATE knowledge SET digested=1 WHERE id=?", [id]);
         }
     }
 
     pub fn knowledge_count(&self, source: &str) -> i64 {
         self.db().query_row("SELECT COUNT(*) FROM knowledge WHERE source=?", [source], |r| r.get(0)).unwrap_or(0)
     }
 
     pub fn clear_knowledge(&self, source: &str) -> Result<usize> {
         Ok(self.db().execute("DELETE FROM knowledge WHERE source=?", [source])?)
     }
 
+    /// Erase everything one source taught. A fact it retired comes back, unless a fact from another
+    /// source later replaced it: then that later fact stays the current version.
     pub fn delete_memories_by_source(&self, source: &str) -> Result<usize> {
-        Ok(self.db().execute("DELETE FROM memories WHERE source=?", [source])?)
+        let mut db = self.db();
+        let tx = db.transaction()?;
+        let next: HashMap<i64, Option<i64>> = tx
+            .prepare("SELECT id, superseded_by FROM memories WHERE source=?")?
+            .query_map([source], |r| Ok((r.get(0)?, r.get(1)?)))?
+            .collect::<rusqlite::Result<_>>()?;
+        let survivors: Vec<(i64, i64)> = tx
+            .prepare("SELECT id, superseded_by FROM memories WHERE source<>?1 AND superseded_by IN (SELECT id FROM memories WHERE source=?1)")?
+            .query_map([source], |r| Ok((r.get(0)?, r.get(1)?)))?
+            .collect::<rusqlite::Result<_>>()?;
+        for (id, mut successor) in survivors {
+            // Skip over the versions being erased to the first one that stays.
+            while let Some(Some(later)) = next.get(&successor) {
+                successor = *later;
+            }
+            if next.contains_key(&successor) {
+                tx.execute("UPDATE memories SET invalid_at=NULL, superseded_by=NULL WHERE id=?", [id])?;
+            } else {
+                tx.execute("UPDATE memories SET superseded_by=? WHERE id=?", params![successor, id])?;
+            }
+        }
+        let deleted = tx.execute("DELETE FROM memories WHERE source=?", [source])?;
+        tx.commit()?;
+        Ok(deleted)
+    }
+
+    /// Credit a fact to whoever stated it last with more authority, so erasing an outside source
+    /// never takes a fact the user also stated.
+    pub fn set_memory_source(&self, id: i64, source: &str) -> Result<()> {
+        self.db().execute("UPDATE memories SET source=? WHERE id=?", params![source, id])?;
+        Ok(())
     }
 
     // ---------- suggestions (proactive work) ----------
 
     pub fn add_suggestion(&self, title: &str, why: &str, prompt: &str) -> Result<i64> {
         let db = self.db();
         db.execute("INSERT INTO suggestions(ts, title, why, prompt) VALUES (?,?,?,?)", params![now_ts(), title, why, prompt])?;
         Ok(db.last_insert_rowid())
     }
 
     /// Newest first. `open_only` keeps what still deserves the user's attention.
     pub fn list_suggestions(&self, open_only: bool, limit: usize) -> Result<Vec<Suggestion>> {
```

## source case 8

- fix: `5ee7a8eb4671c4d6116546e007dcb49f7aaa89a1`
- base: `c239be629b75fe7e3b06844b7e72fbba22583d7a`
- subject: fix(telegram): one gateway per home, and check a new token before it replaces the old one
- files: src/server.rs src/telegram.rs tests/integration.rs 

Commit message body:

```text
- The gateway holds an OS lock on telegram.lock for its lifetime, so `serve`
  and `daemon` sharing a home never both poll the bot. A second gateway waits
  for the first to stop (C35-2c).
- Replacing or removing the token bumps a generation key in the shared store.
  A gateway in another process checks it before and after each poll and stops,
  which also aborts its running phone turns (C35-2c).
- set_telegram calls getMe with a changed token before storing it. A wrong or
  revoked token now returns an error and leaves the pairing, offset and stored
  token as they were; error text never includes the token (C35-1).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

```

File `src/server.rs`: 1268 lines at base, 1275 at fix. Last 4 lines identical at base and fix: **yes**.

Last 4 lines at base:
```rust
            Err(mpsc::error::TrySendError::Closed(_))
        ));
    }
}
```

File `src/telegram.rs`: 758 lines at base, 923 at fix. Last 4 lines identical at base and fix: **yes**.

Last 4 lines at base:
```rust
        assert!(!pairing_locked(&app));
        assert!(!wrong_code(&app), "and the count starts again");
    }
}
```

The fix (`git show -U12`):

```diff
diff --git a/src/server.rs b/src/server.rs
index f70c41f..78f3535 100644
--- a/src/server.rs
+++ b/src/server.rs
@@ -1009,48 +1009,55 @@ async fn get_telegram(State(st): S) -> ApiResult {
     let configured = st.app.cfg().vault_only_secret_present("telegram_token");
     let paired = crate::telegram::paired_chat(&st.app).is_some();
     Ok(Json(json!({"configured": configured, "paired": paired, "code": if configured && !paired { Some(crate::telegram::show_pairing_code(&st.app)) } else { None }})))
 }
 
 async fn set_telegram(State(st): S, Json(b): Json<Value>) -> ApiResult {
     let token = b.get("token").and_then(|t| t.as_str()).map(str::trim).unwrap_or("");
     // Telegram bot tokens look like 123456789:AA...; reject anything else before it is stored or used in a URL.
     let valid = token.split_once(':').map(|(id, key)| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) && key.len() >= 20 && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')).unwrap_or(false);
     if !valid {
         return Err(bad("That does not look like a bot token. Create a bot with @BotFather in Telegram and paste the token it gives you."));
     }
+    let cfg = st.app.cfg();
+    let old_token = cfg.secret("telegram_token");
+    // A wrong or revoked token would otherwise unpair the phone and leave a gateway that never connects.
+    if old_token.as_deref() != Some(token) {
+        crate::telegram::verify_token(&cfg.telegram.api_base, token).await.map_err(|msg| bad(&msg))?;
+    }
     if !cfg!(any(target_os = "macos", windows)) {
         return Err(ApiError(StatusCode::NOT_IMPLEMENTED,
             "Telegram bot credentials require a verified native vault; Linux is not yet supported.".into()));
     }
-    let cfg = st.app.cfg();
-    let old_token = cfg.secret("telegram_token");
     // Install first or fail without changing the previous pairing/task.
     // No plaintext-file fallback is permitted for bot credentials.
     cfg.store_vault_only_secret("telegram_token", token)?;
     if old_token.as_deref() != Some(token) {
         crate::telegram::stop(&st.app);
+        // A gateway in another Heron process sharing this home stops on its next poll.
+        crate::telegram::bump_generation(&st.app);
         if let Some(old) = &old_token {
             crate::telegram::purge_token_spool(&st.app, old);
         }
         crate::telegram::unpair(&st.app);
         let _ = st.app.store.kv_set("telegram_offset", "0");
     }
     // Restarts the connection, so a replaced token is the one in use and there is only ever one reader.
     crate::telegram::spawn_if_configured(&st.app);
     get_telegram(State(st)).await
 }
 
 async fn remove_telegram(State(st): S) -> ApiResult {
     crate::telegram::stop(&st.app);
+    crate::telegram::bump_generation(&st.app);
     // Revocation must purge the account's channel spool before erasing
     // the bot token that identifies its records.
     crate::telegram::unpair(&st.app);
     let cfg = st.app.cfg();
     cfg.delete_vault_only_secret("telegram_token")?;
     // Clear any legacy fallback that may predate the secure connector vault.
     cfg.store_secret("telegram_token", "")?;
     Ok(Json(json!({"configured": false, "paired": false})))
 }
 
 // ---------- onboarding & context sources ----------
 
diff --git a/src/telegram.rs b/src/telegram.rs
index 127522e..c159c43 100644
--- a/src/telegram.rs
+++ b/src/telegram.rs
@@ -476,25 +476,96 @@ async fn forward_inbox(app: &App, bot: &Bot, chat: i64) {
             continue;
         }
         if bot.queue_with_key(chat, &format!("{}\n\n{}", m.title, m.body), &format!("inbox:{}", m.id)).await.is_ok() {
             // Inbox advancement means durably queued, not necessarily delivered.
             let _ = app.store.kv_set("telegram_inbox_last", &m.id.to_string());
         } else {
             break;
         }
     }
 }
 
 /// Run the gateway until the task is dropped. `api_base` is normally https://api.telegram.org.
+/// Changes whenever the bot token is replaced or removed. Every Heron process that shares this
+/// home reads it, so a gateway in another process notices that its token was revoked and stops.
+const GENERATION_KEY: &str = "telegram_generation";
+
+pub fn bump_generation(app: &App) {
+    let _ = app.store.kv_set(GENERATION_KEY, &uuid::Uuid::new_v4().simple().to_string());
+}
+
+fn generation(app: &App) -> String {
+    app.store.kv_get(GENERATION_KEY).unwrap_or_default()
+}
+
+/// One gateway per Heron home, across processes: two readers of one bot split its updates and
+/// race the spool. Waits for the current holder to stop, and gives up if the token changes first.
+async fn take_gateway_lock(app: &App, started: &str) -> Option<std::fs::File> {
+    let cfg = app.cfg();
+    let mut opts = std::fs::OpenOptions::new();
+    opts.write(true).create(true).truncate(false);
+    #[cfg(unix)]
+    {
+        use std::os::unix::fs::OpenOptionsExt;
+        opts.mode(0o600);
+    }
+    let file = match cfg.ensure_home().and_then(|_| Ok(opts.open(cfg.home.join("telegram.lock"))?)) {
+        Ok(file) => file,
+        Err(err) => {
+            eprintln!("HERON Telegram: gateway disabled because its lock file cannot open: {err}");
+            return None;
+        }
+    };
+    let mut told = false;
+    loop {
+        match file.try_lock() {
+            Ok(()) => return Some(file),
+            Err(std::fs::TryLockError::WouldBlock) => {}
+            Err(std::fs::TryLockError::Error(err)) => {
+                eprintln!("HERON Telegram: gateway disabled because its lock cannot be taken: {err}");
+                return None;
+            }
+        }
+        if generation(app) != started {
+            return None;
+        }
+        if !told {
+            eprintln!("HERON Telegram: another Heron process is running the gateway; waiting for it to stop.");
+            told = true;
+        }
+        tokio::time::sleep(Duration::from_millis(250)).await;
+    }
+}
+
+/// Asks Telegram whether a token belongs to a live bot, before it replaces the working one.
+/// Messages never include the token: reqwest errors carry the URL, which contains it.
+pub async fn verify_token(api_base: &str, token: &str) -> std::result::Result<(), String> {
+    let url = format!("{}/bot{token}/getMe", api_base.trim_end_matches('/'));
+    let unreachable = || "Heron could not reach Telegram to check the token. Check your connection and try again.".to_string();
+    let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().map_err(|_| unreachable())?;
+    let resp = client.get(url).send().await.map_err(|_| unreachable())?;
+    let status = resp.status();
+    let body: Value = resp.json().await.unwrap_or(Value::Null);
+    if status.is_success() && body["ok"] == true && body["result"]["is_bot"] == true {
+        return Ok(());
+    }
+    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::NOT_FOUND {
+        return Err("Telegram does not recognise that bot token. Copy it again from @BotFather.".into());
+    }
+    Err(format!("Telegram could not check the token right now (HTTP {}). Try again in a moment.", status.as_u16()))
+}
+
 pub async fn run(app: Arc<App>, api_base: String, token: String) {
+    let started = generation(&app);
+    let Some(_gateway_lock) = take_gateway_lock(&app, &started).await else { return };
     let spool = match ChannelStore::open(&app.cfg().home.join("channels.db")) {
         Ok(spool) => spool,
         Err(err) => {
             eprintln!("HERON Telegram: gateway disabled because its secure local queue cannot open: {err}");
             return;
         }
     };
     let mut bot = Bot {
         client: reqwest::Client::builder().timeout(Duration::from_secs(40)).build().expect("http client"),
         base: format!("{}/bot{token}", api_base.trim_end_matches('/')),
         app: app.clone(),
         account_id: format!("{:x}", Sha256::digest(token.as_bytes())),
@@ -515,31 +586,40 @@ pub async fn run(app: Arc<App>, api_base: String, token: String) {
     if let Ok(count) = bot.spool.mark_interrupted(ChannelKind::Telegram, &bot.account_id) {
         if count > 0 {
             if let Some(chat) = paired_chat(&app) {
                 bot.send(chat, &format!("{count} prior request(s) may have been interrupted. Please resend anything still needed.")).await;
             }
         }
     }
     let shared: Arc<Shared> = Default::default();
     let _abort_children_on_shutdown = AbortAgentsOnDrop(shared.clone());
     let mut offset: i64 = app.store.kv_get("telegram_offset").and_then(|v| v.parse().ok()).unwrap_or(0);
     let poll_secs = if api_base.contains("127.0.0.1") { 0 } else { 25 };
     loop {
+        if generation(&app) != started {
+            eprintln!("HERON Telegram: the bot token changed; this gateway stopped.");
+            return;
+        }
         let updates = match bot.call("getUpdates", json!({"offset": offset, "timeout": poll_secs, "allowed_updates": ["message"]})).await {
             Ok(u) => u,
             Err(_) => {
                 tokio::time::sleep(Duration::from_secs(if poll_secs == 0 { 1 } else { 10 })).await;
                 continue;
             }
         };
+        // A long poll can return after the token was replaced: those messages were sent to the old bot.
+        if generation(&app) != started {
+            eprintln!("HERON Telegram: the bot token changed; this gateway stopped.");
+            return;
+        }
         for u in updates.as_array().cloned().unwrap_or_default() {
             let Some(update_id) = u["update_id"].as_i64().filter(|id| *id > 0) else {
                 continue;
             };
             let next_offset = offset.max(update_id + 1);
             if let Some(chat) = u["message"]["chat"]["id"].as_i64() {
                 // Never authorize groups: someone else could post a command.
                 if u["message"]["chat"]["type"].as_str() == Some("private")
                     && u["message"]["from"]["id"].as_i64() == Some(chat) {
                     let text = u["message"]["text"].as_str().map(str::to_string);
                     match paired_chat(&app) {
                         Some(owner) if owner == chat => match text {
@@ -646,24 +726,109 @@ mod tests {
     use crate::llm::ScriptedLlm;
     use crate::store::Store;
 
     #[test]
     fn splits_telegram_replies_by_utf16_not_unicode_scalar_count() {
         let text = "😃".repeat(2400);
         let chunks = Bot::chunks(&text);
         assert_eq!(chunks.len(), 2);
         assert_eq!(chunks.concat(), text);
         assert!(chunks.iter().all(|part| part.encode_utf16().count() <= MAX_MESSAGE));
     }
 
+    async fn mock_telegram(reply: fn(&str) -> (u16, Value)) -> (String, Arc<Mutex<Vec<String>>>) {
+        let paths = Arc::new(Mutex::new(Vec::<String>::new()));
+        let router = axum::Router::new()
+            .fallback(move |axum::extract::State(p): axum::extract::State<Arc<Mutex<Vec<String>>>>, uri: axum::http::Uri| async move {
+                p.lock().unwrap().push(uri.path().to_string());
+                let (status, body) = reply(uri.path());
+                (axum::http::StatusCode::from_u16(status).unwrap(), axum::Json(body))
+            })
+            .with_state(paths.clone());
+        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
+        let base = format!("http://{}", listener.local_addr().unwrap());
+        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
+        (base, paths)
+    }
+
+    /// A separate Heron process: its own App and connection, the same home and database.
+    async fn heron_process(root: &std::path::Path) -> Arc<App> {
+        let mut cfg = Config::default();
+        cfg.home = root.join("home");
+        cfg.ensure_home().unwrap();
+        Arc::new(App::assemble(
+            cfg, root.join("ws"),
+            Store::open(&root.join("home/heron.db")).unwrap(),
+            Arc::new(ScriptedLlm::new(vec![])),
+        ).await.unwrap())
+    }
+
+    async fn until(what: &str, check: impl Fn() -> bool) {
+        let deadline = std::time::Instant::now() + Duration::from_secs(10);
+        while !check() {
+            assert!(std::time::Instant::now() < deadline, "timed out waiting for: {what}");
+            tokio::time::sleep(Duration::from_millis(20)).await;
+        }
+    }
+
+    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
+    async fn one_gateway_per_home_and_a_replaced_token_stops_the_other_process() {
+        let (base, paths) = mock_telegram(|_| (200, json!({"ok": true, "result": []}))).await;
+        let polls = |bot: &str| {
+            let prefix = format!("/bot{bot}/getUpdates");
+            paths.lock().unwrap().iter().filter(|p| **p == prefix).count()
+        };
+        let dir = tempfile::tempdir().unwrap();
+        let daemon = heron_process(dir.path()).await;
+        let serve = heron_process(dir.path()).await;
+
+        let old = tokio::spawn(run(daemon.clone(), base.clone(), "OLD".into()));
+        until("the first gateway polls", || polls("OLD") > 0).await;
+        let second = tokio::spawn(run(serve.clone(), base.clone(), "SECOND".into()));
+        tokio::time::sleep(Duration::from_millis(600)).await;
+        assert_eq!(polls("SECOND"), 0, "a second gateway polled while the first was running");
+        second.abort();
+
+        // What set_telegram does in the other process when the owner pastes a new token.
+        bump_generation(&serve);
+        let new = tokio::spawn(run(serve.clone(), base.clone(), "NEW".into()));
+        until("the old gateway stops", || old.is_finished()).await;
+        until("the new gateway takes over", || polls("NEW") > 0).await;
+        let after = polls("OLD");
+        tokio::time::sleep(Duration::from_millis(200)).await;
+        assert_eq!(polls("OLD"), after, "the revoked bot is still being polled");
+        new.abort();
+    }
+
+    #[tokio::test]
+    async fn token_check_accepts_a_live_bot_and_never_echoes_the_token() {
+        let (base, paths) = mock_telegram(|path| match path {
+            "/botLIVE/getMe" => (200, json!({"ok": true, "result": {"id": 1, "is_bot": true}})),
+            "/botBUSY/getMe" => (500, json!({"ok": false})),
+            "/botUSER/getMe" => (200, json!({"ok": true, "result": {"id": 2, "is_bot": false}})),
+            _ => (401, json!({"ok": false, "error_code": 401, "description": "Unauthorized"})),
+        }).await;
+        assert_eq!(verify_token(&format!("{base}/"), "LIVE").await, Ok(()));
+        assert_eq!(paths.lock().unwrap().as_slice(), ["/botLIVE/getMe"]);
+        let revoked = verify_token(&base, "REVOKED").await.unwrap_err();
+        assert!(revoked.contains("does not recognise"), "{revoked}");
+        let busy = verify_token(&base, "BUSY").await.unwrap_err();
+        assert!(busy.contains("HTTP 500"), "{busy}");
+        assert!(verify_token(&base, "USER").await.is_err(), "only a bot account may be connected");
+        let offline = verify_token("http://127.0.0.1:1", "OFFLINE").await.unwrap_err();
+        for msg in [&revoked, &busy, &offline] {
+            assert!(!msg.contains("REVOKED") && !msg.contains("BUSY") && !msg.contains("OFFLINE"), "{msg}");
+        }
+    }
+
     #[tokio::test]
     async fn revoked_chat_cannot_receive_direct_approval_prompt() {
         use crate::permissions::Effect;
         let dir = tempfile::tempdir().unwrap();
         let mut cfg = Config::default();
         cfg.home = dir.path().join("home");
         let app = Arc::new(App::assemble(
             cfg, dir.path().join("ws"),
             Store::in_memory().unwrap(),
             Arc::new(ScriptedLlm::new(vec![])),
         ).await.unwrap());
         let code = pairing_code(&app);
diff --git a/tests/integration.rs b/tests/integration.rs
index 801f272..d7be07e 100644
--- a/tests/integration.rs
+++ b/tests/integration.rs
@@ -357,24 +357,58 @@ async fn mcp_client_talks_to_a_real_server_and_detects_changed_tools() {
     assert!(recalled.contains("Bean"), "{recalled}");
     let err = remember.call(&json!({}), &ctx).await.unwrap_err();
     assert!(err.to_string().contains("content"), "tool errors come back as errors: {err}");
 
     // Same definitions as pinned: unchanged. A different pin (the server changed its tools): flagged, always asks.
     let mut again = ToolRegistry::new();
     assert!(!heron::mcp::attach(&mut again, "other-heron", &server, Some(&attached.fingerprint)).await.unwrap().changed);
     let mut changed = ToolRegistry::new();
     assert!(heron::mcp::attach(&mut changed, "other-heron", &server, Some("0000000000000000")).await.unwrap().changed);
     assert_eq!(changed.get("mcp__other-heron__remember").unwrap().risk(&json!({}), &ctx), heron::permissions::Effect::Privileged);
 }
 
+#[tokio::test]
+async fn a_rejected_bot_token_leaves_the_working_pairing_alone() {
+    use axum::extract::State;
+    use std::sync::Mutex;
+    let checked = Arc::new(Mutex::new(Vec::<String>::new()));
+    let telegram = Router::new()
+        .fallback(|State(c): State<Arc<Mutex<Vec<String>>>>, uri: axum::http::Uri| async move {
+            c.lock().unwrap().push(uri.path().to_string());
+            (axum::http::StatusCode::UNAUTHORIZED, Json(json!({"ok": false, "error_code": 401, "description": "Unauthorized"})))
+        })
+        .with_state(checked.clone());
+    let dir = tempfile::tempdir().unwrap();
+    let mut cfg = Config::default();
+    cfg.home = dir.path().join("home");
+    cfg.llm.api_key_env = "HERON_TEST_NO_SUCH_KEY".into();
+    cfg.telegram.api_base = spawn(telegram).await;
+    let app = Arc::new(App::assemble(cfg, dir.path().join("ws"), Store::open(&dir.path().join("home/heron.db")).unwrap(), Arc::new(ScriptedLlm::new(vec![]))).await.unwrap());
+    app.store.kv_set("telegram_chat", "42").unwrap();
+    app.store.kv_set("telegram_offset", "7").unwrap();
+    let token = "test-token".to_string();
+    let base = spawn(heron::server::router(app.clone(), token.clone())).await;
+    let bot_token = "123456789:AAEXAMPLEexampleEXAMPLEexample00";
+    let resp = reqwest::Client::new().post(format!("{base}/api/telegram")).header("x-heron-token", &token).json(&json!({"token": bot_token})).send().await.unwrap();
+    assert_eq!(resp.status(), 400);
+    let body = resp.text().await.unwrap();
+    assert!(body.contains("does not recognise"), "{body}");
+    assert!(!body.contains("AAEXAMPLE"), "the token leaked into the reply: {body}");
+    assert_eq!(checked.lock().unwrap().as_slice(), [format!("/bot{bot_token}/getMe")]);
+    // Nothing changed: the phone stays paired, the stream position is kept, no token was stored.
+    assert_eq!(app.store.kv_get("telegram_chat").as_deref(), Some("42"));
+    assert_eq!(app.store.kv_get("telegram_offset").as_deref(), Some("7"));
+    assert!(!app.cfg().vault_only_secret_present("telegram_token"));
+}
+
 #[tokio::test]
 async fn telegram_gateway_pairs_one_chat_ignores_strangers_and_asks_before_acting() {
     use axum::extract::State;
     use std::collections::VecDeque;
     use std::sync::Mutex;
     #[derive(Default)]
     struct Mock {
         updates: Mutex<VecDeque<Value>>,
         sent: Mutex<Vec<(i64, String)>>,
         next: AtomicUsize,
     }
     let mock = Arc::new(Mock::default());
```

## source case 9

- fix: `0a4f7e52d8e0f6b5ef39d39e40e26a6fd82452a7`
- base: `7c188ecafb03ee936f77c5d295edfb6db0245855`
- subject: fix(connectors): wait briefly for the registry lock instead of failing at once
- files: src/connectors.rs 

Commit message body:

```text
The connector registry lock (flock on connectors.lock) was taken with a single
non-blocking try_lock. Between fork and exec, a child process started by any
thread holds a copy of every open file, this lock included, so closing ours
does not release it until the child execs. A registry change made in that
moment failed with "Connector registry is busy in another HERON instance".
That is the intermittent hosted Linux failure of
connectors::tests::registry_is_atomic_private_and_never_contains_keys (seen on
PR #39 and PR #40), where parallel tests spawn processes; the same can happen in
the app whenever Heron starts a command or MCP server during a registry change.

lock_registry now retries for up to one second (10 ms steps) and reports
"busy" only if the lock is still held after that, which means another
instance really is mid-change. The wait only happens under contention.

Regression test: a_lock_kept_alive_for_a_moment_by_a_spawning_child_does_not_fail_the_next_change
holds a close-on-exec duplicate of the lock for 100 ms, as a forked child would.
It fails 5/5 without this change and the connector tests pass 20/20 in
parallel with it.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

```

File `src/connectors.rs`: 486 lines at base, 518 at fix. Last 4 lines identical at base and fix: **yes**.

Last 4 lines at base:
```rust
            assert!(validate(&spec).is_err());
        }
    }
}
```

The fix (`git show -U12`):

```diff
diff --git a/src/connectors.rs b/src/connectors.rs
index 5b62ea9..fd4da48 100644
--- a/src/connectors.rs
+++ b/src/connectors.rs
@@ -87,26 +87,37 @@ fn registry_path(cfg: &Config) -> PathBuf {
 /// one home. Hold an OS file lock throughout each read-modify-write and vault
 /// mutation, preventing silent loss of successfully registered connectors.
 fn lock_registry(cfg: &Config) -> Result<std::fs::File> {
     cfg.ensure_home()?;
     let mut opts = OpenOptions::new();
     opts.write(true).create(true).truncate(false);
     #[cfg(unix)]
     {
         use std::os::unix::fs::OpenOptionsExt;
         opts.mode(0o600);
     }
     let file = opts.open(cfg.home.join("connectors.lock"))?;
-    file.try_lock().context("Connector registry is busy in another HERON instance; please retry.")?;
-    Ok(file)
+    // A child process started by any thread briefly holds a copy of this open file between fork and
+    // exec, keeping the lock alive after we close it. Wait out that moment; only a lock still held
+    // after a second means another HERON instance is mid-change.
+    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
+    loop {
+        match file.try_lock() {
+            Ok(()) => return Ok(file),
+            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
+                std::thread::sleep(std::time::Duration::from_millis(10));
+            }
+            Err(e) => return Err(e).context("Connector registry is busy in another HERON instance; please retry."),
+        }
+    }
 }
 
 pub fn validate_id(id: &str) -> Result<()> {
     if !(3..=48).contains(&id.len())
         || !id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
         || id.starts_with('-') || id.starts_with('_') || id.starts_with("mcp_")
         || CATALOG.iter().any(|(key, _, _, _)| *key == id)
     {
         bail!("Connector ID must be a unique 3–48 character lowercase slug.");
     }
     Ok(())
 }
@@ -370,24 +381,45 @@ mod tests {
     fn another_process_lock_refuses_racy_mutations() {
         let temp = tempfile::tempdir().unwrap();
         let mut cfg = Config::default();
         cfg.home = temp.path().join("heron");
         let held = lock_registry(&cfg).unwrap();
         // The OS lock protects multiple processes, and also prevents a
         // second independent handle from overwriting another transaction.
         assert!(lock_registry(&cfg).is_err());
         drop(held);
         assert!(lock_registry(&cfg).is_ok());
     }
 
+    #[cfg(unix)]
+    #[test]
+    fn a_lock_kept_alive_for_a_moment_by_a_spawning_child_does_not_fail_the_next_change() {
+        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
+        let temp = tempfile::tempdir().unwrap();
+        let mut cfg = Config::default();
+        cfg.home = temp.path().join("heron");
+        let held = lock_registry(&cfg).unwrap();
+        // Between fork and exec, a child started by another thread holds a copy of every open
+        // file, this lock included, so closing ours does not release it until the child execs.
+        let child_copy = unsafe { OwnedFd::from_raw_fd(libc::fcntl(held.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0)) };
+        drop(held);
+        let exec = std::thread::spawn(move || {
+            std::thread::sleep(std::time::Duration::from_millis(100));
+            drop(child_copy);
+        });
+        register(&cfg, sample()).unwrap();
+        exec.join().unwrap();
+        assert!(list(&cfg).unwrap().iter().any(|card| card.id == "my_api"));
+    }
+
     #[test]
     fn connector_registry_never_activates_unreviewed_remote_tools() {
         let temp = tempfile::tempdir().unwrap();
         let mut cfg = Config::default();
         cfg.home = temp.path().to_path_buf();
         let mut c = sample();
         c.transport = "remote_mcp".into();
         c.permission = "approval_required".into();
         register(&cfg, c).unwrap();
         let card = list(&cfg).unwrap().into_iter().find(|x| x.id == "my_api").unwrap();
         assert_eq!(card.status, "saved_not_active");
         assert_eq!(card.permission, "approval_required");
```

## source case 10

- fix: `448227d941d4e3ceb348e701a25ee4b811509244`
- base: `449231fcbef29726a18aafe5937fa197b9e80c0a`
- subject: fix(security): judge reads by where links point and treat Heron's home as secret
- files: src/tools/fs.rs 

Commit message body:

```text
search_files classified each walked entry by its own name, then std::fs::read
followed the link. A workspace file such as notes.txt linked to ~/.ssh/id_ed25519
was grepped and its contents returned with no approval, although read_file on the
same link asked. Reads were also not gated on Heron's own home (secrets.json,
config.toml with MCP server environment, the database), which writes already
treat as Privileged.

read_effect now marks a read SensitiveRead when either the given path or its
canonical target is a credential path or lies under cfg.home. read_risk and
search_files both use it.

Tests: a link to a credential is skipped by search and asks on read; Heron's
home asks and is skipped, including through a link; ordinary reads inside and
outside the workspace stay LocalRead. The first two fail without this change.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

```

File `src/tools/fs.rs`: 1174 lines at base, 1252 at fix. Last 4 lines identical at base and fix: **no**.

Last 4 lines at base:
```rust
            assert_eq!(std::fs::metadata(&script).unwrap().permissions().mode() & 0o777, 0o755);
        }
    }
}
```
Last 4 lines at fix:
```rust
        assert_eq!(read_risk(&json!({"path": "README.md"}), &c), Effect::LocalRead);
        assert_eq!(read_risk(&json!({"path": dir.path().join("report.txt").to_string_lossy()}), &c), Effect::LocalRead);
    }
}
```

The fix (`git show -U12`):

```diff
diff --git a/src/tools/fs.rs b/src/tools/fs.rs
index 8261271..2a5f373 100644
--- a/src/tools/fs.rs
+++ b/src/tools/fs.rs
@@ -5,25 +5,35 @@ use crate::permissions::{classify_path, resolve_path, Effect};
 use crate::util::preview;
 use anyhow::{anyhow, bail, Context, Result};
 use serde_json::{json, Value};
 use std::path::{Path, PathBuf};
 
 const MAX_READ_BYTES: u64 = 10 * 1024 * 1024;
 
 fn path_summary(args: &Value) -> String {
     args.get("path").and_then(|p| p.as_str()).unwrap_or(".").to_string()
 }
 
 fn read_risk(args: &Value, ctx: &ToolCtx) -> Effect {
-    classify_path(&resolve_path(&ctx.workspace, &path_summary(args)), &ctx.workspace, false)
+    read_effect(&resolve_path(&ctx.workspace, &path_summary(args)), &ctx.cfg.home, &ctx.workspace)
+}
+
+/// Reading a credential file or Heron's own home (keys, settings, memory) needs the user's say-so.
+/// Both the name given and the file a link really points at are judged, so a harmless-looking link
+/// in a repository cannot lead a read to a secret.
+fn read_effect(path: &Path, heron_home: &Path, workspace: &Path) -> Effect {
+    let real = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
+    let home = dunce::canonicalize(heron_home).unwrap_or_else(|_| heron_home.to_path_buf());
+    let secret = |p: &Path| p.starts_with(&home) || p.starts_with(heron_home) || classify_path(p, workspace, false) == Effect::SensitiveRead;
+    if secret(path) || secret(&real) { Effect::SensitiveRead } else { Effect::LocalRead }
 }
 
 fn write_risk(args: &Value, ctx: &ToolCtx) -> Effect {
     let path = resolve_path(&ctx.workspace, &path_summary(args));
     // The agent must never quietly rewrite its own settings, permission rules, skills, memory or keys.
     let home = dunce::canonicalize(&ctx.cfg.home).unwrap_or_else(|_| ctx.cfg.home.clone());
     if path.starts_with(&home) || path.starts_with(&ctx.cfg.home) {
         return Effect::Privileged;
     }
     classify_path(&path, &ctx.workspace, true)
 }
 
@@ -681,40 +691,42 @@ fn walker(root: &Path) -> ignore::Walk {
     ignore::WalkBuilder::new(root).hidden(false).filter_entry(|e| !matches!(e.file_name().to_str(), Some(".git" | "node_modules" | "target" | ".venv" | "__pycache__"))).build()
 }
 
 async fn search_files(args: &Value, ctx: &ToolCtx) -> Result<String> {
     let pattern = arg_str(args, "pattern")?.to_string();
     let root = resolve_path(&ctx.workspace, opt_str(args, "path").unwrap_or("."));
     let glob = match opt_str(args, "glob") {
         Some(g) => Some(globset::Glob::new(g).map_err(|e| anyhow!("bad glob: {e}"))?.compile_matcher()),
         None => None,
     };
     let re = regex::RegexBuilder::new(&pattern).case_insensitive(opt_bool(args, "ignore_case")).build().map_err(|e| anyhow!("bad regex: {e}"))?;
     let max = 200usize;
+    let heron_home = ctx.cfg.home.clone();
+    let workspace = ctx.workspace.clone();
     tokio::task::spawn_blocking(move || {
         let mut out = String::new();
         let mut hits = 0usize;
         'files: for entry in walker(&root).flatten() {
             let p = entry.path();
             if !p.is_file() {
                 continue;
             }
             let rel = p.strip_prefix(&root).unwrap_or(p);
             if let Some(g) = &glob {
                 if !g.is_match(rel) && !p.file_name().map(|f| g.is_match(f)).unwrap_or(false) {
                     continue;
                 }
             }
-            if classify_path(p, &root, false) == Effect::SensitiveRead {
-                continue; // never grep through credential files
+            if read_effect(p, &heron_home, &workspace) == Effect::SensitiveRead {
+                continue; // never grep through credential files or Heron's own data
             }
             let Ok(bytes) = std::fs::read(p) else { continue };
             if bytes.len() as u64 > MAX_READ_BYTES || bytes.iter().take(4096).any(|b| *b == 0) {
                 continue;
             }
             let text = String::from_utf8_lossy(&bytes);
             for (i, line) in text.lines().enumerate() {
                 if re.is_match(line) {
                     out.push_str(&format!("{}:{}: {}\n", rel.display(), i + 1, preview(line, 240)));
                     hits += 1;
                     if hits >= max {
                         out.push_str(&format!("[stopped at {max} matches; narrow the pattern or path]\n"));
@@ -1163,12 +1175,78 @@ mod undo_tests {
         // Writing keeps a script executable, and replaces a link instead of writing through it.
         #[cfg(unix)]
         {
             use std::os::unix::fs::PermissionsExt;
             let script = c.workspace.join("run.sh");
             std::fs::write(&script, "#!/bin/sh\n").unwrap();
             std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
             write_file(&json!({"path": "run.sh", "content": "#!/bin/sh\necho hi\n"}), &c).await.unwrap();
             assert_eq!(std::fs::metadata(&script).unwrap().permissions().mode() & 0o777, 0o755);
         }
     }
 }
+
+#[cfg(all(test, unix))]
+mod read_gate_tests {
+    use super::*;
+    use crate::config::Config;
+    use crate::memory::Memory;
+    use crate::store::Store;
+    use std::sync::Arc;
+
+    fn ctx(ws: &Path, home: &Path) -> ToolCtx {
+        let store = Store::in_memory().unwrap();
+        let mut cfg = Config::default();
+        cfg.home = home.to_path_buf();
+        ToolCtx { workspace: dunce::canonicalize(ws).unwrap(), memory: Memory::new(store.clone(), None), store, session_id: "s".into(), cfg: Arc::new(cfg), tainted: Default::default(), seen_urls: Default::default() }
+    }
+
+    fn dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
+        let dir = tempfile::tempdir().unwrap();
+        let (ws, home) = (dir.path().join("ws"), dir.path().join("heron-home"));
+        std::fs::create_dir_all(&ws).unwrap();
+        std::fs::create_dir_all(&home).unwrap();
+        (dir, ws, home)
+    }
+
+    #[tokio::test]
+    async fn search_never_reads_a_secret_through_a_harmless_looking_link() {
+        let (dir, ws, home) = dirs();
+        let ssh = dir.path().join("elsewhere/.ssh");
+        std::fs::create_dir_all(&ssh).unwrap();
+        std::fs::write(ssh.join("id_ed25519"), "-----BEGIN OPENSSH PRIVATE KEY-----\nFAKEKEY\n").unwrap();
+        std::os::unix::fs::symlink(ssh.join("id_ed25519"), ws.join("notes.txt")).unwrap();
+        std::fs::write(ws.join("plain.txt"), "FAKEKEY is only a word here\n").unwrap();
+        let c = ctx(&ws, &home);
+
+        let out = search_files(&json!({"pattern": "BEGIN|FAKEKEY"}), &c).await.unwrap();
+        assert!(!out.contains("PRIVATE KEY"), "{out}");
+        assert!(out.contains("plain.txt:1:"), "ordinary files are still searched: {out}");
+        assert_eq!(read_risk(&json!({"path": "notes.txt"}), &c), Effect::SensitiveRead);
+    }
+
+    #[tokio::test]
+    async fn heron_own_home_is_a_secret_to_read_and_to_search() {
+        let (_dir, ws, home) = dirs();
+        std::fs::write(home.join("secrets.json"), r#"{"api_key": "sk-FAKE"}"#).unwrap();
+        std::fs::write(home.join("config.toml"), "[mcp.env]\nTOKEN = \"FAKE\"\n").unwrap();
+        std::os::unix::fs::symlink(&home, ws.join("settings")).unwrap();
+        let c = ctx(&ws, &home);
+
+        for path in [home.join("secrets.json"), home.join("config.toml")] {
+            assert_eq!(read_risk(&json!({"path": path.to_string_lossy()}), &c), Effect::SensitiveRead, "{}", path.display());
+        }
+        assert_eq!(read_risk(&json!({"path": "settings/config.toml"}), &c), Effect::SensitiveRead);
+        let out = search_files(&json!({"pattern": "FAKE", "path": home.to_string_lossy()}), &c).await.unwrap();
+        assert_eq!(out, "no matches");
+    }
+
+    #[tokio::test]
+    async fn ordinary_reads_inside_and_outside_the_workspace_do_not_ask() {
+        let (dir, ws, home) = dirs();
+        std::fs::write(ws.join("README.md"), "hello").unwrap();
+        std::fs::write(dir.path().join("report.txt"), "hello").unwrap();
+        let c = ctx(&ws, &home);
+        assert_eq!(read_risk(&json!({"path": "README.md"}), &c), Effect::LocalRead);
+        assert_eq!(read_risk(&json!({"path": dir.path().join("report.txt").to_string_lossy()}), &c), Effect::LocalRead);
+    }
+}
```

