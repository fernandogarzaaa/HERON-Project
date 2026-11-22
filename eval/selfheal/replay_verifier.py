#!/usr/bin/env python3
"""A strict replay-record verifier for ``genesis audit-evaluator --suite json`` whose verdict is
``grade.grade`` (item 20). Standard library only.

genesis hands a verifier ``task.json`` and a candidate completion and expects
``{"pass": <bool>, "score": <0 or 1>, "reward": <0 or 1>}`` on stdout.

Two task shapes reach this verifier.

* **A replay task** (the dataset, ``genesis_cases.jsonl``). The task is one replay case and what
  was measured for it: ``{"case": 3, "fix": "<40 hex>", "outcome": {...}}``, where the outcome has
  the five fields ``run_replay.py`` hands to ``grade``. The completion is the record the replay
  stores for that case: ``{"case", "fix", "outcome", "pass"}``. The verifier parses the completion
  strictly (no duplicate keys, no prose around the JSON, no field outside the record schema, no wrong
  type), checks that it is about this case and carries this outcome, and only then asks
  ``grade.grade``. A record's own ``pass`` is never trusted: a record claiming a pass the grader
  does not give is rejected, and a record that agrees with a passing outcome is accepted only
  because ``grade`` said so.
* **A reference task** (genesis's own ``json`` suite probes, a tool-call payload with a schema and a
  reference answer). It goes through the same strict parser as ``eval/genesis/json_verifier.py``.

Run as ``python3 -I eval/selfheal/replay_verifier.py <task.json> <completion file>...``. Exit 0 with
a verdict on stdout, 2 when the task file is malformed. Anything a completion can cause, however
hostile, is a rejection at exit 0, never a crash.
"""

import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
EVAL = os.path.dirname(HERE)
sys.path.insert(0, EVAL)
sys.path.insert(0, HERE)

import grade as grader  # noqa: E402
from genesis import json_verifier as jv  # noqa: E402

RECORD_KEYS = ("case", "fix", "outcome", "pass")
TASK_KEYS = ("case", "fix", "outcome")
SHA = re.compile(r"[0-9a-f]{40}")


def _fields(value, keys, noun):
    """Problems with ``value`` as a ``noun`` made of exactly ``keys`` with the replay's types."""
    if not isinstance(value, dict):
        return ["a %s must be a JSON object" % noun]
    extra = sorted(set(value) - set(keys))
    if extra:
        return ["the %s has fields outside the schema: %s" % (noun, ", ".join(map(repr, extra)))]
    missing = [key for key in keys if key not in value]
    if missing:
        return ["the %s is missing: %s" % (noun, ", ".join(missing))]
    problems = []
    case = value["case"]
    if isinstance(case, bool) or not isinstance(case, int) or case < 1:
        problems.append("case must be a whole number from 1")
    if not isinstance(value["fix"], str) or not SHA.fullmatch(value["fix"]):
        problems.append("fix must be 40 lowercase hex digits")
    outcome = value["outcome"]
    if not isinstance(outcome, dict) or sorted(outcome) != sorted(grader.OUTCOME_KEYS):
        problems.append("outcome must have exactly: %s" % ", ".join(grader.OUTCOME_KEYS))
    elif not all(isinstance(outcome[key], bool) for key in grader.OUTCOME_KEYS):
        problems.append("every outcome field must be true or false")
    if "pass" in keys and not isinstance(value["pass"], bool):
        problems.append("pass must be true or false")
    return problems


def check_record(value):
    """Raise ``Reject`` unless ``value`` is exactly a replay record."""
    problems = _fields(value, RECORD_KEYS, "replay record")
    if problems:
        raise jv.Reject("; ".join(problems))


def check_task(task):
    """Raise ``ValueError`` unless ``task`` is a well-formed replay task."""
    problems = _fields(task, TASK_KEYS, "task")
    if problems:
        raise ValueError("; ".join(problems))


def replay_verdict(task, completion):
    """The verdict for a replay task. Returns ``(accepted, why)``."""
    check_task(task)
    try:
        record = jv.strict_loads(completion)
        check_record(record)
        for key in TASK_KEYS:
            if record[key] != task[key]:
                raise jv.Reject("the record's %s is not the task's" % key)
        if not grader.grade(task["outcome"]):
            failing = [key for key, want in grader.PASSING.items() if task["outcome"][key] is not want]
            raise jv.Reject("the outcome fails the grade: %s" % ", ".join(failing))
        if record["pass"] is not True:
            raise jv.Reject("the outcome passes the grade but the record says otherwise")
    except jv.Reject as exc:
        return False, "rejected: %s" % exc
    return True, "accepted"


def verdict(task, completion):
    """Return ``(accepted, why)`` for ``completion`` against a normalized ``task``."""
    try:
        if "outcome" in task:
            return replay_verdict(task, completion)
        return jv.reference_verdict(task, completion)
    except RecursionError:
        return False, "rejected: the completion nests too deeply"


def normalize_task(raw):
    """The task in one of its two shapes. genesis hands over a replay task at the top level, under
    ``context``, or (dataset-derived probes, which carry ``id``, ``input`` and ``reference``) as JSON
    text in ``input``. Its own suite probes are reference tasks."""
    jv._expect(isinstance(raw, dict), "the task must be a JSON object")
    candidates = [raw, raw.get("context")]
    if isinstance(raw.get("input"), str):
        try:
            candidates.append(json.loads(raw["input"]))
        except ValueError:
            pass
    for candidate in candidates:
        if isinstance(candidate, dict) and "outcome" in candidate:
            allowed = set(TASK_KEYS) | (jv.ENVELOPE_KEYS if candidate is raw else set())
            extra = sorted(set(candidate) - allowed)
            jv._expect(not extra, "the task has unknown keys: %s" % ", ".join(extra))
            missing = [key for key in TASK_KEYS if key not in candidate]
            jv._expect(not missing, "a replay task needs: %s" % ", ".join(missing))
            return {key: candidate[key] for key in TASK_KEYS}
    task = jv.normalize_task(raw)
    jv._expect("schema" in task, "the task is neither a replay task nor a reference task")
    return task


def load_task(path):
    with open(path, "r", encoding="utf-8") as handle:
        return normalize_task(json.load(handle))


def main(argv):
    if len(argv) < 2:
        sys.stderr.write("usage: replay_verifier.py <task.json> <completion file>...\n")
        return 2
    # genesis names the candidate {completion_file} in its suite probes and {output_file} in its
    # dataset-derived probes, and leaves the other placeholder as literal text: take the one that exists.
    candidates = [p for p in argv[1:] if os.path.isfile(p)]
    if not candidates:
        sys.stderr.write("replay_verifier: none of %r is a file\n" % (argv[1:],))
        return 2
    try:
        task = load_task(argv[0])
        with open(candidates[0], "r", encoding="utf-8", errors="replace") as handle:
            completion = handle.read()
        accepted, why = verdict(task, completion)
    except (OSError, ValueError, AttributeError, KeyError, TypeError) as exc:
        sys.stderr.write("replay_verifier: malformed task: %s\n" % exc)
        return 2
    sys.stderr.write("replay_verifier: %s\n" % why)
    print(json.dumps({"pass": accepted, "score": 1.0 if accepted else 0.0, "reward": 1 if accepted else 0}))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
