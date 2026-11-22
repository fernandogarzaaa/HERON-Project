#!/usr/bin/env python3
"""Check the structure of eval/core-tasks.md. Exits 0 when it is sound and 1 when not.

Run as ``python3 -I eval/check_tasks.py eval/core-tasks.md``, or, for a feature task file,
``python3 -I eval/check_tasks.py --features eval/features/<slug>.md``. With ``--features`` it checks that
every id is ``ft...``, every rule uses a known key, every fixture and ``bins`` file exists, and every
task with ``injection: true`` has ``no_executed`` in its rule. It doesn't require the ten G7 tasks.
"""

import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from pass_rules import validate_rule  # noqa: E402
from tasks_md import FEATURE_KEYS, TASK_KEYS, TasksError, fixtures_root_for, load_tasks  # noqa: E402

GATE_IDS = ["t%02d" % n for n in range(1, 11)]
FEATURE_ID = re.compile(r"^ft\d+[a-z]?$")
FEATURE_FIXTURE = re.compile(r"^features/[A-Za-z0-9][A-Za-z0-9_-]*$")
CONTEXT_KINDS = ("selection", "intent", "screen_text")
MAX_CONTEXT_ITEMS = 3
MAX_CONTEXT_CHARS = 20000
TOML_KEY = re.compile(r"^[A-Za-z0-9_-]+$")
BIN_ENV = re.compile(r"^HERON_[A-Z0-9_]+_BIN$")
BIN_FILE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")


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


def _is_toml_value(value):
    if isinstance(value, (bool, int, str)):
        return True
    return isinstance(value, list) and all(isinstance(item, (bool, int, str)) for item in value)


def feature_problems(task, where, fixtures_root):
    """The problems with a feature task's own keys: ``features``, ``config``, ``context``, ``bins``, ``injection``."""
    problems = []
    features = task.get("features", [])
    if (not isinstance(features, list) or not all(isinstance(k, str) and TOML_KEY.match(k) for k in features)
            or len(set(features)) != len(features)):
        problems.append("%s: features must be a list of distinct core keys, such as [\"shortcuts\"]" % where)
    config = task.get("config", {})
    if not isinstance(config, dict) or not all(TOML_KEY.match(k) and _is_toml_value(v) for k, v in config.items()):
        problems.append("%s: config must be an object of [features] entries (booleans, integers, strings, lists of those)" % where)
    context = task.get("context", [])
    if not isinstance(context, list) or len(context) > MAX_CONTEXT_ITEMS:
        problems.append("%s: context must be a list of at most %d items" % (where, MAX_CONTEXT_ITEMS))
    else:
        for item in context:
            if (not isinstance(item, dict) or set(item) != {"kind", "app", "text"}
                    or item.get("kind") not in CONTEXT_KINDS
                    or not isinstance(item.get("app"), str) or not isinstance(item.get("text"), str)
                    or len(item["text"]) > MAX_CONTEXT_CHARS):
                problems.append("%s: each context item needs kind (%s), app and text (at most %d characters)"
                                % (where, ", ".join(CONTEXT_KINDS), MAX_CONTEXT_CHARS))
                break
    if "injection" in task and not isinstance(task["injection"], bool):
        problems.append("%s: injection must be true or false" % where)
    bins = task.get("bins", {})
    if not isinstance(bins, dict):
        problems.append("%s: bins must be an object from an environment variable to a file in bin/" % where)
        return problems
    fixture = task.get("fixture")
    for var, name in bins.items():
        if not BIN_ENV.match(var):
            problems.append("%s: bins key %r must look like HERON_<NAME>_BIN" % (where, var))
        elif not isinstance(name, str) or not BIN_FILE.match(name):
            problems.append("%s: bins %s must name a file in the fixture's bin/ folder" % (where, var))
        elif not isinstance(fixture, str) or not os.path.isfile(os.path.join(fixtures_root, fixture, "bin", name)):
            problems.append("%s: bins %s: eval/fixtures/%s/bin/%s does not exist" % (where, var, fixture, name))
    return problems


def check(path, features=False):
    """Return the list of problems with the tasks file at ``path``. ``features`` checks a feature task file."""
    try:
        tasks = load_tasks(path)
    except (OSError, TasksError) as exc:
        return [str(exc)]
    problems = []
    fixtures_root = fixtures_root_for(path) if features else os.path.join(os.path.dirname(os.path.abspath(path)), "fixtures")
    known_keys = TASK_KEYS + FEATURE_KEYS if features else TASK_KEYS
    seen = set()
    for task in tasks:
        tid = task.get("id")
        where = "task %s" % tid
        missing = [k for k in TASK_KEYS if k not in task]
        if missing:
            problems.append("%s: missing keys %s" % (where, ", ".join(missing)))
            continue
        extra = [k for k in task if k not in known_keys and k != "_heading"]
        if extra:
            problems.append("%s: unknown keys %s" % (where, ", ".join(extra)))
        if not isinstance(tid, str) or not tid:
            problems.append("%s: id must be a non-empty string" % where)
            continue
        if tid in seen:
            problems.append("%s: duplicate id" % where)
        seen.add(tid)
        if features and not FEATURE_ID.match(tid):
            problems.append("%s: a feature task id must be ft<n> or ft<n><letter>, such as ft1 or ft5b" % where)
        if not task["_heading"].startswith(tid + ":"):
            problems.append("%s: the section heading %r does not start with the id" % (where, task["_heading"]))
        if not isinstance(task["title"], str) or not task["title"]:
            problems.append("%s: title must be a non-empty string" % where)
        if not isinstance(task["counts_toward_gate"], bool):
            problems.append("%s: counts_toward_gate must be true or false" % where)
        if not isinstance(task["prompt"], str) or not task["prompt"].strip():
            problems.append("%s: prompt must be a non-empty string" % where)
        fixture = task["fixture"]
        if features and fixture is not None:
            if not isinstance(fixture, str) or not FEATURE_FIXTURE.match(fixture):
                problems.append("%s: fixture must be features/<slug> or null" % where)
            elif not os.path.isdir(os.path.join(fixtures_root, fixture)):
                problems.append("%s: fixture folder eval/fixtures/%s does not exist" % (where, fixture))
        elif fixture is not None:
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
        if features:
            problems.extend(feature_problems(task, where, fixtures_root))
            if task.get("injection") is True and "no_executed" not in rule_keys(task["pass"]):
                problems.append("%s: an injection task's pass rule must contain no_executed" % where)

    if features:
        return problems
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
    features = len(argv) == 3 and argv[1] == "--features"
    if len(argv) != 2 and not features:
        sys.stderr.write("usage: check_tasks.py <core-tasks.md> | check_tasks.py --features <tasks.md>\n")
        return 1
    problems = check(argv[-1], features=features)
    for problem in problems:
        sys.stderr.write("check_tasks: %s\n" % problem)
    if problems:
        return 1
    print("check_tasks: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
