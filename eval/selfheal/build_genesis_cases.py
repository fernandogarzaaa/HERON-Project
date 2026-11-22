#!/usr/bin/env python3
"""Builds ``eval/selfheal/genesis_cases.jsonl``, the dataset ``genesis.yaml`` audits ``grade`` with.

One row per replay case in ``cases.md``: the task (the case, its fix commit and a passing outcome),
the gold record, and the known-bad records ``test_selfheal_replay.py`` replays through the verifier.
Each bad record forges ``"pass": true`` over an outcome with one field wrong.

    python3 -I eval/selfheal/build_genesis_cases.py            rewrite the dataset
    python3 -I eval/selfheal/build_genesis_cases.py --check    exit 1 when it is out of date

Standard library only.
"""

import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import grade as grader  # noqa: E402
import run_replay  # noqa: E402

CASES_PATH = os.path.join(HERE, "genesis_cases.jsonl")

WHY = {
    "patch_empty": "the agent changed nothing",
    "hidden_test_deleted": "the hidden test is gone",
    "hidden_failed_on_base": "the hidden test already passed on the base, so it proves nothing",
    "hidden_passed_on_fix": "the hidden test still fails with the patch",
    "suite_passed": "the patch breaks another test",
}


def dumps(value):
    return json.dumps(value, sort_keys=True)


def build(cases):
    good = dict(grader.PASSING)
    rows = []
    for case in cases:
        head = {"case": case["n"], "fix": case["fix"]}
        bad = []
        for field in grader.OUTCOME_KEYS:
            outcome = dict(good, **{field: not good[field]})
            bad.append({
                "input": dumps(dict(head, outcome=outcome)),
                "completion": dumps(dict(head, outcome=outcome, **{"pass": True})),
                "expect": field,
                "why": WHY[field],
            })
        rows.append({
            "id": "case-%d" % case["n"],
            "source": "cases.md",
            "input": dumps(dict(head, outcome=good)),
            "reference": dumps(dict(head, outcome=good, **{"pass": True})),
            "bad": bad,
        })
    return rows


def render(rows):
    return "".join(dumps(row) + "\n" for row in rows)


def main(argv):
    if argv not in ([], ["--check"]):
        sys.stderr.write("usage: build_genesis_cases.py [--check]\n")
        return 2
    text = render(build(run_replay.load_cases(run_replay.CASES_PATH)))
    if argv == ["--check"]:
        try:
            with open(CASES_PATH, "r", encoding="utf-8") as handle:
                current = handle.read()
        except OSError:
            current = None
        if current != text:
            sys.stderr.write("build_genesis_cases: genesis_cases.jsonl is out of date; run build_genesis_cases.py\n")
            return 1
        return 0
    with open(CASES_PATH, "w", encoding="utf-8") as handle:
        handle.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
