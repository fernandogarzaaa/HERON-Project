import json
import os
import tempfile
import unittest

import check_gate
from check_gate import compute_totals, decide_topology, judge_gate, schema_problems

GATE_IDS = ["t%02d" % n for n in range(1, 11)]


def run(n, passed=True, tokens=1000, ttft=900, error=None, asks=0):
    return {"run": n, "pass": passed, "failed_rules": [] if passed else ["x"], "reply": "r", "tokens": tokens,
            "ttft_ms": ttft, "asks": asks, "error": error}


def summary(passes, runs_per_task=3, provider="claude-cli", topology="envelope", build="release", driver="eve",
            mode="ask", commit="abc123", tokens=1000, probe=False):
    """``passes`` maps task id -> number of passing runs; every task has ``runs_per_task`` runs."""
    tasks = []
    for tid in GATE_IDS:
        n_pass = passes.get(tid, runs_per_task) if isinstance(passes, dict) else passes
        runs = [run(i + 1, i < n_pass, tokens=tokens) for i in range(runs_per_task)]
        tasks.append({"id": tid, "counts_toward_gate": True, "runs": runs})
    if probe:
        tasks.append({"id": "p1", "counts_toward_gate": False, "runs": [run(1)]})
    return {"schema": 1, "heron_commit": commit, "provider": provider, "cli_version": "2.1.3", "topology": topology,
            "mode": mode, "driver": driver, "build": build, "runs_per_task": runs_per_task, "tasks": tasks,
            "totals": compute_totals(tasks)}


class FileTestCase(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)

    def write(self, name, data):
        path = os.path.join(self._tmp.name, name)
        with open(path, "w") as handle:
            handle.write(data if isinstance(data, str) else json.dumps(data))
        return path

    def gate(self, data):
        return check_gate.main([self.write("s.json", data)])


class G7Rule(FileTestCase):
    def two_of_three(self, count):
        return {tid: 2 for tid in GATE_IDS[:count]}

    def test_26_of_30_passes(self):
        passes, _ = judge_gate(summary(self.two_of_three(4)))
        self.assertTrue(passes)
        self.assertEqual(self.gate(summary(self.two_of_three(4))), 0)

    def test_25_of_30_fails(self):
        passes, messages = judge_gate(summary(self.two_of_three(5)))
        self.assertFalse(passes)
        self.assertTrue(any("25 of 30" in m for m in messages))
        self.assertEqual(self.gate(summary(self.two_of_three(5))), 1)

    def test_one_task_at_1_of_3_fails_even_with_26_passes(self):
        data = summary({"t01": 1})  # 9 * 3 + 1 = 28 of 30
        self.assertEqual(data["totals"]["gate_passes"], 28)
        passes, messages = judge_gate(data)
        self.assertFalse(passes)
        self.assertTrue(any("t01 passed 1 of 3" in m for m in messages))
        self.assertEqual(self.gate(data), 1)

    def test_a_task_at_exactly_2_of_3_is_enough(self):
        passes, _ = judge_gate(summary({"t05": 2}))
        self.assertTrue(passes)

    def test_all_runs_passing(self):
        self.assertEqual(self.gate(summary(3)), 0)

    def test_a_probe_does_not_count(self):
        data = summary(self.two_of_three(4), probe=True)
        self.assertEqual(data["totals"]["gate_runs"], 30)
        self.assertEqual(self.gate(data), 0)

    def test_an_errored_run_counts_as_a_failure(self):
        data = summary(3)
        data["tasks"][0]["runs"][0].update({"pass": False, "error": "boom"})
        data["totals"] = compute_totals(data["tasks"])
        passes, messages = judge_gate(data)
        self.assertTrue(passes)  # 29 of 30 and t01 still has 2 of 3
        data["tasks"][0]["runs"][1].update({"pass": False, "error": "boom"})
        data["totals"] = compute_totals(data["tasks"])
        self.assertFalse(judge_gate(data)[0])

    def test_a_missing_gate_task_fails(self):
        data = summary(3)
        data["tasks"] = data["tasks"][:9]
        data["totals"] = compute_totals(data["tasks"])
        passes, messages = judge_gate(data)
        self.assertFalse(passes)
        self.assertTrue(any("t01 to t10" in m for m in messages))

    def test_a_task_with_too_few_runs_fails(self):
        data = summary(3)
        data["tasks"][3]["runs"] = data["tasks"][3]["runs"][:2]
        data["totals"] = compute_totals(data["tasks"])
        passes, messages = judge_gate(data)
        self.assertFalse(passes)
        self.assertTrue(any("t04 has 2 runs" in m for m in messages))

    def test_required_fields_name_the_offender(self):
        cases = [("runs_per_task", summary(3, runs_per_task=5)), ("driver", summary(3, driver="direct")),
                 ("mode", summary(3, mode="auto")), ("build", summary(3, build="debug")),
                 ("provider", summary(3, provider="demo"))]
        for field, data in cases:
            with self.subTest(field=field):
                passes, messages = judge_gate(data)
                self.assertFalse(passes)
                self.assertTrue(messages[0].startswith(field), messages)
                self.assertEqual(self.gate(data), 1)

    def test_malformed_input_exits_2(self):
        self.assertEqual(check_gate.main([self.write("bad.json", "not json")]), 2)
        self.assertEqual(check_gate.main([self.write("list.json", "[]")]), 2)
        self.assertEqual(check_gate.main([os.path.join(self._tmp.name, "missing.json")]), 2)
        broken = summary(3)
        del broken["tasks"][0]["runs"][0]["tokens"]
        self.assertEqual(self.gate(broken), 2)


class SchemaOnly(FileTestCase):
    def schema_only(self, data):
        return check_gate.main(["--schema-only", self.write("s.json", data)])

    def test_a_valid_summary_passes_whatever_it_scored(self):
        self.assertEqual(self.schema_only(summary(0)), 0)
        self.assertEqual(self.schema_only(summary(3, provider="demo", driver="direct", build="debug", runs_per_task=1)), 0)

    def test_null_ttft_is_allowed(self):
        data = summary(3)
        data["tasks"][0]["runs"][0]["ttft_ms"] = None
        data["totals"] = compute_totals(data["tasks"])
        self.assertEqual(self.schema_only(data), 0)

    def test_a_schema_violation_fails(self):
        mutations = {
            "schema version": lambda d: d.update(schema=2),
            "missing heron_commit": lambda d: d.pop("heron_commit"),
            "bad topology": lambda d: d.update(topology="x"),
            "zero runs_per_task": lambda d: d.update(runs_per_task=0),
            "no tasks": lambda d: d.update(tasks=[]),
            "error of the wrong type": lambda d: d["tasks"][0]["runs"][0].update(error=5),
            "pass of the wrong type": lambda d: d["tasks"][0]["runs"][0].update({"pass": "yes"}),
            "negative asks": lambda d: d["tasks"][0]["runs"][0].update(asks=-1),
            "reply of the wrong type": lambda d: d["tasks"][0]["runs"][0].update(reply=None),
            "failed_rules of the wrong type": lambda d: d["tasks"][0]["runs"][0].update(failed_rules="x"),
            "wrong gate_passes": lambda d: d["totals"].update(gate_passes=99),
            "missing totals": lambda d: d.pop("totals"),
        }
        for name, mutate in mutations.items():
            data = summary(3)
            mutate(data)
            with self.subTest(name):
                self.assertTrue(schema_problems(data), "the mutation should break schema 1")
                self.assertEqual(self.schema_only(data), 1)

    def test_totals_must_match_the_runs(self):
        data = summary(3)
        data["totals"]["gate_passes"] = 29
        problems = schema_problems(data)
        self.assertTrue(any("totals.gate_passes" in p for p in problems))
        self.assertEqual(self.schema_only(data), 1)

    def test_a_passing_run_cannot_carry_an_error(self):
        data = summary(3)
        data["tasks"][0]["runs"][0]["error"] = "x"
        self.assertTrue(any("can't pass" in p for p in schema_problems(data)))

    def test_not_json_is_malformed(self):
        self.assertEqual(self.schema_only("{"), 2)
        self.assertEqual(check_gate.main(["--schema-only", os.path.join(self._tmp.name, "none.json")]), 2)


class SpikeRule(FileTestCase):
    def arms(self, hybrid_passes, hybrid_tokens, envelope_passes=45, envelope_tokens=1000):
        env = summary(self.spread(envelope_passes), runs_per_task=5, build="debug", topology="envelope",
                      tokens=envelope_tokens)
        hyb = summary(self.spread(hybrid_passes), runs_per_task=5, build="debug", topology="hybrid",
                      tokens=hybrid_tokens)
        return env, hyb

    @staticmethod
    def spread(total):
        """Spread ``total`` passes of 50 over ten tasks of five runs."""
        counts = {}
        for i, tid in enumerate(GATE_IDS):
            counts[tid] = min(5, max(0, total - 5 * i))
        return counts

    def test_a_lead_of_3_is_inconclusive(self):
        env, hyb = self.arms(48, 1000)
        self.assertEqual(hyb["totals"]["gate_passes"] - env["totals"]["gate_passes"], 3)
        self.assertEqual(decide_topology(env, hyb)[0], "envelope")

    def test_a_lead_of_4_with_equal_tokens_picks_hybrid(self):
        env, hyb = self.arms(49, 1000)
        topology, reason = decide_topology(env, hyb)
        self.assertEqual(topology, "hybrid")
        self.assertIn("lead", reason)

    def test_a_lead_of_4_with_6_percent_more_tokens_keeps_the_envelope(self):
        env, hyb = self.arms(49, 1060)
        self.assertEqual(decide_topology(env, hyb)[0], "envelope")

    def test_exactly_5_percent_more_tokens_is_allowed(self):
        env, hyb = self.arms(49, 1050)
        self.assertEqual(decide_topology(env, hyb)[0], "hybrid")

    def test_a_negative_lead_keeps_the_envelope(self):
        env, hyb = self.arms(40, 500)
        self.assertEqual(decide_topology(env, hyb)[0], "envelope")

    def test_report_round_trip_through_the_cli(self):
        env, hyb = self.arms(49, 1000)
        e, h = self.write("e.json", env), self.write("h.json", hyb)
        report = os.path.join(self._tmp.name, "topology.md")
        self.assertEqual(check_gate.main(["--spike", e, h, "--write-report", report]), 0)
        with open(report) as handle:
            text = handle.read()
        self.assertEqual(text.splitlines()[0], "Chosen default: topology=hybrid")
        self.assertIn("## Envelope arm", text)
        self.assertIn("## Hybrid arm", text)
        self.assertIn("## Reason", text)
        self.assertEqual(check_gate.main(["--spike", e, h, "--report", report]), 0)

    def test_a_report_that_disagrees_fails(self):
        env, hyb = self.arms(48, 1000)
        e, h = self.write("e.json", env), self.write("h.json", hyb)
        self.assertEqual(check_gate.main(["--spike", e, h, "--report", self.write("r.md", "Chosen default: topology=hybrid\n")]), 1)
        self.assertEqual(check_gate.main(["--spike", e, h, "--report", self.write("r2.md", "Chosen default: topology=envelope\n")]), 0)

    def test_the_choice_line_must_be_first_and_unique(self):
        env, hyb = self.arms(48, 1000)
        e, h = self.write("e.json", env), self.write("h.json", hyb)
        late = "# Spike\nChosen default: topology=envelope\n"
        twice = "Chosen default: topology=envelope\nChosen default: topology=envelope\n"
        for text in (late, twice, "no choice here\n", "Chosen default: topology=both\n"):
            with self.subTest(text=text):
                self.assertEqual(check_gate.main(["--spike", e, h, "--report", self.write("r.md", text)]), 1)

    def test_missing_report_is_malformed(self):
        env, hyb = self.arms(48, 1000)
        e, h = self.write("e.json", env), self.write("h.json", hyb)
        self.assertEqual(check_gate.main(["--spike", e, h, "--report", os.path.join(self._tmp.name, "none.md")]), 2)

    def test_arm_checks(self):
        base_env, base_hyb = self.arms(49, 1000)
        report = os.path.join(self._tmp.name, "r.md")

        def spike(env, hyb):
            return check_gate.main(["--spike", self.write("e.json", env), self.write("h.json", hyb), "--write-report", report])

        self.assertEqual(spike(base_env, base_hyb), 0)
        cases = {
            "commit": lambda e, h: h.update(heron_commit="other"),
            "runs_per_task": lambda e, h: None,
            "build": lambda e, h: e.update(build="release"),
            "provider": lambda e, h: h.update(provider="codex-cli"),
            "topology": lambda e, h: e.update(topology="hybrid"),
        }
        for name, mutate in cases.items():
            env, hyb = self.arms(49, 1000)
            mutate(env, hyb)
            if name == "runs_per_task":
                env = summary(self.spread(45), runs_per_task=3, build="debug", topology="envelope")
            with self.subTest(name=name):
                self.assertEqual(spike(env, hyb), 1)

    def test_a_probe_in_an_arm_is_refused(self):
        env, hyb = self.arms(49, 1000)
        env["tasks"].append({"id": "p1", "counts_toward_gate": False, "runs": [run(1)]})
        env["totals"] = compute_totals(env["tasks"])
        self.assertEqual(check_gate.main(["--spike", self.write("e.json", env), self.write("h.json", hyb),
                                          "--write-report", os.path.join(self._tmp.name, "r.md")]), 1)

    def test_usage_errors_exit_2(self):
        self.assertEqual(check_gate.main([]), 2)
        e = self.write("e.json", summary(3))
        self.assertEqual(check_gate.main(["--spike", e, e]), 2)
        self.assertEqual(check_gate.main(["--schema-only", e, e]), 2)


class Totals(unittest.TestCase):
    def test_medians_skip_errored_runs_and_probes(self):
        tasks = [
            {"id": "t01", "counts_toward_gate": True, "runs": [run(1, tokens=100, ttft=10), run(2, tokens=300, ttft=30),
                                                              run(3, False, tokens=0, ttft=None, error="x")]},
            {"id": "p1", "counts_toward_gate": False, "runs": [run(1, tokens=99999, ttft=99999)]},
        ]
        totals = compute_totals(tasks)
        self.assertEqual(totals, {"gate_runs": 3, "gate_passes": 2, "median_ttft_ms": 20, "median_tokens_per_task": 200})

    def test_no_clean_runs_gives_null_medians(self):
        tasks = [{"id": "t01", "counts_toward_gate": True, "runs": [run(1, False, error="x", ttft=None)]}]
        totals = compute_totals(tasks)
        self.assertIsNone(totals["median_ttft_ms"])
        self.assertIsNone(totals["median_tokens_per_task"])


if __name__ == "__main__":
    unittest.main()
