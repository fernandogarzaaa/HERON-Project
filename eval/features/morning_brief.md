# Feature tasks: F6, morning brief

Spec: `docs/designs/heron-v1-features.md`, F6. The format is the one of `eval/features/README.md`.

`ft6` checks that the model reaches for `today_counts` and reports its numbers. `seed_sql` puts two
calendar `knowledge` rows at noon and 1 pm in the local day, so both start today, and one open task
due yesterday, so the counts are `3` and `Today: 2 events, 1 overdue task`. The task has no `features`
key because `today_counts` has no core key. That the brief spends no tokens is the Rust test
`brief_workflow_runs_without_a_model`, not this task.

## ft6: Morning brief counts

```json
{
  "id": "ft6",
  "title": "Morning brief counts",
  "counts_toward_gate": true,
  "prompt": "Use today_counts and tell me what needs me today.",
  "fixture": null,
  "seed_sql": [
    "INSERT INTO knowledge(source, ref, title, body, ts) VALUES ('calendar', 'eval-ft6-event-1', 'Team standup', 'Daily standup', CAST(strftime('%s','now','localtime','start of day','+12 hours','utc') AS INTEGER))",
    "INSERT INTO knowledge(source, ref, title, body, ts) VALUES ('calendar', 'eval-ft6-event-2', 'Dentist', 'Cleaning at the clinic', CAST(strftime('%s','now','localtime','start of day','+13 hours','utc') AS INTEGER))",
    "INSERT INTO tasks(title, notes, status, priority, due, created_at) VALUES ('Send the invoice', '', 'open', 2, date('now','localtime','-1 day'), CAST(strftime('%s','now') AS INTEGER))"
  ],
  "approve": [],
  "pass": {"all": [
    {"audit_has": {"decision": "allowed", "tool": "today_counts"}},
    {"reply_matches": {"regex": "2 events"}}
  ]}
}
```
