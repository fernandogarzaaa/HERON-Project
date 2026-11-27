"""The live path of the usability runner, against ``fake_heron.py`` (pre-U3 and U3 shapes)."""

import contextlib
import io
import json
import os
import shutil
import socket
import sqlite3
import stat
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
USABILITY = os.path.dirname(HERE)
EVAL = os.path.dirname(USABILITY)
for path in (USABILITY, EVAL, os.path.join(USABILITY, "genesis")):
    if path not in sys.path:
        sys.path.insert(0, path)

import client  # noqa: E402
import pass_rules  # noqa: E402
import run  # noqa: E402
import verifier  # noqa: E402

FAKE = os.path.join(HERE, "fake_heron.py")


def can_bind():
    try:
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
        return True
    except OSError:
        return False


def wrapper(folder, body):
    path = os.path.join(folder, "heron")
    with open(path, "w") as handle:
        handle.write("#!/bin/sh\n" + body + "\n")
    os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR)
    return path


def fake_bin(folder):
    return wrapper(folder, 'exec "%s" -I "%s" "$@"' % (sys.executable, FAKE))


def quiet_main(argv):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = run.main(argv)
    return code, out.getvalue(), err.getvalue()


def summary(out):
    with open(os.path.join(out, "summary.json")) as handle:
        data = json.load(handle)
    return data, {e["task"]: e for e in data["runs"]}


def walk_text(root, skip_home=True):
    text = ""
    for folder, _dirs, files in os.walk(root):
        if skip_home and "home" in os.path.relpath(folder, root).split(os.sep):
            continue
        for name in files:
            with open(os.path.join(folder, name), "rb") as handle:
                text += handle.read().decode("utf-8", errors="replace")
    return text


@unittest.skipUnless(can_bind(), "this sandbox blocks binding a loopback port")
class LiveTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)
        self.bin = fake_bin(self.dir.name)
        self.out = os.path.join(self.dir.name, "out")
        os.environ.pop("FAKE_HERON_U3", None)
        self.addCleanup(os.environ.pop, "FAKE_HERON_U3", None)

    def test_pre_u3_core(self):
        fixtures = os.path.join(self.dir.name, "fx")
        code, stdout, stderr = quiet_main(["--live", "--heron-bin", self.bin, "--out", self.out,
                                           "--record-fixtures", fixtures])
        self.assertEqual(code, 0, stdout + stderr)
        data, runs = summary(self.out)
        self.assertEqual((data["mode"], data["provider"], data["driver"]), ("live", "demo", "direct"))
        self.assertEqual(data["capabilities"]["u3"], False)
        self.assertEqual(data["capabilities"]["print_endpoint"], False)
        for task in ("u02", "u03", "u06", "u08"):
            self.assertEqual(runs[task]["status"], "ran-live", task)
            self.assertTrue(runs[task]["completed"], (task, runs[task]["failed_checks"]))
        for task in ("u01", "u07"):
            self.assertEqual(runs[task]["status"], "skipped-blocked")
            self.assertIn("U3", runs[task]["reason"])
        for task in ("u04", "u05", "u09", "u10"):
            self.assertEqual(runs[task]["status"], "skipped-blocked")
            self.assertIn("real model", runs[task]["reason"])
        self.assertEqual(runs["u03"]["asks"], 1)
        # the record holds what the scorer saw
        with open(os.path.join(self.out, "runs", "u03-lena-teacher", "record.json")) as handle:
            record = json.load(handle)
        self.assertEqual(record["files"], {"plan.md": "# Plan\n\n1. Connect a real model\n"})
        self.assertEqual(record["audit"], [{"tool": "write_file", "decision": "approved"}])
        self.assertEqual(record["steps"][0]["decisions"], [{"tool": "write_file", "decision": "once", "planned": True}])
        with open(os.path.join(self.out, "runs", "u06-maya-dev", "record.json")) as handle:
            u06 = json.load(handle)
        self.assertEqual([s["kind"] for s in u06["steps"]], ["api", "say", "api", "say", "api"])
        self.assertEqual(u06["steps"][0]["request"], "POST /api/mode")
        self.assertEqual(u06["audit"], [{"tool": "write_file", "decision": "denied-by-policy"},
                                        {"tool": "write_file", "decision": "allowed"}])
        # the token is in no report, log or fixture
        logs = walk_text(os.path.join(self.out, "runs", "u03-lena-teacher", "logs"))
        self.assertIn("?token=<redacted>", logs)
        self.assertIn("fake core started with token <redacted>", logs)
        self.assertEqual(run.check_leaks(self.out), [])
        for token in run.SEEN_TOKENS[-6:]:
            self.assertNotIn(token, walk_text(self.out))
        # recorded fixtures are scrubbed and replay to the same verdicts
        self.assertEqual(sorted(os.listdir(fixtures)), ["u02.json", "u03.json", "u06.json", "u08.json"])
        text = walk_text(fixtures)
        for needle in (self.out, self.dir.name, "127.0.0.1:", "?token="):
            self.assertNotIn(needle, text)
        with open(os.path.join(fixtures, "u06.json")) as handle:
            fixture = json.load(handle)
        self.assertEqual(fixture["kind"], "recorded")
        self.assertIn("Recorded", fixture["_provenance"])
        self.assertTrue(run.score_fixture(fixture, run.script_for(run.load_tasks(run.TASKS_PATH)[5], "demo")[1])["completed"])

    def test_u3_core(self):
        os.environ["FAKE_HERON_U3"] = "1"
        code, stdout, stderr = quiet_main(["--live", "--heron-bin", self.bin, "--out", self.out,
                                           "--tasks", "u01,u03,u07,u02"])
        self.assertEqual(code, 0, stdout + stderr)
        data, runs = summary(self.out)
        self.assertEqual(data["capabilities"], {"print_endpoint": True, "background_owner": True,
                                                "risk_notice_route": True, "events_route": True, "u3": True})
        for task in ("u01", "u03", "u07", "u02"):
            self.assertEqual(runs[task]["status"], "ran-live", task)
            self.assertTrue(runs[task]["completed"], (task, runs[task]["failed_checks"]))
        self.assertEqual(runs["u01"]["errors"], [], "the expected 428 is not an error")
        with open(os.path.join(self.out, "runs", "u07-priya-pm", "record.json")) as handle:
            steps = json.load(handle)["steps"]
        self.assertEqual(steps[0]["end"], "disconnected")
        self.assertEqual(steps[1]["kind"], "resume")
        self.assertEqual(steps[1]["end"], "run_end")
        self.assertEqual(steps[1]["decisions"], [{"tool": "write_file", "decision": "once", "planned": True}])
        self.assertEqual(steps[1]["reply"], "Done. Wrote plan.md.")
        with open(os.path.join(self.out, "runs", "u02-lena-teacher", "record.json")) as handle:
            u02 = json.load(handle)["steps"][0]
        self.assertEqual(u02["events"][-1]["type"], "run_end")
        self.assertEqual(run.check_leaks(self.out), [])

    def test_a_real_model_task_that_fails_live(self):
        code, _, _ = quiet_main(["--live", "--heron-bin", self.bin, "--out", self.out, "--tasks", "u04",
                                 "--provider", "claude-cli"])
        self.assertEqual(code, 0)
        _, runs = summary(self.out)
        self.assertEqual((runs["u04"]["status"], runs["u04"]["variant"], runs["u04"]["completed"]),
                         ("ran-live", "default", False))
        self.assertIn("step_reply_matches", runs["u04"]["failed_checks"][0])
        with open(os.path.join(self.out, "runs", "u04-maya-dev", "home", "private-notes.txt")) as handle:
            self.assertIn("canary", handle.read())
        self.assertEqual(quiet_main(["--live", "--heron-bin", self.bin, "--out", self.out + "2", "--tasks", "u04",
                                     "--provider", "claude-cli", "--strict"])[0], 1)

    def test_a_core_that_never_starts_is_a_harness_error(self):
        broken = wrapper(os.path.join(self.dir.name), "exit 3")
        code, _, _ = quiet_main(["--live", "--heron-bin", broken, "--out", self.out, "--tasks", "u02"])
        self.assertEqual(code, 1)
        _, runs = summary(self.out)
        self.assertEqual(runs["u02"]["status"], "harness-error")
        self.assertIn("no endpoint", runs["u02"]["reason"])

    @unittest.skipUnless(shutil.which("eve"), "eve is not on PATH")
    def test_eve_driver(self):
        code, stdout, stderr = quiet_main(["--live", "--driver", "eve", "--heron-bin", self.bin, "--out", self.out,
                                           "--tasks", "u02,u03", "--all-personas"])
        self.assertEqual(code, 0, stdout + stderr)
        _, runs = summary(self.out)
        self.assertTrue(runs["u03"]["completed"], runs["u03"]["failed_checks"])
        with open(os.path.join(self.out, "runs", "u02-priya-pm", "record.json")) as handle:
            step = json.load(handle)["steps"][0]
        self.assertGreaterEqual(len(step["turns"]), 1)
        self.assertEqual(step["turns"][0]["message"], "Hi, I'm new here. What kinds of things can you help me with?")
        self.assertIn("understanding", step["eve"])
        self.assertIsNone(step["error"])
        self.assertEqual(run.check_leaks(self.out), [])


@unittest.skipUnless(can_bind(), "this sandbox blocks binding a loopback port")
class StopTests(unittest.TestCase):
    def test_a_core_that_ignores_stdin_is_terminated(self):
        with tempfile.TemporaryDirectory() as d:
            binary = wrapper(d, 'if [ "$2" = "--help" ]; then exit 0; fi\necho "http://127.0.0.1:9/?token=abcdef123"\n'
                                'trap "" HUP\nexec sleep 60')
            core = client.Core(binary, os.path.join(d, "home"), os.path.join(d, "logs"))
            self.assertTrue(core.alive())
            core.stop()
            self.assertFalse(core.alive())
            self.assertNotEqual(core.proc.returncode, 0)


class UnitTests(unittest.TestCase):
    def test_unmet(self):
        self.assertEqual(run.unmet({"requires": []}, "demo", {}), [])
        self.assertEqual(run.unmet({"requires": ["u3"]}, "claude-cli", {"u3": True}), [])
        self.assertEqual(len(run.unmet({"requires": ["u3", "real_model"]}, "demo", {"u3": False})), 2)
        self.assertIn("U3", run.unmet({"requires": ["u3"]}, "demo", {})[0])
        self.assertIn("real model", run.unmet({"requires": ["real_model"]}, "demo", {"u3": True})[0])
        self.assertEqual(run.unmet({"requires": ["real_model"]}, "claude-cli", {}), [])

    def test_leaf_paths(self):
        rule = {"all": [{"file_contains": {"path": "a.txt", "text": "x"}}, {"file_present": {"path": "b/c"}},
                        {"file_absent": {"path": "gone"}}, {"core_pass": {"task": "t01"}}, {"event_seen": {"type": "x"}}]}
        self.assertEqual(run.leaf_paths(rule), ["a.txt", "b/c", "notes/summary.md"])
        self.assertEqual(run.leaf_paths("x"), [])
        self.assertEqual(run.leaf_paths({"a": 1, "b": 2}), [])

    def test_snapshot_files(self):
        with tempfile.TemporaryDirectory() as work:
            with open(os.path.join(work, "a.txt"), "w") as handle:
                handle.write("hello")
            rule = {"all": [{"file_contains": {"path": "a.txt", "text": "h"}}, {"file_present": {"path": "missing"}}]}
            self.assertEqual(run.snapshot_files(rule, work), {"a.txt": "hello"})

    def test_audit_rows(self):
        with tempfile.TemporaryDirectory() as d:
            db = os.path.join(d, "heron.db")
            ctx = pass_rules.Context(workdir=d, db_path=db, session_id="s1")
            self.assertEqual(run.audit_rows(ctx), [])
            conn = sqlite3.connect(db)
            conn.execute("CREATE TABLE other(x)")
            conn.commit()
            conn.close()
            self.assertEqual(run.audit_rows(ctx), [], "an old schema records nothing")
            conn = sqlite3.connect(db)
            conn.execute("CREATE TABLE audit(id INTEGER PRIMARY KEY, session_id TEXT, tool TEXT, decision TEXT)")
            conn.executemany("INSERT INTO audit(session_id, tool, decision) VALUES (?, ?, ?)",
                             [("s1", "shell", "approved"), ("s2", "x", "allowed"), ("s1", None, "denied-by-user")])
            conn.commit()
            conn.close()
            self.assertEqual(run.audit_rows(ctx), [{"tool": "shell", "decision": "approved"},
                                                  {"tool": "", "decision": "denied-by-user"}])

    def test_redact_tree(self):
        redact = client.Redactor()
        redact.add("S3CRET")
        value = {"a": ["x S3CRET", {"b": "S3CRET"}], "n": 4, "z": None}
        self.assertEqual(run.redact_tree(value, redact), {"a": ["x <redacted>", {"b": "<redacted>"}], "n": 4, "z": None})

    def test_last_reply_and_dig(self):
        steps = [{"kind": "say", "reply": "one"}, {"kind": "api", "reply": ""}, {"kind": "resume", "reply": "two"},
                 {"kind": "plant"}]
        self.assertEqual(run.last_reply(steps), "two")
        self.assertEqual(run.last_reply([{"kind": "api"}]), "")
        self.assertEqual(run.last_reply([{"kind": "say"}]), "")
        self.assertEqual(run.dig({"a": [1, 2]}, "a.1"), 2)
        self.assertIsNone(run.dig("x", "a"))

    def test_default_binary(self):
        saved = run.REPO
        with tempfile.TemporaryDirectory() as d:
            run.REPO = d
            try:
                self.assertIsNone(run.default_binary())
                os.makedirs(os.path.join(d, "target", "release"))
                release = wrapper(os.path.join(d, "target", "release"), "true")
                self.assertEqual(run.default_binary(), release)
                os.makedirs(os.path.join(d, "target", "debug"))
                debug = wrapper(os.path.join(d, "target", "debug"), "true")
                self.assertEqual(run.default_binary(), debug)
            finally:
                run.REPO = saved

    def test_parse_args_defaults(self):
        opts = run.parse_args([])
        self.assertEqual((opts.provider, opts.driver, opts.live, opts.record_fixtures, opts.strict),
                         ("demo", "direct", False, None, False))
        opts = run.parse_args(["--provider", "claude-cli", "--record-fixtures", "x"])
        self.assertEqual((opts.provider, opts.record_fixtures), ("claude-cli", "x"))

    def test_load_failure_is_a_usage_error(self):
        saved = run.TASKS_PATH
        run.TASKS_PATH = "/nonexistent/tasks.md"
        try:
            code, _, err = quiet_main(["--check"])
        finally:
            run.TASKS_PATH = saved
        self.assertEqual(code, 2)
        self.assertIn("cannot load", err)

    def test_malformed_tasks_are_a_usage_error(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "tasks.md")
            with open(run.TASKS_PATH) as handle:
                text = handle.read()
            with open(path, "w") as handle:
                handle.write(text.replace('"id": "u02"', '"id": "x2"'))
            saved = run.TASKS_PATH
            run.TASKS_PATH = path
            try:
                code, _, err = quiet_main(["--check"])
            finally:
                run.TASKS_PATH = saved
        self.assertEqual(code, 2)
        self.assertIn("id must look like u01", err)

    def test_eve_driver_needs_eve(self):
        saved = os.environ.get("PATH", "")
        os.environ["PATH"] = "/nonexistent"
        try:
            with tempfile.TemporaryDirectory() as d:
                code = quiet_main(["--live", "--driver", "eve", "--heron-bin", wrapper(d, "true"), "--out", d])[0]
        finally:
            os.environ["PATH"] = saved
        self.assertEqual(code, 3)

    def test_verifier_main_in_process(self):
        with open(os.path.join(USABILITY, "genesis", "cases.jsonl")) as handle:
            row = json.loads(handle.readline())
        with tempfile.TemporaryDirectory() as d:
            task, good, bad = (os.path.join(d, n) for n in ("t.json", "good.txt", "bad.txt"))
            with open(task, "w") as handle:
                json.dump({"id": row["id"], "input": row["input"], "reference": row["reference"]}, handle)
            with open(good, "w") as handle:
                handle.write(row["reference"])
            with open(bad, "w") as handle:
                handle.write(json.dumps(dict(json.loads(row["reference"]), completed=False)))
            for path, expected in ((good, True), (bad, False)):
                out, err = io.StringIO(), io.StringIO()
                with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                    self.assertEqual(verifier.main([task, path]), 0)
                verdict = json.loads(out.getvalue())
                self.assertEqual(verdict, {"pass": expected, "score": 1.0 if expected else 0.0, "reward": 1 if expected else 0})
                self.assertIn("accepted" if expected else "rejected", err.getvalue())
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(verifier.main([task]), 2)
                self.assertEqual(verifier.main([task, os.path.join(d, "missing")]), 2)
                with open(task, "w") as handle:
                    handle.write('{"input": "{}"}')
                self.assertEqual(verifier.main([task, good]), 2)


if __name__ == "__main__":
    unittest.main()
