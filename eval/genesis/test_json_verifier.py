import copy
import json
import os
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import pass_rules
import run_core_tasks
from tasks_md import load_tasks

from genesis import build_cases
from genesis import json_verifier as jv

HERE = os.path.dirname(os.path.abspath(__file__))
VERIFIER = os.path.join(HERE, "json_verifier.py")
LEAF_KINDS = ("file_contains", "file_present", "file_absent", "reply_matches", "sql_at_least", "audit_has",
              "no_request", "transcript_lacks")


def load_rows():
    with open(build_cases.CASES_PATH, "r", encoding="utf-8") as handle:
        return [json.loads(line) for line in handle]


ROWS = load_rows()


def verdict(task_text, completion):
    return jv.verdict(jv.normalize_task({"id": "x", "input": task_text, "reference": ""}), completion)


def rule_kinds(rule):
    key = next(iter(rule))
    if key == "all":
        return {"all"}.union(*(rule_kinds(sub) for sub in rule["all"]))
    return {key}


# A small task of our own for the strictness tests: the reply must say "hi".
def small_task(**run):
    run.setdefault("events", build_cases.events("hi", 5, 5))
    return {"rule": {"reply_matches": {"regex": "hi"}}, "run": run}


def record(**changes):
    base = {"run": 1, "pass": True, "failed_rules": [], "reply": "hi", "tokens": 10, "ttft_ms": 5, "asks": 0,
            "error": None}
    base.update(changes)
    return base


def small_verdict(completion, task=None):
    return jv.verdict(task or small_task(), completion if isinstance(completion, str) else json.dumps(completion))


class DatasetCases(unittest.TestCase):
    def test_the_committed_dataset_is_what_the_builder_makes(self):
        with open(build_cases.CASES_PATH, "r", encoding="utf-8") as handle:
            committed = handle.read()
        self.assertEqual(committed, build_cases.render(build_cases.build(load_tasks(build_cases.TASKS_PATH))))

    def test_every_known_good_record_is_accepted(self):
        for row in ROWS:
            with self.subTest(row["id"]):
                ok, why = verdict(row["input"], row["reference"])
                self.assertTrue(ok, why)

    def test_every_known_bad_record_is_rejected_for_its_rule(self):
        count = 0
        for row in ROWS:
            for bad in row["bad"]:
                count += 1
                with self.subTest(msg="%s: %s" % (row["id"], bad["why"])):
                    ok, why = verdict(bad["input"], bad["completion"])
                    self.assertFalse(ok)
                    self.assertIn("the evidence fails the rule", why)
                    self.assertIn(bad["expect"], why)
        self.assertGreaterEqual(count, 20)

    def test_a_known_bad_record_is_well_formed_and_consistent(self):
        # What makes it bad is the evidence, not its shape: it parses, and it agrees with its own context.
        for row in ROWS:
            for bad in row["bad"]:
                with self.subTest(msg="%s: %s" % (row["id"], bad["why"])):
                    jv.check_record(jv.strict_loads(bad["completion"]))
                    self.assertNotEqual(bad["input"], row["input"])
                    self.assertTrue(json.loads(bad["completion"])["pass"])

    def test_every_rule_kind_has_a_good_and_a_bad_record(self):
        good = set().union(*(rule_kinds(json.loads(row["input"])["rule"]) for row in ROWS))
        self.assertEqual(good, set(pass_rules.RULE_KEYS))
        failing = {bad["expect"] for row in ROWS for bad in row["bad"]}
        self.assertEqual(failing, set(LEAF_KINDS))
        all_rows = [row for row in ROWS if "all" in json.loads(row["input"])["rule"]]
        self.assertGreaterEqual(len(all_rows), 3)
        self.assertTrue(all(row["bad"] for row in all_rows))

    def test_each_rule_comes_from_a_core_task(self):
        tasks = {t["id"]: t for t in load_tasks(build_cases.TASKS_PATH)}
        for row in ROWS:
            with self.subTest(row["id"]):
                rule = json.loads(row["input"])["rule"]
                pass_rule = tasks[row["source"]]["pass"]
                leaves = pass_rule["all"] if "all" in pass_rule else []
                self.assertIn(rule, [pass_rule] + leaves)

    def test_the_rows_use_the_real_fixtures(self):
        for row in ROWS:
            run = json.loads(row["input"])["run"]
            fixture = row["source"]
            if os.path.isdir(os.path.join(build_cases.FIXTURES, fixture)):
                for path, text in build_cases.fixture_files(fixture).items():
                    if path not in ("old.log",) or "old.log" in run["files"]:
                        self.assertIn(path, run["files"], row["id"])
                        if not path.startswith("notes/summary") and path != "todo.txt":
                            self.assertEqual(run["files"][path], text, row["id"])

    def test_a_record_from_another_row_is_never_accepted(self):
        for row in ROWS:
            for other in ROWS:
                if other is row:
                    continue
                with self.subTest(msg="%s vs %s" % (row["id"], other["id"])):
                    ok, why = verdict(row["input"], other["reference"])
                    self.assertFalse(ok)
                    self.assertIn("the record's", why)

    def test_gold_records_are_all_different(self):
        for key in ("tokens", "reply"):
            values = [json.loads(row["reference"])[key] for row in ROWS]
            self.assertEqual(len(values), len(set(values)), key)

    def test_the_builder_uses_the_runners_ask_decisions(self):
        self.assertEqual(build_cases.ASK, run_core_tasks.ASK_DECISIONS)

    def test_check_mode_flags_a_stale_dataset(self):
        self.assertEqual(build_cases.main(["--check"]), 0)
        with mock.patch.object(build_cases, "CASES_PATH", os.path.join(HERE, "__init__.py")):
            self.assertEqual(build_cases.main(["--check"]), 1)
        self.assertEqual(build_cases.main(["--bogus"]), 2)


class StrictParsing(unittest.TestCase):
    def reject(self, completion, reason, task=None):
        ok, why = small_verdict(completion, task)
        self.assertFalse(ok, why)
        self.assertIn(reason, why)

    def test_the_baseline_record_is_accepted(self):
        self.assertTrue(small_verdict(record())[0])

    def test_whitespace_and_key_order_do_not_matter(self):
        reordered = json.dumps(record(), sort_keys=True, indent=4)
        self.assertTrue(small_verdict("\n  " + reordered + "  \n\n")[0])

    def test_duplicate_keys_are_rejected_at_every_depth(self):
        self.reject('{"run": 9, ' + json.dumps(record())[1:], "duplicate key 'run'")
        with self.assertRaises(jv.Reject) as caught:
            jv.strict_loads('{"a": {"b": 1, "b": 2}}')
        self.assertIn("duplicate key 'b'", str(caught.exception))

    def test_prose_and_fences_around_the_json_are_rejected(self):
        body = json.dumps(record())
        for completion in ("Here is the record: " + body, body + "\nHope that helps!", "```json\n" + body + "\n```",
                           body + " " + body, body + ",", "{'run': 1}"):
            with self.subTest(completion[:30]):
                self.reject(completion, "not a single clean JSON document")

    def test_empty_and_blank_completions_are_rejected(self):
        for completion in ("", "   \n\t "):
            self.reject(completion, "the completion is empty")

    def test_a_byte_order_mark_is_rejected(self):
        self.reject("﻿" + json.dumps(record()), "not a single clean JSON document")

    def test_non_standard_numbers_are_rejected(self):
        for constant in ("NaN", "Infinity", "-Infinity"):
            with self.subTest(constant):
                self.reject(json.dumps(record()).replace('"ttft_ms": 5', '"ttft_ms": ' + constant), "is not JSON")

    def test_an_infinite_float_is_rejected(self):
        self.reject(json.dumps(record()).replace('"ttft_ms": 5', '"ttft_ms": 1e999'), "ttft_ms must be finite")

    def test_a_record_must_be_an_object(self):
        for completion in ("[]", '"x"', "null", "5", "true", "[%s]" % json.dumps(record())):
            with self.subTest(completion[:20]):
                self.reject(completion, "must be a JSON object")

    def test_too_deep_a_document_is_a_rejection_not_a_crash(self):
        deep = "[" * 200000 + "]" * 200000
        self.reject(deep, "the document nests too deeply")
        with self.assertRaises(jv.Reject):
            jv.strict_loads(deep)
        # Whatever else escapes as a RecursionError is still a rejection.
        with mock.patch.object(jv, "strict_loads", side_effect=RecursionError):
            self.assertEqual(jv.verdict(small_task(), "{}"), (False, "rejected: the completion nests too deeply"))
            self.assertEqual(jv.verdict({"reference": "1", "schema": {}}, "{}"),
                             (False, "rejected: the completion nests too deeply"))

    def test_a_field_outside_the_schema_is_rejected_whatever_it_says(self):
        for extra in ({"verdict": "pass"}, {"score": 1}, {"Pass": True}, {"": 1}):
            with self.subTest(extra):
                name = next(iter(extra))
                self.reject(record(**extra), "outside the schema: %r" % name)

    def test_every_field_is_required(self):
        for key in jv.RECORD_KEYS:
            with self.subTest(key):
                partial = record()
                del partial[key]
                self.reject(partial, "the run record is missing: " + key)

    def test_values_of_the_wrong_type_are_rejected(self):
        bad = {"run": ["0", 0, True, 1.5], "pass": ["true", 1, 0, None], "failed_rules": ["x", [1], None, {}],
               "reply": [None, 5, ["hi"]], "tokens": ["10", True, 10.5, -1, None], "ttft_ms": ["5", -1, True, [5]],
               "asks": ["0", False, -1, 0.5, None]}
        for key, values in bad.items():
            for value in values:
                with self.subTest(msg="%s=%r" % (key, value)):
                    self.reject(record(**{key: value}), "wrong types")

    def test_null_and_number_ttft_are_both_fine(self):
        self.assertTrue(small_verdict(record(ttft_ms=None))[0])
        self.assertTrue(small_verdict(record(ttft_ms=12.5))[0])

    def test_invalid_utf8_in_a_completion_file_is_rejected_by_the_command(self):
        with tempfile.TemporaryDirectory() as root:
            task, out = os.path.join(root, "task.json"), os.path.join(root, "output.json")
            with open(task, "w") as handle:
                json.dump({"rule": small_task()["rule"], "run": small_task()["run"]}, handle)
            with open(out, "wb") as handle:
                handle.write(b'{"run": 1, "reply": "\xff\xfe"}')
            proc = subprocess.run([sys.executable, "-I", VERIFIER, task, "{completion_file}", out],
                                  capture_output=True, text=True)
            self.assertEqual(proc.returncode, 0)
            self.assertFalse(json.loads(proc.stdout)["pass"])


class RecordAgainstEvidence(unittest.TestCase):
    def reject(self, completion, reason, task=None):
        ok, why = small_verdict(completion, task)
        self.assertFalse(ok, why)
        self.assertIn(reason, why)

    def test_a_reply_that_is_not_in_the_events_is_rejected(self):
        self.reject(record(reply="hi!"), "the record's reply is not the reply in the run's events")

    def test_the_record_keeps_only_the_first_two_thousand_characters_of_the_reply(self):
        task = {"rule": {"reply_matches": {"regex": "x"}}, "run": {"events": build_cases.events("x" * 2500, 5, 5)}}
        self.assertTrue(small_verdict(record(reply="x" * 2000), task)[0])
        self.reject(record(reply="x" * 2001), "reply is not the reply", task)
        self.reject(record(reply="x" * 1999), "reply is not the reply", task)
        self.reject(record(reply="x" * 2500), "reply is not the reply", task)

    def test_a_token_count_that_is_not_the_events_total_is_rejected(self):
        self.reject(record(tokens=11), "the record's tokens (11) are not the events' total (10)")
        self.reject(record(tokens=9), "the record's tokens (9)")

    def test_an_ask_count_that_is_not_the_audits_is_rejected(self):
        self.reject(record(asks=1), "the record's asks (1) are not the audit's count (0)")
        task = small_task(audit=[{"tool": "shell", "decision": "approved"}])
        self.reject(record(asks=0), "the record's asks (0) are not the audit's count (1)", task)
        self.assertTrue(small_verdict(record(asks=1), task)[0])

    def test_only_this_sessions_asks_count(self):
        audit = [{"tool": "a", "decision": "approved"}, {"tool": "b", "decision": "denied-by-user"},
                 {"tool": "c", "decision": "approved-always"}, {"tool": "d", "decision": "approved-for-conversation"},
                 {"tool": "e", "decision": "allowed"}, {"tool": "f", "decision": "denied-by-policy"},
                 {"tool": "g", "decision": "approved", "session_id": "other"}]
        self.assertTrue(small_verdict(record(asks=4), small_task(audit=audit))[0])
        self.assertFalse(small_verdict(record(asks=5), small_task(audit=audit))[0])
        self.assertFalse(small_verdict(record(asks=3), small_task(audit=audit))[0])

    def test_a_record_that_reports_an_error_is_rejected(self):
        self.reject(record(error="RunError: the turn ended with 'timeout'", **{"pass": False}),
                    "the record reports an error: RunError")
        # A run that errored cannot also claim a pass: the shared run-record schema refuses it.
        self.reject(record(error="RunError: x"), "a run with an error can't pass")

    def test_a_record_that_denies_a_passing_run_is_rejected(self):
        self.reject(record(**{"pass": False}), "the evidence passes the rule but the record says otherwise")
        self.reject(record(failed_rules=["reply_matches regex=hi"]), "the record says otherwise")

    def test_a_forged_pass_does_not_survive_failing_evidence(self):
        task = small_task(events=build_cases.events("bye", 5, 5))
        self.reject(record(reply="bye"), "the evidence fails the rule: reply_matches regex=hi", task)

    def test_a_forged_failure_is_not_a_pass(self):
        task = small_task(events=build_cases.events("bye", 5, 5))
        self.reject(record(reply="bye", **{"pass": False, "failed_rules": ["reply_matches regex=hi"]}),
                    "the evidence fails the rule", task)


class DerivedFromEvents(unittest.TestCase):
    def test_the_done_text_is_the_reply_and_ends_the_stream(self):
        events = [{"type": "text", "text": "par"}, {"type": "done", "text": "final"}, {"type": "text", "text": "late"},
                  {"type": "usage", "prompt": 9, "completion": 9}]
        self.assertEqual(jv.derive(events), ("final", 0))

    def test_without_done_the_text_events_are_joined(self):
        self.assertEqual(jv.derive([{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]), ("ab", 0))

    def test_an_empty_done_text_still_wins(self):
        self.assertEqual(jv.derive([{"type": "text", "text": "a"}, {"type": "done", "text": ""}]), ("", 0))

    def test_tokens_come_from_the_last_usage_event(self):
        events = [{"type": "usage", "prompt": 1, "completion": 2}, {"type": "usage", "prompt": 10, "completion": 20}]
        self.assertEqual(jv.derive(events), ("", 30))

    def test_no_events_means_an_empty_reply_and_no_tokens(self):
        self.assertEqual(jv.derive([]), ("", 0))


class TheVerdictIsPassRules(unittest.TestCase):
    def test_a_failing_evaluate_rejects_every_known_good_record(self):
        for row in ROWS:
            with self.subTest(row["id"]):
                with mock.patch.object(pass_rules, "evaluate", return_value=(False, ["stubbed rule"])):
                    ok, why = verdict(row["input"], row["reference"])
                self.assertFalse(ok)
                self.assertEqual(why, "rejected: the evidence fails the rule: stubbed rule")

    def test_a_passing_evaluate_accepts_a_consistent_record(self):
        # The verifier adds strictness and consistency checks and nothing else: the verdict on the
        # evidence is evaluate's.
        for row in ROWS:
            for bad in row["bad"]:
                with self.subTest(msg="%s: %s" % (row["id"], bad["why"])):
                    with mock.patch.object(pass_rules, "evaluate", return_value=(True, [])):
                        self.assertTrue(verdict(bad["input"], bad["completion"])[0])

    def test_evaluate_gets_the_rule_and_the_materialized_run(self):
        row = next(r for r in ROWS if r["id"] == "t01-file_contains")
        seen = {}

        def spy(rule, ctx):
            seen["rule"] = rule
            with open(os.path.join(ctx.workdir, "notes", "summary.md"), encoding="utf-8") as handle:
                seen["file"] = handle.read()
            seen["reply"], seen["session"] = ctx.reply, ctx.session_id
            conn = __import__("sqlite3").connect(ctx.db_path)
            seen["audit"] = conn.execute("SELECT session_id, tool, decision FROM audit").fetchall()
            conn.close()
            return True, []

        with mock.patch.object(pass_rules, "evaluate", side_effect=spy):
            self.assertTrue(verdict(row["input"], row["reference"])[0])
        self.assertEqual(seen["rule"], json.loads(row["input"])["rule"])
        self.assertIn("Thursday", seen["file"])
        self.assertEqual(seen["reply"], "Saved the summary to notes/summary.md.")
        self.assertEqual(seen["session"], "s1")
        self.assertEqual(seen["audit"], [("s1", "write_file", "approved")])

    def test_a_rule_that_cannot_be_evaluated_is_a_rejection(self):
        task = {"rule": {"sql_at_least": {"query": "SELECT COUNT(*) FROM nowhere", "n": 0}},
                "run": {"events": build_cases.events("hi", 5, 5)}}
        ok, why = jv.verdict(task, json.dumps(record()))
        self.assertFalse(ok)
        self.assertIn("the rule could not be evaluated", why)

    def test_a_missing_working_file_is_a_failed_rule_not_a_crash(self):
        task = {"rule": {"file_contains": {"path": "nope.txt", "text": "x"}}, "run": {"events": build_cases.events("hi", 5, 5)}}
        ok, why = jv.verdict(task, json.dumps(record()))
        self.assertFalse(ok)
        self.assertIn("the evidence fails the rule: file_contains", why)

    def test_a_malformed_rule_is_a_malformed_task(self):
        task = {"rule": {"file_exists": {"path": "x"}}, "run": {}}
        with self.assertRaises(ValueError) as caught:
            jv.verdict(task, json.dumps(record()))
        self.assertIn("the task's rule is malformed", str(caught.exception))


class RunContext(unittest.TestCase):
    def bad(self, run, message):
        with self.assertRaises(ValueError) as caught:
            jv.check_run(run)
        self.assertIn(message, str(caught.exception))

    def test_a_well_formed_context_passes(self):
        jv.check_run({})
        jv.check_run({"files": {"a/b.txt": "x"}, "session_id": "s", "web_log": ["/a"], "sql": ["SELECT 1"],
                      "audit": [{"decision": "approved", "tool": "t", "session_id": "s"}], "events": [{"type": "x"}]})

    def test_malformed_contexts_are_named(self):
        self.bad([], "must be an object")
        self.bad({"extra": 1}, "unknown keys: extra")
        self.bad({"files": []}, "run.files must map paths to text")
        self.bad({"files": {"a": 1}}, "run.files must map paths to text")
        self.bad({"files": {"../a": "x"}}, "must stay inside the working folder")
        self.bad({"files": {"/a": "x"}}, "must stay inside the working folder")
        self.bad({"files": {"": "x"}}, "must stay inside the working folder")
        self.bad({"session_id": 1}, "run.session_id must be a string")
        self.bad({"web_log": "/a"}, "run.web_log must be a list of paths")
        self.bad({"web_log": [1]}, "run.web_log must be a list of paths")
        self.bad({"sql": "x"}, "run.sql must be a list of statements")
        self.bad({"sql": [1]}, "run.sql must be a list of statements")
        self.bad({"audit": {}}, "run.audit must be a list of rows")
        self.bad({"audit": [{"tool": "t"}]}, "each audit row needs a decision")
        self.bad({"audit": [{"decision": "d", "x": 1}]}, "each audit row needs a decision")
        self.bad({"audit": [{"decision": "d", "tool": 1}]}, "audit fields must be strings")
        self.bad({"events": {}}, "run.events must be a list of objects")
        self.bad({"events": ["x"]}, "run.events must be a list of objects")
        self.bad({"events": [{"text": 1}]}, "an event's text must be a string")

    def test_files_land_in_the_working_folder_and_audit_rows_in_the_db(self):
        run = {"files": {"a/b/c.txt": "deep", "top.txt": "top"}, "web_log": ["/x"],
               "audit": [{"decision": "approved", "tool": "shell"}, {"decision": "allowed", "session_id": "s9"}],
               "sql": ["CREATE TABLE t(a)", "INSERT INTO t VALUES (7)"], "events": build_cases.events("r", 1, 2)}
        with tempfile.TemporaryDirectory() as root:
            ctx, tokens = jv.materialize(run, root)
            with open(os.path.join(ctx.workdir, "a", "b", "c.txt"), encoding="utf-8") as handle:
                self.assertEqual(handle.read(), "deep")
            self.assertTrue(os.path.isfile(os.path.join(ctx.workdir, "top.txt")))
            self.assertEqual((ctx.reply, tokens, ctx.session_id, ctx.web_requests), ("r", 3, "s1", ["/x"]))
            conn = __import__("sqlite3").connect(ctx.db_path)
            self.assertEqual(conn.execute("SELECT session_id, tool, decision FROM audit ORDER BY id").fetchall(),
                             [("s1", "shell", "approved"), ("s9", None, "allowed")])
            self.assertEqual(conn.execute("SELECT a FROM t").fetchall(), [(7,)])
            conn.close()
            self.assertEqual(os.path.dirname(ctx.db_path), root)
            self.assertEqual(ctx.events, run["events"])


class TaskShapes(unittest.TestCase):
    RULE = {"file_present": {"path": "a"}}

    def test_a_pass_rule_task_in_each_of_its_three_shapes(self):
        task = {"rule": self.RULE, "run": {}}
        shapes = [dict(task), {"id": "x", "context": dict(task)},
                  {"id": "x", "input": json.dumps(task), "reference": "r", "source": "t01", "bad": []}]
        for shape in shapes:
            with self.subTest(sorted(shape)):
                self.assertEqual(jv.normalize_task(shape), task)

    def test_unknown_or_missing_keys_in_a_pass_rule_task_are_refused(self):
        for raw, message in (({"rule": self.RULE}, "needs a run context"),
                             ({"input": json.dumps({"rule": self.RULE, "run": {}, "extra": 1})}, "unknown keys: extra"),
                             ({"context": {"rule": self.RULE, "run": {}, "pass": True}}, "unknown keys: pass"),
                             ({"rule": self.RULE, "run": {}, "oops": 1}, "unknown keys: oops")):
            with self.subTest(message):
                with self.assertRaises(ValueError) as caught:
                    jv.normalize_task(raw)
                self.assertIn(message, str(caught.exception))

    def test_a_task_that_is_neither_shape_is_refused(self):
        for raw in ([], {"input": "not json"}, {"input": "{}"}, {"context": {"prompt": "x"}}, {}):
            with self.subTest(raw):
                with self.assertRaises(ValueError) as caught:
                    jv.normalize_task(raw)
                self.assertTrue(str(caught.exception))

    def test_a_reference_task_needs_its_reference_as_text(self):
        with self.assertRaises(ValueError):
            jv.normalize_task({"context": {"schema": {}, "reference": {"a": 1}}})
        self.assertEqual(jv.normalize_task({"reference": "1", "context": {"schema": {"type": "object"}}}),
                         {"reference": "1", "schema": {"type": "object"}})


# The shape genesis 0.4.0's json suite hands over (captured from a real run).
SUITE_SCHEMA = {"type": "object", "required": ["city", "units"], "additionalProperties": False,
                "properties": {"city": {"type": "string", "minLength": 1}, "units": {"type": "string", "enum": ["c", "f"]}}}
SUITE_TASK = {"id": "json/schema-only-1", "input": "Call the `get_weather` tool for Paris, in Celsius.",
              "reference": '{"city":"Paris","units":"c"}',
              "context": {"prompt": "Call it.", "reference": '{"city":"Paris","units":"c"}', "schema": SUITE_SCHEMA},
              "kind": "reference-based"}


class SuiteProbes(unittest.TestCase):
    def check(self, completion):
        return jv.verdict(jv.normalize_task(SUITE_TASK), completion)

    def test_the_two_suite_controls_are_accepted(self):
        self.assertTrue(self.check('{"city":"Paris","units":"c"}')[0])
        self.assertTrue(self.check('\n{ "units": "c",\n  "city": "Paris" }\n')[0])

    def test_the_five_suite_exploits_are_rejected(self):
        cases = [('{"city":"","units":"c"}', "shorter than 1"),
                 ('{"city":"Nowhere","units":"f"}', "not the reference answer"),
                 ('{"city":"Paris","units":"c","debug":true,"note":"x"}', "undeclared field 'debug'"),
                 ('{"city":"Nowhere","city":"Paris","units":"c"}', "duplicate key 'city'"),
                 ('Sure! It could be {"city":"Nowhere","units":"f"} but more likely {"city":"Paris","units":"c"} - ok?',
                  "not a single clean JSON document")]
        for completion, reason in cases:
            with self.subTest(reason):
                ok, why = self.check(completion)
                self.assertFalse(ok)
                self.assertIn(reason, why)

    def test_schema_violations_are_named(self):
        for completion, reason in (('{"city":"Paris","units":"C"}', "$.units is not one of the allowed values"),
                                   ('{"city":5,"units":"c"}', "$.city must be of type string"),
                                   ('{"city":"Paris"}', "$ is missing 'units'"), ('[]', "$ must be of type object"),
                                   ('"Paris"', "$ must be of type object")):
            with self.subTest(completion):
                ok, why = self.check(completion)
                self.assertFalse(ok)
                self.assertIn(reason, why)

    def test_a_valid_payload_that_is_not_the_reference_is_rejected(self):
        ok, why = self.check('{"city":"Paris","units":"f"}')
        self.assertEqual((ok, why), (False, "rejected: the payload is not the reference answer"))

    def test_a_reference_must_match_in_type_not_only_in_value(self):
        task = {"reference": '{"n": 1}', "schema": {"type": "object"}}
        self.assertTrue(jv.verdict(task, '{"n": 1}')[0])
        self.assertFalse(jv.verdict(task, '{"n": 1.0}')[0])
        self.assertFalse(jv.verdict(task, '{"n": true}')[0])

    def test_a_schema_keyword_the_verifier_cannot_check_is_a_malformed_task(self):
        with self.assertRaises(ValueError) as caught:
            jv.verdict({"reference": "{}", "schema": {"type": "object", "pattern": "x"}}, "{}")
        self.assertIn("does not check: pattern", str(caught.exception))
        with self.assertRaises(ValueError):
            jv.verdict({"reference": "{}", "schema": {"type": "integer"}}, "{}")

    def test_a_reference_that_is_not_clean_json_is_a_malformed_task(self):
        with self.assertRaises(ValueError):
            jv.verdict({"reference": "{'a': 1}", "schema": {}}, "{}")

    def test_schema_types_and_nested_properties(self):
        schema = {"type": "object", "properties": {"a": {"type": "array"}, "b": {"type": "boolean"},
                                                   "c": {"type": "object", "required": ["d"]}}}
        task = {"reference": '{"a": [], "b": true, "c": {"d": 1}}', "schema": schema}
        self.assertTrue(jv.verdict(task, '{"a": [], "b": true, "c": {"d": 1}}')[0])
        for completion, reason in (('{"a": {}, "b": true, "c": {"d": 1}}', "$.a must be of type array"),
                                   ('{"a": [], "b": 1, "c": {"d": 1}}', "$.b must be of type boolean"),
                                   ('{"a": [], "b": true, "c": {}}', "$.c is missing 'd'")):
            with self.subTest(reason):
                self.assertIn(reason, jv.verdict(task, completion)[1])

    def test_enum_compares_by_type_too(self):
        schema = {"enum": [1, "a"]}
        self.assertEqual(jv._schema_problems(1, schema), [])
        self.assertTrue(jv._schema_problems(True, schema))
        self.assertTrue(jv._schema_problems("b", schema))


class CommandLine(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = self._tmp.name

    def write(self, name, text):
        path = os.path.join(self.root, name)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(text)
        return path

    def run_cli(self, *args):
        return subprocess.run([sys.executable, "-I", VERIFIER, *args], capture_output=True, text=True)

    def dataset_task(self, row):
        return self.write("task.json", json.dumps({"id": row["id"], "input": row["input"], "reference": row["reference"]}))

    def test_a_known_good_record_prints_a_pass_verdict(self):
        row = ROWS[0]
        proc = self.run_cli(self.dataset_task(row), "{completion_file}", self.write("output.json", row["reference"]))
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(json.loads(proc.stdout), {"pass": True, "score": 1.0, "reward": 1})
        self.assertIn("accepted", proc.stderr)

    def test_a_known_bad_record_prints_a_reject_verdict_at_exit_zero(self):
        row = next(r for r in ROWS if r["bad"])
        bad = row["bad"][0]
        task = self.write("task.json", json.dumps({"id": row["id"], "input": bad["input"], "reference": row["reference"]}))
        proc = self.run_cli(task, self.write("c.txt", bad["completion"]), "{output_file}")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(json.loads(proc.stdout), {"pass": False, "score": 0.0, "reward": 0})
        self.assertIn("the evidence fails the rule", proc.stderr)

    def test_a_suite_probe_in_genesis_shape_is_judged(self):
        task = self.write("task.json", json.dumps(SUITE_TASK))
        good = self.run_cli(task, "{completion_file}", self.write("o1.json", '{"city":"Paris","units":"c"}'))
        bad = self.run_cli(task, "{completion_file}", self.write("o2.json", '{"city":"Paris","units":"c","x":1}'))
        self.assertTrue(json.loads(good.stdout)["pass"])
        self.assertFalse(json.loads(bad.stdout)["pass"])

    def test_a_malformed_task_exits_two_and_prints_no_verdict(self):
        for text in ("[]", "not json", json.dumps({"rule": {"nope": {}}, "run": {}}), json.dumps({"id": "x"})):
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
        proc = subprocess.run([sys.executable, "-I", VERIFIER, self.dataset_task(row), self.write("o.json", row["reference"])],
                              capture_output=True, text=True, cwd=self.root)
        self.assertEqual(json.loads(proc.stdout)["pass"], True, proc.stderr)


if __name__ == "__main__":
    unittest.main()
