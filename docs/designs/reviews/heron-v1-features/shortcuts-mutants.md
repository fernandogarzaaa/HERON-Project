# F5 gate: surviving mutants in `src/tools/shortcuts.rs` (node F5mut)

F5's mutation run left four survivors. All four are now killed by tests in `src/tools/shortcuts.rs`.
One kill is partial, and the gap is justified below. Line numbers are those of the tree this node
started from.

Each test was checked by applying its mutant by hand and watching the test fail, then restoring the
code (the source diff of this node is additions only).

## Killed

- L46, `PIPE_CAP = 1024 * 1024`, `*` -> `+`: killed by `child_output_is_kept_up_to_one_mebibyte`. `list`
  is given a child that writes one line of 1,048,575, 1,048,576, 1,048,577, 1,148,576 and 3,145,728
  bytes. The name that comes back is exactly 1,048,575, 1,048,576, 1,048,576, 1,048,576 and 1,048,576
  bytes long: whole up to the cap, cut at it, and the child never blocks on a full pipe. With the cap
  at 2,048 the name is 2,048 bytes long.
- L97, `list`, `e.kind() == NotFound` -> `true`: killed by `a_binary_that_cannot_start_is_not_a_missing_binary`.
  A missing binary is `Ok(None)` (the control: `GET /api/shortcuts` shows `available: false` only for
  this). A file with mode 0644 and a folder in the binary's place are errors that say "could not start"
  and name the path, never `None`.
- L151, `run`, `e.kind() == NotFound` -> `true`: killed by `an_unreadable_output_file_is_an_error_not_an_empty_result`.
  A shortcut that exits 0 and leaves a folder where the output file should be gets "could not read
  the output of shortcut Get Notes". The control, no output file at all, is still the empty result.
- L258, `RunShortcut::description` -> `"xyzzy"`: killed by `the_description_tells_the_model_which_shortcuts_can_run`.
  The description must say that only shortcuts the user has ticked ("Only shortcuts the user has
  ticked", "Settings › Features › Shortcuts") can run, by their "exact name", that the model should
  "ask the user" when unsure of a name, that optional `input` is passed, and what the shortcut
  "outputs".

## Justified: the description does not say that each run asks

The task asked the description test to pin that each run asks. It does not, because the description
does not say so and F5 does not require it to. F5's spec ("The tool") requires the tool to refuse an
unticked name before the decision function, and requires approval behaviour (`ExternalWrite`, once
only, the name and input shown to the owner). It does not require the model to be told about the
approval. The asking is enforced in code, not by what the model knows, and is pinned by
`decision_table_run_shortcut`, `run_shortcut_is_external_write_with_untrusted_output` and
`shortcut_name_starting_with_a_dash_is_refused`. A description test that required the words "asks"
or "approval" would fail against the shipped text, and this node may change tests only. If the owner
wants the model told that every run asks, that is a one-line change to the description plus one
assertion, for a follow-up node that may edit source.
