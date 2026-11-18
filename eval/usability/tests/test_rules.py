import json
import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
USABILITY = os.path.dirname(HERE)
EVAL = os.path.dirname(USABILITY)
for path in (USABILITY, EVAL):
    if path not in sys.path:
        sys.path.insert(0, path)

import pass_rules  # noqa: E402
import rules  # noqa: E402


def say(events=(), reply="", **extra):
    record = {"kind": "say", "status": 200, "body": "", "events": list(events), "reply": reply, "end": "done",
              "error": None, "ms": 10, "ttft_ms": 5, "decisions": []}
    record.update(extra)
    return record


def api(status, body=""):
    return {"kind": "api", "status": status, "body": body, "events": [], "reply": "", "error": None, "ms": 1}


def evidence(steps, workdir="/nonexistent", reply=None):
    events = [e for s in steps for e in s.get("events", [])]
    last = reply if reply is not None else next((s["reply"] for s in reversed(steps) if s["kind"] == "say"), "")
    ctx = pass_rules.Context(workdir=workdir, reply=last, events=events)
    return rules.Evidence(ctx=ctx, steps=steps)


APPROVAL = {"type": "approval", "tool": "shell", "summary": "ls", "detail": "ls", "can_session": False, "can_always": False}


class ValidateTests(unittest.TestCase):
    def test_well_formed_rules(self):
        for rule in (
            {"event_seen": {"type": "done"}},
            {"event_absent": {"type": "approval", "step": 0, "where": {"tool": "^shell$"}}},
            {"http_status": {"step": 1, "status": 204}},
            {"http_status": {"step": 1, "status": 400, "body_regex": "BotFather"}},
            {"step_reply_matches": {"step": 0, "regex": "x"}},
            {"core_pass": {"task": "t01"}},
            {"all": [{"reply_matches": {"regex": "a"}}, {"event_seen": {"type": "done"}}]},
        ):
            self.assertEqual(rules.validate_rule(rule, n_steps=2), [], rule)

    def test_malformed_rules(self):
        cases = {
            "not an object": [],
            "two keys": {"event_seen": {"type": "x"}, "all": []},
            "unknown key": {"event_happened": {"type": "x"}},
            "empty all": {"all": []},
            "missing type": {"event_seen": {}},
            "unknown field": {"event_seen": {"type": "x", "kind": 1}},
            "step not int": {"event_seen": {"type": "x", "step": "0"}},
            "step is bool": {"http_status": {"step": True, "status": 200}},
            "negative step": {"event_seen": {"type": "x", "step": -1}},
            "step past script": {"event_seen": {"type": "x", "step": 2}},
            "where not object": {"event_seen": {"type": "x", "where": "tool"}},
            "where bad name": {"event_seen": {"type": "x", "where": {"a.b": "x"}}},
            "where regex not string": {"event_seen": {"type": "x", "where": {"ok": True}}},
            "where bad regex": {"event_seen": {"type": "x", "where": {"tool": "("}}},
            "status out of range": {"http_status": {"step": 0, "status": 99}},
            "status too high": {"http_status": {"step": 0, "status": 600}},
            "bad body regex": {"http_status": {"step": 0, "status": 200, "body_regex": "["}},
            "bad reply regex": {"step_reply_matches": {"step": 0, "regex": "("}},
            "pass_rules leaf checked by pass_rules": {"file_present": {"path": "../x"}},
            "nested bad leaf": {"all": [{"event_seen": {"type": "x"}}, {"http_status": {"step": 0}}]},
        }
        for name, rule in cases.items():
            self.assertNotEqual(rules.validate_rule(rule, n_steps=2), [], name)

    def test_status_boundaries(self):
        self.assertEqual(rules.validate_rule({"http_status": {"step": 0, "status": 100}}), [])
        self.assertEqual(rules.validate_rule({"http_status": {"step": 0, "status": 599}}), [])

    def test_step_on_last_index_is_fine(self):
        self.assertEqual(rules.validate_rule({"event_seen": {"type": "x", "step": 1}}, n_steps=2), [])
        self.assertEqual(rules.validate_rule({"event_seen": {"type": "x", "step": 0}}, n_steps=None), [])

    def test_evaluate_raises_on_malformed_rule(self):
        with self.assertRaises(rules.RuleError):
            rules.evaluate({"nope": {}}, evidence([]))


class EvaluateTests(unittest.TestCase):
    def test_event_seen_in_step_and_anywhere(self):
        ev = evidence([say([{"type": "done"}]), say([APPROVAL])])
        self.assertTrue(rules.evaluate({"event_seen": {"type": "approval"}}, ev)[0])
        self.assertTrue(rules.evaluate({"event_seen": {"type": "approval", "step": 1}}, ev)[0])
        self.assertFalse(rules.evaluate({"event_seen": {"type": "approval", "step": 0}}, ev)[0])
        self.assertFalse(rules.evaluate({"event_seen": {"type": "approval", "step": 5}}, ev)[0])

    def test_event_absent(self):
        ev = evidence([say([{"type": "done"}]), say([APPROVAL])])
        self.assertTrue(rules.evaluate({"event_absent": {"type": "approval", "step": 0}}, ev)[0])
        self.assertFalse(rules.evaluate({"event_absent": {"type": "approval"}}, ev)[0])
        self.assertTrue(rules.evaluate({"event_absent": {"type": "approval", "where": {"tool": "^read_file$"}}}, ev)[0])

    def test_where_matches_json_text_of_non_strings(self):
        ev = evidence([say([APPROVAL, {"type": "tool_end", "ok": True, "n": 3}])])
        self.assertTrue(rules.evaluate({"event_seen": {"type": "approval", "where": {"can_session": "^false$"}}}, ev)[0])
        self.assertFalse(rules.evaluate({"event_seen": {"type": "approval", "where": {"can_session": "^true$"}}}, ev)[0])
        self.assertTrue(rules.evaluate({"event_seen": {"type": "tool_end", "where": {"ok": "^true$", "n": "^3$"}}}, ev)[0])

    def test_where_needs_the_field(self):
        ev = evidence([say([{"type": "approval", "tool": "shell"}])])
        self.assertFalse(rules.evaluate({"event_seen": {"type": "approval", "where": {"can_session": "."}}}, ev)[0])
        self.assertTrue(rules.evaluate({"event_absent": {"type": "approval", "where": {"can_session": "."}}}, ev)[0])

    def test_where_is_case_insensitive_and_all_fields_must_match(self):
        ev = evidence([say([{"type": "approval", "tool": "Shell", "risk": "exec"}])])
        self.assertTrue(rules.evaluate({"event_seen": {"type": "approval", "where": {"tool": "^shell$"}}}, ev)[0])
        self.assertFalse(rules.evaluate({"event_seen": {"type": "approval", "where": {"tool": "shell", "risk": "write"}}}, ev)[0])

    def test_events_that_are_not_objects_are_ignored(self):
        ev = evidence([say(["approval", {"type": "done"}])])
        self.assertFalse(rules.evaluate({"event_seen": {"type": "approval"}}, ev)[0])
        self.assertTrue(rules.evaluate({"event_seen": {"type": "done", "step": 0}}, ev)[0])

    def test_http_status(self):
        ev = evidence([api(400, '{"error":"Create a bot with @BotFather"}'), api(204)])
        self.assertTrue(rules.evaluate({"http_status": {"step": 0, "status": 400}}, ev)[0])
        self.assertTrue(rules.evaluate({"http_status": {"step": 0, "status": 400, "body_regex": "botfather"}}, ev)[0])
        self.assertFalse(rules.evaluate({"http_status": {"step": 0, "status": 400, "body_regex": "token"}}, ev)[0])
        self.assertFalse(rules.evaluate({"http_status": {"step": 0, "status": 200}}, ev)[0])
        self.assertTrue(rules.evaluate({"http_status": {"step": 1, "status": 204}}, ev)[0])
        self.assertFalse(rules.evaluate({"http_status": {"step": 2, "status": 204}}, ev)[0])

    def test_http_status_with_missing_body(self):
        ev = evidence([{"kind": "api", "status": 204, "events": []}])
        self.assertTrue(rules.evaluate({"http_status": {"step": 0, "status": 204}}, ev)[0])
        self.assertFalse(rules.evaluate({"http_status": {"step": 0, "status": 204, "body_regex": "x"}}, ev)[0])

    def test_step_reply_matches(self):
        ev = evidence([say(reply="The codename is HERON-ALPHA-7."), say(reply="3 tests ran")])
        self.assertTrue(rules.evaluate({"step_reply_matches": {"step": 0, "regex": "heron-alpha-7"}}, ev)[0])
        self.assertFalse(rules.evaluate({"step_reply_matches": {"step": 1, "regex": "HERON"}}, ev)[0])
        self.assertFalse(rules.evaluate({"step_reply_matches": {"step": 3, "regex": "."}}, ev)[0])

    def test_pass_rules_leaves_use_the_last_reply_and_all_events(self):
        ev = evidence([say([{"type": "text", "text": "canary-1"}], reply="first"), say(reply="second")])
        self.assertTrue(rules.evaluate({"reply_matches": {"regex": "^second$"}}, ev)[0])
        self.assertFalse(rules.evaluate({"reply_matches": {"regex": "first"}}, ev)[0])
        self.assertFalse(rules.evaluate({"transcript_lacks": {"text": "canary-1"}}, ev)[0])

    def test_all_reports_every_failed_leaf(self):
        ev = evidence([say([{"type": "done"}])])
        ok, failed = rules.evaluate({"all": [{"event_seen": {"type": "done"}}, {"event_seen": {"type": "run_end"}},
                                             {"reply_matches": {"regex": "zzz"}}]}, ev)
        self.assertFalse(ok)
        self.assertEqual(len(failed), 2)
        self.assertIn("run_end", failed[0])
        self.assertIn("reply_matches", failed[1])

    def test_core_pass_uses_the_core_tasks_rule(self):
        with tempfile.TemporaryDirectory() as work:
            ev = evidence([say()], workdir=work)
            self.assertFalse(rules.evaluate({"core_pass": {"task": "t01"}}, ev)[0])
            os.makedirs(os.path.join(work, "notes"))
            with open(os.path.join(work, "notes", "summary.md"), "w") as handle:
                handle.write("Moved to Thursday")
            self.assertTrue(rules.evaluate({"core_pass": {"task": "t01"}}, ev)[0])

    def test_core_pass_unknown_task(self):
        with self.assertRaises(rules.RuleError):
            rules.evaluate({"core_pass": {"task": "t99"}}, evidence([say()]))


class MetricsTests(unittest.TestCase):
    def test_expected_status_is_not_an_error(self):
        rule = {"all": [{"http_status": {"step": 0, "status": 428}}]}
        m = rules.metrics(rule, [say(status=428, end="http"), api(500)])
        self.assertEqual(m["errors"], ["step 1: HTTP 500"])

    def test_unexpected_4xx_and_error_events_and_step_errors(self):
        m = rules.metrics({"event_seen": {"type": "done"}},
                          [say(status=409, end="http"), say([{"type": "error", "code": "session_busy", "message": "busy"}]),
                           say(error="POST /api/chat failed")])
        self.assertEqual(len(m["errors"]), 3)
        self.assertIn("HTTP 409", m["errors"][0])
        self.assertIn("session_busy", m["errors"][1])
        self.assertIn("POST /api/chat failed", m["errors"][2])

    def test_time_ttft_and_asks(self):
        m = rules.metrics({"event_seen": {"type": "done"}},
                          [api(200), say([APPROVAL], ms=100, ttft_ms=None), say([APPROVAL, APPROVAL], ms=50, ttft_ms=7)])
        self.assertEqual(m["time_ms"], 151)
        self.assertEqual(m["ttft_ms"], 7)
        self.assertEqual(m["asks"], 3)

    def test_confusion_points(self):
        unclear = {"type": "approval", "tool": "write_file", "summary": "", "detail": "x"}
        steps = [
            say([unclear], reply="ok"),
            say(reply="", decisions=[{"tool": "shell", "decision": "deny", "planned": False}]),
            say(reply="Could you clarify which folder?"),
            say(reply="half", end="timeout"),
            say(reply="", end="disconnected"),
            say(reply="", status=428, end="http"),
            say(reply="fine", eve_findings=["silent miss", {"title": "2 silent misses", "description": "d"},
                                            {"description": "only d"}]),
            say(reply="ok", decisions=[{"tool": "shell", "decision": "once", "planned": True}]),
        ]
        kinds = [(c["step"], c["kind"]) for c in rules.metrics({"event_seen": {"type": "done"}}, steps)["confusion_points"]]
        self.assertEqual(kinds, [(0, "approval_unclear"), (1, "unexpected_ask"), (1, "no_reply"),
                                 (2, "confused_reply"), (3, "cut_off"), (6, "eve"), (6, "eve"), (6, "eve")])
        details = [c["detail"] for c in rules.metrics({}, steps)["confusion_points"] if c["kind"] == "eve"]
        self.assertEqual(details, ["silent miss", "2 silent misses", "only d"])

    def test_demo_canned_reply_counts_as_confusion(self):
        reply = "This is **demo mode**: my replies are canned, but the actions are real."
        kinds = [c["kind"] for c in rules.metrics({}, [say(reply=reply)])["confusion_points"]]
        self.assertEqual(kinds, ["confused_reply"])

    def test_expected_statuses(self):
        rule = {"all": [{"http_status": {"step": 0, "status": 200}}, {"all": [{"http_status": {"step": 2, "status": 428}}]},
                        {"event_seen": {"type": "done"}}]}
        self.assertEqual(rules.expected_statuses(rule), {0: 200, 2: 428})
        self.assertEqual(rules.expected_statuses({"http_status": {"step": 1, "status": 204}}), {1: 204})
        self.assertEqual(rules.expected_statuses("x"), {})

    def test_score_combines_verdict_and_metrics(self):
        out = rules.score({"event_seen": {"type": "done"}}, evidence([say([{"type": "done"}])]))
        self.assertTrue(out["completed"])
        self.assertEqual(out["failed_checks"], [])
        self.assertEqual(out["time_ms"], 10)
        out = rules.score({"event_seen": {"type": "run_end"}}, evidence([say([{"type": "done"}])]))
        self.assertFalse(out["completed"])
        self.assertEqual(json.loads(out["failed_checks"][0].split(" ", 1)[1]), {"type": "run_end"})


if __name__ == "__main__":
    unittest.main()
