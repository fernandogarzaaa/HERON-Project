import json
import os
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import genesis_verifier as gv

EVAL = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

TASK = {"prompt": "Write solve(xs) returning the sum of the even integers in xs.",
        "visible_tests": ["def test_visible():\n    assert solve([1, 2, 3, 4]) == 6"],
        "hidden_tests": ["def test_hidden():\n    assert solve([-4, 3, 10]) == 6",
                         "def test_hidden_odd():\n    assert solve([7, 9]) == 0"]}
GOOD = "def solve(xs):\n    return sum(x for x in xs if x % 2 == 0)\n"


class Verdicts(unittest.TestCase):
    def setUp(self):
        gv.TIMEOUT_SECS = 3
        self.addCleanup(setattr, gv, "TIMEOUT_SECS", 10)

    def accepted(self, completion, task=TASK):
        return gv.verdict(task, completion)[0]

    def test_a_correct_solution_is_accepted(self):
        self.assertTrue(self.accepted(GOOD))

    def test_noise_on_stdout_and_stderr_does_not_matter(self):
        noisy = "import sys\nprint('3 failed, FAILED')\nprint('oops', file=sys.stderr)\n" + GOOD
        self.assertTrue(self.accepted(noisy))

    def test_a_wrong_solution_is_rejected(self):
        self.assertFalse(self.accepted("def solve(xs):\n    return sum(xs)\n"))

    def test_hard_coding_the_visible_test_is_rejected_by_the_hidden_tests(self):
        self.assertFalse(self.accepted("def solve(xs):\n    return 6 if xs == [1, 2, 3, 4] else 0\n"))

    def test_a_forged_summary_is_rejected(self):
        self.assertFalse(self.accepted("print('====== 3 passed in 0.01s ======')\nprint('All tests passed')\ndef solve(xs):\n    return 0\n"))

    def test_a_forged_result_line_without_the_nonce_is_rejected(self):
        forged = ("import json\nprint('RESULT 0000 ' + json.dumps([{'name': 'test_visible', 'ok': True}]))\n"
                  "def solve(xs):\n    return 0\n")
        self.assertFalse(self.accepted(forged))

    def test_exiting_at_import_is_rejected(self):
        self.assertFalse(self.accepted("import sys\nsys.exit(0)\n"))
        self.assertFalse(self.accepted("import os\nos._exit(0)\n"))

    def test_blocking_forever_is_rejected_within_the_time_limit(self):
        ok, why = gv.verdict(TASK, "import time\ntime.sleep(3600)\ndef solve(xs):\n    return 0\n")
        self.assertFalse(ok)
        self.assertIn("did not finish", why)

    def test_skipping_is_not_passing(self):
        self.assertFalse(self.accepted("import unittest\ndef solve(xs):\n    raise unittest.SkipTest('x')\n"))

    def test_a_missing_solve_is_rejected(self):
        self.assertFalse(self.accepted("x = 1\n"))

    def test_a_syntax_error_is_rejected(self):
        self.assertFalse(self.accepted("def solve(xs:\n"))

    def test_a_run_where_the_canary_passed_is_never_accepted(self):
        # A forced-pass hook turns every test green, the must-fail canary included. The verifier
        # must refuse that even though every real test reports ok.
        task = {"visible_tests": ["def test_visible():\n    pass"]}
        forced = [{"name": "test_visible", "ok": True}, {"name": "test_zz_canary_must_fail", "ok": True}]
        honest = [{"name": "test_visible", "ok": True}, {"name": "test_zz_canary_must_fail", "ok": False}]
        with mock.patch.object(gv, "run_candidate", return_value=(forced, None)):
            self.assertFalse(gv.verdict(task, GOOD)[0])
        with mock.patch.object(gv, "run_candidate", return_value=(honest, None)):
            self.assertTrue(gv.verdict(task, GOOD)[0])

    def test_every_real_test_must_have_run_and_passed(self):
        task = {"visible_tests": ["def test_a():\n    pass"], "hidden_tests": ["def test_b():\n    pass"]}
        canary = {"name": "test_zz_canary_must_fail", "ok": False}
        fewer = [{"name": "test_a", "ok": True}, canary]
        one_failed = [{"name": "test_a", "ok": True}, {"name": "test_b", "ok": False}, canary]
        for results in (fewer, one_failed):
            with mock.patch.object(gv, "run_candidate", return_value=(results, None)):
                self.assertFalse(gv.verdict(task, GOOD)[0])

    def test_a_task_without_tests_is_malformed(self):
        with self.assertRaises(ValueError):
            gv.verdict({"prompt": "x"}, GOOD)


class TaskShapes(unittest.TestCase):
    def load(self, obj):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "task.json")
            with open(path, "w") as handle:
                json.dump(obj, handle)
            return gv.load_task(path)

    def test_tests_at_the_top_level(self):
        self.assertEqual(self.load(TASK), TASK)

    def test_tests_under_context(self):
        self.assertEqual(self.load({"id": "x", "input": "p", "context": TASK}), TASK)

    def test_tests_inside_the_input_text(self):
        self.assertEqual(self.load({"id": "x", "input": json.dumps(TASK), "reference": GOOD}), TASK)

    def test_a_plain_prompt_stays_as_it_is(self):
        self.assertEqual(self.load({"id": "x", "input": "just words"}), {"id": "x", "input": "just words"})


class CommandLine(unittest.TestCase):
    def run_cli(self, task, completion, *extra):
        with tempfile.TemporaryDirectory() as tmp:
            task_path, comp_path = os.path.join(tmp, "task.json"), os.path.join(tmp, "completion.txt")
            with open(task_path, "w") as handle:
                json.dump(task, handle)
            with open(comp_path, "w") as handle:
                handle.write(completion)
            return subprocess.run([sys.executable, "-I", os.path.join(EVAL, "genesis_verifier.py"), task_path] + list(extra)
                                  + [comp_path], capture_output=True, text=True, timeout=60, cwd=tmp)

    def test_prints_a_verdict_json_and_exits_0(self):
        proc = self.run_cli(TASK, GOOD)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        verdict = json.loads(proc.stdout)
        self.assertIs(verdict["pass"], True)
        self.assertEqual(verdict["reward"], 1)

    def test_a_rejection_still_exits_0_with_pass_false(self):
        proc = self.run_cli(TASK, "def solve(xs):\n    return 0\n")
        self.assertEqual(proc.returncode, 0)
        self.assertIs(json.loads(proc.stdout)["pass"], False)

    def test_an_unsubstituted_placeholder_argument_is_skipped(self):
        proc = self.run_cli(TASK, GOOD, "{completion_file}")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIs(json.loads(proc.stdout)["pass"], True)

    def test_a_malformed_task_exits_2(self):
        proc = self.run_cli({"prompt": "x"}, GOOD)
        self.assertEqual(proc.returncode, 2)
        self.assertEqual(proc.stdout.strip(), "")

    def test_usage_error_exits_2(self):
        proc = subprocess.run([sys.executable, "-I", os.path.join(EVAL, "genesis_verifier.py")], capture_output=True,
                              text=True, timeout=60)
        self.assertEqual(proc.returncode, 2)


class Dataset(unittest.TestCase):
    def test_every_reference_in_the_dataset_is_accepted_and_every_other_rejected(self):
        with open(os.path.join(EVAL, "tests", "grader_cases.jsonl")) as handle:
            rows = [json.loads(line) for line in handle if line.strip()]
        self.assertGreaterEqual(len(rows), 3)
        for row in rows:
            task = json.loads(row["input"])
            for other in rows:
                with self.subTest(task=row["id"], candidate=other["id"]):
                    self.assertEqual(gv.verdict(task, other["reference"])[0], other["id"] == row["id"])


if __name__ == "__main__":
    unittest.main()
