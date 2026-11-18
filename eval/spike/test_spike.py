"""Tests for run-spike.sh and make_pairs.py, with a fake runner and a fake `claude`; nothing here spends quota.

    python3 -I -m unittest discover -s eval/spike -p 'test_*.py' -t eval/spike
"""

import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
EVAL = os.path.dirname(HERE)
REPO = os.path.dirname(EVAL)
sys.path.insert(0, HERE)
sys.path.insert(0, EVAL)

import make_pairs  # noqa: E402
from check_gate import compute_totals  # noqa: E402

BASH = shutil.which("bash")
GATE = ["t%02d" % n for n in range(1, 11)]

FAKE_RUNNER = r'''#!/usr/bin/env python3
import json, os, sys
sys.path.insert(0, %(eval)r)
from check_gate import compute_totals
argv = sys.argv[1:]
opt = {a: argv[i + 1] for i, a in enumerate(argv) if a.startswith("--") and i + 1 < len(argv)}
with open(os.environ["FAKE_LOG"], "a") as log:
    log.write(" ".join(argv) + "\n")
topology = opt["--topology"]
if os.environ.get("FAKE_FAIL") == topology:
    sys.exit(1)
passes = int(os.environ["FAKE_PASSES_" + topology.upper()])
tasks = []
left = passes
for task_id in opt["--tasks"].split(","):
    runs = []
    for n in range(1, int(opt["--runs"]) + 1):
        ok = left > 0
        left -= 1 if ok else 0
        runs.append({"run": n, "pass": ok, "failed_rules": [], "reply": "%%s %%s run %%d" %% (topology, task_id, n),
                     "tokens": int(os.environ["FAKE_TOKENS_" + topology.upper()]), "ttft_ms": 100, "asks": 0,
                     "error": None})
    tasks.append({"id": task_id, "counts_toward_gate": True, "runs": runs})
summary = {"schema": 1, "heron_commit": os.environ.get("FAKE_COMMIT", "abc"), "provider": "claude-cli",
           "cli_version": "1.2.3", "topology": topology, "mode": "ask", "driver": "eve", "build": "debug",
           "runs_per_task": int(opt["--runs"]), "tasks": tasks, "totals": compute_totals(tasks)}
os.makedirs(opt["--out"], exist_ok=True)
with open(os.path.join(opt["--out"], "summary.json"), "w") as handle:
    json.dump(summary, handle)
'''


def make_summary(topology, passes=40, tokens=1000, commit="abc"):
    tasks = []
    left = passes
    for task_id in GATE:
        runs = []
        for n in range(1, 6):
            ok = left > 0
            left -= 1 if ok else 0
            runs.append({"run": n, "pass": ok, "failed_rules": [], "reply": "%s %s run %d" % (topology, task_id, n),
                         "tokens": tokens, "ttft_ms": 100, "asks": 0, "error": None})
        tasks.append({"id": task_id, "counts_toward_gate": True, "runs": runs})
    return {"schema": 1, "heron_commit": commit, "provider": "claude-cli", "cli_version": "1.2.3",
            "topology": topology, "mode": "ask", "driver": "eve", "build": "debug", "runs_per_task": 5,
            "tasks": tasks, "totals": compute_totals(tasks)}


def read(path):
    with open(path, encoding="utf-8") as handle:
        return handle.read()


class PairsTests(unittest.TestCase):
    def setUp(self):
        self.env = make_summary("envelope")
        self.hyb = make_summary("hybrid")
        self.prompts = {t: "do %s" % t for t in GATE}

    def test_one_pair_per_gate_task_in_a_random_order(self):
        pairs, key = make_pairs.build_pairs(self.env, self.hyb, self.prompts, "seed")
        self.assertEqual(sorted(p["task"] for p in pairs), GATE)
        self.assertNotEqual([p["task"] for p in pairs], GATE)
        self.assertEqual([p["n"] for p in pairs], list(range(1, 11)))
        self.assertEqual([k["n"] for k in key], list(range(1, 11)))

    def test_key_matches_what_is_shown(self):
        pairs, key = make_pairs.build_pairs(self.env, self.hyb, self.prompts, "seed")
        for pair, entry in zip(pairs, key):
            self.assertEqual(pair["task"], entry["task"])
            self.assertEqual({entry["A"], entry["B"]}, {"envelope", "hybrid"})
            self.assertTrue(pair["A"].startswith(entry["A"] + " " + pair["task"]))
            self.assertTrue(pair["B"].startswith(entry["B"] + " " + pair["task"]))
            self.assertTrue(pair["A"].endswith("run %d" % entry[entry["A"] + "_run"]))

    def test_both_arms_appear_in_both_positions(self):
        _, key = make_pairs.build_pairs(self.env, self.hyb, self.prompts, "seed")
        self.assertEqual({k["A"] for k in key}, {"envelope", "hybrid"})

    def test_same_input_gives_same_sheet_and_other_seed_does_not(self):
        first = make_pairs.build_pairs(self.env, self.hyb, self.prompts, "seed")
        self.assertEqual(first, make_pairs.build_pairs(self.env, self.hyb, self.prompts, "seed"))
        self.assertNotEqual(first, make_pairs.build_pairs(self.env, self.hyb, self.prompts, "other"))

    def test_errored_runs_are_never_shown(self):
        for task in self.env["tasks"]:
            for run in task["runs"][1:]:
                run["error"] = "RunError: boom"
                run["reply"] = "BROKEN"
        pairs, key = make_pairs.build_pairs(self.env, self.hyb, self.prompts, "seed")
        self.assertFalse(any("BROKEN" in p["A"] + p["B"] for p in pairs))
        self.assertEqual({k["envelope_run"] for k in key}, {1})

    def test_an_arm_without_a_usable_run_is_marked(self):
        for run in self.hyb["tasks"][0]["runs"]:
            run["error"] = "x"
        pairs, key = make_pairs.build_pairs(self.env, self.hyb, self.prompts, "seed")
        bad = [k for k in key if k["task"] == "t01"][0]
        self.assertIsNone(bad["hybrid_run"])
        shown = [p for p in pairs if p["task"] == "t01"][0]
        self.assertIn(make_pairs.NO_REPLY, (shown["A"], shown["B"]))

    def test_the_sheet_names_no_topology(self):
        pairs, _ = make_pairs.build_pairs(self.env, self.hyb, self.prompts, "seed")
        text = make_pairs.render_pairs(pairs).replace("envelope vs hybrid", "")
        self.assertEqual(text.count("### Answer"), 20)
        for pair in pairs:  # the fake replies name their arm; the sheet must only show them verbatim
            self.assertIn(pair["A"], text)
        self.assertNotIn("Chosen default", text)
        self.assertIn("Better: ", text)

    def test_fences_in_a_reply_cannot_break_the_sheet(self):
        pairs = [{"n": 1, "task": "t01", "prompt": "p", "A": "a ```x``` b", "B": "b"}]
        self.assertEqual(make_pairs.render_pairs(pairs).count("```"), 4)


@unittest.skipUnless(BASH, "bash is not available")
class RunSpikeTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.tmp = tmp.name
        self.out = os.path.join(self.tmp, "out")
        os.makedirs(self.out)
        self.bin = os.path.join(self.tmp, "bin")
        os.makedirs(self.bin)
        self.runner = self.script("runner", FAKE_RUNNER % {"eval": EVAL})
        self.log = os.path.join(self.tmp, "calls.log")
        self.head = subprocess.run(["git", "-C", REPO, "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
        self.script("bin/claude", "#!/bin/sh\nexit 0\n")

    def script(self, name, body):
        path = os.path.join(self.tmp, name)
        with open(path, "w") as handle:
            handle.write(body)
        os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR)
        return path

    def spike(self, *args, path=None, **env):
        full = dict(os.environ)
        full.update({"SPIKE_RUNNER": self.runner, "SPIKE_OUT_DIR": self.out, "FAKE_LOG": self.log,
                     "FAKE_PASSES_ENVELOPE": "40", "FAKE_PASSES_HYBRID": "44", "FAKE_TOKENS_ENVELOPE": "1000",
                     "FAKE_TOKENS_HYBRID": "1000", "FAKE_COMMIT": self.head,
                     "PATH": path if path is not None else self.bin + os.pathsep + os.environ["PATH"],
                     "TMPDIR": self.tmp})
        full.update(env)
        # a runner of "bash <script>": the fake is a python file, so wrap it
        wrapper = self.script("wrap.sh", "#!/bin/sh\nexec python3 %s \"$@\"\n" % self.runner)
        full["SPIKE_RUNNER"] = wrapper
        return subprocess.run([BASH, os.path.join(HERE, "run-spike.sh"), *args], capture_output=True, text=True,
                              env=full, timeout=120)

    def calls(self):
        return read(self.log).splitlines()

    def test_runs_both_arms_with_the_spec_flags_and_writes_the_reports(self):
        proc = self.spike()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        calls = self.calls()
        self.assertEqual(len(calls), 2)
        for call, arm in zip(calls, ("envelope", "hybrid")):
            self.assertIn("--provider claude-cli --build debug --runs 5 --tasks %s --topology %s" % (",".join(GATE), arm), call)
            self.assertIn("--out %s" % os.path.join(self.out, arm), call)
            self.assertIn("--artifacts", call)
            self.assertNotIn("--resume", call)
        report = read(os.path.join(self.out, "topology.md"))
        self.assertTrue(report.startswith("Chosen default: topology=hybrid"), report)
        sheet = read(os.path.join(self.out, "pairs.md"))
        self.assertEqual(sheet.count("## Pair "), 10)
        key = json.loads(read(os.path.join(self.out, "pairs-key.json")))
        self.assertEqual(len(key["pairs"]), 10)

    def test_the_written_report_passes_the_completion_check(self):
        self.assertEqual(self.spike().returncode, 0)
        proc = subprocess.run([sys.executable, "-I", os.path.join(EVAL, "check_gate.py"), "--spike",
                               os.path.join(self.out, "envelope", "summary.json"),
                               os.path.join(self.out, "hybrid", "summary.json"),
                               "--report", os.path.join(self.out, "topology.md")], capture_output=True, text=True)
        self.assertEqual(proc.returncode, 0, proc.stderr)

    def test_a_small_lead_keeps_the_envelope(self):
        self.assertEqual(self.spike(FAKE_PASSES_HYBRID="43").returncode, 0)
        self.assertTrue(read(os.path.join(self.out, "topology.md")).startswith("Chosen default: topology=envelope"))

    def test_resume_passes_the_flag_through(self):
        self.assertEqual(self.spike().returncode, 0)
        self.assertEqual(self.spike("--resume").returncode, 0)
        self.assertEqual(sum("--resume" in c for c in self.calls()), 2)

    def test_resume_refuses_a_summary_from_another_commit(self):
        self.assertEqual(self.spike(FAKE_COMMIT="old").returncode, 0)
        before = len(self.calls())
        proc = self.spike("--resume")
        self.assertEqual(proc.returncode, 1)
        self.assertIn("old", proc.stderr)
        self.assertEqual(len(self.calls()), before, "nothing may run when the commit differs")

    def test_a_fresh_run_keeps_the_previous_summary(self):
        self.assertEqual(self.spike().returncode, 0)
        proc = self.spike()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertTrue(os.path.isfile(os.path.join(self.out, "envelope", "summary.previous.json")))

    def test_a_failing_arm_stops_the_spike_and_writes_no_report(self):
        proc = self.spike(FAKE_FAIL="hybrid")
        self.assertEqual(proc.returncode, 1)
        self.assertIn("--resume", proc.stderr)
        self.assertFalse(os.path.exists(os.path.join(self.out, "topology.md")))
        self.assertFalse(os.path.exists(os.path.join(self.out, "pairs.md")))

    def test_missing_claude_exits_3_and_runs_nothing(self):
        bare = os.path.join(self.tmp, "bare")
        os.makedirs(bare)
        for tool in ("bash", "sh", "env", "dirname", "git", "mktemp", "python3", "rm", "mkdir", "mv", "cat"):
            found = shutil.which(tool)
            if found:
                os.symlink(found, os.path.join(bare, tool))
        proc = self.spike(path=bare)
        self.assertEqual(proc.returncode, 3, proc.stderr)
        self.assertFalse(os.path.exists(self.log))

    def test_a_stray_argument_is_a_usage_error(self):
        self.assertEqual(self.spike("--fast").returncode, 2)
        self.assertFalse(os.path.exists(self.log))


if __name__ == "__main__":
    unittest.main()
