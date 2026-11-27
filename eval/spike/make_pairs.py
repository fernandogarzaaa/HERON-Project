#!/usr/bin/env python3
"""Write the owner's blind-rating sheet for the topology spike (work unit U2s).

    python3 -I eval/spike/make_pairs.py ENVELOPE_SUMMARY HYBRID_SUMMARY --pairs pairs.md --key pairs-key.json

One pair per gate task (10 tasks), in random order. A pair holds one final reply from each arm, picked at
random from that arm's runs that finished without an error, shown as "A" and "B" in random order with no
topology named. The key file says which arm each letter was. The randomness is seeded from the two summaries,
so the same measurements always give the same sheet and the key can never drift from it. Standard library only.
"""

import argparse
import hashlib
import json
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(os.path.dirname(HERE)))

from tasks_md import TasksError, load_tasks  # noqa: E402

TASKS_FILE = os.path.join(os.path.dirname(HERE), "core-tasks.md")
NO_REPLY = "(this arm has no run that finished without an error for this task)"


def usable_runs(task):
    return [r for r in task["runs"] if r.get("error") is None and isinstance(r.get("reply"), str)]


def build_pairs(envelope, hybrid, prompts, seed):
    """Return ``(pairs, key)``. ``pairs`` is a list of ``{n, task, prompt, A, B}``; ``key`` says which arm each was."""
    rng = random.Random(seed)
    arms = {"envelope": {t["id"]: t for t in envelope["tasks"]}, "hybrid": {t["id"]: t for t in hybrid["tasks"]}}
    ids = sorted(i for i in arms["envelope"] if i in arms["hybrid"] and arms["envelope"][i]["counts_toward_gate"])
    rng.shuffle(ids)
    pairs, key = [], []
    for n, task_id in enumerate(ids, 1):
        picked = {}
        for arm in ("envelope", "hybrid"):
            runs = usable_runs(arms[arm][task_id])
            picked[arm] = rng.choice(runs) if runs else None
        order = ["envelope", "hybrid"]
        rng.shuffle(order)
        shown = {}
        for letter, arm in zip(("A", "B"), order):
            shown[letter] = picked[arm]["reply"] if picked[arm] else NO_REPLY
        pairs.append({"n": n, "task": task_id, "prompt": prompts.get(task_id, ""), "A": shown["A"], "B": shown["B"]})
        key.append({"n": n, "task": task_id, "A": order[0], "B": order[1],
                    "envelope_run": picked["envelope"]["run"] if picked["envelope"] else None,
                    "hybrid_run": picked["hybrid"]["run"] if picked["hybrid"] else None})
    return pairs, key


def render_pairs(pairs):
    lines = ["# Blind rating: envelope vs hybrid", "",
             "For each pair, decide which final answer is better for the task, or call it a tie. "
             "Write your choice on the `Better:` line (A, B or tie). The rating is reported but decides nothing: "
             "the topology rule in the design doc uses pass counts and tokens only. "
             "Don't open `pairs-key.json` until you have rated every pair.", ""]
    for pair in pairs:
        lines += ["## Pair %d (task %s)" % (pair["n"], pair["task"]), ""]
        if pair["prompt"]:
            lines += ["Task message:", "", "> " + pair["prompt"].replace("\n", "\n> "), ""]
        for letter in ("A", "B"):
            lines += ["### Answer %s" % letter, "", "```text", pair[letter].replace("```", "'''"), "```", ""]
        lines += ["Better: ", ""]
    return "\n".join(lines)


def read_text(path):
    with open(path, "r", encoding="utf-8") as handle:
        return handle.read()


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("envelope")
    parser.add_argument("hybrid")
    parser.add_argument("--pairs", required=True)
    parser.add_argument("--key", required=True)
    parser.add_argument("--tasks-file", default=TASKS_FILE, help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    try:
        raw_e, raw_h = read_text(args.envelope), read_text(args.hybrid)
        envelope, hybrid = json.loads(raw_e), json.loads(raw_h)
        prompts = {t["id"]: t.get("prompt", "") for t in load_tasks(args.tasks_file)}
    except (OSError, ValueError, TasksError) as exc:
        sys.stderr.write("make_pairs: %s\n" % exc)
        return 2
    try:
        seed = hashlib.sha256((raw_e + "\0" + raw_h).encode("utf-8")).hexdigest()
        pairs, key = build_pairs(envelope, hybrid, prompts, seed)
    except (KeyError, TypeError, AttributeError) as exc:
        sys.stderr.write("make_pairs: a summary is not schema 1: %r\n" % (exc,))
        return 2
    if not pairs:
        sys.stderr.write("make_pairs: the arms share no gate task\n")
        return 1
    with open(args.pairs, "w", encoding="utf-8") as handle:
        handle.write(render_pairs(pairs))
    with open(args.key, "w", encoding="utf-8") as handle:
        json.dump({"seed": seed, "pairs": key}, handle, indent=2)
        handle.write("\n")
    print("make_pairs: wrote %s and %s (%d pairs)" % (args.pairs, args.key, len(pairs)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
