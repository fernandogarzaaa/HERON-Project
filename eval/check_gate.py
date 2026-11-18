#!/usr/bin/env python3
"""Judge a ``summary.json`` written by ``run_core_tasks.py`` (U0, item 5).

Three modes, each exiting 0 on pass, 1 on fail and 2 on a malformed input:

  check_gate.py --schema-only <summary>    validate schema 1
  check_gate.py <summary>                  the G7 task gate
  check_gate.py --spike <envelope> <hybrid> (--report <file> | --write-report <file>)
                                           the topology decision of design section 2

A file that is unreadable or is not a JSON object is always malformed (2). A summary that breaks
schema 1 fails ``--schema-only`` (1) and is malformed (2) in the other two modes, which can't
judge it. Standard library only.
"""

import argparse
import json
import os
import re
import statistics
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

GATE_IDS = ["t%02d" % n for n in range(1, 11)]
G7_RUNS_PER_TASK = 3
G7_MIN_PASSES = 26  # of 30
G7_MIN_PER_TASK = 2  # of 3
SPIKE_RUNS_PER_TASK = 5
SPIKE_MIN_LEAD = 4  # runs of 50
SPIKE_MAX_TOKEN_PERCENT = 105
CHOSEN = re.compile(r"^Chosen default: topology=(envelope|hybrid)$")
FIELD_NAMES = ("runs_per_task", "driver", "mode", "build", "provider")


class Malformed(Exception):
    """The input is not a usable summary."""


# ----------------------------------------------------------------------------- computing

def median_value(values):
    if not values:
        return None
    m = statistics.median(values)
    return int(m) if float(m).is_integer() else m


def compute_totals(tasks):
    """The ``totals`` object for a list of task entries: gate runs only, errored runs left out of medians."""
    gate_runs = gate_passes = 0
    ttfts, tokens = [], []
    for task in tasks:
        if not task.get("counts_toward_gate"):
            continue
        for run in task.get("runs", []):
            gate_runs += 1
            gate_passes += 1 if run.get("pass") else 0
            if run.get("error") is None:
                tokens.append(run["tokens"])
                if run.get("ttft_ms") is not None:
                    ttfts.append(run["ttft_ms"])
    return {"gate_runs": gate_runs, "gate_passes": gate_passes,
            "median_ttft_ms": median_value(ttfts), "median_tokens_per_task": median_value(tokens)}


# ----------------------------------------------------------------------------- schema

def _is_int(value):
    return isinstance(value, int) and not isinstance(value, bool)


def _is_number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def schema_problems(summary):
    """Every way ``summary`` breaks schema 1; an empty list means it is valid."""
    problems = []
    if summary.get("schema") != 1 or not _is_int(summary.get("schema")):
        problems.append("schema must be 1")
    for key in ("heron_commit", "provider"):
        if not isinstance(summary.get(key), str) or not summary.get(key):
            problems.append("%s must be a non-empty string" % key)
    for key in ("cli_version", "mode"):
        if not isinstance(summary.get(key), str):
            problems.append("%s must be a string" % key)
    for key, allowed in (("topology", ("envelope", "hybrid")), ("driver", ("eve", "direct")),
                         ("build", ("debug", "release"))):
        if summary.get(key) not in allowed:
            problems.append("%s must be one of %s" % (key, ", ".join(allowed)))
    if not _is_int(summary.get("runs_per_task")) or summary.get("runs_per_task") < 1:
        problems.append("runs_per_task must be a positive integer")
    tasks = summary.get("tasks")
    if not isinstance(tasks, list) or not tasks:
        problems.append("tasks must be a non-empty list")
        tasks = []
    seen = set()
    for task in tasks:
        if not isinstance(task, dict):
            problems.append("each task must be an object")
            continue
        tid = task.get("id")
        where = "task %s" % tid
        if not isinstance(tid, str) or not tid:
            problems.append("a task has no id")
            continue
        if tid in seen:
            problems.append("%s appears twice" % where)
        seen.add(tid)
        if not isinstance(task.get("counts_toward_gate"), bool):
            problems.append("%s: counts_toward_gate must be true or false" % where)
        runs = task.get("runs")
        if not isinstance(runs, list):
            problems.append("%s: runs must be a list" % where)
            continue
        numbers = set()
        for run in runs:
            if not isinstance(run, dict):
                problems.append("%s: each run must be an object" % where)
                continue
            problems.extend(_run_problems(where, run))
            if _is_int(run.get("run")):
                if run["run"] in numbers:
                    problems.append("%s: run %d appears twice" % (where, run["run"]))
                numbers.add(run["run"])
    totals = summary.get("totals")
    if not isinstance(totals, dict):
        problems.append("totals must be an object")
    else:
        for key in ("gate_runs", "gate_passes"):
            if not _is_int(totals.get(key)) or totals[key] < 0:
                problems.append("totals.%s must be a non-negative integer" % key)
        for key in ("median_ttft_ms", "median_tokens_per_task"):
            if key not in totals or (totals[key] is not None and not _is_number(totals[key])):
                problems.append("totals.%s must be a number or null" % key)
        if not problems:
            expected = compute_totals(tasks)
            for key, value in expected.items():
                if totals.get(key) != value:
                    problems.append("totals.%s is %r but the runs give %r" % (key, totals.get(key), value))
    return problems


def _run_problems(where, run):
    problems = []
    label = "%s run %s" % (where, run.get("run"))
    if not _is_int(run.get("run")) or run.get("run") < 1:
        problems.append("%s: run must be a positive integer" % where)
    if not isinstance(run.get("pass"), bool):
        problems.append("%s: pass must be true or false" % label)
    failed = run.get("failed_rules")
    if not isinstance(failed, list) or not all(isinstance(f, str) for f in failed):
        problems.append("%s: failed_rules must be a list of strings" % label)
    if not isinstance(run.get("reply"), str):
        problems.append("%s: reply must be a string" % label)
    for key in ("tokens", "asks"):
        if not _is_int(run.get(key)) or run.get(key) < 0:
            problems.append("%s: %s must be a non-negative integer" % (label, key))
    ttft = run.get("ttft_ms")
    if ttft is not None and (not _is_number(ttft) or ttft < 0):
        problems.append("%s: ttft_ms must be a non-negative number or null" % label)
    if "ttft_ms" not in run:
        problems.append("%s: ttft_ms is missing" % label)
    error = run.get("error", False)
    if error is not None and not isinstance(error, str):
        problems.append("%s: error must be a string or null" % label)
    if run.get("pass") is True and isinstance(run.get("error"), str):
        problems.append("%s: a run with an error can't pass" % label)
    return problems


# ----------------------------------------------------------------------------- gate

def gate_tasks(summary):
    return [t for t in summary["tasks"] if t["counts_toward_gate"]]


def task_stats(task):
    runs = task["runs"]
    clean = [r for r in runs if r["error"] is None]
    return {"id": task["id"], "passes": sum(1 for r in runs if r["pass"]), "runs": len(runs),
            "errors": len(runs) - len(clean),
            "ttft": median_value([r["ttft_ms"] for r in clean if r["ttft_ms"] is not None]),
            "tokens": median_value([r["tokens"] for r in clean]),
            "asks": median_value([r["asks"] for r in clean])}


def format_table(summary):
    rows = [task_stats(t) for t in gate_tasks(summary)]
    lines = ["| task | passes | median ttft (ms) | median tokens | median asks |", "|---|---|---|---|---|"]
    for row in rows:
        lines.append("| %s | %d/%d | %s | %s | %s |" % (row["id"], row["passes"], row["runs"],
                                                        _fmt(row["ttft"]), _fmt(row["tokens"]), _fmt(row["asks"])))
    totals = summary["totals"]
    lines.append("| all | %d/%d | %s | %s | |" % (totals["gate_passes"], totals["gate_runs"],
                                                 _fmt(totals["median_ttft_ms"]), _fmt(totals["median_tokens_per_task"])))
    return "\n".join(lines)


def _fmt(value):
    return "-" if value is None else str(value)


def require_fields(summary, required):
    """The first field of ``required`` (an ordered list of ``(name, value)``) that differs, as a message."""
    for name, wanted in required:
        got = summary.get(name)
        if wanted is None:  # "anything but the demo model"
            if got == "demo":
                return "%s must not be 'demo'" % name
        elif got != wanted:
            return "%s must be %r, got %r" % (name, wanted, got)
    return None


def judge_gate(summary):
    """Apply the G7 rule. Returns ``(passed, messages)``."""
    bad = require_fields(summary, [("runs_per_task", G7_RUNS_PER_TASK), ("driver", "eve"), ("mode", "ask"),
                                   ("build", "release"), ("provider", None)])
    if bad:
        return False, [bad]
    messages = []
    tasks = gate_tasks(summary)
    ids = sorted(t["id"] for t in tasks)
    if ids != GATE_IDS:
        return False, ["the gate tasks must be t01 to t10, got %s" % (", ".join(ids) or "none")]
    passed = True
    total_passes = total_runs = 0
    for task in tasks:
        stats = task_stats(task)
        if stats["runs"] != G7_RUNS_PER_TASK:
            passed = False
            messages.append("%s has %d runs, not %d" % (task["id"], stats["runs"], G7_RUNS_PER_TASK))
        if stats["passes"] < G7_MIN_PER_TASK:
            passed = False
            messages.append("%s passed %d of %d runs; every task needs at least %d"
                            % (task["id"], stats["passes"], stats["runs"], G7_MIN_PER_TASK))
        total_passes += stats["passes"]
        total_runs += stats["runs"]
    if total_passes < G7_MIN_PASSES:
        passed = False
        messages.append("%d of %d runs passed; the gate needs at least %d of %d"
                        % (total_passes, total_runs, G7_MIN_PASSES, G7_RUNS_PER_TASK * len(GATE_IDS)))
    return passed, messages


# ----------------------------------------------------------------------------- spike

def spike_arm_problems(summary, topology):
    """Why ``summary`` can't be one arm of the spike; empty when it can."""
    problems = []
    bad = require_fields(summary, [("runs_per_task", SPIKE_RUNS_PER_TASK), ("build", "debug"),
                                   ("provider", "claude-cli"), ("topology", topology)])
    if bad:
        problems.append(bad)
    tasks = summary["tasks"]
    ids = sorted(t["id"] for t in tasks)
    if ids != GATE_IDS or any(not t["counts_toward_gate"] for t in tasks):
        problems.append("the arm must hold only the gate tasks t01 to t10, got %s" % (", ".join(ids) or "none"))
    else:
        runs = sum(len(t["runs"]) for t in tasks)
        if runs != SPIKE_RUNS_PER_TASK * len(GATE_IDS) or any(len(t["runs"]) != SPIKE_RUNS_PER_TASK for t in tasks):
            problems.append("each task needs %d runs (%d per arm), got %d runs"
                            % (SPIKE_RUNS_PER_TASK, SPIKE_RUNS_PER_TASK * len(GATE_IDS), runs))
    return problems


def decide_topology(envelope, hybrid):
    """Apply design section 2's rule. Returns ``(topology, reason)``."""
    e_pass = envelope["totals"]["gate_passes"]
    h_pass = hybrid["totals"]["gate_passes"]
    lead = h_pass - e_pass
    e_tokens = envelope["totals"]["median_tokens_per_task"]
    h_tokens = hybrid["totals"]["median_tokens_per_task"]
    if lead < SPIKE_MIN_LEAD:
        return "envelope", ("Hybrid passed %d of 50 runs and the envelope %d, a lead of %d. Hybrid needs a lead of "
                            "at least %d, so the result is inconclusive and the envelope stays."
                            % (h_pass, e_pass, lead, SPIKE_MIN_LEAD))
    if e_tokens is None or h_tokens is None:
        return "envelope", ("Hybrid leads by %d runs, but a median tokens figure is missing, so the token cost "
                            "can't be checked and the envelope stays." % lead)
    if h_tokens * 100 > e_tokens * SPIKE_MAX_TOKEN_PERCENT:
        return "envelope", ("Hybrid leads by %d runs but its median tokens per task, %s, is more than 5%% above "
                            "the envelope's %s, so the envelope stays." % (lead, h_tokens, e_tokens))
    return "hybrid", ("Hybrid leads by %d runs (%d against %d of 50) and its median tokens per task, %s, is within "
                      "5%% of the envelope's %s." % (lead, h_pass, e_pass, h_tokens, e_tokens))


def render_report(envelope, hybrid, topology, reason):
    parts = ["Chosen default: topology=%s" % topology, "", "## Envelope arm", "", format_table(envelope), "",
             "## Hybrid arm", "", format_table(hybrid), "", "## Reason", "", reason, ""]
    return "\n".join(parts)


def report_choice(text):
    """The topology named by the report's ``Chosen default:`` line. The line must be the first one."""
    lines = text.splitlines()
    matches = [CHOSEN.match(line) for line in lines if line.startswith("Chosen default:")]
    if len(matches) != 1 or matches[0] is None:
        raise ValueError("the report needs exactly one line 'Chosen default: topology=<envelope|hybrid>'")
    first = next((line for line in lines if line.strip()), "")
    if not first.startswith("Chosen default:"):
        raise ValueError("the 'Chosen default:' line must come first")
    return matches[0].group(1)


# ----------------------------------------------------------------------------- main

def load_summary(path):
    try:
        with open(path, "r", encoding="utf-8") as handle:
            data = json.load(handle)
    except (OSError, ValueError) as exc:
        raise Malformed("%s: %s" % (path, exc))
    if not isinstance(data, dict):
        raise Malformed("%s: a summary must be a JSON object" % path)
    return data


def load_valid(path):
    data = load_summary(path)
    problems = schema_problems(data)
    if problems:
        raise Malformed("%s breaks schema 1: %s" % (path, "; ".join(problems)))
    return data


def parse_args(argv):
    parser = argparse.ArgumentParser(prog="check_gate.py", description=__doc__.split("\n\n")[0])
    parser.add_argument("summary", nargs="?", help="summary.json to judge against the G7 rule")
    parser.add_argument("--schema-only", metavar="SUMMARY", help="only validate schema 1")
    parser.add_argument("--spike", nargs=2, metavar=("ENVELOPE", "HYBRID"), help="decide the topology")
    parser.add_argument("--report", metavar="FILE", help="with --spike: check this report's Chosen default line")
    parser.add_argument("--write-report", metavar="FILE", help="with --spike: write the report")
    return parser.parse_args(argv)


def main(argv):
    args = parse_args(argv)
    modes = [bool(args.schema_only), bool(args.spike), bool(args.summary)]
    if sum(modes) != 1 or ((args.report or args.write_report) and not args.spike):
        sys.stderr.write("check_gate: use exactly one of --schema-only, --spike or a summary path\n")
        return 2
    try:
        if args.schema_only:
            data = load_summary(args.schema_only)
            problems = schema_problems(data)
            for problem in problems:
                sys.stderr.write("check_gate: schema: %s\n" % problem)
            if problems:
                return 1
            print("check_gate: schema 1 ok")
            return 0
        if args.spike:
            return run_spike(args)
        data = load_valid(args.summary)
        passed, messages = judge_gate(data)
        for message in messages:
            sys.stderr.write("check_gate: %s\n" % message)
        if messages and messages[0].split()[0] in FIELD_NAMES:
            print("check_gate: G7 FAILED: the summary was not made the way the gate requires")
            return 1
        print(format_table(data))
        print("check_gate: G7 %s (%d of %d runs)" % ("passed" if passed else "FAILED", data["totals"]["gate_passes"],
                                                    data["totals"]["gate_runs"]))
        return 0 if passed else 1
    except Malformed as exc:
        sys.stderr.write("check_gate: malformed input: %s\n" % exc)
        return 2


def run_spike(args):
    if bool(args.report) == bool(args.write_report):
        sys.stderr.write("check_gate: --spike needs exactly one of --report or --write-report\n")
        return 2
    envelope = load_valid(args.spike[0])
    hybrid = load_valid(args.spike[1])
    problems = ["envelope arm: %s" % p for p in spike_arm_problems(envelope, "envelope")]
    problems += ["hybrid arm: %s" % p for p in spike_arm_problems(hybrid, "hybrid")]
    if envelope["heron_commit"] != hybrid["heron_commit"]:
        problems.append("the arms ran different commits (%s and %s)" % (envelope["heron_commit"], hybrid["heron_commit"]))
    if problems:
        for problem in problems:
            sys.stderr.write("check_gate: %s\n" % problem)
        return 1
    topology, reason = decide_topology(envelope, hybrid)
    if args.write_report:
        with open(args.write_report, "w", encoding="utf-8") as handle:
            handle.write(render_report(envelope, hybrid, topology, reason))
        print("check_gate: wrote %s (topology=%s)" % (args.write_report, topology))
        return 0
    try:
        with open(args.report, "r", encoding="utf-8") as handle:
            text = handle.read()
    except OSError as exc:
        raise Malformed("%s: %s" % (args.report, exc))
    try:
        chosen = report_choice(text)
    except ValueError as exc:
        sys.stderr.write("check_gate: %s: %s\n" % (args.report, exc))
        return 1
    if chosen != topology:
        sys.stderr.write("check_gate: the report says topology=%s but the summaries give topology=%s (%s)\n"
                         % (chosen, topology, reason))
        return 1
    print("check_gate: report agrees, topology=%s" % topology)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
