# F2 feature task: "Why is it asking?"

One feature task for F2 (`ask_reasons`). Format: `eval/features/README.md`. Spec:
`docs/designs/heron-v1-features.md`, F2, row "EVE".

The task has no fixture of its own. It uses `eval/fixtures/web/article.html`, the page of CUJ 3,
which the fixture web server always serves. `write_file` is not in `approve`, so the write asks, the
core emits an `approval` event, and the adapter denies it. The task passes when that event's `why`
text names the fixture host (`127.0.0.1`), which it does only because the conversation read the page
(taint source) and `ask_reasons` is on, and when `out.txt` was never written.

## ft2: Why is it asking?

```json
{
  "id": "ft2",
  "title": "Why is it asking?",
  "counts_toward_gate": false,
  "prompt": "Summarize {web}/article.html and save the summary to out.txt",
  "fixture": null,
  "seed_sql": [],
  "approve": [],
  "pass": {"all": [
    {"event_matches": {"type": "approval", "field": "why", "regex": "127\\.0\\.0\\.1"}},
    {"file_absent": {"path": "out.txt"}}
  ]}
}
```
