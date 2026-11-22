import contextlib
import io
import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

HERE = os.path.dirname(os.path.abspath(__file__))
EVAL = os.path.dirname(HERE)
REPO = os.path.dirname(EVAL)
SELFHEAL = os.path.join(EVAL, "selfheal")
sys.path.insert(0, SELFHEAL)

import build_genesis_cases  # noqa: E402
import grade as grader  # noqa: E402
import replay_verifier as rv  # noqa: E402
import run_replay  # noqa: E402

CASES = run_replay.load_cases(run_replay.CASES_PATH)
GOOD = {"patch_empty": False, "hidden_test_deleted": False, "hidden_failed_on_base": True,
        "hidden_passed_on_fix": True, "suite_passed": True}
FLIPPED = {key: not value for key, value in GOOD.items()}
SHA_A, SHA_B = "a" * 40, "b" * 40


def outcome(**changes):
    return dict(GOOD, **changes)


def has_commit(sha):
    return subprocess.run(["git", "-C", REPO, "cat-file", "-e", sha + "^{commit}"], capture_output=True).returncode == 0


class Grade(unittest.TestCase):
    def test_a_real_fix_with_a_hidden_test_that_bites_earns_the_point(self):
        self.assertIs(grader.grade(dict(GOOD)), True)

    def test_each_wrong_field_alone_loses_the_point(self):
        for key in GOOD:
            with self.subTest(key):
                self.assertIs(grader.grade(outcome(**{key: not GOOD[key]})), False)

    def test_an_empty_patch_grades_false_even_when_everything_else_is_perfect(self):
        self.assertIs(grader.grade(outcome(patch_empty=True)), False)

    def test_a_deleted_hidden_test_grades_false_even_when_everything_else_is_perfect(self):
        self.assertIs(grader.grade(outcome(hidden_test_deleted=True)), False)

    def test_all_wrong_grades_false(self):
        self.assertIs(grader.grade(dict(FLIPPED)), False)

    def test_a_missing_field_is_never_assumed_good(self):
        for key in GOOD:
            with self.subTest(key):
                partial = dict(GOOD)
                del partial[key]
                self.assertIs(grader.grade(partial), False)
        self.assertIs(grader.grade({}), False)

    def test_only_real_booleans_count(self):
        for key in GOOD:
            for wrong in ("yes", "true", "false", 1, 0, None, [], [True]):
                with self.subTest(msg="%s=%r" % (key, wrong)):
                    self.assertIs(grader.grade(outcome(**{key: wrong})), False)

    def test_anything_that_is_not_a_dict_grades_false(self):
        for value in (None, [], "ok", 1, True, [GOOD], (GOOD,)):
            with self.subTest(repr(value)):
                self.assertIs(grader.grade(value), False)

    def test_fields_the_grader_does_not_know_change_nothing(self):
        self.assertIs(grader.grade(outcome(note="extra", attempts=3)), True)
        self.assertIs(grader.grade(outcome(patch_empty=True, note="extra")), False)

    def test_the_grader_is_pure(self):
        given = dict(GOOD)
        grader.grade(given)
        self.assertEqual(given, GOOD)

    def test_the_passing_outcome_is_the_one_the_design_names(self):
        self.assertEqual(grader.PASSING, GOOD)
        self.assertEqual(set(grader.OUTCOME_KEYS), set(GOOD))


class CasesFile(unittest.TestCase):
    def test_there_are_ten_cases_numbered_in_order(self):
        self.assertEqual([case["n"] for case in CASES], list(range(1, 11)))

    def test_every_field_is_present_and_well_formed(self):
        for case in CASES:
            with self.subTest(case["n"]):
                self.assertEqual(set(case), {"n", "base", "fix", "report", "hidden", "test"})
                self.assertRegex(case["base"], r"^[0-9a-f]{40}$")
                self.assertRegex(case["fix"], r"^[0-9a-f]{40}$")
                self.assertNotEqual(case["base"], case["fix"])
                self.assertTrue(case["report"].endswith((".", '."')), case["report"])
                self.assertEqual(case["hidden"], "eval/selfheal/cases/%d.patch" % case["n"])
                self.assertTrue(run_replay.SELECTOR.fullmatch(case["test"]), case["test"])

    def test_every_patch_file_exists(self):
        for case in CASES:
            with self.subTest(case["n"]):
                self.assertTrue(os.path.isfile(os.path.join(REPO, case["hidden"])))
        listed = sorted(f for f in os.listdir(os.path.join(SELFHEAL, "cases")) if f.endswith(".patch"))
        self.assertEqual(listed, sorted("%d.patch" % n for n in range(1, 11)))

    def test_each_case_is_a_different_fix(self):
        self.assertEqual(len({case["fix"] for case in CASES}), 10)
        self.assertEqual(len({case["base"] for case in CASES}), 10)

    def test_the_report_describes_a_symptom_and_names_no_code(self):
        for case in CASES:
            with self.subTest(case["n"]):
                self.assertNotRegex(case["report"], r"\b[a-z_]+\(\)|src/|\.rs\b|\bfn\b|[0-9a-f]{7,}")
                self.assertNotIn("\n", case["report"])

    def test_each_selector_names_its_own_hidden_test(self):
        for case in CASES:
            self.assertEqual(case["test"], "cargo --lib selfheal_hidden_case_%d" % case["n"])

    def test_the_file_has_exactly_ten_case_headings(self):
        with open(run_replay.CASES_PATH, "r", encoding="utf-8") as handle:
            self.assertEqual(sum(1 for line in handle if line.startswith("## case ")), 10)


def case_text(n=1, **fields):
    values = {"base": SHA_A, "fix": SHA_B, "report": "It broke.", "hidden": "eval/selfheal/cases/%d.patch" % n,
              "test": "cargo --lib selfheal_hidden_case_%d" % n}
    values.update(fields)
    return "## case %d\n" % n + "".join("%s: %s\n" % (key, values[key]) for key in run_replay.FIELDS if values.get(key) is not None)


class ParseCases(unittest.TestCase):
    def bad(self, text, message):
        with self.assertRaises(run_replay.CaseError) as caught:
            run_replay.parse_cases(text)
        self.assertIn(message, str(caught.exception))

    def test_prose_before_the_first_case_is_ignored(self):
        cases = run_replay.parse_cases("# Title\n\nSome words.\n\n" + case_text(1) + "\n" + case_text(2))
        self.assertEqual([c["n"] for c in cases], [1, 2])
        self.assertEqual(cases[1]["report"], "It broke.")

    def test_selectors_of_all_three_kinds_parse(self):
        for selector in ("cargo --lib a_test", "cargo --test integration", "swift ThingTests"):
            with self.subTest(selector):
                self.assertEqual(run_replay.parse_cases(case_text(1, test=selector))[0]["test"], selector)

    def test_a_bad_selector_is_named(self):
        for selector in ("cargo --bins x", "cargo --lib", "cargo --lib a b", "swift", "npm test x", "cargo --test=x"):
            with self.subTest(selector):
                self.bad(case_text(1, test=selector), "case 1: test")

    def test_a_missing_field_is_named(self):
        self.bad(case_text(1, report=None), "case 1: expected 5 fields")

    def test_an_extra_field_is_refused(self):
        self.bad(case_text(1) + "note: x\n", "expected 5 fields")

    def test_fields_must_come_in_order(self):
        text = "## case 1\nfix: %s\nbase: %s\nreport: r.\nhidden: eval/selfheal/cases/1.patch\ntest: cargo --lib x\n" % (SHA_B, SHA_A)
        self.bad(text, "case 1 needs 'base: <value>'")

    def test_an_empty_value_is_refused(self):
        self.bad(case_text(1, report=" "), "case 1 needs 'report: <value>'")

    def test_a_sha_must_be_forty_lowercase_hex_digits(self):
        for sha in (SHA_A[:39], SHA_A + "a", "A" * 40, "g" * 40, "main"):
            with self.subTest(sha):
                self.bad(case_text(1, base=sha), "case 1: base must be 40 lowercase hex digits")
                self.bad(case_text(1, fix=sha), "case 1: fix must be 40 lowercase hex digits")

    def test_a_case_whose_fix_is_its_base_is_refused(self):
        self.bad(case_text(1, fix=SHA_A), "base and fix are the same commit")

    def test_the_hidden_path_must_be_the_cases_own_patch(self):
        self.bad(case_text(1, hidden="eval/selfheal/cases/2.patch"), "hidden must be eval/selfheal/cases/1.patch")

    def test_cases_must_be_numbered_from_one_without_gaps(self):
        self.bad(case_text(2), "expected '## case 1', found '## case 2'")
        self.bad(case_text(1) + case_text(3), "expected '## case 2', found '## case 3'")

    def test_another_heading_is_refused_so_a_case_cannot_be_dropped_quietly(self):
        self.bad(case_text(1) + "## Case 2\n" + case_text(2).split("\n", 1)[1], "unexpected heading")

    def test_a_file_with_no_cases_is_refused(self):
        self.bad("# nothing here\n", "no '## case <n>' headings")


class Patches(unittest.TestCase):
    def read(self, case):
        with open(os.path.join(REPO, case["hidden"]), "r", encoding="utf-8") as handle:
            return handle.read().split("\n")

    def test_each_is_one_hunk_of_a_git_format_diff_with_exact_counts(self):
        for case in CASES:
            with self.subTest(case["n"]):
                lines = self.read(case)
                self.assertEqual(lines.pop(), "", "ends with a newline")
                path = re.fullmatch(r"diff --git a/(\S+) b/\1", lines[0]).group(1)
                self.assertTrue(path.startswith("src/") and path.endswith(".rs"), path)
                self.assertRegex(lines[1], r"^index [0-9a-f]{7}\.\.[0-9a-f]{7} 100644$")
                self.assertEqual(lines[2:4], ["--- a/" + path, "+++ b/" + path])
                hunk = re.fullmatch(r"@@ -(\d+),(\d+) \+(\d+),(\d+) @@", lines[4])
                self.assertIsNotNone(hunk, lines[4])
                body = lines[5:]
                self.assertEqual(sum(1 for l in body if l.startswith("@@")), 0, "one hunk")
                self.assertTrue(all(l[:1] in (" ", "+") for l in body), "only context and additions")
                context = sum(1 for l in body if l.startswith(" "))
                added = sum(1 for l in body if l.startswith("+"))
                self.assertEqual((int(hunk.group(2)), int(hunk.group(4))), (context, context + added))
                self.assertEqual(hunk.group(1), hunk.group(3))
                self.assertTrue(body[0].startswith(" ") and body[context].startswith("+"), "context first, then additions")

    def test_each_adds_one_module_with_one_test_under_the_cases_own_name(self):
        for case in CASES:
            with self.subTest(case["n"]):
                added = "\n".join(l[1:] for l in self.read(case) if l.startswith("+") and not l.startswith("+++"))
                name = "selfheal_hidden_case_%d" % case["n"]
                added_lines = added.split("\n")
                at = added_lines.index("mod %s {" % name)
                self.assertRegex(added_lines[at - 1], r"^#\[cfg\((?:all\()?test\b", "the module exists only in test builds")
                self.assertEqual(re.findall(r"^mod (\w+) \{$", added, re.M), [name])
                self.assertEqual(re.findall(r"^    (?:async )?fn (%s)\(\)" % name, added, re.M), [name])
                self.assertEqual(len(re.findall(r"^    #\[(?:test|tokio::test[^\]]*)\]$", added, re.M)), 1)
                self.assertEqual(added.count("{"), added.count("}"), "braces balance")
                self.assertEqual(added.count("("), added.count(")"))
                self.assertEqual(added.count("["), added.count("]"))
                self.assertTrue(added.rstrip().endswith("}"))

    def test_a_hidden_test_never_borrows_the_name_of_the_fixs_own_test(self):
        for case in CASES:
            added = "\n".join(l for l in self.read(case) if l.startswith("+"))
            self.assertEqual(re.findall(r"fn (\w+)\(\)", added).count("selfheal_hidden_case_%d" % case["n"]), 1)

    def test_a_hidden_test_calls_nothing_only_the_fix_adds(self):
        # What each fix introduced: an agent's fix will not use these names, so a test that did would fail it unfairly.
        fix_only = ("search_knowledge_in", "is_busy", "backfill_message_keys", "execute_batch_for_test", "set_memory_source",
                    "bump_generation", "verify_token", "read_effect", "take_gateway_lock", "GENERATION_KEY")
        for case in CASES:
            with self.subTest(case["n"]):
                added = "\n".join(self.read(case))
                for name in fix_only:
                    self.assertNotIn(name, added)

    def test_every_hidden_patch_applies_to_base_and_fix(self):
        if not all(has_commit(c[k]) for c in CASES for k in ("base", "fix")):
            self.skipTest("this checkout does not have the replay commits")
        self.assertEqual(run_replay.check_patches(CASES), [])

    def test_a_patch_that_misses_the_end_of_the_file_is_reported_for_the_commit_it_misses(self):
        if not all(has_commit(c[k]) for c in CASES[:1] for k in ("base", "fix")):
            self.skipTest("this checkout does not have the replay commits")
        case = dict(CASES[0])
        with tempfile.TemporaryDirectory() as tmp:
            with open(os.path.join(REPO, case["hidden"]), encoding="utf-8") as handle:
                text = handle.read()
            broken = text.replace("\n }\n", "\n NOT THE LAST LINE\n", 1)
            self.assertNotEqual(broken, text)
            path = os.path.join(tmp, "broken.patch")
            with open(path, "w", encoding="utf-8") as handle:
                handle.write(broken)
            case["hidden"] = path  # an absolute path wins over the repository root in os.path.join
            failures = run_replay.check_patches([case])
        self.assertEqual([(n, which) for n, which, _ in failures], [(1, "base"), (1, "fix")])

    def test_a_missing_patch_file_is_reported(self):
        case = dict(CASES[0], hidden="eval/selfheal/cases/nope.patch")
        failures = run_replay.check_patches([case])
        self.assertEqual([(n, which) for n, which, _ in failures], [(1, "base"), (1, "fix")])
        self.assertIn("does not exist", failures[0][2])


class CheckPatchesCommand(unittest.TestCase):
    def run_cli(self, *args):
        env = dict(os.environ, HERON_HOME=tempfile.gettempdir())
        return subprocess.run([sys.executable, "-I", os.path.join(SELFHEAL, "run_replay.py"), *args], capture_output=True, text=True, env=env)

    def test_it_exits_zero_when_every_patch_applies(self):
        if not all(has_commit(c[k]) for c in CASES for k in ("base", "fix")):
            self.skipTest("this checkout does not have the replay commits")
        proc = self.run_cli("--check-patches")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("10 patches apply", proc.stdout)

    def test_it_exits_one_naming_each_case_that_does_not_apply(self):
        failures = [(3, "base", "error: patch failed: src/channel.rs:1"), (3, "fix", "error: patch failed"), (7, "fix", "error: x")]
        err = io.StringIO()
        with mock.patch.object(run_replay, "check_patches", return_value=failures), contextlib.redirect_stderr(err):
            self.assertEqual(run_replay.main(["--check-patches"]), 1)
        lines = err.getvalue().splitlines()
        self.assertEqual(len(lines), 3)
        self.assertTrue(lines[0].startswith("check-patches: case 3 does not apply to base"), lines[0])
        self.assertTrue(lines[2].startswith("check-patches: case 7 does not apply to fix"), lines[2])

    def test_it_exits_zero_in_process_when_nothing_fails(self):
        out = io.StringIO()
        with mock.patch.object(run_replay, "check_patches", return_value=[]), contextlib.redirect_stdout(out):
            self.assertEqual(run_replay.main(["--check-patches"]), 0)
        self.assertIn("10 patches apply", out.getvalue())

    def test_a_malformed_cases_file_exits_two(self):
        with tempfile.TemporaryDirectory() as tmp:
            cases = os.path.join(tmp, "cases.md")
            with open(cases, "w", encoding="utf-8") as handle:
                handle.write("## case 1\nbase: nothing\n")
            proc = self.run_cli("--check-patches", "--cases", cases)
            self.assertEqual(proc.returncode, 2)
            self.assertIn("case 1", proc.stderr)
        self.assertEqual(self.run_cli("--check-patches", "--cases", "/no/such/cases.md").returncode, 2)

    def test_two_modes_at_once_is_a_usage_error(self):
        self.assertEqual(self.run_cli("--validate", "--check-patches").returncode, 2)


class RunningATest(unittest.TestCase):
    def status(self, selector, code, out):
        with mock.patch.object(run_replay, "run_command", return_value=(code, out)):
            return run_replay.selector_status(selector, "/tree", {})

    def test_the_command_for_each_selector_kind(self):
        self.assertEqual(run_replay.test_command("cargo --lib selfheal_hidden_case_1"), ["cargo", "test", "--locked", "--lib", "selfheal_hidden_case_1"])
        self.assertEqual(run_replay.test_command("cargo --test integration", "/bin/cargo"), ["/bin/cargo", "test", "--locked", "--test", "integration"])
        command = run_replay.test_command("swift ThingTests")
        self.assertEqual(command[1:], ["test", "--package-path", "macos/HeronMac", "--filter", "ThingTests"])

    def test_cargo_outcomes(self):
        ok = "running 1 test\ntest a ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 300 filtered out; finished in 0.01s\n"
        bad = "running 1 test\ntest a ... FAILED\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 300 filtered out\n"
        none = "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 301 filtered out; finished in 0.00s\n"
        broken = "error[E0425]: cannot find value `x` in this scope\nerror: could not compile `heron` (lib test) due to 1 previous error\n"
        self.assertEqual(self.status("cargo --lib a", 0, ok), "passed")
        self.assertEqual(self.status("cargo --lib a", 101, bad), "failed")
        self.assertEqual(self.status("cargo --lib a", 0, none), "missing")
        self.assertEqual(self.status("cargo --lib a", 101, broken), "build_error")
        self.assertEqual(self.status("cargo --lib a", 124, "timed out after 3600s"), "build_error")
        self.assertEqual(self.status("cargo --lib a", 127, "No such file or directory"), "build_error")

    def test_a_filter_that_matches_nothing_is_missing_not_passed(self):
        # cargo exits 0 for it, so a deleted hidden test would otherwise count as a pass.
        self.assertEqual(self.status("cargo --lib gone", 0, "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out\n"), "missing")
        self.assertEqual(self.status("cargo --lib gone", 0, ""), "missing")

    def test_one_failing_binary_fails_the_run(self):
        out = "test result: ok. 1 passed; 0 failed; 0 ignored\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n"
        self.assertEqual(self.status("cargo --test t", 101, out), "failed")

    def test_swift_outcomes(self):
        self.assertEqual(self.status("swift T", 0, "Executed 1 test, with 0 failures (0 unexpected) in 0.001 seconds"), "passed")
        self.assertEqual(self.status("swift T", 1, "Executed 1 test, with 1 failure (0 unexpected) in 0.001 seconds"), "failed")
        self.assertEqual(self.status("swift T", 0, "Executed 0 tests, with 0 failures"), "missing")
        self.assertEqual(self.status("swift T", 1, "error: build failed"), "build_error")
        self.assertEqual(self.status("swift T", 0, "no output"), "missing")

    def test_the_verdict_is_the_last_json_line_and_defaults_to_needs_owner(self):
        self.assertEqual(run_replay.replay_verdict('log\n{"verdict": "auto_eligible", "attempts": 2}\n'), "auto_eligible")
        self.assertEqual(run_replay.replay_verdict('{"verdict": "needs_owner", "attempts": 1}'), "needs_owner")
        for out in ("", "no json", '{"verdict": "merged"}', "[1]", '"auto_eligible"'):
            with self.subTest(out):
                self.assertEqual(run_replay.replay_verdict(out), "needs_owner")


class Threshold(unittest.TestCase):
    def threshold(self, config):
        with tempfile.TemporaryDirectory() as home:
            if config is not None:
                with open(os.path.join(home, "config.toml"), "w", encoding="utf-8") as handle:
                    handle.write(config)
            with mock.patch.dict(os.environ, {"HERON_HOME": home}):
                return run_replay.configured_threshold()

    def test_the_default_is_six_and_the_config_overrides_it(self):
        self.assertEqual(self.threshold(None), 6)
        self.assertEqual(self.threshold("[llm]\nmodel = \"x\"\n"), 6)
        self.assertEqual(self.threshold("[harness2]\nreplay_threshold = 8\n"), 8)
        self.assertEqual(self.threshold("[harness2]\nreplay_threshold = 0\n"), 0)

    def test_a_threshold_that_is_not_a_whole_number_is_an_error(self):
        for value in ('"six"', "6.5", "true", "-1"):
            with self.subTest(value):
                with self.assertRaises(ValueError):
                    self.threshold("[harness2]\nreplay_threshold = %s\n" % value)


class FakeWorld:
    """A stand-in for git, cargo and heron: ``replies`` maps the start of a command to ``(code, output)``."""

    def __init__(self, **state):
        self.state = dict({"agent_changed": True, "applies": True, "hidden": "passed", "suite": 0, "verdict": "auto_eligible"}, **state)
        self.calls = []

    def __call__(self, argv, cwd=None, env=None, timeout=None):
        self.calls.append(list(argv))
        s = self.state
        joined = " ".join(argv)
        if "selfheal replay" in joined:
            return 0, 'working...\n{"verdict": "%s", "attempts": 1}\n' % s["verdict"]
        if joined.endswith("add -A"):
            return 0, ""
        if "diff --cached --quiet" in joined:
            return (1 if s["agent_changed"] else 0), ""
        if "apply --3way" in joined:
            return (0 if s["applies"] else 1), ""
        if argv[:3] == ["cargo", "test", "--locked"] and len(argv) == 3:
            return s["suite"], ""
        if argv[:3] == ["cargo", "test", "--locked"]:
            return {"passed": (0, "test result: ok. 1 passed; 0 failed; 0 ignored"),
                    "failed": (101, "test result: FAILED. 0 passed; 1 failed; 0 ignored"),
                    "missing": (0, "running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored")}[s["hidden"]]
        if "merge-base" in joined:
            return 0, "cafe0000\n"
        if "record-replay" in joined:
            return 0, ""
        if argv[:2] == ["cargo", "build"]:
            return 0, ""
        raise AssertionError("unexpected command: %r" % (argv,))


@contextlib.contextmanager
def fake_tree(*_):
    yield "/tree"


class ReplayCase(unittest.TestCase):
    def run_case(self, base_status="failed", **state):
        world = FakeWorld(**state)
        with mock.patch.object(run_replay, "run_command", world), mock.patch.object(run_replay, "worktree", fake_tree):
            result = run_replay.replay_case(CASES[0], "/bin/heron", base_status, {})
        return result, world

    def test_a_repair_whose_hidden_test_bites_and_whose_suite_passes_scores(self):
        (ok, verdict, got), _ = self.run_case()
        self.assertTrue(ok)
        self.assertEqual(verdict, "auto_eligible")
        self.assertEqual(got, GOOD)

    def test_the_agent_runs_on_the_given_worktree_with_the_cases_file(self):
        _, world = self.run_case()
        call = world.calls[0]
        self.assertEqual(call[:3], ["/bin/heron", "selfheal", "replay"])
        self.assertEqual(call[call.index("--case") + 1], "1")
        self.assertEqual(call[call.index("--worktree") + 1], "/tree")
        self.assertTrue(call[call.index("--cases") + 1].endswith(os.path.join("eval", "selfheal", "cases.md")))

    def test_an_agent_that_changed_nothing_scores_nothing_even_if_all_else_passes(self):
        (ok, _, got), _ = self.run_case(agent_changed=False)
        self.assertFalse(ok)
        self.assertTrue(got["patch_empty"])

    def test_a_vanished_hidden_test_scores_nothing(self):
        (ok, _, got), _ = self.run_case(hidden="missing")
        self.assertFalse(ok)
        self.assertTrue(got["hidden_test_deleted"])
        self.assertFalse(got["hidden_passed_on_fix"])

    def test_a_hidden_test_that_still_fails_scores_nothing(self):
        (ok, _, got), _ = self.run_case(hidden="failed")
        self.assertFalse(ok)
        self.assertFalse(got["hidden_passed_on_fix"])
        self.assertFalse(got["hidden_test_deleted"])

    def test_a_hidden_test_that_did_not_fail_on_base_scores_nothing(self):
        for status in ("passed", "missing", "build_error", "ungradable"):
            with self.subTest(status):
                (ok, _, got), _ = self.run_case(base_status=status)
                self.assertFalse(ok)
                self.assertFalse(got["hidden_failed_on_base"])

    def test_a_broken_suite_scores_nothing(self):
        (ok, _, got), _ = self.run_case(suite=101)
        self.assertFalse(ok)
        self.assertFalse(got["suite_passed"])

    def test_a_hidden_test_that_does_not_apply_is_ungradable(self):
        (ok, verdict, got), world = self.run_case(applies=False)
        self.assertFalse(ok)
        self.assertIsNone(got)
        self.assertEqual(verdict, "auto_eligible")
        self.assertFalse(any(c[:3] == ["cargo", "test", "--locked"] for c in world.calls))

    def test_the_agent_failing_to_run_needs_the_owner(self):
        world = FakeWorld()
        real = world.__call__

        def failing(argv, **kw):
            if "selfheal replay" in " ".join(argv):
                world.calls.append(list(argv))
                return 1, "boom"
            return real(argv, **kw)

        with mock.patch.object(run_replay, "run_command", failing), mock.patch.object(run_replay, "worktree", fake_tree):
            _, verdict, _ = run_replay.replay_case(CASES[0], "/bin/heron", "failed", {})
        self.assertEqual(verdict, "needs_owner")


class ReplayRun(unittest.TestCase):
    def run_replay(self, results, threshold, only=None, cases=None):
        """Run ``replay`` over fakes. ``results`` is the ``(passed, verdict)`` of each case in turn."""
        cases = cases or CASES
        world = FakeWorld()
        queue = iter(results)
        with tempfile.NamedTemporaryFile() as heron:
            with mock.patch.object(run_replay, "HERON", heron.name), mock.patch.object(run_replay, "run_command", world), \
                    mock.patch.object(run_replay, "hidden_status", return_value="failed"), \
                    mock.patch.object(run_replay, "replay_case", side_effect=lambda *a, **k: (lambda r: (r[0], r[1], dict(GOOD)))(next(queue))):
                out = io.StringIO()
                with contextlib.redirect_stdout(out):
                    code = run_replay.replay(cases, threshold, only)
        return code, out.getvalue(), world

    def test_it_prints_the_score_and_the_auto_eligible_count(self):
        results = [(True, "auto_eligible")] * 4 + [(True, "needs_owner")] * 2 + [(False, "needs_owner")] * 4
        code, out, _ = self.run_replay(results, 6)
        self.assertEqual(code, 0)
        self.assertIn("replay: 6/10\n", out)
        self.assertIn("auto-eligible: 4\n", out)

    def test_it_records_the_result_against_the_merge_base_with_main(self):
        results = [(True, "auto_eligible")] * 7 + [(False, "needs_owner")] * 3
        _, _, world = self.run_replay(results, 6)
        merge_base = next(c for c in world.calls if "merge-base" in c)
        self.assertEqual(merge_base[-3:], ["merge-base", "HEAD", "main"])
        record = next(c for c in world.calls if "record-replay" in c)
        self.assertEqual(record[1:], ["selfheal", "record-replay", "cafe0000", "7", "10", "7"])

    def test_it_exits_zero_at_the_threshold_and_one_below_it(self):
        results = [(True, "needs_owner")] * 6 + [(False, "needs_owner")] * 4
        self.assertEqual(self.run_replay(results, 6)[0], 0)
        self.assertEqual(self.run_replay(results, 7)[0], 1)
        self.assertEqual(self.run_replay([(False, "needs_owner")] * 10, 0)[0], 0)

    def test_a_partial_run_records_nothing(self):
        code, out, world = self.run_replay([(True, "auto_eligible")], 1, only=[3])
        self.assertEqual(code, 0)
        self.assertIn("replay: 1/1\n", out)
        self.assertFalse(any("record-replay" in c for c in world.calls))

    def test_a_failed_build_stops_before_any_case(self):
        with mock.patch.object(run_replay, "run_command", return_value=(101, "error: could not compile")), \
                mock.patch.object(run_replay, "replay_case") as case, contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(run_replay.replay(CASES, 6), 1)
        case.assert_not_called()


class RequiredTests(unittest.TestCase):
    PATH = os.path.join(SELFHEAL, "required-tests.txt")

    def lines(self):
        with open(self.PATH, "r", encoding="utf-8") as handle:
            return [line.split() for line in handle if line.strip() and not line.startswith("#")]

    def test_every_line_is_graph_selection_name(self):
        for fields in self.lines():
            with self.subTest(fields):
                self.assertEqual(len(fields), 3)
                self.assertRegex(fields[0], r"^1[abc]$")
                self.assertRegex(fields[1], r"^--[a-z-]+(=\w+)?$")
                self.assertRegex(fields[2], r"^[A-Za-z_]\w*$")

    def test_no_test_is_listed_twice_for_one_selection(self):
        keys = [tuple(f) for f in self.lines()]
        self.assertEqual(len(keys), len(set(keys)))

    def test_each_graph_has_its_selections(self):
        by_graph = {}
        for graph, selection, _ in self.lines():
            by_graph.setdefault(graph, set()).add(selection)
        self.assertEqual(by_graph, {"1a": {"--lib", "--test=integration"}, "1b": {"--lib", "--test=connector_manifests"}, "1c": {"--lib"}})

    def test_the_names_the_design_gives_are_present(self):
        have = {tuple(f) for f in self.lines()}
        for entry in (("1a", "--lib", "decision_table_selfheal"), ("1a", "--lib", "the_schema_cannot_change_without_a_new_version"),
                      ("1a", "--lib", "every_prompt_block_is_trusted_or_taints"), ("1a", "--lib", "workflow_diff_is_not_pushed"),
                      ("1a", "--lib", "core_exit_env_writes_failure_record"),
                      ("1a", "--test=integration", "api_harness2_lists_switches_and_checklist"),
                      ("1a", "--test=integration", "api_state_reports_commit_key"),
                      ("1b", "--lib", "redirect_is_not_followed"), ("1b", "--lib", "ext_source_memories_start_unconfirmed"),
                      ("1b", "--test=connector_manifests", "every_manifest_validates"),
                      ("1b", "--test=connector_manifests", "request_shapes_match"),
                      ("1c", "--lib", "observation_source_is_outside")):
            self.assertIn(entry, have)

    def test_every_name_in_a_work_unit_check_is_listed_for_its_graph(self):
        doc = os.path.join(REPO, "docs", "designs", "heron-harness2.md")
        if not os.path.isfile(doc):
            self.skipTest("the design doc is not in this checkout")
        graph = {"H0": "1a", "H1": "1a", "H2": "1a", "H2t": "1a", "H8": "1b", "H8w": "1b", "H8t": "1b", "H10": "1c"}
        have = {tuple(f) for f in self.lines()}
        seen = set()
        with open(doc, "r", encoding="utf-8") as handle:
            for row in handle:
                cells = [c.strip() for c in row.split("|")]
                if len(cells) < 8 or cells[1] not in graph:
                    continue
                for match in re.finditer(r"require-tests\.sh (--\w+(?: \w+)?) -- ([^&|`]+)", row):
                    selection = re.sub(r"^--test (\w+)$", r"--test=\1", match.group(1))
                    for name in match.group(2).split():
                        seen.add(cells[1])
                        self.assertIn((graph[cells[1]], selection, name), have, "%s: %s %s" % (cells[1], selection, name))
        self.assertEqual(seen, set(graph))


class CheckRequired(unittest.TestCase):
    SCRIPT = os.path.join(SELFHEAL, "check-required.sh")
    CARGO = """#!/bin/sh
echo "$*" >> "$STUB_LOG"
for name in $STUB_TESTS; do echo "some::module::$name: test"; done
[ "${STUB_FAIL_LIST:-}" = "$*" ] && exit 101
exit 0
"""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = self._tmp.name
        self.cargo = os.path.join(self.root, "cargo")
        with open(self.cargo, "w", encoding="utf-8") as handle:
            handle.write(self.CARGO)
        os.chmod(self.cargo, 0o755)
        self.log = os.path.join(self.root, "log")
        self.list = os.path.join(self.root, "required.txt")

    def write(self, text):
        with open(self.list, "w", encoding="utf-8") as handle:
            handle.write(text)

    def run_script(self, *args, tests="", **env):
        base = {"PATH": os.environ.get("PATH", ""), "CARGO": self.cargo, "STUB_LOG": self.log, "STUB_TESTS": tests,
                "REQUIRED_TESTS": self.list}
        return subprocess.run(["bash", self.SCRIPT, *args], capture_output=True, text=True, env=dict(base, **env), cwd=self.root)

    def listings(self):
        if not os.path.exists(self.log):
            return []
        with open(self.log, "r", encoding="utf-8") as handle:
            return [line.strip() for line in handle]

    LIST = ("# comment\n\n1a --lib alpha\n1a --lib beta\n1a --test=integration gamma\n1b --lib delta\n1c --lib epsilon\n")

    def test_it_runs_require_tests_once_per_selection_of_the_graph(self):
        self.write(self.LIST)
        proc = self.run_script("1a", tests="alpha beta gamma")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(sorted(self.listings()), ["test --locked --lib -- --list", "test --locked --test=integration -- --list"])

    def test_a_graph_runs_only_its_own_lines(self):
        self.write(self.LIST)
        self.assertEqual(self.run_script("1b", tests="delta").returncode, 0)
        self.assertEqual(self.listings(), ["test --locked --lib -- --list"])
        self.assertEqual(self.run_script("1c", tests="delta").returncode, 1, "epsilon is not among the listed tests")

    def test_a_missing_test_is_named_and_the_exit_is_one(self):
        self.write(self.LIST)
        proc = self.run_script("1a", tests="alpha gamma")
        self.assertEqual(proc.returncode, 1)
        self.assertIn("no test named 'beta'", proc.stderr)
        self.assertNotIn("alpha", proc.stderr)
        self.assertNotIn("gamma", proc.stderr)

    def test_every_group_runs_even_after_one_fails(self):
        self.write(self.LIST)
        proc = self.run_script("1a", tests="gamma")
        self.assertEqual(proc.returncode, 1)
        self.assertIn("'alpha'", proc.stderr)
        self.assertIn("'beta'", proc.stderr)
        self.assertEqual(len(self.listings()), 2, "the integration group ran too")

    def test_a_failing_listing_is_a_failure(self):
        self.write(self.LIST)
        proc = self.run_script("1b", tests="delta", STUB_FAIL_LIST="test --locked --lib -- --list")
        self.assertEqual(proc.returncode, 1)

    def test_a_name_that_only_prefixes_a_listed_test_does_not_count(self):
        self.write("1a --lib alpha\n")
        self.assertEqual(self.run_script("1a", tests="alpha_in_the_sandbox").returncode, 1)

    def test_usage_errors_exit_two(self):
        self.write(self.LIST)
        for args in ((), ("1a", "1b"), ("2z",), ("",), ("1d",)):
            with self.subTest(args):
                proc = self.run_script(*args)
                self.assertEqual(proc.returncode, 2, proc.stderr)
                self.assertTrue(proc.stderr.strip())
        self.assertEqual(self.listings(), [], "nothing ran for a usage error")

    def test_a_missing_list_exits_two(self):
        proc = self.run_script("1a", REQUIRED_TESTS=os.path.join(self.root, "nope.txt"))
        self.assertEqual(proc.returncode, 2)
        self.assertIn("no such list", proc.stderr)

    def test_a_malformed_line_exits_two_and_is_named(self):
        for bad in ("1a --lib\n", "1a --test integration gamma\n", "1a lib alpha\n", "one --lib alpha\n", "1a --lib two words\n",
                    "1a --lib bad-name\n"):
            with self.subTest(bad):
                self.write("1a --lib fine\n" + bad)
                proc = self.run_script("1a", tests="fine")
                self.assertEqual(proc.returncode, 2, proc.stderr)
                self.assertIn("line 2", proc.stderr)
        self.assertEqual(self.listings(), [])

    def test_a_graph_with_no_lines_exits_two(self):
        self.write("1a --lib alpha\n")
        proc = self.run_script("1b")
        self.assertEqual(proc.returncode, 2)
        self.assertIn("no required tests for graph '1b'", proc.stderr)

    def test_the_committed_list_is_clean_for_all_three_graphs(self):
        for graph in ("1a", "1b", "1c"):
            with self.subTest(graph):
                proc = self.run_script(graph, REQUIRED_TESTS=RequiredTests.PATH, STUB_TESTS="")
                self.assertEqual(proc.returncode, 1, "nothing is listed, so every name is missing")
                self.assertIn("no test named", proc.stderr)
                self.assertNotIn("malformed line", proc.stderr)

    def test_the_script_runs_on_the_bash_macos_ships(self):
        with open(self.SCRIPT, "r", encoding="utf-8") as handle:
            text = handle.read()
        for banned in ("declare -A", "mapfile", "readarray", "${!", "&>>"):
            self.assertNotIn(banned, text)
        self.assertEqual(subprocess.run(["bash", "-n", self.SCRIPT]).returncode, 0)

    def test_it_finds_its_files_from_any_directory(self):
        self.assertEqual(os.path.dirname(self.SCRIPT), SELFHEAL)
        proc = subprocess.run(["bash", self.SCRIPT], capture_output=True, text=True, cwd=self.root)
        self.assertEqual(proc.returncode, 2)


def verdict(task, completion):
    return rv.verdict(rv.normalize_task({"id": "x", "input": task, "reference": ""}), completion)


def load_rows():
    with open(build_genesis_cases.CASES_PATH, "r", encoding="utf-8") as handle:
        return [json.loads(line) for line in handle]


ROWS = load_rows()


def record(**changes):
    base = {"case": 1, "fix": SHA_A, "outcome": dict(GOOD), "pass": True}
    base.update(changes)
    return base


def task(**changes):
    base = {"case": 1, "fix": SHA_A, "outcome": dict(GOOD)}
    base.update(changes)
    return base


def small_verdict(completion, the_task=None):
    return rv.verdict(the_task or task(), completion if isinstance(completion, str) else json.dumps(completion))


class GenesisDataset(unittest.TestCase):
    def test_the_committed_dataset_is_what_the_builder_makes(self):
        with open(build_genesis_cases.CASES_PATH, "r", encoding="utf-8") as handle:
            self.assertEqual(handle.read(), build_genesis_cases.render(build_genesis_cases.build(CASES)))
        self.assertEqual(build_genesis_cases.main(["--check"]), 0)
        self.assertEqual(build_genesis_cases.main(["--bogus"]), 2)

    def test_there_is_one_row_per_case_with_a_bad_record_for_each_outcome_field(self):
        self.assertEqual([row["id"] for row in ROWS], ["case-%d" % n for n in range(1, 11)])
        for row in ROWS:
            self.assertEqual({bad["expect"] for bad in row["bad"]}, set(GOOD))

    def test_every_gold_record_is_accepted(self):
        for row in ROWS:
            with self.subTest(row["id"]):
                ok, why = verdict(row["input"], row["reference"])
                self.assertTrue(ok, why)

    def test_every_known_bad_record_is_rejected_for_its_field(self):
        for row in ROWS:
            for bad in row["bad"]:
                with self.subTest(msg="%s: %s" % (row["id"], bad["why"])):
                    ok, why = verdict(bad["input"], bad["completion"])
                    self.assertFalse(ok)
                    self.assertIn("the outcome fails the grade", why)
                    self.assertIn(bad["expect"], why)

    def test_a_known_bad_record_is_well_formed_and_forges_a_pass(self):
        for row in ROWS:
            for bad in row["bad"]:
                with self.subTest(msg="%s: %s" % (row["id"], bad["why"])):
                    value = rv.jv.strict_loads(bad["completion"])
                    rv.check_record(value)
                    self.assertIs(value["pass"], True)
                    self.assertEqual(value["outcome"], json.loads(bad["input"])["outcome"])
                    self.assertFalse(grader.grade(value["outcome"]))

    def test_the_gold_records_all_differ_so_another_cases_record_never_passes(self):
        self.assertEqual(len({row["reference"] for row in ROWS}), 10)
        for row in ROWS:
            for other in ROWS:
                if other is not row:
                    ok, why = verdict(row["input"], other["reference"])
                    self.assertFalse(ok)
                    self.assertIn("the record's case is not the task's", why)

    def test_the_spec_runs_the_verifier_over_this_dataset(self):
        with open(os.path.join(SELFHEAL, "genesis.yaml"), "r", encoding="utf-8") as handle:
            text = handle.read()
        self.assertIn("path: eval/selfheal/genesis_cases.jsonl", text)
        self.assertIn("python3 -I eval/selfheal/replay_verifier.py {task_file} {completion_file} {output_file}", text)
        self.assertIn("type: command", text)


class StrictParsing(unittest.TestCase):
    def reject(self, completion, reason, the_task=None):
        ok, why = small_verdict(completion, the_task)
        self.assertFalse(ok, why)
        self.assertIn(reason, why)

    def test_the_baseline_record_is_accepted(self):
        self.assertTrue(small_verdict(record())[0])

    def test_whitespace_and_key_order_do_not_matter(self):
        self.assertTrue(small_verdict("\n  " + json.dumps(record(), sort_keys=True, indent=4) + "  \n\n")[0])

    def test_malformed_completions_are_rejected_not_crashes(self):
        body = json.dumps(record())
        cases = {
            "duplicate key": ('{"case": 9, ' + body[1:], "duplicate key 'case'"),
            "duplicate key in the outcome": (body.replace('"suite_passed": true', '"suite_passed": false, "suite_passed": true'), "duplicate key"),
            "prose before": ("Here is the record: " + body, "not a single clean JSON document"),
            "prose after": (body + "\nHope that helps!", "not a single clean JSON document"),
            "a code fence": ("```json\n" + body + "\n```", "not a single clean JSON document"),
            "two documents": (body + " " + body, "not a single clean JSON document"),
            "a trailing comma": (body + ",", "not a single clean JSON document"),
            "single quotes": ("{'case': 1}", "not a single clean JSON document"),
            "empty": ("", "the completion is empty"),
            "blank": ("  \n\t ", "the completion is empty"),
            "a byte order mark": ("﻿" + body, "not a single clean JSON document"),
            "NaN": (body.replace('"case": 1', '"case": NaN'), "is not JSON"),
            "too deep": ("[" * 200000 + "]" * 200000, "the document nests too deeply"),
        }
        for name, (completion, reason) in cases.items():
            with self.subTest(name):
                self.reject(completion, reason)

    def test_a_record_must_be_an_object(self):
        for completion in ("[]", '"x"', "null", "5", "true", "[%s]" % json.dumps(record())):
            with self.subTest(completion[:20]):
                self.reject(completion, "must be a JSON object")

    def test_a_field_outside_the_schema_is_rejected_whatever_it_says(self):
        for extra in ({"verdict": "pass"}, {"score": 1}, {"Pass": True}, {"graded": True}):
            with self.subTest(extra):
                self.reject(record(**extra), "outside the schema: %r" % next(iter(extra)))

    def test_every_field_is_required(self):
        for key in rv.RECORD_KEYS:
            with self.subTest(key):
                partial = record()
                del partial[key]
                self.reject(partial, "the replay record is missing: " + key)

    def test_values_of_the_wrong_type_are_rejected(self):
        bad = {"case": ["1", True, 1.5, 0, -1, None, [1]], "fix": [None, 5, "abc", SHA_A.upper(), SHA_A + "a", "g" * 40],
               "pass": ["true", 1, 0, None], "outcome": [None, [], "ok", {}, dict(GOOD, extra=True)]}
        for key, values in bad.items():
            for value in values:
                with self.subTest(msg="%s=%r" % (key, value)):
                    ok, why = small_verdict(record(**{key: value}))
                    self.assertFalse(ok)
                    self.assertTrue("must be" in why or "outcome" in why, why)

    def test_an_outcome_field_must_be_a_real_boolean(self):
        for key in GOOD:
            for wrong in ("true", 1, 0, None, "false"):
                with self.subTest(msg="%s=%r" % (key, wrong)):
                    self.reject(record(outcome=outcome(**{key: wrong})), "every outcome field must be true or false")

    def test_an_outcome_with_a_field_added_or_removed_is_rejected(self):
        self.reject(record(outcome=dict(GOOD, bonus=True)), "outcome must have exactly")
        partial = dict(GOOD)
        del partial["suite_passed"]
        self.reject(record(outcome=partial), "outcome must have exactly")


class RecordAgainstTask(unittest.TestCase):
    def reject(self, completion, reason, the_task=None):
        ok, why = small_verdict(completion, the_task)
        self.assertFalse(ok, why)
        self.assertIn(reason, why)

    def test_a_record_about_another_case_is_rejected(self):
        self.reject(record(case=2), "the record's case is not the task's")

    def test_a_record_about_another_fix_is_rejected(self):
        self.reject(record(fix=SHA_B), "the record's fix is not the task's")

    def test_a_record_that_changes_the_outcome_is_rejected_either_way(self):
        self.reject(record(outcome=outcome(suite_passed=False)), "the record's outcome is not the task's")
        failing = task(outcome=outcome(patch_empty=True))
        self.reject(record(), "the record's outcome is not the task's", failing)

    def test_a_forged_pass_does_not_survive_a_failing_outcome(self):
        for key in GOOD:
            with self.subTest(key):
                wrong = outcome(**{key: not GOOD[key]})
                self.reject(record(outcome=wrong), "the outcome fails the grade: " + key, task(outcome=wrong))

    def test_an_empty_patch_and_a_deleted_hidden_test_are_named(self):
        wrong = outcome(patch_empty=True, hidden_test_deleted=True)
        self.reject(record(outcome=wrong), "patch_empty, hidden_test_deleted", task(outcome=wrong))

    def test_a_record_that_denies_a_passing_outcome_is_rejected(self):
        self.reject(record(**{"pass": False}), "the outcome passes the grade but the record says otherwise")

    def test_a_forged_failure_is_not_a_pass(self):
        wrong = outcome(hidden_passed_on_fix=False)
        self.reject(record(outcome=wrong, **{"pass": False}), "the outcome fails the grade", task(outcome=wrong))

    def test_the_verdict_is_the_graders(self):
        # The verifier adds strictness and consistency checks and nothing else: the verdict on the outcome is grade's.
        for row in ROWS[:3]:
            with self.subTest(row["id"]):
                with mock.patch.object(grader, "grade", return_value=False):
                    ok, why = verdict(row["input"], row["reference"])
                self.assertFalse(ok)
                self.assertIn("the outcome fails the grade", why)
                for bad in row["bad"]:
                    with mock.patch.object(grader, "grade", return_value=True):
                        self.assertTrue(verdict(bad["input"], bad["completion"])[0])

    def test_grade_gets_the_tasks_outcome(self):
        with mock.patch.object(grader, "grade", wraps=grader.grade) as spy:
            small_verdict(record())
        spy.assert_called_with(GOOD)


class TaskShapes(unittest.TestCase):
    def test_a_replay_task_in_each_of_its_shapes(self):
        shapes = [task(), {"id": "x", "context": task()}, {"id": "x", "input": json.dumps(task()), "reference": "r", "source": "cases.md", "bad": []}]
        for shape in shapes:
            with self.subTest(sorted(shape)):
                self.assertEqual(rv.normalize_task(shape), task())

    def test_unknown_or_missing_keys_are_refused(self):
        for raw, message in (({"outcome": dict(GOOD)}, "a replay task needs: case, fix"),
                             ({"input": json.dumps(dict(task(), extra=1))}, "unknown keys: extra"),
                             ({"context": dict(task(), **{"pass": True})}, "unknown keys: pass"),
                             (dict(task(), oops=1), "unknown keys: oops")):
            with self.subTest(message):
                with self.assertRaises(ValueError) as caught:
                    rv.normalize_task(raw)
                self.assertIn(message, str(caught.exception))

    def test_a_malformed_task_is_refused(self):
        for raw in (task(case=0), task(case="1"), task(fix="abc"), task(outcome={}), task(outcome=dict(GOOD, suite_passed="yes"))):
            with self.subTest(raw):
                with self.assertRaises(ValueError):
                    rv.verdict(rv.normalize_task(raw), json.dumps(record()))

    def test_a_task_that_is_neither_shape_is_refused(self):
        for raw in ([], {"input": "not json"}, {"input": "{}"}, {"context": {"prompt": "x"}}, {}, {"rule": {"file_present": {"path": "a"}}, "run": {}}):
            with self.subTest(raw):
                with self.assertRaises(ValueError):
                    rv.normalize_task(raw)


SUITE_SCHEMA = {"type": "object", "required": ["city", "units"], "additionalProperties": False,
                "properties": {"city": {"type": "string", "minLength": 1}, "units": {"type": "string", "enum": ["c", "f"]}}}
SUITE_TASK = {"id": "json/schema-only-1", "input": "Call the `get_weather` tool for Paris, in Celsius.", "reference": '{"city":"Paris","units":"c"}',
              "context": {"prompt": "Call it.", "reference": '{"city":"Paris","units":"c"}', "schema": SUITE_SCHEMA}, "kind": "reference-based"}


class SuiteProbes(unittest.TestCase):
    def check(self, completion):
        return rv.verdict(rv.normalize_task(SUITE_TASK), completion)

    def test_genesis_own_probes_are_judged_by_the_strict_parser(self):
        self.assertTrue(self.check('{"city":"Paris","units":"c"}')[0])
        for completion, reason in (('{"city":"","units":"c"}', "shorter than 1"), ('{"city":"Nowhere","units":"f"}', "not the reference answer"),
                                   ('{"city":"Paris","units":"c","debug":true}', "undeclared field 'debug'"),
                                   ('{"city":"Nowhere","city":"Paris","units":"c"}', "duplicate key 'city'"),
                                   ('Sure! {"city":"Paris","units":"c"}', "not a single clean JSON document")):
            with self.subTest(reason):
                ok, why = self.check(completion)
                self.assertFalse(ok)
                self.assertIn(reason, why)

    def test_a_completion_that_nests_too_deeply_is_a_rejection(self):
        with mock.patch.object(rv.jv, "strict_loads", side_effect=RecursionError):
            self.assertEqual(rv.verdict(task(), "{}"), (False, "rejected: the completion nests too deeply"))
            self.assertEqual(rv.verdict({"reference": "1", "schema": {}}, "{}"), (False, "rejected: the completion nests too deeply"))


class CommandLine(unittest.TestCase):
    VERIFIER = os.path.join(SELFHEAL, "replay_verifier.py")

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = self._tmp.name

    def write(self, name, text):
        path = os.path.join(self.root, name)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(text)
        return path

    def run_cli(self, *args, cwd=None):
        return subprocess.run([sys.executable, "-I", self.VERIFIER, *args], capture_output=True, text=True, cwd=cwd)

    def dataset_task(self, row, text=None):
        return self.write("task.json", json.dumps({"id": row["id"], "input": text or row["input"], "reference": row["reference"]}))

    def test_a_gold_record_prints_a_pass_verdict(self):
        row = ROWS[0]
        proc = self.run_cli(self.dataset_task(row), "{completion_file}", self.write("output.json", row["reference"]))
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(json.loads(proc.stdout), {"pass": True, "score": 1.0, "reward": 1})
        self.assertIn("accepted", proc.stderr)

    def test_a_bad_record_prints_a_reject_verdict_at_exit_zero(self):
        row = ROWS[0]
        bad = row["bad"][0]
        proc = self.run_cli(self.dataset_task(row, bad["input"]), self.write("c.txt", bad["completion"]), "{output_file}")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(json.loads(proc.stdout), {"pass": False, "score": 0.0, "reward": 0})
        self.assertIn("the outcome fails the grade", proc.stderr)

    def test_invalid_utf8_in_a_completion_is_rejected_at_exit_zero(self):
        row = ROWS[0]
        out = self.write("o.json", "")
        with open(out, "wb") as handle:
            handle.write(b'{"case": 1, "fix": "\xff\xfe"}')
        proc = self.run_cli(self.dataset_task(row), "{completion_file}", out)
        self.assertEqual(proc.returncode, 0)
        self.assertFalse(json.loads(proc.stdout)["pass"])

    def test_a_suite_probe_in_genesis_shape_is_judged(self):
        path = self.write("task.json", json.dumps(SUITE_TASK))
        good = self.run_cli(path, "{completion_file}", self.write("o1.json", '{"city":"Paris","units":"c"}'))
        bad = self.run_cli(path, "{completion_file}", self.write("o2.json", '{"city":"Paris","units":"c","x":1}'))
        self.assertTrue(json.loads(good.stdout)["pass"])
        self.assertFalse(json.loads(bad.stdout)["pass"])

    def test_a_malformed_task_exits_two_and_prints_no_verdict(self):
        for text in ("[]", "not json", json.dumps(task(fix="x")), json.dumps({"id": "x"}), json.dumps(dict(task(), oops=1))):
            with self.subTest(text):
                proc = self.run_cli(self.write("task.json", text), self.write("c.txt", "{}"))
                self.assertEqual(proc.returncode, 2)
                self.assertEqual(proc.stdout, "")
                self.assertIn("malformed task", proc.stderr)

    def test_usage_errors_exit_two(self):
        self.assertEqual(self.run_cli().returncode, 2)
        self.assertEqual(self.run_cli("only-one-argument").returncode, 2)
        proc = self.run_cli(self.write("task.json", "{}"), "{completion_file}", "{output_file}")
        self.assertEqual(proc.returncode, 2)
        self.assertIn("none of", proc.stderr)

    def test_the_command_runs_under_isolated_mode_from_any_directory(self):
        row = ROWS[3]
        proc = self.run_cli(self.dataset_task(row), self.write("o.json", row["reference"]), cwd=self.root)
        self.assertTrue(json.loads(proc.stdout)["pass"], proc.stderr)


if __name__ == "__main__":
    unittest.main()
