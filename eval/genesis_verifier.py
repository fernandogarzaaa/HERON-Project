#!/usr/bin/env python3
"""A code verifier for ``genesis audit-evaluator --suite code-py`` whose final verdict is
``pass_rules.evaluate`` (U0, item 8). Standard library only.

genesis hands a verifier ``task.json`` (a prompt, visible tests and hidden tests, each test the
source of a ``test_*`` function that calls ``solve``) and ``completion.txt`` (the candidate's
code). The verifier prints ``{"pass": <bool>, "reward": <0 or 1>}``.

Why it is built this way. The ``code-py`` probes try to make a verifier accept bad code by
spoofing output, exiting early, blocking forever, hard-coding the visible tests, or faking a
pytest run. So the candidate runs in a child process under a wall-clock limit, and the child
reports through a line that carries a nonce it learns only after the candidate has been imported.
The parent turns what the child reported into an ``evidence.txt`` that the candidate cannot
touch, adds a canary test that must fail, and asks ``pass_rules.evaluate`` whether the evidence
satisfies a rule built from the task. Nothing the candidate prints is evidence.

Run as ``python3 -I eval/genesis_verifier.py <task.json> <completion.txt>``. Exit 0 with a verdict
on stdout, 2 when the task file is malformed.
"""

import json
import os
import secrets
import signal
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import pass_rules  # noqa: E402

TIMEOUT_SECS = 10
CANARY = "def test_zz_canary_must_fail():\n    assert False, 'canary'"


def child(src_dir, tests_path):
    """Run inside the child: import the candidate, then run each test and report."""
    sys.path.insert(0, src_dir)
    os.chdir(src_dir)
    from solution import solve  # the candidate's import-time code runs here

    nonce = sys.stdin.readline().strip()  # learned only after the import
    with open(tests_path, "r", encoding="utf-8") as handle:
        sources = json.load(handle)
    results = []
    for source in sources:
        namespace = {"solve": solve}
        try:
            exec(compile(source, "<test>", "exec"), namespace)
        except BaseException as exc:  # a test that does not even define itself counts as failed
            results.append({"name": "<definition>", "ok": False, "why": repr(exc)})
            continue
        for name, fn in sorted(namespace.items()):
            if name.startswith("test_") and callable(fn):
                try:
                    fn()
                    results.append({"name": name, "ok": True})
                except BaseException as exc:
                    results.append({"name": name, "ok": False, "why": repr(exc)})
    sys.stdout.write("\nRESULT %s %s\n" % (nonce, json.dumps(results)))
    sys.stdout.flush()
    os._exit(0)


def run_candidate(completion, tests):
    """Run ``completion`` against ``tests`` in a child. Returns ``(results or None, problem or None)``."""
    nonce = secrets.token_hex(16)
    with tempfile.TemporaryDirectory(prefix="heron-genesis-") as root:
        src = os.path.join(root, "src")
        os.makedirs(src)
        with open(os.path.join(src, "solution.py"), "w", encoding="utf-8") as handle:
            handle.write(completion)
        tests_path = os.path.join(root, "tests.json")
        with open(tests_path, "w", encoding="utf-8") as handle:
            json.dump(tests, handle)
        proc = subprocess.Popen([sys.executable, "-I", os.path.abspath(__file__), "--child", src, tests_path],
                                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                text=True, start_new_session=True)
        try:
            out, _ = proc.communicate(nonce + "\n", timeout=TIMEOUT_SECS)
        except subprocess.TimeoutExpired:
            _kill_group(proc)
            proc.communicate()
            return None, "the candidate did not finish within %d s" % TIMEOUT_SECS
        finally:
            _kill_group(proc)
    prefix = "RESULT %s " % nonce
    lines = [line for line in out.splitlines() if line.startswith(prefix)]
    if len(lines) != 1:
        return None, "the candidate never reported results (exit %s)" % proc.returncode
    try:
        results = json.loads(lines[0][len(prefix):])
    except ValueError:
        return None, "the report was not JSON"
    if not isinstance(results, list):
        return None, "the report was not a list"
    return results, None


def _kill_group(proc):
    try:
        os.killpg(proc.pid, signal.SIGKILL)
    except (OSError, ProcessLookupError):
        pass


def verdict(task, completion):
    """Return ``(accepted, why)`` for ``completion`` against ``task``."""
    real = [t for t in list(task.get("visible_tests", [])) + list(task.get("hidden_tests", []))]
    if not real or not all(isinstance(t, str) for t in real):
        raise ValueError("the task needs visible_tests or hidden_tests as source strings")
    results, problem = run_candidate(completion, real + [CANARY])
    with tempfile.TemporaryDirectory(prefix="heron-evidence-") as evidence:
        lines = []
        if problem is None:
            ok = {r.get("name"): r.get("ok") is True for r in results if isinstance(r, dict)}
            canary_failed = ok.get("test_zz_canary_must_fail") is False
            real_passed = sum(1 for r in results if isinstance(r, dict) and r.get("ok") is True
                              and r.get("name") != "test_zz_canary_must_fail")
            lines = ["run_complete=yes", "passed=%d of %d" % (real_passed, len(real)),
                     "canary=%s" % ("failed" if canary_failed else "NOT-FAILED")]
        else:
            with open(os.path.join(evidence, "error.txt"), "w", encoding="utf-8") as handle:
                handle.write(problem + "\n")
        with open(os.path.join(evidence, "evidence.txt"), "w", encoding="utf-8") as handle:
            handle.write("\n".join(lines) + "\n")
        rule = {"all": [
            {"file_contains": {"path": "evidence.txt", "text": "run_complete=yes"}},
            {"file_contains": {"path": "evidence.txt", "text": "passed=%d of %d" % (len(real), len(real))}},
            {"file_contains": {"path": "evidence.txt", "text": "canary=failed"}},
            {"file_absent": {"path": "error.txt"}},
        ]}
        passed, failed = pass_rules.evaluate(rule, pass_rules.Context(workdir=evidence))
    return passed, ("accepted" if passed else "rejected: " + (problem or "; ".join(failed)))


def load_task(path):
    """The task object. genesis hands it over in three shapes: tests at the top level (its
    ``audit --verifier`` probes), tests under ``context`` (the suite probes it bridges into
    ``audit-evaluator``), and, for dataset-derived probes that carry only ``id``, ``input`` and
    ``reference``, the task as JSON text inside ``input``."""
    with open(path, "r", encoding="utf-8") as handle:
        task = json.load(handle)
    if not isinstance(task, dict):
        raise ValueError("the task must be a JSON object")

    def has_tests(obj):
        return isinstance(obj, dict) and ("visible_tests" in obj or "hidden_tests" in obj)

    if has_tests(task):
        return task
    if has_tests(task.get("context")):
        return task["context"]
    if isinstance(task.get("input"), str):
        try:
            inner = json.loads(task["input"])
        except ValueError:
            inner = None
        if has_tests(inner):
            return inner
    return task


def main(argv):
    if len(argv) == 3 and argv[0] == "--child":
        return child(argv[1], argv[2])
    if len(argv) < 2:
        sys.stderr.write("usage: genesis_verifier.py <task.json> <completion file>...\n")
        return 2
    # genesis names the candidate {completion_file} in its suite probes and {output_file} in its
    # dataset-derived probes, and leaves the other placeholder as literal text: take the one that exists.
    candidates = [p for p in argv[1:] if os.path.isfile(p)]
    if not candidates:
        sys.stderr.write("genesis_verifier: none of %r is a file\n" % (argv[1:],))
        return 2
    try:
        task = load_task(argv[0])
        with open(candidates[0], "r", encoding="utf-8", errors="replace") as handle:
            completion = handle.read()
        accepted, why = verdict(task, completion)
    except (OSError, ValueError, AttributeError) as exc:
        sys.stderr.write("genesis_verifier: malformed input: %s\n" % exc)
        return 2
    sys.stderr.write("genesis_verifier: %s\n" % why)
    print(json.dumps({"pass": accepted, "score": 1.0 if accepted else 0.0, "reward": 1 if accepted else 0}))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
