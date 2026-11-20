#!/usr/bin/env python3
"""A strict verifier for G7's provider-limit classifier, for ``genesis audit-evaluator --suite json``
(spec 35u). Standard library only.

A G7 run whose error reads as a provider limit is excluded from the score (step 7 stops that CLI and
fails as not measured in full), so the classifier must never read an ordinary failure as a limit.

Two task shapes reach this verifier.

* **A limit task** (the dataset, ``limit-cases.jsonl``, built from ``eval/fixtures/g7_limit_samples.json``):
  ``{"error": "<raw error line>"}``. The completion must be exactly ``{"limit": <bool>}`` (strict
  parse: no duplicate keys, no prose, no extra field, a real boolean), and it is accepted only when
  that boolean equals ``g7.is_limit(error)``. What the completion claims never decides on its own.
* **A reference task** (genesis's own ``json`` suite probes): handled by ``json_verifier``'s strict
  parse, schema and exact-match check, so the suite's probes exercise the same parser.

Run as ``python3 -I eval/genesis/limit_verifier.py <task.json> <completion file>...``. Exit 0 with
``{"pass", "score", "reward"}`` on stdout, 2 when the task file is malformed.
"""

import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
EVAL = os.path.dirname(HERE)
sys.path.insert(0, EVAL)
sys.path.insert(0, HERE)

import g7  # noqa: E402
import json_verifier as jv  # noqa: E402

ENVELOPE_KEYS = jv.ENVELOPE_KEYS


def limit_verdict(error, completion):
    try:
        value = jv.strict_loads(completion)
        if not isinstance(value, dict) or set(value) != {"limit"}:
            raise jv.Reject("the completion must be exactly {\"limit\": <bool>}")
        if not isinstance(value["limit"], bool):
            raise jv.Reject("limit must be a boolean")
        truth = g7.is_limit(error)
        if value["limit"] is not truth:
            raise jv.Reject("the classifier reads this error as %s" % ("a limit" if truth else "not a limit"))
    except jv.Reject as exc:
        return False, "rejected: %s" % exc
    return True, "accepted"


def normalize_task(raw):
    if not isinstance(raw, dict):
        raise ValueError("the task must be a JSON object")
    candidates = [raw, raw.get("context")]
    if isinstance(raw.get("input"), str):
        try:
            candidates.append(json.loads(raw["input"]))
        except ValueError:
            pass
    for candidate in candidates:
        if isinstance(candidate, dict) and "error" in candidate:
            allowed = {"error"} | (ENVELOPE_KEYS if candidate is raw else set())
            extra = sorted(set(candidate) - allowed)
            if extra:
                raise ValueError("the task has unknown keys: %s" % ", ".join(extra))
            if not isinstance(candidate["error"], str):
                raise ValueError("a limit task's error must be text")
            return {"error": candidate["error"]}
    return jv.normalize_task(raw)


def main(argv):
    if len(argv) < 2:
        sys.stderr.write("usage: limit_verifier.py <task.json> <completion file>...\n")
        return 2
    candidates = [p for p in argv[1:] if os.path.isfile(p)]
    if not candidates:
        sys.stderr.write("limit_verifier: none of %r is a file\n" % (argv[1:],))
        return 2
    try:
        with open(argv[0], "r", encoding="utf-8") as handle:
            task = normalize_task(json.load(handle))
        with open(candidates[0], "r", encoding="utf-8", errors="replace") as handle:
            completion = handle.read()
        if "error" in task:
            accepted, why = limit_verdict(task["error"], completion)
        else:
            accepted, why = jv.verdict(task, completion)
    except (OSError, ValueError, AttributeError, KeyError, TypeError) as exc:
        sys.stderr.write("limit_verifier: malformed task: %s\n" % exc)
        return 2
    sys.stderr.write("limit_verifier: %s\n" % why)
    print(json.dumps({"pass": accepted, "score": 1.0 if accepted else 0.0, "reward": 1 if accepted else 0}))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
