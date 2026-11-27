import json
import os
import shutil
import sqlite3
import stat
import subprocess
import tempfile
import time
import unittest

EVAL = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO = os.path.dirname(EVAL)
BASH = shutil.which("bash")
SQLITE3 = shutil.which("sqlite3")


def bash(script, *args, env=None, cwd=None):
    full_env = dict(os.environ)
    full_env.update(env or {})
    return subprocess.run([BASH, os.path.join(EVAL, script)] + list(args), capture_output=True, text=True,
                          env=full_env, cwd=cwd, timeout=120)


@unittest.skipUnless(BASH, "bash is not available")
class RequireTests(unittest.TestCase):
    LISTING = "\n".join([
        "tools::tests::every_registry_config_dir_is_refused_in_the_sandbox: test",
        "permissions::tests::read_verdict_table: test",
        "permissions::tests::refused_beats_secret_in_every_mode: test",
        "store::tests::some_bench: benchmark",
        "",
        "3 tests, 1 benchmark",
    ])

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.fake = os.path.join(self._tmp.name, "cargo")
        self.write_cargo('printf "%s\\n" "$LISTING"')

    def write_cargo(self, body):
        with open(self.fake, "w") as handle:
            handle.write("#!/usr/bin/env bash\necho \"$@\" > \"%s/argv\"\n%s\n" % (self._tmp.name, body))
        os.chmod(self.fake, os.stat(self.fake).st_mode | stat.S_IXUSR)

    def require(self, *args):
        return bash("require-tests.sh", *args, env={"CARGO": self.fake, "LISTING": self.LISTING})

    def test_all_names_present_exits_0(self):
        proc = self.require("--lib", "--", "read_verdict_table", "refused_beats_secret_in_every_mode")
        self.assertEqual(proc.returncode, 0, proc.stderr)

    def test_a_missing_name_exits_1_and_names_it(self):
        proc = self.require("--lib", "--", "read_verdict_table", "renamed_test")
        self.assertEqual(proc.returncode, 1)
        self.assertIn("renamed_test", proc.stderr)
        self.assertNotIn("read_verdict_table", proc.stderr)

    def test_every_missing_name_is_reported(self):
        proc = self.require("--lib", "--", "nope_one", "nope_two")
        self.assertEqual(proc.returncode, 1)
        self.assertIn("nope_one", proc.stderr)
        self.assertIn("nope_two", proc.stderr)

    def test_a_name_is_not_a_prefix_match(self):
        proc = self.require("--lib", "--", "every_registry_config_dir_is_refused")
        self.assertEqual(proc.returncode, 1)
        proc = self.require("--lib", "--", "every_registry_config_dir_is_refused_in_the_sandbox")
        self.assertEqual(proc.returncode, 0, proc.stderr)

    def test_a_name_is_not_a_suffix_or_substring_match(self):
        self.assertEqual(self.require("--lib", "--", "table").returncode, 1)
        self.assertEqual(self.require("--lib", "--", "verdict").returncode, 1)

    def test_only_the_last_path_component_counts(self):
        self.assertEqual(self.require("--lib", "--", "tests::read_verdict_table").returncode, 1)

    def test_a_benchmark_is_not_a_test(self):
        self.assertEqual(self.require("--lib", "--", "some_bench").returncode, 1)

    def test_the_selection_and_list_flags_reach_cargo(self):
        self.require("--test", "integration", "--", "read_verdict_table")
        with open(os.path.join(self._tmp.name, "argv")) as handle:
            self.assertEqual(handle.read().split(), ["test", "--locked", "--test", "integration", "--", "--list"])

    def test_an_empty_selection_is_allowed(self):
        proc = self.require("--", "read_verdict_table")
        self.assertEqual(proc.returncode, 0, proc.stderr)

    def test_a_failing_cargo_exits_1(self):
        self.write_cargo("exit 101")
        proc = self.require("--lib", "--", "read_verdict_table")
        self.assertEqual(proc.returncode, 1)
        self.assertIn("failed", proc.stderr)

    def test_a_failing_cargo_stderr_is_shown_before_the_summary_line(self):
        self.write_cargo("echo 'error[E0425]: cannot find value `nope` in this scope' >&2\nexit 101")
        proc = self.require("--lib", "--", "read_verdict_table")
        self.assertEqual(proc.returncode, 1)
        self.assertIn("error[E0425]: cannot find value `nope`", proc.stderr)
        self.assertLess(proc.stderr.index("E0425"), proc.stderr.index("require-tests:"))

    def test_usage_errors_exit_2(self):
        self.assertEqual(self.require("--lib").returncode, 2)
        self.assertEqual(self.require("--lib", "--").returncode, 2)
        self.assertEqual(self.require().returncode, 2)


def noon(day):
    return int(time.mktime(time.strptime(day + " 12", "%Y-%m-%d %H")))


@unittest.skipUnless(BASH and SQLITE3, "bash and sqlite3 are needed")
class OwnerUse(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.home = self._tmp.name
        self.db = os.path.join(self.home, "heron.db")

    def make_db(self, rows, version=3):
        conn = sqlite3.connect(self.db)
        conn.executescript("CREATE TABLE sessions(id TEXT PRIMARY KEY, origin TEXT NOT NULL DEFAULT 'user');"
                           "CREATE TABLE audit(id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, session_id TEXT NOT NULL, "
                           "tool TEXT NOT NULL DEFAULT 'x', decision TEXT NOT NULL);")
        origins = {}
        for i, (day, origin, decision) in enumerate(rows):
            sid = origins.setdefault(origin, "s-" + origin)
            conn.execute("INSERT OR IGNORE INTO sessions(id, origin) VALUES (?, ?)", (sid, origin))
            conn.execute("INSERT INTO audit(ts, session_id, decision) VALUES (?, ?, ?)", (noon(day), sid, decision))
        conn.execute("PRAGMA user_version = %d" % version)
        conn.commit()
        conn.close()

    def count(self, *args):
        return bash("owner-use.sh", self.home, *args)

    def test_counts_distinct_days_and_prints_them(self):
        self.make_db([("2026-12-01", "user", "approved"), ("2026-12-01", "user", "allowed"),
                      ("2026-12-03", "telegram", "approved-always")])
        proc = self.count("2026-12-01", "2026-12-14")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(proc.stdout.split(), ["2026-12-01", "2026-12-03", "qualifying_days=2"])

    def test_only_executed_decisions_count(self):
        self.make_db([("2026-12-01", "user", "denied-by-user"), ("2026-12-02", "user", "refused"),
                      ("2026-12-03", "user", "approved-later"), ("2026-12-04", "user", "approved-for-conversation")])
        self.assertIn("qualifying_days=2", self.count("2026-12-01", "2026-12-14").stdout)

    def test_only_user_and_telegram_origins_count(self):
        self.make_db([("2026-12-01", "schedule", "approved"), ("2026-12-02", "proactive", "allowed"),
                      ("2026-12-03", "telegram", "approved")])
        proc = self.count("2026-12-01", "2026-12-14")
        self.assertEqual(proc.stdout.split(), ["2026-12-03", "qualifying_days=1"])

    def test_the_window_is_inclusive_and_excludes_outside_days(self):
        self.make_db([("2026-11-30", "user", "approved"), ("2026-12-01", "user", "approved"),
                      ("2026-12-14", "user", "approved"), ("2026-12-15", "user", "approved")])
        self.assertEqual(self.count("2026-12-01", "2026-12-14").stdout.split(),
                         ["2026-12-01", "2026-12-14", "qualifying_days=2"])

    def test_no_activity_counts_zero(self):
        self.make_db([])
        proc = self.count("2026-12-01", "2026-12-14")
        self.assertEqual((proc.returncode, proc.stdout.strip()), (0, "qualifying_days=0"))

    def test_min_passes_at_exactly_k_and_fails_below(self):
        days = ["2026-12-%02d" % d for d in range(1, 13)]
        self.make_db([(d, "user", "approved") for d in days])
        self.assertEqual(self.count("2026-12-01", "2026-12-14", "--min", "12").returncode, 0)
        self.assertEqual(self.count("2026-12-01", "2026-12-14", "--min", "13").returncode, 1)

    def test_without_min_a_low_count_still_exits_0(self):
        self.make_db([("2026-12-01", "user", "approved")])
        self.assertEqual(self.count("2026-12-01", "2026-12-14").returncode, 0)

    def test_with_min_the_window_must_be_14_days(self):
        self.make_db([("2026-12-01", "user", "approved")])
        for to in ("2026-12-13", "2026-12-15", "2026-12-01"):
            with self.subTest(to=to):
                proc = self.count("2026-12-01", to, "--min", "1")
                self.assertEqual(proc.returncode, 2)
                self.assertIn("14", proc.stderr)
        self.assertEqual(self.count("2026-12-01", "2026-12-14", "--min", "1").returncode, 0)

    def test_the_14_day_window_spans_a_month_end(self):
        self.make_db([("2026-12-25", "user", "approved")])
        self.assertEqual(self.count("2026-12-25", "2027-01-07", "--min", "1").returncode, 0)

    def test_bad_dates_exit_2(self):
        self.make_db([])
        for args in (("2026-1-01", "2026-12-14"), ("2026-12-01", "tomorrow"), ("2026-13-01", "2026-14-14"),
                     ("2026-02-30", "2026-03-15"), ("2026-12-14", "2026-12-01"), ("20261201", "20261214")):
            with self.subTest(args=args):
                self.assertEqual(self.count(*args).returncode, 2)

    def test_old_data_format_exits_2(self):
        self.make_db([("2026-12-01", "user", "approved")], version=2)
        proc = self.count("2026-12-01", "2026-12-14")
        self.assertEqual(proc.returncode, 2)
        self.assertIn("needs data format 3", proc.stderr)

    def test_format_4_is_fine(self):
        self.make_db([("2026-12-01", "user", "approved")], version=4)
        self.assertEqual(self.count("2026-12-01", "2026-12-14").returncode, 0)

    def test_missing_database_and_usage_errors_exit_2(self):
        self.assertEqual(self.count("2026-12-01", "2026-12-14").returncode, 2)
        self.make_db([])
        self.assertEqual(bash("owner-use.sh", self.home, "2026-12-01").returncode, 2)
        self.assertEqual(self.count("2026-12-01", "2026-12-14", "--min").returncode, 2)
        self.assertEqual(self.count("2026-12-01", "2026-12-14", "--min", "x").returncode, 2)
        self.assertEqual(self.count("2026-12-01", "2026-12-14", "--max", "3").returncode, 2)

    def test_the_database_is_opened_read_only(self):
        self.make_db([("2026-12-01", "user", "approved")])
        before = os.stat(self.db).st_mtime_ns
        self.count("2026-12-01", "2026-12-14")
        self.assertEqual(os.stat(self.db).st_mtime_ns, before)


@unittest.skipUnless(BASH, "bash is not available")
class DiscoverDirs(unittest.TestCase):
    def setUp(self):
        self._out = tempfile.TemporaryDirectory()
        self.addCleanup(self._out.cleanup)
        self.out = os.path.join(self._out.name, "claude-cli.dirs.json")

    def run_discover(self, id_, login=None):
        env = {"DISCOVER_DIRS_OUT_DIR": self._out.name}
        if login is not None:
            env["LOGIN_COMMAND"] = login
        return bash(os.path.join("containment", "discover-dirs.sh"), id_, env=env)

    def test_unknown_id_exits_2(self):
        self.assertEqual(self.run_discover("no-such-cli").returncode, 2)
        self.assertEqual(self.run_discover("../etc").returncode, 2)

    def test_a_placeholder_in_the_login_command_exits_2(self):
        proc = self.run_discover("cline-cli")
        self.assertEqual(proc.returncode, 2)
        self.assertIn("LOGIN_COMMAND", proc.stderr)

    def test_a_failing_login_exits_1_and_writes_nothing(self):
        proc = self.run_discover("claude-cli", "exit 3")
        self.assertEqual(proc.returncode, 1)
        self.assertFalse(os.path.exists(self.out))

    def test_it_lists_what_the_login_created_as_home_relative_paths(self):
        proc = self.run_discover("claude-cli", 'mkdir -p "$HOME/.claude" && touch "$HOME/.claude/auth" "$HOME/.claude.json"')
        self.assertEqual(proc.returncode, 0, proc.stderr)
        with open(self.out) as handle:
            data = json.load(handle)
        self.assertEqual(data["id"], "claude-cli")
        self.assertEqual(data["paths"], ["~/.claude", "~/.claude.json", "~/.claude/auth"])

    def test_the_login_runs_under_a_fresh_home(self):
        proc = self.run_discover("claude-cli", 'test "$HOME" != "%s" && touch "$HOME/marker"' % os.environ.get("HOME", ""))
        self.assertEqual(proc.returncode, 0, proc.stderr)
        with open(self.out) as handle:
            self.assertEqual(json.load(handle)["paths"], ["~/marker"])


if __name__ == "__main__":
    unittest.main()
