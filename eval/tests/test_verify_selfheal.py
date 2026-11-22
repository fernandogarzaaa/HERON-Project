"""Tests for ``scripts/verify-selfheal.sh`` (item 21), the gate ``selfheal.yml`` runs on a repair PR.

Skipped until the script exists. Set ``VERIFY_SELFHEAL_SCRIPT`` to test another copy of it.

Each test builds a small git repository in a temp folder, ``<tmp>/fix``, holding a base commit and a
fix commit, and runs ``bash verify-selfheal.sh <base> <fix> <tmp>/fix``. The script makes its base
checkout at ``<tmp>/base`` (``git -C <fix-dir> worktree add ../base <base>``). The tools are stubs,
passed the way the design says, through the ``CARGO``, ``SWIFT`` and ``TESTTEETH`` variables.

The stub protocol (what the script can rely on, and nothing more)
------------------------------------------------------------------
Every stub appends one line to ``$STUB_LOG`` per call, so a test can see what the script ran.

* **cargo** and **swift** find the checkout they run in with ``git rev-parse --show-toplevel``
  from the current directory. Its folder name is the role: ``fix`` or ``base``. Run from anywhere
  else, they exit 99, so the script must run them inside those two checkouts. ``--test=<stem>`` is
  logged as ``--test <stem>``.

  - ``cargo test ... --test <stem>`` exits 101 when ``tests/<stem>.rs`` is not in that checkout,
    as cargo does for an unknown test target. The script must copy the new test files onto the base
    checkout before it runs them there.
  - ``swift test --filter <Class>`` exits 1 when no ``<Class>.swift`` is under
    ``macos/HeronMac/Tests`` in that checkout.
  - Otherwise the outcome comes from ``STUB_BASE`` on the base checkout (``fails``, ``passes`` or
    ``no_compile``) and ``STUB_FIX`` on the fix checkout (``passes`` or ``fails``). ``no_compile``
    makes every call exit 101 (swift: 1), a build error. ``--no-run`` (cargo) and ``swift build``
    exit 0 unless the checkout does not compile. A test run prints ``test result: ok. 1 passed; 0
    failed`` and exits 0, or prints ``test result: FAILED. 0 passed; 1 failed`` and exits 101
    (swift prints ``Executed 1 test, with 0 failures`` or ``with 1 failure``, and exits 0 or 1).
* **testteeth** prints ``mutation score: <STUB_SCORE>%`` (default 84) and exits 0 when the score is at
  least 70, else 1. The script reads the score from that line.

A scenario is one row of the R2 table in §4 (``passes``, ``catches_nothing``, ``no_compile_on_base``,
``fails_on_fix``, ``new_connector``), or a way the script is called, or a field of
``gate-report.json`` (§11). Where the design does not say, a test does not assert.
"""

import json
import os
import re
import shutil
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
SCRIPT = os.environ.get("VERIFY_SELFHEAL_SCRIPT") or os.path.join(REPO, "scripts", "verify-selfheal.sh")

CARGO_STUB = r"""#!/bin/sh
tree="$(git rev-parse --show-toplevel 2>/dev/null)" || { echo "stub cargo: not in a git checkout" >&2; exit 99; }
role="$(basename "$tree")"
case "$role" in base|fix) ;; *) echo "stub cargo: run in the base or fix checkout, not $tree" >&2; exit 99 ;; esac
args=""; norun=0; target=""; prev=""
for a in "$@"; do
  case "$a" in
    --no-run) norun=1 ;;
    --test=*) target="${a#--test=}"; a="--test $target" ;;
  esac
  [ "$prev" = "--test" ] && target="$a"
  args="$args $a"
  prev="$a"
done
echo "$role cargo$args" >> "$STUB_LOG"
if [ "$role" = base ]; then mode="${STUB_BASE:-fails}"; else mode="${STUB_FIX:-passes}"; fi
if [ "$mode" = no_compile ]; then echo "error[E0432]: unresolved import" >&2; exit 101; fi
if [ -n "$target" ] && [ ! -f "$tree/tests/$target.rs" ]; then echo "error: no test target named \`$target\`" >&2; exit 101; fi
[ "$norun" = 1 ] && exit 0
if [ "$mode" = fails ]; then echo "test result: FAILED. 0 passed; 1 failed"; exit 101; fi
echo "test result: ok. 1 passed; 0 failed"
"""

SWIFT_STUB = r"""#!/bin/sh
tree="$(git rev-parse --show-toplevel 2>/dev/null)" || { echo "stub swift: not in a git checkout" >&2; exit 99; }
role="$(basename "$tree")"
case "$role" in base|fix) ;; *) echo "stub swift: run in the base or fix checkout, not $tree" >&2; exit 99 ;; esac
args=""; class=""; prev=""
for a in "$@"; do
  [ "$prev" = "--filter" ] && class="$a"
  args="$args $a"
  prev="$a"
done
echo "$role swift$args" >> "$STUB_LOG"
if [ "$role" = base ]; then mode="${STUB_BASE:-fails}"; else mode="${STUB_FIX:-passes}"; fi
if [ "$mode" = no_compile ]; then echo "error: build failed" >&2; exit 1; fi
if [ -n "$class" ] && [ -z "$(find "$tree/macos/HeronMac/Tests" -name "$class.swift" 2>/dev/null)" ]; then
  echo "error: no tests found matching $class" >&2; exit 1
fi
[ "$1" = build ] && exit 0
if [ "$mode" = fails ]; then echo "Executed 1 test, with 1 failure"; exit 1; fi
echo "Executed 1 test, with 0 failures"
"""

TESTTEETH_STUB = r"""#!/bin/sh
echo "testteeth $*" >> "$STUB_LOG"
score="${STUB_SCORE:-84}"
echo "mutation score: ${score}%"
[ "$score" -ge 70 ]
"""

SCHEDULE_TEST = "tests/schedule_local_time.rs"
BASE_RUST = {"src/schedule.rs": "pub fn next() -> u8 { 1 }\n", "tests/old_case.rs": "// existing\n",
             "tests/connector_manifests.rs": "// manifests\n"}
FIX_RUST = {"src/schedule.rs": "pub fn next() -> u8 { 2 }\n", SCHEDULE_TEST: "// new test\n"}


@unittest.skipUnless(os.path.isfile(SCRIPT), "scripts/verify-selfheal.sh does not exist yet")
class VerifySelfheal(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="verify-selfheal-")
        self.addCleanup(self._tmp.cleanup)
        self.root = os.path.realpath(self._tmp.name)
        self.fix_dir = os.path.join(self.root, "fix")
        self.bin = os.path.join(self.root, "bin")
        self.log = os.path.join(self.root, "stub.log")
        os.makedirs(self.fix_dir)
        os.makedirs(self.bin)
        for name, text in (("cargo", CARGO_STUB), ("swift", SWIFT_STUB), ("testteeth", TESTTEETH_STUB)):
            path = os.path.join(self.bin, name)
            with open(path, "w", encoding="utf-8") as handle:
                handle.write(text)
            os.chmod(path, 0o755)
        # No global or system git configuration, so the test behaves the same on every machine.
        self.env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": self.root, "GIT_CONFIG_GLOBAL": os.devnull,
                    "GIT_CONFIG_NOSYSTEM": "1", "GIT_TERMINAL_PROMPT": "0", "STUB_LOG": self.log,
                    "CARGO": os.path.join(self.bin, "cargo"), "SWIFT": os.path.join(self.bin, "swift"),
                    "TESTTEETH": os.path.join(self.bin, "testteeth")}
        self.git("init", "-q", "-b", "main")

    def git(self, *args):
        cmd = ["git", "-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false",
               "-c", "core.hooksPath=" + os.devnull, "-C", self.fix_dir, *args]
        proc = subprocess.run(cmd, env=self.env, capture_output=True, text=True)
        self.assertEqual(proc.returncode, 0, "%s: %s" % (args, proc.stderr))
        return proc.stdout.strip()

    def commit(self, files, message):
        for path, text in files.items():
            full = os.path.join(self.fix_dir, *path.split("/"))
            os.makedirs(os.path.dirname(full), exist_ok=True)
            with open(full, "w", encoding="utf-8") as handle:
                handle.write(text)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)
        return self.git("rev-parse", "HEAD")

    def repo(self, base_files, fix_files):
        """Commit ``base_files``, then ``fix_files`` on top. Returns ``(base, fix)``."""
        base = self.commit(base_files, "base")
        return base, self.commit(fix_files, "fix")

    def run_gate(self, base, fix, **stub_env):
        env = dict(self.env, **stub_env)
        proc = subprocess.run(["bash", SCRIPT, base, fix, self.fix_dir], env=env, capture_output=True, text=True, cwd=self.root)
        return proc

    def report(self):
        with open(os.path.join(self.fix_dir, "gate-report.json"), "r", encoding="utf-8") as handle:
            return json.load(handle)

    def calls(self, tool):
        """The logged calls of one tool, as ``(role, arguments)``; testteeth has no role."""
        if not os.path.exists(self.log):
            return []
        with open(self.log, "r", encoding="utf-8") as handle:
            lines = [line.rstrip("\n") for line in handle]
        if tool == "testteeth":
            return [("", line[len("testteeth "):]) for line in lines if line.startswith("testteeth ")]
        found = []
        for line in lines:
            match = re.match(r"(base|fix) %s ?(.*)" % tool, line)
            if match:
                found.append((match.group(1), match.group(2)))
        return found

    def ran_test(self, role, tool, selector):
        return any(r == role and re.search(r"\btest\b", a) and selector in a for r, a in self.calls(tool))

    # ------------------------------------------------------------------ the R2 table

    def test_passes_when_the_new_test_fails_on_base_and_passes_on_fix(self):
        base, fix = self.repo(BASE_RUST, FIX_RUST)
        proc = self.run_gate(base, fix, STUB_BASE="fails", STUB_FIX="passes")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        report = self.report()
        self.assertEqual(report["r2"], "passes")
        self.assertEqual(report["base"], base)
        self.assertEqual(report["fix"], fix)

    def test_runs_the_new_test_by_its_stem_after_compiling_it_on_base(self):
        base, fix = self.repo(BASE_RUST, FIX_RUST)
        self.run_gate(base, fix)
        cargo = self.calls("cargo")
        base_calls = [a for r, a in cargo if r == "base"]
        self.assertTrue(any("--locked" in a and "--no-run" in a for a in base_calls), base_calls)
        self.assertTrue(self.ran_test("base", "cargo", "--test schedule_local_time"), cargo)
        self.assertTrue(self.ran_test("fix", "cargo", "--test schedule_local_time"), cargo)
        compiled = min(i for i, (r, a) in enumerate(cargo) if r == "base" and "--no-run" in a)
        ran = min(i for i, (r, a) in enumerate(cargo) if r == "base" and "--test schedule_local_time" in a and "--no-run" not in a)
        self.assertLess(compiled, ran, "the new test is compiled before it is run")

    def test_only_added_test_files_are_run(self):
        base, fix = self.repo(BASE_RUST, dict(FIX_RUST, **{"tests/old_case.rs": "// changed\n"}))
        self.run_gate(base, fix)
        self.assertFalse(any("--test old_case" in a for _, a in self.calls("cargo")), self.calls("cargo"))
        self.assertFalse(any("--test connector_manifests" in a for _, a in self.calls("cargo")), self.calls("cargo"))

    def test_catches_nothing_is_a_gate_failure(self):
        base, fix = self.repo(BASE_RUST, FIX_RUST)
        proc = self.run_gate(base, fix, STUB_BASE="passes", STUB_FIX="passes")
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertEqual(self.report()["r2"], "catches_nothing")

    def test_a_test_that_needs_a_new_api_is_needs_owner_not_a_failure(self):
        base, fix = self.repo(BASE_RUST, FIX_RUST)
        proc = self.run_gate(base, fix, STUB_BASE="no_compile", STUB_FIX="passes")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertEqual(self.report()["r2"], "no_compile_on_base")

    def test_a_new_test_that_fails_on_fix_is_a_gate_failure_whatever_base_does(self):
        for base_mode in ("fails", "no_compile"):
            with self.subTest(base=base_mode):
                self.reset_repo()
                base, fix = self.repo(BASE_RUST, FIX_RUST)
                proc = self.run_gate(base, fix, STUB_BASE=base_mode, STUB_FIX="fails")
                self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
                self.assertEqual(self.report()["r2"], "fails_on_fix")

    def test_a_new_connector_passes_when_its_pairs_pass(self):
        manifest = {"connectors/echo.toml": 'name = "echo"\n', "connectors/echo.test.json": "[]\n"}
        base, fix = self.repo(BASE_RUST, manifest)
        proc = self.run_gate(base, fix, STUB_BASE="fails", STUB_FIX="passes")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertEqual(self.report()["r2"], "new_connector")
        self.assertFalse(any("--test echo" in a for _, a in self.calls("cargo")), "a manifest is not a test target")

    def test_swift_tests_are_built_and_filtered_by_class(self):
        base_files = {"macos/HeronMac/Sources/HeronUI/Thing.swift": "let a = 1\n"}
        fix_files = {"macos/HeronMac/Sources/HeronUI/Thing.swift": "let a = 2\n",
                     "macos/HeronMac/Tests/HeronUITests/ThingTests.swift": "// new test\n"}
        base, fix = self.repo(base_files, fix_files)
        proc = self.run_gate(base, fix, STUB_BASE="fails", STUB_FIX="passes")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertEqual(self.report()["r2"], "passes")
        swift = self.calls("swift")
        self.assertTrue(any(r == "base" and "build" in a and "--build-tests" in a for r, a in swift), swift)
        self.assertTrue(self.ran_test("base", "swift", "--filter ThingTests"), swift)
        self.assertTrue(self.ran_test("fix", "swift", "--filter ThingTests"), swift)
        self.assertEqual(self.calls("testteeth"), [], "no Rust changed, so there is nothing to mutate")

    def test_swift_catches_nothing(self):
        base_files = {"macos/HeronMac/Sources/HeronUI/Thing.swift": "let a = 1\n"}
        fix_files = {"macos/HeronMac/Sources/HeronUI/Thing.swift": "let a = 2\n",
                     "macos/HeronMac/Tests/HeronUITests/ThingTests.swift": "// new test\n"}
        base, fix = self.repo(base_files, fix_files)
        proc = self.run_gate(base, fix, STUB_BASE="passes", STUB_FIX="passes")
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertEqual(self.report()["r2"], "catches_nothing")

    # ------------------------------------------------------------------ mutation and the report

    def test_mutation_runs_against_the_base_with_the_design_threshold(self):
        base, fix = self.repo(BASE_RUST, FIX_RUST)
        self.run_gate(base, fix)
        calls = self.calls("testteeth")
        self.assertEqual(len(calls), 1, calls)
        args = calls[0][1].split()
        self.assertEqual(args[0], "run")
        self.assertEqual(args[args.index("--base") + 1], base)
        self.assertEqual(args[args.index("--fail-under") + 1], "70")

    def test_the_report_has_every_field_of_the_contract(self):
        base, fix = self.repo(BASE_RUST, FIX_RUST)
        proc = self.run_gate(base, fix, STUB_SCORE="84")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        report = self.report()
        self.assertEqual(set(report), {"base", "fix", "r2", "mutation", "changed"})
        self.assertEqual(report["mutation"], {"score": 84, "passed": True})
        self.assertEqual(report["changed"], ["src/schedule.rs", SCHEDULE_TEST])
        for key in ("base", "fix"):
            self.assertRegex(report[key], r"^[0-9a-f]{40}$")
        self.assertIn(report["r2"], {"passes", "catches_nothing", "no_compile_on_base", "fails_on_fix", "new_connector"})

    def test_a_mutation_score_below_the_threshold_is_a_gate_failure(self):
        base, fix = self.repo(BASE_RUST, FIX_RUST)
        proc = self.run_gate(base, fix, STUB_SCORE="60")
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        report = self.report()
        self.assertEqual(report["mutation"], {"score": 60, "passed": False})
        self.assertEqual(report["r2"], "passes")

    def test_the_report_is_written_when_the_gate_fails(self):
        base, fix = self.repo(BASE_RUST, FIX_RUST)
        proc = self.run_gate(base, fix, STUB_BASE="passes")
        self.assertEqual(proc.returncode, 1)
        self.assertTrue(os.path.isfile(os.path.join(self.fix_dir, "gate-report.json")))

    # ------------------------------------------------------------------ calling it wrong

    def test_usage_errors_exit_two_and_run_nothing(self):
        base, fix = self.repo(BASE_RUST, FIX_RUST)
        missing = os.path.join(self.root, "nowhere")
        cases = {"no arguments": [], "one argument": [base], "two arguments": [base, fix],
                 "four arguments": [base, fix, self.fix_dir, "extra"], "a short sha": [base[:7], fix, self.fix_dir],
                 "a branch name instead of a sha": [base, "main", self.fix_dir], "a sha with a non-hex digit": ["g" * 40, fix, self.fix_dir],
                 "a folder that does not exist": [base, fix, missing]}
        for name, args in cases.items():
            with self.subTest(name):
                proc = subprocess.run(["bash", SCRIPT, *args], env=self.env, capture_output=True, text=True, cwd=self.root)
                self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
                self.assertTrue(proc.stderr.strip(), "a usage error says what is wrong")
        self.assertFalse(os.path.exists(os.path.join(self.fix_dir, "gate-report.json")))
        self.assertFalse(os.path.exists(os.path.join(self.root, "base")), "no base checkout for a usage error")
        self.assertEqual(self.calls("cargo") + self.calls("testteeth"), [])

    def reset_repo(self):
        """Start the repository over, for a subtest that needs a second scenario."""
        shutil.rmtree(os.path.join(self.root, "base"), ignore_errors=True)
        shutil.rmtree(self.fix_dir)
        os.makedirs(self.fix_dir)
        if os.path.exists(self.log):
            os.remove(self.log)
        self.git("init", "-q", "-b", "main")


if __name__ == "__main__":
    unittest.main()
