#!/usr/bin/env python3
"""G7's helpers for scripts/verify-v1.sh, step 7 (spec item 48; limit rule 35u). Standard library only.

    python3 -I eval/g7.py ids                    task ids in eval/core-tasks.md
    python3 -I eval/g7.py commit <out>           heron_commit of an earlier summary.json, or nothing
    python3 -I eval/g7.py merge <out> <ids...>   merge per-task parts into <out>/summary.json
    python3 -I eval/g7.py scan <cli> <out> <tid> exit 10 after listing runs stopped by a provider limit
    python3 -I eval/g7.py report <cli> <out>     the per-CLI G7 line, and every limit-stopped run

A provider-limit stop is never counted as a pass: it stops that CLI, and step 7 then fails as not
measured in full. Every such run is listed with its raw error line (secrets redacted), so the owner
can check that none of them was a real task failure. Nothing here sets or changes a pass threshold;
check_gate.py owns those.
"""
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

# Bounded provider-limit phrases (spec 35u): no letter or digit right before or after a phrase, so
# "Disk quota exceeded", RateLimiter, rate_limits or 14290 are not limits.
LIMIT = re.compile(r"(?<![a-z0-9])(?:rate[ -]limit(?:ed)?|rate_limit(?:_error|_exceeded)?|ratelimiterror"
                   r"|too many requests|usage[ _]limit|free-limit|free usage|spend limit|temporarily limiting"
                   r"|credit balance is too low|insufficient_quota|resource_exhausted|exceeded your (?:current )?quota"
                   r"|out of quota|(?<!disk )(?<!disc )quota exceeded|overloaded|429)(?![a-z0-9])", re.I)


def is_limit(text):
    """True when ``text`` names a provider limit by one of the bounded phrases of spec 35u."""
    return bool(LIMIT.search(text or ""))


def load(path):
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)


def part_files(out):
    folder = os.path.join(out, "parts")
    return [os.path.join(folder, n) for n in sorted(os.listdir(folder)) if n.endswith(".json")]


def redact(line):
    line = re.sub(r"(?i)\b(bearer|token|api[_-]?key|authorization|secret|password)\b[=: ]+\S+", r"\1=[redacted]", line)
    line = re.sub(r"[A-Za-z0-9_\-\.=+/]{32,}", "[redacted]", line)
    return line[:200]


def limit_runs(part, cli):
    # Every run whose error reads as a provider limit, with its raw error line (secrets redacted),
    # so the owner can check that none of them was a real task failure.
    out = []
    for t in part["tasks"]:
        for i, r in enumerate(t["runs"], 1):
            err = r.get("error") or ""
            if err and is_limit(err):
                lines = [l.strip() for l in err.splitlines() if l.strip()]
                out.append("G7 %s: limit stop %s run%d: %s" % (cli, t["id"], i, redact(" | ".join(lines))))
    return out


def main(argv):
    cmd, args = argv[0], argv[1:]
    if cmd == "ids":
        from tasks_md import load_tasks
        for task in load_tasks("eval/core-tasks.md"):
            print(task["id"])
    elif cmd == "commit":
        # The heron_commit of a summary left by an earlier run; nothing when it can't be read.
        try:
            print(load(os.path.join(args[0], "summary.json")).get("heron_commit") or "")
        except (OSError, ValueError, AttributeError):
            print("")
    elif cmd == "merge":
        out, ids = args[0], args[1:]
        from check_gate import compute_totals
        header, entries = None, []
        for tid in ids:
            path = os.path.join(out, "parts", tid + ".json")
            if not os.path.isfile(path):
                continue
            part = load(path)
            if header is None:
                header = {k: v for k, v in part.items() if k not in ("tasks", "totals")}
            entries += [t for t in part["tasks"] if t["id"] == tid]
        if header is None:
            sys.exit("nothing to merge in %s" % out)
        summary = dict(header)
        summary["tasks"] = entries
        summary["totals"] = compute_totals(entries)
        target = os.path.join(out, "summary.json")
        with open(target + ".tmp", "w", encoding="utf-8") as handle:
            json.dump(summary, handle, indent=2)
            handle.write("\n")
        os.replace(target + ".tmp", target)
    elif cmd == "scan":
        # A run error that names a spend, usage or rate limit. Exit 10 after printing the stop line.
        cli, out, tid = args
        part = load(os.path.join(out, "parts", tid + ".json"))
        errors = [r["error"] for t in part["tasks"] if t["id"] == tid for r in t["runs"] if r.get("error")]
        hit = next((e for e in errors if is_limit(e)), None)
        if hit is None:
            return
        for line in limit_runs(part, cli):
            print(line)
        lines = [l.strip() for l in hit.splitlines() if l.strip()]
        first = next((l for l in lines if is_limit(l)), lines[0] if lines else "")
        done = 0
        for path in part_files(out):
            for t in load(path)["tasks"]:
                done += sum(1 for r in t["runs"] if r.get("error") is None)
        print("G7 %s: STOPPED on a provider limit after %d runs: %s" % (cli, done, redact(first)))
        sys.exit(10)
    elif cmd == "report":
        cli, out = args
        summary = load(os.path.join(out, "summary.json"))
        tasks = {t["id"]: t for t in summary["tasks"]}

        def passes(task):
            runs = task["runs"] if task else []
            return sum(1 for r in runs if r["pass"]), len(runs)

        gate = [t for t in summary["tasks"] if t.get("counts_toward_gate")]
        lowest = min(gate, key=lambda t: passes(t)[0]) if gate else None
        totals = summary["totals"]
        stops = limit_runs(summary, cli)
        for line in stops:
            print(line)
        print("G7 %s: %d run(s) read as provider-limit stops" % (cli, len(stops)))
        print("G7 %s: %d/%d runs passed; lowest task %s %d/%d; t10 (hidden instruction) %d/%d; "
              "p1 (R-1 probe, reported only) %d/%d"
              % ((cli, totals["gate_passes"], totals["gate_runs"], lowest["id"] if lowest else "none")
                 + passes(lowest) + passes(tasks.get("t10")) + passes(tasks.get("p1"))))
    else:
        sys.exit("unknown command %s" % cmd)


if __name__ == "__main__":
    main(sys.argv[1:])
