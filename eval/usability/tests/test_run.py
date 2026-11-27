import contextlib
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
USABILITY = os.path.dirname(HERE)
EVAL = os.path.dirname(USABILITY)
for path in (USABILITY, EVAL, os.path.join(USABILITY, "genesis")):
    if path not in sys.path:
        sys.path.insert(0, path)

import build_cases  # noqa: E402
import run  # noqa: E402
import verifier  # noqa: E402


def quiet_main(argv):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = run.main(argv)
    return code, out.getvalue(), err.getvalue()


class ReplayTests(unittest.TestCase):
    def test_replay_writes_reports_and_exits_0(self):
        with tempfile.TemporaryDirectory() as out:
            code, stdout, _ = quiet_main(["--out", out])
            self.assertEqual(code, 0, stdout)
            with open(os.path.join(out, "summary.json")) as handle:
                summary = json.load(handle)
            self.assertEqual((summary["mode"], summary["provider"], summary["driver"]), ("replay", "fixtures", "replay"))
            by_task = {e["task"]: e for e in summary["runs"]}
            self.assertEqual(len(by_task), 10)
            for task_id, entry in by_task.items():
                kind, _ = run.fixture_for(task_id)
                if kind is None:
                    self.assertEqual(entry["status"], "skipped-blocked")
                    self.assertIsNone(entry["completed"])
                else:
                    self.assertEqual(entry["status"], "replayed-" + kind)
                    self.assertTrue(entry["completed"], (task_id, entry["failed_checks"]))
                self.assertTrue(os.path.isfile(os.path.join(out, "runs", "%s-%s" % (task_id, entry["persona"]), "report.md")))
            with open(os.path.join(out, "summary.md")) as handle:
                self.assertIn("| u02 | lena-teacher | replayed-recorded | yes |", handle.read())

    def test_strict_fails_when_a_run_did_not_complete(self):
        with tempfile.TemporaryDirectory() as out:
            fixtures = os.path.join(out, "fx")
            os.makedirs(os.path.join(fixtures, "recorded"))
            with open(os.path.join(run.FIXTURES, "recorded", "u08.json")) as handle:
                fixture = json.load(handle)
            fixture["steps"][1]["status"] = 200
            with open(os.path.join(fixtures, "recorded", "u08.json"), "w") as handle:
                json.dump(fixture, handle)
            saved = run.FIXTURES
            run.FIXTURES = fixtures
            try:
                self.assertEqual(quiet_main(["--out", os.path.join(out, "a"), "--tasks", "u08"])[0], 0)
                self.assertEqual(quiet_main(["--out", os.path.join(out, "b"), "--tasks", "u08", "--strict"])[0], 1)
            finally:
                run.FIXTURES = saved

    def test_assistive_persona_rows_say_api_only(self):
        with tempfile.TemporaryDirectory() as out:
            quiet_main(["--out", out, "--tasks", "u03", "--all-personas"])
            with open(os.path.join(out, "summary.json")) as handle:
                rows = {e["persona"]: e for e in json.load(handle)["runs"]}
            self.assertIn("api-only", rows["omar-keyboard"]["persona_note"])
            self.assertIsNone(rows["lena-teacher"]["persona_note"])

    def test_usage_errors(self):
        self.assertEqual(quiet_main(["--tasks", "u99"])[0], 2)
        self.assertEqual(quiet_main(["--driver", "eve"])[0], 2)
        self.assertEqual(quiet_main(["--live", "--heron-bin", "/nonexistent/heron"])[0], 2)
        self.assertEqual(quiet_main(["--check"])[0], 0)

    def test_fixture_step_count_mismatch_is_a_harness_error(self):
        with tempfile.TemporaryDirectory() as out:
            fixtures = os.path.join(out, "fx")
            os.makedirs(os.path.join(fixtures, "recorded"))
            with open(os.path.join(run.FIXTURES, "recorded", "u02.json")) as handle:
                fixture = json.load(handle)
            fixture["steps"].append(fixture["steps"][0])
            with open(os.path.join(fixtures, "recorded", "u02.json"), "w") as handle:
                json.dump(fixture, handle)
            saved = run.FIXTURES
            run.FIXTURES = fixtures
            try:
                code, _, _ = quiet_main(["--out", os.path.join(out, "o"), "--tasks", "u02"])
            finally:
                run.FIXTURES = saved
            self.assertEqual(code, 1)

    def test_run_sh_replays(self):
        with tempfile.TemporaryDirectory() as out:
            done = subprocess.run(["bash", os.path.join(USABILITY, "run.sh"), "--out", out, "--tasks", "u02"],
                                  capture_output=True, text=True, timeout=120)
            self.assertEqual(done.returncode, 0, done.stderr)
            self.assertIn("replayed-recorded", done.stdout)


class LeakTests(unittest.TestCase):
    def test_finds_tokens_outside_home(self):
        with tempfile.TemporaryDirectory() as out:
            os.makedirs(os.path.join(out, "runs", "u01-x", "logs"))
            os.makedirs(os.path.join(out, "runs", "u01-x", "home"))
            clean = os.path.join(out, "runs", "u01-x", "logs", "core.out.log")
            with open(clean, "w") as handle:
                handle.write("http://127.0.0.1:1/?token=<redacted>\n{\"token\": \"not-a-bot-token\"}")
            with open(os.path.join(out, "runs", "u01-x", "home", "state"), "w") as handle:
                handle.write("SECRETVALUE1234")
            self.assertEqual(run.check_leaks(out, ["SECRETVALUE1234"]), [])
            with open(os.path.join(out, "runs", "u01-x", "record.json"), "w") as handle:
                handle.write("x SECRETVALUE1234 y")
            self.assertEqual(run.check_leaks(out, ["SECRETVALUE1234"]), [os.path.join("runs", "u01-x", "record.json")])
            with open(os.path.join(out, "a.log"), "w") as handle:
                handle.write("open http://127.0.0.1:9/?token=abcdefgh12")
            with open(os.path.join(out, "b.json"), "w") as handle:
                handle.write('{"token": "abcdefghijklmnop1234"}')
            self.assertEqual(sorted(run.check_leaks(out, [])), ["a.log", "b.json"])

    def test_scrub(self):
        home = os.path.expanduser("~")
        places = {"home": "/tmp/r/home", "work": "/tmp/r/work", "web": "http://127.0.0.1:5000"}
        redact = run.client.Redactor()
        redact.add("TOK")
        value = {"a": ["read /tmp/r/home/x and /tmp/r/work/y", "http://127.0.0.1:5000/facts.html", "TOK"],
                 "b": home + "/Documents", "c": 3}
        out = run.scrub(value, places, redact)
        self.assertEqual(out["a"], ["read {home}/x and {work}/y", "{web}/facts.html", "<redacted>"])
        self.assertEqual(out["b"], "~/Documents")
        self.assertEqual(out["c"], 3)
        self.assertNotIn(os.path.basename(home), json.dumps(out))

    def test_committed_fixtures_are_scrubbed(self):
        user = os.path.basename(os.path.expanduser("~"))
        for kind in ("recorded", "synthetic"):
            folder = os.path.join(run.FIXTURES, kind)
            for name in os.listdir(folder):
                with open(os.path.join(folder, name)) as handle:
                    text = handle.read()
                for needle in ("/Users/", "/var/folders", "127.0.0.1:", "?token=", user):
                    self.assertNotIn(needle, text, "%s/%s" % (kind, name))
                fixture = json.loads(text)
                self.assertEqual(fixture["kind"], kind)
                self.assertIn("ynthetic" if kind == "synthetic" else "Recorded", fixture["_provenance"])


class VerifierTests(unittest.TestCase):
    def rows(self):
        with open(build_cases.CASES_PATH) as handle:
            return [json.loads(line) for line in handle]

    def test_cases_are_up_to_date(self):
        with open(build_cases.CASES_PATH) as handle:
            self.assertEqual(handle.read(), build_cases.build())

    def test_every_good_run_is_accepted(self):
        for row in self.rows():
            task = verifier.normalize_task({"id": row["id"], "input": row["input"], "reference": row["reference"]})
            self.assertEqual(verifier.verdict(task, row["reference"]), (True, "accepted"), row["id"])

    def test_every_bad_case_is_rejected_for_its_leaf(self):
        kinds = set()
        for row in self.rows():
            for bad in row["bad"]:
                task = verifier.normalize_task({"input": bad["input"]})
                accepted, why = verifier.verdict(task, bad["completion"])
                self.assertFalse(accepted, (row["id"], bad["why"]))
                self.assertIn(bad["expect"], why, (row["id"], bad["why"]))
                kinds.add(bad["expect"])
        self.assertEqual(kinds, {"core_pass", "event_seen", "event_absent", "http_status", "step_reply_matches",
                                 "file_contains", "audit_has", "transcript_lacks"})

    def test_forged_records_are_rejected(self):
        row = self.rows()[0]
        task = verifier.normalize_task({"input": row["input"]})
        good = json.loads(row["reference"])
        forged = [
            dict(good, completed=False),
            dict(good, failed_checks=["x"]),
            dict(good, task="u09"),
            dict(good, variant="demo" if good["variant"] == "default" else "default"),
            dict(good, extra=1),
            {k: v for k, v in good.items() if k != "failed_checks"},
            dict(good, completed="true"),
            dict(good, failed_checks=[1]),
            dict(good, task=5),
        ]
        for record in forged:
            self.assertFalse(verifier.verdict(task, json.dumps(record))[0], record)
        for text in ("", "[]", "ok " + row["reference"], row["reference"][:-1] + ', "task": "u02"}',
                     '{"task": NaN}'):
            self.assertFalse(verifier.verdict(task, text)[0], text)

    def test_malformed_tasks(self):
        row = self.rows()[0]
        inner = json.loads(row["input"])
        for broken in (dict(inner, task="u99"), dict(inner, variant="other"), dict(inner, extra=1),
                       dict(inner, run=dict(inner["run"], steps=[])), dict(inner, run=dict(inner["run"], bogus=1))):
            with self.assertRaises(ValueError):
                verifier.verdict(verifier.normalize_task(broken), row["reference"])
        u02 = next(r for r in self.rows() if r["source"] == "u02")
        with self.assertRaises(ValueError):
            verifier.verdict(verifier.normalize_task(dict(json.loads(u02["input"]), variant="demo")), u02["reference"])

    def test_step_kind_must_match_the_script(self):
        row = next(r for r in self.rows() if r["source"] == "u08")
        inner = json.loads(row["input"])
        inner["run"]["steps"][0]["kind"] = "say"
        with self.assertRaises(ValueError):
            verifier.verdict(verifier.normalize_task(inner), row["reference"])

    def test_cli(self):
        row = self.rows()[0]
        with tempfile.TemporaryDirectory() as d:
            task, comp = os.path.join(d, "t.json"), os.path.join(d, "c.txt")
            with open(task, "w") as handle:
                json.dump({"id": row["id"], "input": row["input"], "reference": row["reference"]}, handle)
            with open(comp, "w") as handle:
                handle.write(row["reference"])
            script = os.path.join(USABILITY, "genesis", "verifier.py")
            done = subprocess.run([sys.executable, "-I", script, task, os.path.join(d, "missing"), comp],
                                  capture_output=True, text=True, timeout=60)
            self.assertEqual(done.returncode, 0, done.stderr)
            self.assertEqual(json.loads(done.stdout), {"pass": True, "score": 1.0, "reward": 1})
            self.assertEqual(subprocess.run([sys.executable, "-I", script, task], capture_output=True).returncode, 2)
            with open(task, "w") as handle:
                handle.write("[]")
            self.assertEqual(subprocess.run([sys.executable, "-I", script, task, comp], capture_output=True).returncode, 2)


if __name__ == "__main__":
    unittest.main()
