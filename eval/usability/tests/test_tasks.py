import copy
import os
import shutil
import subprocess
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
USABILITY = os.path.dirname(HERE)
EVAL = os.path.dirname(USABILITY)
for path in (USABILITY, EVAL):
    if path not in sys.path:
        sys.path.insert(0, path)

import run  # noqa: E402
import rules  # noqa: E402
from tasks_md import load_tasks  # noqa: E402

# `eve personas` and `eve professions` in EVE 0.5.x. The live check below re-reads them when eve is on PATH.
EVE_PERSONAS = {"first-time-user", "power-user", "office-worker", "developer-as-customer", "project-manager",
                "non-technical-user", "student", "accessibility-user", "elderly-user", "color-blind-user",
                "impatient-user", "distracted-user", "curious-explorer", "slow-reader", "fast-reader",
                "anxious-user", "confident-user"}
EVE_PROFESSIONS = {"doctor", "teacher", "lawyer", "designer", "accountant", "student", "salesperson", "executive"}


def load():
    return load_tasks(run.TASKS_PATH), run.load_personas(), {t["id"]: t for t in load_tasks(rules.CORE_TASKS)}


class FilesTests(unittest.TestCase):
    def test_committed_files_are_well_formed(self):
        tasks, personas, core = load()
        self.assertEqual(run.check_all(tasks, personas, core), [])

    def test_task_count_and_coverage(self):
        tasks, _, _ = load()
        self.assertTrue(8 <= len(tasks) <= 12)
        titles = " ".join(t["title"].lower() for t in tasks)
        for topic in ("risk notice", "normal chat", "approval", "secret", "web page", "permission mode",
                      "disconnect", "telegram"):
            self.assertIn(topic, titles)

    def test_personas(self):
        _, personas, _ = load()
        self.assertTrue(4 <= len(personas) <= 6)
        for p in personas.values():
            self.assertIn(p["eve_persona"], EVE_PERSONAS, p["id"])
            self.assertTrue(p["eve_profession"] is None or p["eve_profession"] in EVE_PROFESSIONS, p["id"])
            self.assertIn("fictional", p["name"])
            self.assertIn(p["tech_depth"], ("low", "medium", "high"))
        self.assertTrue(any(p["tech_depth"] == "low" for p in personas.values()))
        self.assertTrue(any(p["eve_persona"] == "non-technical-user" for p in personas.values()))
        assistive = [p for p in personas.values() if p["assistive"]]
        self.assertTrue(assistive)
        for p in assistive:
            self.assertEqual(p["surfaces"], ["mac"], "an assistive persona's API runs prove nothing about access")

    @unittest.skipUnless(shutil.which("eve"), "eve is not on PATH")
    def test_personas_against_installed_eve(self):
        listed = {line.split()[0] for line in subprocess.run(["eve", "personas"], capture_output=True, text=True,
                                                             timeout=60).stdout.splitlines() if line.strip()}
        for p in run.load_personas().values():
            self.assertIn(p["eve_persona"], listed)

    def test_reused_core_prompts_are_not_copied(self):
        tasks, _, core = load()
        with open(run.TASKS_PATH, encoding="utf-8") as handle:
            text = handle.read()
        for task in tasks:
            if task["reuses"]:
                self.assertNotIn(core[task["reuses"]]["prompt"], text, task["id"])


class CheckTests(unittest.TestCase):
    def setUp(self):
        self.tasks, self.personas, self.core = load()

    def broken(self, mutate, index=0):
        tasks = copy.deepcopy(self.tasks)
        mutate(tasks[index])
        return run.check_all(tasks, self.personas, self.core)

    def assertCaught(self, mutate, text, index=0):
        problems = self.broken(mutate, index)
        self.assertTrue(any(text in p for p in problems), problems)

    def test_catches_each_mistake(self):
        self.assertCaught(lambda t: t.update(id="x1"), "id must look like u01")
        self.assertCaught(lambda t: t.update(_heading="u99: x"), "heading id")
        self.assertCaught(lambda t: t.pop("criteria"), "missing 'criteria'")
        self.assertCaught(lambda t: t.update(personas=["nobody"]), "personas")
        self.assertCaught(lambda t: t.update(personas=[]), "personas")
        self.assertCaught(lambda t: t.update(requires=["u9"]), "requires")
        self.assertCaught(lambda t: t.update(metrics=["completion"]), "metrics")
        self.assertCaught(lambda t: t.update(criteria=[""]), "criteria")
        self.assertCaught(lambda t: t.update(reuses="t99"), "reuses unknown")
        self.assertCaught(lambda t: t.update(steps=[]), "steps must be")
        self.assertCaught(lambda t: t["steps"].append({"say": "a", "api": "GET /api/x"}), "exactly one of")
        self.assertCaught(lambda t: t["steps"].append({"api": "PUT /api/x"}), "api must be")
        self.assertCaught(lambda t: t["steps"].append({"api": "GET /x"}), "api must be")
        self.assertCaught(lambda t: t["steps"].append({"say_from": "t99"}), "say_from names unknown")
        self.assertCaught(lambda t: t["steps"].append({"say": "a", "approve_from": "t99"}), "approve_from names unknown")
        self.assertCaught(lambda t: t["steps"].append({"say": "a", "approve": [], "approve_from": "t01"}), "not both")
        self.assertCaught(lambda t: t["steps"].append({"say": "a", "approve": [{"tool": "x", "decision": "all"}]}),
                          "unknown decision")
        self.assertCaught(lambda t: t["steps"].append({"plant": {"path": "/etc/x", "text": ""}}), "plant.path")
        self.assertCaught(lambda t: t["steps"].append({"plant": {"path": "{home}/../x", "text": ""}}), "plant.path")
        self.assertCaught(lambda t: t["steps"].append({"plant": {"path": "{web}/x", "text": ""}}), "plant.path")
        self.assertCaught(lambda t: t.update(success={"event_seen": {"type": "done", "step": 9}}), "past the script")
        self.assertCaught(lambda t: t.update(success={"core_pass": {"task": "t99"}}), "core_pass names unknown")
        self.assertCaught(lambda t: t.update(success={"all": [{"core_pass": {"task": "t99"}}]}), "core_pass names unknown")

    def test_catches_mistakes_in_the_demo_variant(self):
        index = next(i for i, t in enumerate(self.tasks) if "demo" in t)
        self.assertCaught(lambda t: t["demo"].update(steps=[]), "(demo): steps", index)
        self.assertCaught(lambda t: t["demo"].update(success={"event_seen": {"type": "x", "step": 99}}), "(demo) success", index)

    def test_duplicate_ids(self):
        tasks = copy.deepcopy(self.tasks)
        tasks[1]["id"] = tasks[0]["id"]
        self.assertTrue(any("duplicate id" in p for p in run.check_all(tasks, self.personas, self.core)))

    def test_plant_paths_that_are_fine(self):
        self.assertEqual(run._check_steps("x", [{"plant": {"path": "{work}/a/b.txt", "text": ""}}], self.core), [])


class ScriptTests(unittest.TestCase):
    def test_script_for(self):
        tasks = {t["id"]: t for t in load_tasks(run.TASKS_PATH)}
        steps, _success, variant = run.script_for(tasks["u03"], "demo")
        self.assertEqual(variant, "demo")
        self.assertEqual(steps, tasks["u03"]["demo"]["steps"])
        self.assertEqual(run.script_for(tasks["u03"], "claude-cli")[2], "default")
        self.assertEqual(run.script_for(tasks["u02"], "demo")[2], "default")

    def test_expand_step(self):
        _, _, core = load()
        out = run.expand_step({"say_from": "t01", "approve_from": "t01"}, core)
        self.assertEqual(out["say"], core["t01"]["prompt"])
        self.assertEqual(out["approve"], [{"tool": "write_file", "decision": "once"}])
        self.assertNotIn("say_from", out)
        self.assertEqual(run.expand_step({"say": "a"}, core), {"say": "a"})

    def test_substitute(self):
        places = {"web": "http://w", "home": "/h", "work": "/w"}
        saved = {"version": 3, "name": "x"}
        self.assertEqual(run.substitute({"version": "{{version}}"}, places, saved), {"version": 3})
        self.assertEqual(run.substitute("v{{version}} at {home}/a", places, saved), "v3 at /h/a")
        self.assertEqual(run.substitute(["{web}/f", 5, None], places, saved), ["http://w/f", 5, None])
        with self.assertRaises(run.UsageError):
            run.substitute("{{missing}}", places, saved)
        with self.assertRaises(run.UsageError):
            run.substitute("a {{missing}}", places, saved)

    def test_dig(self):
        value = {"a": {"b": [10, {"c": "x"}]}}
        self.assertEqual(run.dig(value, "a.b.1.c"), "x")
        self.assertEqual(run.dig(value, "a.b.0"), 10)
        self.assertIsNone(run.dig(value, "a.b.5"))
        self.assertIsNone(run.dig(value, "a.z"))
        self.assertIsNone(run.dig(value, "a.b.0.c"))


if __name__ == "__main__":
    unittest.main()
