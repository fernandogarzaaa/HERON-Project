"""The `eve` driver's slow-turn path, with a fake `eve` on PATH and a fake Heron that answers late.

EVE's HTTP backend gives up on a reply after 30 seconds; the runner must then wait for the
adapter's record instead of reading nothing. The fake `eve` gives up after half a second.
"""
import os
import stat
import tempfile
import time
import types
import unittest
from unittest import mock

import run_core_tasks as rct
from tests.test_adapter import TOKEN, FakeHeron

EVAL = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CONFIG = os.path.join(EVAL, "eve", "heron-core.yaml")
TASK = {"id": "t01", "approve": [{"tool": "write_file"}]}

FAKE_EVE = """#!/usr/bin/env python3
import sys, json, urllib.request
argv = sys.argv[1:]
mode = %(mode)r
if mode == "usage":
    sys.stderr.write("bad flag\\n")
    sys.exit(2)
if mode == "silent":
    sys.exit(0)
target = argv[1]
escaped = json.dumps(argv[argv.index("--goal") + 1])[1:-1]  # EVE JSON-escapes {{message}}
body = argv[argv.index("--body") + 1].replace("{{message}}", escaped)
request = urllib.request.Request(target, data=body.encode(), headers={"content-type": "application/json"}, method="POST")
try:
    urllib.request.urlopen(request, timeout=0.5).read()
except Exception:
    pass  # gave up waiting, as EVE does after 30 s
sys.exit(1)  # EVE's exit status reflects its findings, not failure
"""


class EveDriver(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.bin = os.path.join(self._tmp.name, "bin")
        os.makedirs(self.bin)
        self.run_dir = os.path.join(self._tmp.name, "run")
        os.makedirs(self.run_dir)

    def install_eve(self, mode):
        path = os.path.join(self.bin, "eve")
        with open(path, "w") as handle:
            handle.write(FAKE_EVE % {"mode": mode})
        os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR)

    def drive(self, heron):
        core = types.SimpleNamespace(base=heron.base, token=TOKEN)
        path = self.bin + os.pathsep + os.environ.get("PATH", "")
        with mock.patch.dict(os.environ, {"PATH": path}):
            return rct.drive_with_eve(TASK, core, self.run_dir, "please save it", CONFIG)

    def test_the_runner_waits_for_a_turn_that_outlasts_eves_patience(self):
        self.install_eve("post-and-give-up")
        heron = FakeHeron(delay=2.0)
        self.addCleanup(heron.close)
        started = time.monotonic()
        record = self.drive(heron)
        self.assertGreaterEqual(time.monotonic() - started, 2.0)
        self.assertEqual(record["reply"], "Hello world")
        self.assertEqual(record["tokens"], 42)
        self.assertEqual(record["message"], "please save it")
        self.assertEqual(heron.approvals, [{"id": "ap-1", "decision": "once"}])

    def test_an_eve_that_never_sends_a_turn_is_an_error_not_a_wait(self):
        self.install_eve("silent")
        heron = FakeHeron()
        self.addCleanup(heron.close)
        started = time.monotonic()
        with self.assertRaises(rct.RunError) as ctx:
            self.drive(heron)
        self.assertIn("without sending a turn", str(ctx.exception))
        self.assertLess(time.monotonic() - started, 30)

    def test_an_eve_that_rejects_its_arguments_is_an_error(self):
        self.install_eve("usage")
        heron = FakeHeron()
        self.addCleanup(heron.close)
        with self.assertRaises(rct.RunError) as ctx:
            self.drive(heron)
        self.assertIn("rejected its arguments", str(ctx.exception))

    def test_a_prompt_with_shell_and_json_characters_arrives_unchanged(self):
        self.install_eve("post-and-give-up")
        heron = FakeHeron()
        self.addCleanup(heron.close)
        core = types.SimpleNamespace(base=heron.base, token=TOKEN)
        prompt = 'Run `python3 -m unittest discover -s proj` and say "how many" ($HOME \\b3\\b)'
        path = self.bin + os.pathsep + os.environ.get("PATH", "")
        with mock.patch.dict(os.environ, {"PATH": path}):
            record = rct.drive_with_eve({"id": "t01", "approve": []}, core, self.run_dir, prompt, CONFIG)
        self.assertEqual(record["message"], prompt)


if __name__ == "__main__":
    unittest.main()
