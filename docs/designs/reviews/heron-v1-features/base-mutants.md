# Base gate: surviving mutants (node Bmut)

The base gate's mutation run (testteeth with cargo-mutants, against commit 39dbdf5) left 15 survivors.
Thirteen are killed by tests added in this node. Two are justified below because their only effect is
a notification banner on a real macOS desktop. Every survivor in a security path (`Agent::assess`,
`taint_detail`, `own_var`, `terminal_ask`, `web_search`) is killed, not justified.

Each killing test was checked by applying a mutant of that shape by hand and watching the test fail
(for `web_search`, the `Ok("xyzzy")` form; the empty-string form fails the same assertions), then
passing again with the original code. Line numbers are those of the tree this node started from.

## Killed

- `src/agent.rs` `taint_detail` L402, `t.starts_with("browser_")` -> `true`: killed by
  `agent::tests::only_web_and_browser_tools_name_a_host_for_taint`. A tool that is not `web_*` or
  `browser_*` (`read_file`, `search_knowledge`, `shell`, and names that only contain "browser") is
  given a page address in its arguments and must still record no host; a `browser_*` tool records the
  page's host, never the path or query.
- `src/agent.rs` `Agent::assess` L774, `let tool = self.tools.get_for(...)?` / body -> `None`: killed by
  `agent::tests::assess_gives_the_effect_of_available_tools_only`. A tool with no feature key is
  assessed (`search_knowledge` reads only, `write_file` does not), a tool whose feature key is off gets
  `None` (it is refused, never treated as read-only), the same tool with its key on is assessed,
  and an unknown tool or arguments that are not JSON also get `None`.
- `src/app.rs` `App::take_features` L1549, `cfg.revision += 1` -> `-= 1` and `*= 1`: killed by
  `app::tests::take_features_applies_a_changed_table_as_one_new_revision`. A changed `[features]`
  table is applied and the revision rises by exactly one (counted from where it was, not from zero), the
  same table again leaves the revision alone, and the other settings are kept.
- `src/browser.rs` `tools` L535, body -> `vec![]`: killed by
  `browser::tests::the_browser_tools_are_offered_with_their_risk_tiers`. The four tools are offered in
  order (`browser_open`, `browser_read`, `browser_click`, `browser_type`), all treat their output as
  outside content, and their risk is the spec's: an unseen address is `NetworkEgress`, a seen one
  `NetworkRead`, reading the open page `LocalRead`, clicking something that is not a known link and
  typing are `ExternalWrite`.
- `src/child.rs` `own_var` L13, `std::env::var(key).ok()` -> `None`: killed by
  `child::tests::own_var_reads_heron_own_environment`. `own_var("PATH")` is Heron's `PATH`, an unset
  variable is `None`, and a child environment built through `own_var` carries that `PATH` and the
  private `TMPDIR` and nothing outside the allowlist.
- `src/main.rs` `terminal_ask` L194, remove the `"n" | "no" | ""` branch: killed by
  `tests::terminal_ask_reads_the_answer_the_user_gives`. Each of n, no, N, No, NO, an empty line and
  a line of spaces declines and ends the question; the test follows each with a `y`, which would
  approve if the question went on. (Without that `y`, the end of input also declines, which is why this
  mutant survived before.)
- `src/main.rs` `terminal_ask` L195, `other.len() > 3` -> `false` and `>` -> `==`: killed by the same
  test. A reply longer than three letters (`nope`, `stop`, `not now`, upper case and padded forms)
  declines with `the user declined and said: <the words>`; replies of up to three letters that are
  neither yes nor no (`yep`, `nah`, `ok`, `abc`, `x`, `?`) are not an answer and the question is asked
  again, so the `y` after them approves.
- `src/main.rs` `turn` L289, body -> `Ok(String::new())`: killed by
  `tests::turn_returns_the_reply_and_keeps_the_conversation`. With a scripted model, `turn` returns the
  model's reply, asks the model once, and leaves the user's message and the reply in the conversation
  and in the saved messages.
- `src/main.rs` `repl` L330, body -> `Ok(())`: killed by `tests/chat_repl.rs`
  (`chat_runs_the_commands_it_is_given_and_exits_cleanly`, `chat_ends_when_its_input_ends`). They run the
  real `heron chat` process with piped input: `/mode`, `/help`, an unknown command and `/tokens` are
  answered, nothing after `/exit` runs, end of input is a clean exit, and the typed lines are saved to
  the history file on the way out. `repl` reads the terminal's own input, so it cannot be called
  in-process; it needs the process.
- `src/tools/web.rs` `web_search` L158, body -> `Ok(String::new())` and `Ok("xyzzy".into())`: killed by
  `tools::web::tests::web_search_needs_a_query_and_sends_nothing_without_one`. Without a text `query`
  (missing, a number, `null`) the tool, both through its registered entry and as the function, fails
  with `missing required string argument 'query'` before anything is sent, so a stub that answers with
  any text is caught. The test needs no network: the search endpoint is a constant, and a test that
  reached it would depend on the machine being online.

## Justified: untestable

- `src/app.rs` `App::post_desktop_notice` L1302, body -> `()`: the non-test method only forwards to the
  free function `post_desktop_notice`, whose one effect is to start `osascript` and show a macOS
  notification banner (see the next bullet). In the lib's own tests the method is compiled out (the
  `#[cfg(test)]` twin records the notice, and `notify` is tested through that twin). A test of the
  non-test one, through the binary's tests, could only observe the banner on a real desktop session;
  it would pop a notification on the screen of whoever runs the tests, and `osascript` is refused in
  the sandbox the tests run in. No behaviour other than the banner depends on it.
- `src/app.rs` free `post_desktop_notice` L2405 (`#[cfg(target_os = "macos")]` block), body -> `()`:
  the mutated code is the `osascript` call that shows the banner. It runs only on macOS, only outside
  `cfg(test)`, and its result (a banner shown to a logged-in desktop user) cannot be read back by a
  banner appears, not what it says. The arguments go through argv, never into the script text (the
  comment in the code says so), and that is unchanged.
