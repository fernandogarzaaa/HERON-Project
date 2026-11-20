#!/usr/bin/env python3
"""Write eval/genesis/limit-cases.jsonl from eval/fixtures/g7_limit_samples.json (spec 35u).

Each row: the error line as the task, its recorded class as the reference, and the wrong class plus
three malformed answers as known-bad completions. Run from the repository root after changing the
fixture: python3 -I eval/genesis/build_limit_cases.py
"""

import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
EVAL = os.path.dirname(HERE)


def main():
    with open(os.path.join(EVAL, "fixtures", "g7_limit_samples.json"), "r", encoding="utf-8") as handle:
        samples = json.load(handle)["samples"]
    rows = []
    for i, sample in enumerate(samples, 1):
        truth = sample["limit"]
        rows.append({
            "id": "limit-%02d" % i,
            "source": "g7_limit_samples.json",
            "input": json.dumps({"error": sample["text"]}),
            "reference": json.dumps({"limit": truth}),
            "bad": [
                {"completion": json.dumps({"limit": not truth}), "expect": "wrong class"},
                {"completion": json.dumps({"limit": "true" if truth else "false"}), "expect": "not a boolean"},
                {"completion": json.dumps({"limit": truth, "note": "ok"}), "expect": "extra field"},
                {"completion": "The answer is " + json.dumps({"limit": truth}), "expect": "prose around the JSON"},
            ],
        })
    with open(os.path.join(HERE, "limit-cases.jsonl"), "w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
