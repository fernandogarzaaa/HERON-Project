#!/usr/bin/env python3
"""A strict verifier for ``genesis audit-evaluator --suite json`` whose verdict is the usability
scorer's ``completed`` (``eval/usability/rules.py`` through ``run.score_fixture``). Standard
library only.

genesis hands a verifier ``task.json`` and a candidate completion and expects
``{"pass": <bool>, "score": <0 or 1>, "reward": <0 or 1>}`` on stdout.

Two task shapes reach it:

* **A usability task** (the dataset, ``cases.jsonl``): ``{"task": "u05", "variant":
  "default"|"demo", "run": {...}}``. ``run`` is the evidence of one run, in the fixture shape
  (``steps``, ``files``, ``audit``, ``web_log``, ``session_id``). The rule is never taken from the
  task: it is read from ``eval/usability/tasks.md`` for that task and variant, so a row can't
  bring its own weaker rule. The completion is a verdict record, exactly ``{"task", "variant",
  "completed", "failed_checks"}``. It is accepted only when the record names the same task and
  variant, the scorer finds the evidence completes the task, and the record says so (``completed:
  true``, no failed checks). A record that claims a completion the evidence doesn't support is
  rejected, and nothing the record says can make failing evidence pass.
* **A reference task** (genesis's own ``json`` suite probes): handed to U0g's
  ``json_verifier.reference_verdict`` (strict parse, schema, exact match), so the suite's probes
  exercise the same strict parser.

    python3 -I eval/usability/genesis/verifier.py <task.json> <completion file>...

Exit 0 with a verdict on stdout; 2 when the task file is malformed. Anything a completion can
cause is a rejection at exit 0.
"""

import json
import os
import sqlite3
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
USABILITY = os.path.dirname(HERE)
EVAL = os.path.dirname(USABILITY)
for path in (USABILITY, EVAL, os.path.join(EVAL, "genesis")):
    if path not in sys.path:
        sys.path.insert(0, path)

import json_verifier  # noqa: E402  (U0g: strict_loads, Reject, reference tasks)
import rules  # noqa: E402
import run as usability_run  # noqa: E402
from tasks_md import load_tasks  # noqa: E402

Reject = json_verifier.Reject
RECORD_KEYS = ("task", "variant", "completed", "failed_checks")
TASK_KEYS = ("task", "variant", "run")
RUN_KEYS = ("steps", "files", "audit", "web_log", "session_id")
STEP_KIND = {"say": "say", "say_from": "say", "api": "api", "resume": "resume", "plant": "plant"}


def _expect(condition, message):
    if not condition:
        raise ValueError(message)


def script(task_id, variant):
    tasks = {t["id"]: t for t in load_tasks(usability_run.TASKS_PATH)}
    _expect(task_id in tasks, "unknown usability task %r" % task_id)
    task = tasks[task_id]
    _expect(variant in ("default", "demo"), "variant must be default or demo")
    if variant == "demo":
        _expect("demo" in task, "task %s has no demo variant" % task_id)
        return task["demo"]["steps"], task["demo"]["success"]
    return task["steps"], task["success"]


def check_run(run, steps):
    _expect(isinstance(run, dict), "run must be an object")
    _expect(not (set(run) - set(RUN_KEYS)), "run has unknown keys: %s" % sorted(set(run) - set(RUN_KEYS)))
    records = run.get("steps")
    _expect(isinstance(records, list) and all(isinstance(r, dict) for r in records), "run.steps must be a list of objects")
    _expect(len(records) == len(steps), "run.steps has %d steps, the script %d" % (len(records), len(steps)))
    for i, (record, spec) in enumerate(zip(records, steps, strict=True)):
        kind = next(STEP_KIND[k] for k in STEP_KIND if k in spec)
        _expect(record.get("kind") == kind, "run step %d is %r, the script's is %r" % (i, record.get("kind"), kind))
        _expect(isinstance(record.get("events", []), list), "run step %d events must be a list" % i)
    json_verifier.check_run({k: run[k] for k in ("files", "audit", "web_log", "session_id") if k in run})


def check_record(value):
    if not isinstance(value, dict):
        raise Reject("a verdict record must be a JSON object")
    extra = sorted(set(value) - set(RECORD_KEYS))
    if extra:
        raise Reject("the verdict record has fields outside the schema: %s" % ", ".join(map(repr, extra)))
    missing = [k for k in RECORD_KEYS if k not in value]
    if missing:
        raise Reject("the verdict record is missing: %s" % ", ".join(missing))
    if not isinstance(value["task"], str) or not isinstance(value["variant"], str):
        raise Reject("task and variant must be strings")
    if not isinstance(value["completed"], bool):
        raise Reject("completed must be true or false")
    if not isinstance(value["failed_checks"], list) or not all(isinstance(f, str) for f in value["failed_checks"]):
        raise Reject("failed_checks must be a list of strings")


def usability_verdict(task, completion):
    steps, success = script(task["task"], task["variant"])
    check_run(task["run"], steps)
    try:
        record = json_verifier.strict_loads(completion)
        check_record(record)
        if record["task"] != task["task"] or record["variant"] != task["variant"]:
            raise Reject("the record is for %s/%s, the task is %s/%s" % (
                record["task"], record["variant"], task["task"], task["variant"]))
        result = usability_run.score_fixture(task["run"], success)
        if not result["completed"]:
            raise Reject("the evidence does not complete the task: " + "; ".join(result["failed_checks"]))
        if record["completed"] is not True or record["failed_checks"] != []:
            raise Reject("the evidence completes the task but the record says otherwise")
    except Reject as exc:
        return False, "rejected: %s" % exc
    except (rules.RuleError, sqlite3.Error, OSError) as exc:
        return False, "rejected: the rule could not be evaluated: %s" % exc
    return True, "accepted"


def normalize_task(raw):
    _expect(isinstance(raw, dict), "the task must be a JSON object")
    candidates = [raw, raw.get("context")]
    if isinstance(raw.get("input"), str):
        try:
            candidates.append(json.loads(raw["input"]))
        except ValueError:
            pass
    for candidate in candidates:
        if isinstance(candidate, dict) and "run" in candidate and "task" in candidate:
            allowed = set(TASK_KEYS) | (json_verifier.ENVELOPE_KEYS if candidate is raw else set())
            extra = sorted(set(candidate) - allowed)
            _expect(not extra, "the task has unknown keys: %s" % ", ".join(extra))
            return {"task": candidate["task"], "variant": candidate.get("variant", "default"), "run": candidate["run"]}
    return json_verifier.normalize_task(raw)


def verdict(task, completion):
    if "run" in task:
        return usability_verdict(task, completion)
    return json_verifier.reference_verdict(task, completion)


def main(argv):
    if len(argv) < 2:
        sys.stderr.write("usage: verifier.py <task.json> <completion file>...\n")
        return 2
    candidates = [p for p in argv[1:] if os.path.isfile(p)]
    if not candidates:
        sys.stderr.write("verifier: none of %r is a file\n" % (argv[1:],))
        return 2
    try:
        with open(argv[0], "r", encoding="utf-8") as handle:
            task = normalize_task(json.load(handle))
        with open(candidates[0], "r", encoding="utf-8", errors="replace") as handle:
            completion = handle.read()
        accepted, why = verdict(task, completion)
    except (OSError, ValueError, AttributeError, KeyError, TypeError, StopIteration) as exc:
        sys.stderr.write("verifier: malformed task: %s\n" % exc)
        return 2
    sys.stderr.write("verifier: %s\n" % why)
    print(json.dumps({"pass": accepted, "score": 1.0 if accepted else 0.0, "reward": 1 if accepted else 0}))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
