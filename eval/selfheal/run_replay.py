#!/usr/bin/env python3
"""The self-heal replay suite (item 20): can Heron's repair agent fix ten bugs its own history fixed?

    python3 -I eval/selfheal/run_replay.py                  replay all cases, record the result
    python3 -I eval/selfheal/run_replay.py --validate       check every hidden test on base and fix (slow)
    python3 -I eval/selfheal/run_replay.py --check-patches  check every hidden patch applies (no build)

A replay builds ``target/release/heron`` from HEAD, then for each case in ``cases.md`` checks out
``base`` in a throwaway worktree, runs ``heron selfheal replay`` on it, applies the hidden test and
grades the outcome with ``grade.grade``. It prints ``replay: N/M`` and ``auto-eligible: K``, records
them with ``heron selfheal record-replay`` and exits 0 when N reaches ``[harness2] replay_threshold``
(default 6). Standard library only. Exit 2 is a usage error.
"""

import argparse
import contextlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)

import grade as grader  # noqa: E402

CASES_PATH = os.path.join(HERE, "cases.md")
HERON = os.path.join(ROOT, "target", "release", "heron")
FIELDS = ("base", "fix", "report", "hidden", "test")
SHA = re.compile(r"[0-9a-f]{40}")
SELECTOR = re.compile(r"cargo --lib \w+|cargo --test \w+|swift \w+")
DEFAULT_THRESHOLD = 6
CASE_TIMEOUT = 3600


class CaseError(ValueError):
    """cases.md is malformed; the message says where."""


def parse_cases(text):
    """The cases in ``cases.md``, as dicts with ``n`` and the five fields. Anything else between a
    ``## case <n>`` heading and the next is an error, so a typo can't quietly drop a case."""
    blocks, current = [], None
    for number, line in enumerate(text.splitlines(), 1):
        heading = re.fullmatch(r"## case (\d+)", line)
        if heading:
            current = {"n": int(heading.group(1)), "line": number, "lines": []}
            blocks.append(current)
        elif line.startswith("## "):
            raise CaseError("line %d: unexpected heading %r" % (number, line))
        elif current is not None and line.strip():
            current["lines"].append((number, line))
    cases = []
    for index, block in enumerate(blocks, 1):
        n = block["n"]
        if n != index:
            raise CaseError("line %d: expected '## case %d', found '## case %d'" % (block["line"], index, n))
        if len(block["lines"]) != len(FIELDS):
            raise CaseError("case %d: expected %d fields (%s), found %d" % (n, len(FIELDS), ", ".join(FIELDS), len(block["lines"])))
        case = {"n": n}
        for (number, line), field in zip(block["lines"], FIELDS):
            key, sep, value = line.partition(": ")
            if key != field or not sep or not value.strip():
                raise CaseError("line %d: case %d needs '%s: <value>', found %r" % (number, n, field, line))
            case[field] = value.strip()
        for field in ("base", "fix"):
            if not SHA.fullmatch(case[field]):
                raise CaseError("case %d: %s must be 40 lowercase hex digits" % (n, field))
        if case["base"] == case["fix"]:
            raise CaseError("case %d: base and fix are the same commit" % n)
        if case["hidden"] != "eval/selfheal/cases/%d.patch" % n:
            raise CaseError("case %d: hidden must be eval/selfheal/cases/%d.patch" % (n, n))
        if not SELECTOR.fullmatch(case["test"]):
            raise CaseError("case %d: test %r is not 'cargo --lib <name>', 'cargo --test <stem>' or 'swift <Class>'" % (n, case["test"]))
        cases.append(case)
    if not cases:
        raise CaseError("no '## case <n>' headings found")
    return cases


def load_cases(path):
    with open(path, "r", encoding="utf-8") as handle:
        return parse_cases(handle.read())


def run_command(argv, cwd=None, env=None, timeout=None):
    """Run a command, returning ``(exit code, combined output)``. A command that can't start or
    outlives its timeout is exit code 127 or 124 with the reason as its output."""
    try:
        proc = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, text=True, errors="replace", timeout=timeout)
    except FileNotFoundError as exc:
        return 127, str(exc)
    except subprocess.TimeoutExpired:
        return 124, "timed out after %ss" % timeout
    return proc.returncode, proc.stdout + proc.stderr


# ----------------------------------------------------------------------------- --check-patches

def check_patches(cases, root=ROOT, git="git"):
    """Names of the cases whose hidden patch does not apply to the tree of both ``base`` and ``fix``
    (a ``(case number, which commit, why)`` tuple per failure). Each tree is read into a temporary
    index, so the checkout and its real index are never touched."""
    failures = []
    for case in cases:
        patch = os.path.join(root, case["hidden"])
        for which in ("base", "fix"):
            with tempfile.TemporaryDirectory(prefix="selfheal-index-") as tmp:
                env = dict(os.environ, GIT_INDEX_FILE=os.path.join(tmp, "index"))
                if not os.path.isfile(patch):
                    failures.append((case["n"], which, "%s does not exist" % case["hidden"]))
                    continue
                code, out = run_command([git, "-C", root, "read-tree", case[which]], env=env)
                if code == 0:
                    code, out = run_command([git, "-C", root, "apply", "--cached", "--check", patch], env=env)
                if code != 0:
                    failures.append((case["n"], which, out.strip().splitlines()[-1] if out.strip() else "exit %d" % code))
    return failures


# ----------------------------------------------------------------------------- running a hidden test

def test_command(selector, cargo="cargo"):
    kind, *rest = selector.split()
    if kind == "swift":
        return [os.environ.get("SWIFT", "swift"), "test", "--package-path", "macos/HeronMac", "--filter", rest[0]]
    flag, name = rest
    return [cargo, "test", "--locked", flag, name]


def selector_status(selector, tree, env, cargo="cargo"):
    """``passed``, ``failed``, ``missing`` (the build worked and no test matched) or ``build_error``.
    A cargo filter that matches nothing exits 0, so the counts in the output are what decide."""
    code, out = run_command(test_command(selector, cargo), cwd=tree, env=env, timeout=CASE_TIMEOUT)
    if selector.startswith("swift"):
        ran = re.search(r"Executed (\d+) tests?, with (\d+) failures?", out)
        if not ran:
            return "build_error" if code else "missing"
        if int(ran.group(1)) == 0:
            return "missing"
        return "passed" if code == 0 and int(ran.group(2)) == 0 else "failed"
    if re.search(r"could not compile|error(\[E\d+\])?: ", out) and "test result:" not in out:
        return "build_error"
    results = re.findall(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed", out)
    if any(verdict == "FAILED" or int(failed) for verdict, _, failed in results):
        return "failed"
    if code == 0 and any(int(passed) for _, passed, _ in results):
        return "passed"
    return "build_error" if code else "missing"


@contextlib.contextmanager
def worktree(commit, root=ROOT, git="git"):
    """A detached checkout of ``commit`` in a temporary folder, removed afterwards."""
    parent = tempfile.mkdtemp(prefix="selfheal-")
    tree = os.path.join(parent, "tree")
    code, out = run_command([git, "-C", root, "worktree", "add", "--detach", tree, commit])
    if code != 0:
        shutil.rmtree(parent, ignore_errors=True)
        raise RuntimeError("git worktree add %s failed: %s" % (commit[:12], out.strip()))
    try:
        yield tree
    finally:
        run_command([git, "-C", root, "worktree", "remove", "--force", tree])
        shutil.rmtree(parent, ignore_errors=True)


def apply_hidden(case, tree, root=ROOT, git="git"):
    """Apply the case's hidden test to ``tree``. False when it conflicts with what is there."""
    patch = os.path.join(root, case["hidden"])
    code, _ = run_command([git, "-C", tree, "apply", "--3way", patch])
    return code == 0


def hidden_status(case, commit, env, root=ROOT):
    """What the hidden test does on a clean checkout of ``commit``."""
    with worktree(commit, root) as tree:
        if not apply_hidden(case, tree, root):
            return "ungradable"
        return selector_status(case["test"], tree, env)


def tool_env():
    # One target folder for every checkout, so each case rebuilds only what changed.
    return dict(os.environ, CARGO_TARGET_DIR=os.path.join(ROOT, "target", "selfheal-replay"))


def validate(cases, only=None):
    """Check every hidden test fails on ``base`` and passes on ``fix``. Returns the number of bad cases."""
    env, bad = tool_env(), 0
    for case in cases:
        if only and case["n"] not in only:
            continue
        base = hidden_status(case, case["base"], env)
        fix = hidden_status(case, case["fix"], env)
        good = base == "failed" and fix == "passed"
        bad += not good
        print("case %d: %s (base: %s, fix: %s)" % (case["n"], "ok" if good else "BAD", base, fix))
    return bad


# ----------------------------------------------------------------------------- the replay

def replay_case(case, heron, base_status, env, root=ROOT):
    """Run the repair agent on one case and grade it. Returns ``(passed, verdict, outcome)``;
    ``outcome`` is None when the hidden test could not be applied to what the agent left."""
    with worktree(case["base"], root) as tree:
        code, out = run_command([heron, "selfheal", "replay", "--case", str(case["n"]), "--worktree", tree,
                                 "--cases", os.path.join(root, "eval", "selfheal", "cases.md")],
                                env=env, timeout=CASE_TIMEOUT)
        verdict = replay_verdict(out) if code == 0 else "needs_owner"
        run_command(["git", "-C", tree, "add", "-A"])
        empty = run_command(["git", "-C", tree, "diff", "--cached", "--quiet", case["base"]])[0] == 0
        if not apply_hidden(case, tree, root):
            return False, verdict, None
        status = selector_status(case["test"], tree, env)
        suite = run_command([env.get("CARGO", "cargo"), "test", "--locked"], cwd=tree, env=env, timeout=CASE_TIMEOUT)[0] == 0
        outcome = {
            "patch_empty": empty,
            "hidden_test_deleted": status == "missing",
            "hidden_failed_on_base": base_status == "failed",
            "hidden_passed_on_fix": status == "passed",
            "suite_passed": suite,
        }
        return grader.grade(outcome), verdict, outcome


def replay_verdict(output):
    """The ``verdict`` of the JSON line ``selfheal replay`` prints last; anything else needs the owner."""
    for line in reversed(output.strip().splitlines()):
        try:
            value = json.loads(line)
        except ValueError:
            continue
        if isinstance(value, dict) and value.get("verdict") in ("auto_eligible", "needs_owner"):
            return value["verdict"]
        break
    return "needs_owner"


def configured_threshold():
    home = os.environ.get("HERON_HOME") or os.path.join(os.path.expanduser("~"), ".heron")
    path = os.path.join(home, "config.toml")
    if not os.path.isfile(path):
        return DEFAULT_THRESHOLD
    with open(path, "rb") as handle:
        value = tomllib.load(handle).get("harness2", {}).get("replay_threshold", DEFAULT_THRESHOLD)
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError("%s: [harness2] replay_threshold must be a whole number" % path)
    return value


def replay(cases, threshold, only=None):
    code, out = run_command(["cargo", "build", "--release", "--locked"], cwd=ROOT)
    heron = HERON
    if code != 0 or not os.path.isfile(heron):
        sys.stderr.write("run_replay: building heron failed:\n%s\n" % out[-2000:])
        return 1
    env = tool_env()
    passed = auto = ran = 0
    for case in cases:
        if only and case["n"] not in only:
            continue
        ran += 1
        base_status = hidden_status(case, case["base"], env)
        ok, verdict, outcome = replay_case(case, heron, base_status, env)
        passed += ok
        auto += verdict == "auto_eligible"
        print("case %d: %s (%s)" % (case["n"], "pass" if ok else ("ungradable" if outcome is None else "fail"), verdict))
        if outcome is not None and not ok:
            print("  outcome: %s" % json.dumps(outcome, sort_keys=True))
    print("replay: %d/%d" % (passed, ran))
    print("auto-eligible: %d" % auto)
    if not only:
        base = run_command(["git", "-C", ROOT, "merge-base", "HEAD", "main"])[1].strip()
        recorded = run_command([heron, "selfheal", "record-replay", base, str(passed), str(ran), str(auto)])
        if recorded[0] != 0:
            sys.stderr.write("run_replay: record-replay failed: %s\n" % recorded[1].strip())
            return 1
    return 0 if passed >= threshold else 1


def main(argv):
    parser = argparse.ArgumentParser(prog="run_replay.py", description=__doc__.split("\n\n")[0])
    parser.add_argument("--cases", default=CASES_PATH, help="the cases file (default: eval/selfheal/cases.md)")
    parser.add_argument("--case", type=int, action="append", dest="only", help="run only this case (repeatable; nothing is recorded)")
    parser.add_argument("--threshold", type=int, help="cases that must pass (default: [harness2] replay_threshold, or 6)")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--validate", action="store_true", help="check each hidden test fails on base and passes on fix")
    mode.add_argument("--check-patches", action="store_true", help="check each hidden patch applies to base and fix")
    args = parser.parse_args(argv)
    try:
        cases = load_cases(args.cases)
    except (OSError, ValueError) as exc:
        sys.stderr.write("run_replay: %s\n" % exc)
        return 2
    if args.check_patches:
        failures = check_patches(cases)
        for n, which, why in failures:
            sys.stderr.write("check-patches: case %d does not apply to %s: %s\n" % (n, which, why))
        if not failures:
            print("check-patches: %d patches apply to base and fix" % len(cases))
        return 1 if failures else 0
    if args.validate:
        return 1 if validate(cases, args.only) else 0
    try:
        threshold = args.threshold if args.threshold is not None else configured_threshold()
    except (OSError, ValueError) as exc:
        sys.stderr.write("run_replay: %s\n" % exc)
        return 2
    return replay(cases, threshold, args.only)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
