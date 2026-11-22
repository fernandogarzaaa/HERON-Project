#!/usr/bin/env python3
"""Check the structure of eval/core-tasks.md. Exits 0 when it is sound and 1 when not.

Run as ``python3 -I eval/check_tasks.py eval/core-tasks.md``.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from pass_rules import validate_rule  # noqa: E402
from tasks_md import TASK_KEYS, TasksError, load_tasks  # noqa: E402

GATE_IDS = ["t%02d" % n for n in range(1, 11)]


def rule_keys(rule):
    """Every rule key used anywhere in ``rule``, including inside ``all``."""
    if not isinstance(rule, dict):
        return set()
    keys = set()
    for key, body in rule.items():
        keys.add(key)
        if key == "all" and isinstance(body, list):
            for sub in body:
                keys |= rule_keys(sub)
    return keys


def check(path):
    """Return the list of problems with the tasks file at ``path``."""
    try:
        tasks = load_tasks(path)
    except (OSError, TasksError) as exc:
        return [str(exc)]
    problems = []
    fixtures_root = os.path.join(os.path.dirname(os.path.abspath(path)), "fixtures")
    seen = set()
    for task in tasks:
        tid = task.get("id")
        where = "task %s" % tid
        missing = [k for k in TASK_KEYS if k not in task]
        if missing:
            problems.append("%s: missing keys %s" % (where, ", ".join(missing)))
            continue
        extra = [k for k in task if k not in TASK_KEYS and k != "_heading"]
        if extra:
            problems.append("%s: unknown keys %s" % (where, ", ".join(extra)))
        if not isinstance(tid, str) or not tid:
            problems.append("%s: id must be a non-empty string" % where)
            continue
        if tid in seen:
            problems.append("%s: duplicate id" % where)
        seen.add(tid)
        if not task["_heading"].startswith(tid + ":"):
            problems.append("%s: the section heading %r does not start with the id" % (where, task["_heading"]))
        if not isinstance(task["title"], str) or not task["title"]:
            problems.append("%s: title must be a non-empty string" % where)
        if not isinstance(task["counts_toward_gate"], bool):
            problems.append("%s: counts_toward_gate must be true or false" % where)
        if not isinstance(task["prompt"], str) or not task["prompt"].strip():
            problems.append("%s: prompt must be a non-empty string" % where)
        fixture = task["fixture"]
        if fixture is not None:
            if not isinstance(fixture, str) or not fixture or "/" in fixture or fixture in (".", ".."):
                problems.append("%s: fixture must be a folder name or null" % where)
            elif not os.path.isdir(os.path.join(fixtures_root, fixture)):
                problems.append("%s: fixture folder eval/fixtures/%s does not exist" % (where, fixture))
        seed = task["seed_sql"]
        if not isinstance(seed, list) or not all(isinstance(s, str) for s in seed):
            problems.append("%s: seed_sql must be a list of strings" % where)
        approve = task["approve"]
        if not isinstance(approve, list):
            problems.append("%s: approve must be a list" % where)
        else:
            for item in approve:
                if (not isinstance(item, dict) or not isinstance(item.get("tool"), str)
                        or not set(item) <= {"tool", "summary_regex"}):
                    problems.append("%s: each approve entry needs a tool and an optional summary_regex" % where)
                    break
        problems.extend(validate_rule(task["pass"], "task %s pass" % tid))

    gate = [t for t in tasks if t.get("counts_toward_gate") is True]
    gate_ids = sorted(str(t.get("id")) for t in gate)
    if len(gate) != 10 or gate_ids != GATE_IDS:
        problems.append("there must be exactly ten tasks with counts_toward_gate true, ids t01 to t10; got %s"
                        % (", ".join(gate_ids) or "none"))
    if not any(t.get("counts_toward_gate") is False for t in tasks):
        problems.append("there must be at least one probe (counts_toward_gate false)")
    for t in tasks:
        tid = str(t.get("id"))
        if t.get("counts_toward_gate") is False and tid in GATE_IDS:
            problems.append("task %s: ids t01 to t10 must count toward the gate" % tid)
    t10 = next((t for t in tasks if t.get("id") == "t10"), None)
    if t10 is not None:
        keys = rule_keys(t10.get("pass"))
        for needed in ("no_request", "transcript_lacks"):
            if needed not in keys:
                problems.append("task t10: its pass rule must contain %s" % needed)
    return problems


def main(argv):
    if len(argv) != 2:
        sys.stderr.write("usage: check_tasks.py <core-tasks.md>\n")
        return 1
    problems = check(argv[1])
    for problem in problems:
        sys.stderr.write("check_tasks: %s\n" % problem)
    if problems:
        return 1
    print("check_tasks: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
