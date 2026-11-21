import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest

CONTAINMENT = os.path.dirname(os.path.abspath(__file__))
RUN_SH = os.path.join(CONTAINMENT, "run.sh")
CHECK_PY = os.path.join(CONTAINMENT, "check.py")
BASH = shutil.which("bash")

GOOD_REPORT = {
    "provider": "x", "outside_canary": "/tmp/heron-canary-1.txt", "sandbox": "seatbelt",
    "version": "2.1.327 (Claude Code)", "version_parsed": "2.1.327", "tools_off": True,
    "write_outside_failed": True, "tool_calls": 0, "env": ["PATH", "HOME"], "env_ok": True,
    "reply": "I cannot use tools.", "error": None, "passed": True,
}


class Sandbox(unittest.TestCase):
    """A temp HOME, a bin folder of fake CLIs and a fake cargo that plays back canned reports."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        root = self._tmp.name
        self.home = os.path.join(root, "home")
        self.bin = os.path.join(root, "bin")
        self.pybin = os.path.join(root, "pybin")
        self.out = os.path.join(root, "out")
        self.reports = os.path.join(root, "reports")
        self.log = os.path.join(root, "cargo.log")
        for folder in (self.home, self.bin, self.pybin, self.out, self.reports):
            os.makedirs(folder)
        os.symlink(sys.executable, os.path.join(self.pybin, "python3"))
        self.cargo = os.path.join(root, "cargo")
        with open(self.cargo, "w") as handle:
            handle.write(
                '#!/bin/sh\n'
                'echo "id=$HERON_CANARY_PROVIDER out=$HERON_CANARY_OUT args=$* path=$PATH" >> "%s"\n'
                '[ -f "%s/$HERON_CANARY_PROVIDER.json" ] || exit 101\n'
                'cp "%s/$HERON_CANARY_PROVIDER.json" "$HERON_CANARY_OUT"\n'
                'exit "${FAKE_CARGO_STATUS:-0}"\n' % (self.log, self.reports, self.reports)
            )
        self.executable(self.cargo)

    def executable(self, path):
        os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR)

    def install(self, binary, folder=None):
        folder = folder or self.bin
        os.makedirs(folder, exist_ok=True)
        path = os.path.join(folder, binary)
        with open(path, "w") as handle:
            handle.write("#!/bin/sh\nexit 0\n")
        self.executable(path)
        return path

    def report(self, id_, **changes):
        raw = dict(GOOD_REPORT, provider=id_, **changes)
        with open(os.path.join(self.reports, id_ + ".json"), "w") as handle:
            json.dump(raw, handle)

    def dirs(self, id_, record):
        with open(os.path.join(self.out, id_ + ".dirs.json"), "w") as handle:
            json.dump(record, handle)

    def env(self, **extra):
        env = {"HOME": self.home, "PATH": os.pathsep.join([self.bin, self.pybin, "/usr/bin", "/bin"]),
               "CARGO": self.cargo, "CONTAINMENT_OUT_DIR": self.out, "CONTAINMENT_SYSTEM_DIRS": "",
               "TMPDIR": self._tmp.name}
        env.update(extra)
        return env

    def run_sh(self, **extra):
        return subprocess.run([BASH, RUN_SH], capture_output=True, text=True, env=self.env(**extra), timeout=60)

    def result(self, id_):
        with open(os.path.join(self.out, id_ + ".json")) as handle:
            return json.load(handle)

    def cargo_calls(self):
        if not os.path.exists(self.log):
            return []
        with open(self.log) as handle:
            return handle.read().splitlines()


@unittest.skipUnless(BASH, "bash is not available")
class RunSh(Sandbox):
    def test_every_candidate_gets_a_result_in_the_documented_shape(self):
        self.install("claude")
        self.install("codex")
        self.install("gemini")
        self.report("claude-cli")
        self.report("codex-cli", tools_off=False, passed=False)
        self.dirs("gemini-cli", {"id": "gemini-cli", "paths": [], "unavailable_reason": "Google login refused"})
        proc = self.run_sh()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(self.result("claude-cli")["status"], "passed")
        self.assertEqual(self.result("codex-cli")["status"], "failed")
        for missing in ("opencode-cli", "cline-cli", "antigravity-cli"):
            self.assertEqual(self.result(missing), {"id": missing, "status": "not-installed"})
        self.assertEqual(self.result("gemini-cli"), {"id": "gemini-cli", "status": "unavailable", "reason": "Google login refused"})
        calls = self.cargo_calls()
        self.assertEqual(len(calls), 2, "gemini-cli is unavailable, so nothing runs for it: %s" % calls)
        self.assertIn("id=claude-cli out=%s " % os.path.join(self.out, "claude-cli.json"), calls[0])
        self.assertIn("args=test --locked --lib canary_real_cli -- --ignored", calls[0])
        self.assertIn("id=codex-cli", calls[1])

    def test_result_records_version_containment_observations_and_time(self):
        self.install("claude")
        self.report("claude-cli")
        self.dirs("claude-cli", {"id": "claude-cli", "paths": ["~/.claude", "~/.claude.json", "~/.claude/backups"],
                                 "credential_store": "macos-keychain", "login_completed": False,
                                 "known_paths": ["~/.claude", "~/.claude.json"]})
        self.assertEqual(self.run_sh().returncode, 0)
        result = self.result("claude-cli")
        self.assertEqual(result["version"], "2.1.327")
        self.assertEqual(result["version_output"], "2.1.327 (Claude Code)")
        self.assertEqual(result["containment"], {"tools_off_canary": True, "child_env_is_exactly_the_allowlist": True,
                                                 "write_outside_run_folder_fails": True})
        self.assertEqual(result["config_dirs"], ["~/.claude", "~/.claude.json", "~/.claude/backups"],
                         "paths and known_paths together, without repeats")
        self.assertIs(result["config_dirs_confirmed"], True, "a Keychain login counts as confirmed")
        self.assertEqual(result["credential_store"], "macos-keychain")
        self.assertEqual(result["keychain_login"], "ok")
        self.assertEqual(result["oauth_refresh"], "unknown")
        self.assertEqual(result["tcc_prompts"], "unknown")
        self.assertEqual(result["codex_nested_sandbox"], "not-applicable")
        self.assertRegex(result["ran_at"], r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$")
        self.assertEqual(result["canary"]["tools_off"], True, "the raw report is kept")

    def test_config_dirs_unconfirmed_without_a_dirs_file(self):
        self.install("claude")
        self.report("claude-cli")
        self.assertEqual(self.run_sh().returncode, 0)
        result = self.result("claude-cli")
        self.assertEqual(result["config_dirs"], "unconfirmed")
        self.assertIs(result["config_dirs_confirmed"], False)
        self.assertEqual(result["keychain_login"], "unknown")

    def test_old_dirs_file_counts_as_home_folder_and_completed(self):
        self.install("codex")
        self.report("codex-cli")
        self.dirs("codex-cli", {"id": "codex-cli", "paths": ["~/.codex", "~/.codex/auth.json"]})
        self.assertEqual(self.run_sh().returncode, 0)
        result = self.result("codex-cli")
        self.assertEqual(result["config_dirs"], ["~/.codex", "~/.codex/auth.json"])
        self.assertIs(result["config_dirs_confirmed"], True)
        self.assertEqual(result["credential_store"], "home-folder")
        self.assertEqual(result["keychain_login"], "not-applicable")
        self.assertEqual(result["codex_nested_sandbox"], "ok")

    def test_home_folder_login_that_did_not_complete_is_unconfirmed(self):
        self.install("opencode")
        self.report("opencode-cli")
        self.dirs("opencode-cli", {"id": "opencode-cli", "paths": ["~/.config/opencode"], "credential_store": "home-folder",
                                   "login_completed": False, "known_paths": []})
        self.assertEqual(self.run_sh().returncode, 0)
        self.assertIs(self.result("opencode-cli")["config_dirs_confirmed"], False)

    def test_shared_parents_are_copied_not_judged_here(self):
        # U2w drops them; this unit only records what discover-dirs.sh listed.
        self.install("opencode")
        self.report("opencode-cli")
        self.dirs("opencode-cli", {"id": "opencode-cli", "paths": ["~/.cache", "~/.cache/opencode"],
                                   "credential_store": "home-folder", "login_completed": True, "known_paths": []})
        self.assertEqual(self.run_sh().returncode, 0)
        self.assertEqual(self.result("opencode-cli")["config_dirs"], ["~/.cache", "~/.cache/opencode"])

    def test_a_report_that_disagrees_with_its_own_checks_is_a_failure(self):
        self.install("claude")
        self.report("claude-cli", write_outside_failed=False)  # `passed` still says True
        self.assertEqual(self.run_sh().returncode, 0)
        self.assertEqual(self.result("claude-cli")["status"], "failed")

    def test_a_missing_check_value_is_a_failure(self):
        self.install("claude")
        raw = dict(GOOD_REPORT)
        del raw["env_ok"]
        with open(os.path.join(self.reports, "claude-cli.json"), "w") as handle:
            json.dump(raw, handle)
        self.assertEqual(self.run_sh().returncode, 0)
        self.assertEqual(self.result("claude-cli")["status"], "failed")

    def test_a_cli_that_could_not_run_a_turn_is_failed_with_the_error(self):
        self.install("claude")
        self.report("claude-cli", passed=False, error="Claude Code is logged out: run `claude auth login`", reply="")
        self.dirs("claude-cli", {"id": "claude-cli", "paths": ["~/.claude"], "credential_store": "macos-keychain",
                                 "login_completed": False, "known_paths": ["~/.claude"]})
        self.assertEqual(self.run_sh().returncode, 0)
        result = self.result("claude-cli")
        self.assertEqual(result["status"], "failed")
        self.assertIn("logged out", result["error"])
        self.assertEqual(result["keychain_login"], "failed")

    def test_codex_nested_sandbox_failure_is_observed(self):
        self.install("codex")
        self.report("codex-cli", passed=False, error="sandbox: sandbox_apply: Operation not permitted")
        self.assertEqual(self.run_sh().returncode, 0)
        self.assertEqual(self.result("codex-cli")["codex_nested_sandbox"], "failed")

    def test_no_canary_report_exits_1_and_leaves_no_result_not_even_a_stale_one(self):
        self.install("claude")
        stale = os.path.join(self.out, "claude-cli.json")
        with open(stale, "w") as handle:
            json.dump({"id": "claude-cli", "status": "passed"}, handle)
        proc = self.run_sh()  # no canned report: the fake cargo exits 101 and writes nothing
        self.assertEqual(proc.returncode, 1)
        self.assertFalse(os.path.exists(stale))
        self.assertIn("wrote no canary report", proc.stderr)
        # The other candidates are still recorded.
        self.assertEqual(self.result("opencode-cli")["status"], "not-installed")

    def test_unreadable_report_exits_1_and_leaves_no_result(self):
        self.install("claude")
        with open(os.path.join(self.reports, "claude-cli.json"), "w") as handle:
            handle.write("not json")
        proc = self.run_sh()
        self.assertEqual(proc.returncode, 1)
        self.assertFalse(os.path.exists(os.path.join(self.out, "claude-cli.json")))

    def test_cargo_exit_status_does_not_hide_a_report_that_was_written(self):
        self.install("claude")
        self.report("claude-cli")
        proc = self.run_sh(FAKE_CARGO_STATUS="101")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(self.result("claude-cli")["status"], "passed")
        self.assertIn("cargo exited 101", proc.stderr)

    def test_binary_outside_path_is_found_like_the_registry_does_and_put_on_path(self):
        self.install("claude", os.path.join(self.home, ".local", "bin"))
        self.install("opencode", os.path.join(self.home, ".opencode", "bin"))
        self.report("claude-cli")
        self.report("opencode-cli")
        self.assertEqual(self.run_sh().returncode, 0)
        self.assertEqual(self.result("claude-cli")["status"], "passed")
        calls = self.cargo_calls()
        self.assertIn(os.path.join(self.home, ".opencode", "bin"), calls[1].split("path=")[1])
        self.assertEqual(self.result("opencode-cli")["status"], "passed")

    def test_non_executable_file_is_not_an_install(self):
        path = self.install("claude")
        os.chmod(path, 0o644)
        self.assertEqual(self.run_sh().returncode, 0)
        self.assertEqual(self.result("claude-cli"), {"id": "claude-cli", "status": "not-installed"})

    def test_not_installed_wins_over_an_unavailable_record(self):
        self.dirs("gemini-cli", {"id": "gemini-cli", "paths": [], "unavailable_reason": "refused"})
        self.assertEqual(self.run_sh().returncode, 0)
        self.assertEqual(self.result("gemini-cli"), {"id": "gemini-cli", "status": "not-installed"})


def write_result(directory, id_, status, **extra):
    record = {"id": id_, "status": status}
    record.update(extra)
    with open(os.path.join(directory, id_ + ".json"), "w") as handle:
        json.dump(record, handle)


IDS = ["claude-cli", "opencode-cli", "cline-cli", "codex-cli", "antigravity-cli", "gemini-cli"]


class CheckPy(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.dir = self._tmp.name

    def fill(self, **statuses):
        """Every candidate not-installed, except the ones named (by id with `_` for `-`)."""
        for id_ in IDS:
            write_result(self.dir, id_, statuses.get(id_.replace("-", "_"), "not-installed"))

    def check(self, *args):
        return subprocess.run([sys.executable, "-I", CHECK_PY, "--dir", self.dir] + list(args), capture_output=True, text=True, timeout=60)

    def tier1(self):
        proc = self.check("--list-tier1")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        return proc.stdout.split()

    # --recorded
    def test_recorded_accepts_every_valid_status(self):
        self.fill(claude_cli="failed", opencode_cli="passed", cline_cli="unavailable")
        self.assertEqual(self.check("--recorded").returncode, 0)

    def test_recorded_passes_even_when_a_candidate_failed(self):
        self.fill(claude_cli="failed")
        self.assertEqual(self.check("--recorded").returncode, 0)

    def test_recorded_fails_on_a_missing_file_and_names_it(self):
        self.fill()
        os.remove(os.path.join(self.dir, "codex-cli.json"))
        proc = self.check("--recorded")
        self.assertEqual(proc.returncode, 1)
        self.assertIn("codex-cli", proc.stderr)

    def test_recorded_fails_on_an_invalid_status(self):
        self.fill()
        write_result(self.dir, "cline-cli", "maybe")
        proc = self.check("--recorded")
        self.assertEqual(proc.returncode, 1)
        self.assertIn("cline-cli", proc.stderr)

    def test_recorded_fails_on_a_raw_canary_report_without_status(self):
        self.fill()
        with open(os.path.join(self.dir, "cline-cli.json"), "w") as handle:
            json.dump({"provider": "cline-cli", "passed": True}, handle)
        self.assertEqual(self.check("--recorded").returncode, 1)

    def test_recorded_fails_on_invalid_json_and_on_a_wrong_id(self):
        self.fill()
        with open(os.path.join(self.dir, "cline-cli.json"), "w") as handle:
            handle.write("{")
        self.assertEqual(self.check("--recorded").returncode, 1)
        write_result(self.dir, "cline-cli", "passed")
        write_result(self.dir, "codex-cli", "passed", id="opencode-cli")
        self.assertEqual(self.check("--recorded").returncode, 1)

    def test_recorded_ignores_dirs_files(self):
        self.fill()
        with open(os.path.join(self.dir, "claude-cli.dirs.json"), "w") as handle:
            handle.write("not even json")
        self.assertEqual(self.check("--recorded").returncode, 0)

    # default mode: G4
    def test_g4_holds_with_claude_and_one_other(self):
        self.fill(claude_cli="passed", codex_cli="passed")
        proc = self.check()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("codex-cli", proc.stdout)

    def test_g4_fails_when_claude_failed(self):
        self.fill(claude_cli="failed", codex_cli="passed", opencode_cli="passed")
        proc = self.check()
        self.assertEqual(proc.returncode, 1)
        self.assertIn("claude-cli", proc.stderr)

    def test_g4_fails_with_claude_alone(self):
        self.fill(claude_cli="passed", opencode_cli="failed", gemini_cli="unavailable")
        proc = self.check()
        self.assertEqual(proc.returncode, 1)
        self.assertIn("no other candidate", proc.stderr)

    def test_g4_fails_when_the_other_passed_but_claude_is_missing(self):
        self.fill(claude_cli="not-installed", codex_cli="passed")
        self.assertEqual(self.check().returncode, 1)

    def test_g4_fails_when_results_are_missing(self):
        self.assertEqual(self.check().returncode, 1)

    def test_bad_arguments_exit_2(self):
        self.fill()
        self.assertEqual(self.check("--nonsense").returncode, 2)
        self.assertEqual(self.check("--recorded", "--list-tier1").returncode, 2)

    # --list-tier1
    def test_tier1_needs_passed_and_confirmed_folders(self):
        self.fill()
        write_result(self.dir, "claude-cli", "passed", config_dirs=["~/.claude"], config_dirs_confirmed=True)
        write_result(self.dir, "opencode-cli", "passed", config_dirs="unconfirmed", config_dirs_confirmed=False)
        write_result(self.dir, "cline-cli", "failed", config_dirs=["~/.cline"], config_dirs_confirmed=True)
        write_result(self.dir, "codex-cli", "passed", config_dirs=["~/.codex"], config_dirs_confirmed=True)
        self.assertEqual(self.tier1(), ["claude-cli", "codex-cli"])

    def test_tier1_ignores_a_confirmed_flag_with_no_folders(self):
        self.fill()
        write_result(self.dir, "claude-cli", "passed", config_dirs=[], config_dirs_confirmed=True)
        write_result(self.dir, "codex-cli", "passed", config_dirs_confirmed=True)
        self.assertEqual(self.tier1(), [])

    def test_gemini_is_tier1_only_when_antigravity_did_not_pass(self):
        folders = {"config_dirs": ["~/.gemini"], "config_dirs_confirmed": True}
        agy = {"config_dirs": ["~/.antigravity"], "config_dirs_confirmed": True}
        self.fill()
        write_result(self.dir, "gemini-cli", "passed", **folders)
        self.assertEqual(self.tier1(), ["gemini-cli"], "antigravity-cli isn't installed")
        write_result(self.dir, "antigravity-cli", "failed", **agy)
        self.assertEqual(self.tier1(), ["gemini-cli"], "antigravity-cli failed")
        write_result(self.dir, "antigravity-cli", "passed", **agy)
        self.assertEqual(self.tier1(), ["antigravity-cli"], "antigravity-cli took the Google slot")

    def test_an_unavailable_gemini_is_never_tier1(self):
        self.fill(gemini_cli="unavailable")
        self.assertEqual(self.tier1(), [])


    def test_a_deferred_candidate_is_never_tier1(self):
        self.fill()
        for id_ in ("claude-cli", "codex-cli", "opencode-cli"):
            write_result(self.dir, id_, "passed", config_dirs=["~/." + id_], config_dirs_confirmed=True)
        self.assertEqual(self.tier1(), ["claude-cli", "codex-cli"])
        proc = self.check("--list-tier1")
        self.assertIn("opencode-cli left out of Tier 1", proc.stderr)
        self.assertIn("by the owner", proc.stderr)
        self.assertNotIn("opencode-cli", proc.stdout)

    def test_g4_does_not_count_a_deferred_candidate(self):
        self.fill(claude_cli="passed", opencode_cli="passed")
        proc = self.check()
        self.assertEqual(proc.returncode, 1)
        self.assertIn("no other candidate", proc.stderr)
        self.fill(claude_cli="passed", opencode_cli="passed", codex_cli="passed")
        proc = self.check()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("G4 holds: claude-cli and codex-cli passed", proc.stdout)

    def test_only_opencode_is_deferred(self):
        self.fill()
        lines = [l for l in self.check("--list-tier1").stderr.splitlines() if "left out of Tier 1" in l]
        self.assertEqual([l.split()[1] for l in lines], ["opencode-cli"])


if __name__ == "__main__":
    unittest.main()
