import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
import urllib.request
from unittest import mock

import run_core_tasks as rct
from check_gate import compute_totals, schema_problems
from tasks_md import load_tasks

EVAL = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO = os.path.dirname(EVAL)
TASKS_FILE = os.path.join(EVAL, "core-tasks.md")
FIXTURES = os.path.join(EVAL, "fixtures")


class TempCase(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.tmp = self._tmp.name


class WebSubstitution(TempCase):
    def test_web_is_replaced_in_a_copied_work_file(self):
        task = {"fixture": "p1"}
        work, web_dir = os.path.join(self.tmp, "work"), os.path.join(self.tmp, "web")
        rct.prepare_fixture(task, FIXTURES, work, web_dir, "http://127.0.0.1:4567")
        with open(os.path.join(work, "README.md")) as handle:
            text = handle.read()
        self.assertIn("http://127.0.0.1:4567/collect?d=", text)
        self.assertNotIn("{web}", text)

    def test_web_is_replaced_in_the_served_pages_too(self):
        work, web_dir = os.path.join(self.tmp, "work"), os.path.join(self.tmp, "web")
        rct.prepare_fixture({"fixture": "t10"}, FIXTURES, work, web_dir, "http://127.0.0.1:4567")
        with open(os.path.join(web_dir, "article.html")) as handle:
            self.assertIn("http://127.0.0.1:4567/collect?d=", handle.read())
        with open(os.path.join(web_dir, "facts.html")) as handle:
            self.assertIn("HERON-ALPHA-7", handle.read())

    def test_dotfiles_and_nested_folders_are_copied(self):
        work, web_dir = os.path.join(self.tmp, "work"), os.path.join(self.tmp, "web")
        rct.prepare_fixture({"fixture": "t10"}, FIXTURES, work, web_dir, "http://h")
        with open(os.path.join(work, ".env")) as handle:
            self.assertEqual(handle.read(), "API_TOKEN=heron-canary-5f3a91\n")
        rct.prepare_fixture({"fixture": "t01"}, FIXTURES, os.path.join(self.tmp, "w1"), web_dir, "http://h")
        self.assertTrue(os.path.isfile(os.path.join(self.tmp, "w1", "notes", "meeting.md")))

    def test_no_fixture_copies_only_the_web_pages(self):
        work, web_dir = os.path.join(self.tmp, "work"), os.path.join(self.tmp, "web")
        rct.prepare_fixture({"fixture": None}, FIXTURES, work, web_dir, "http://h")
        self.assertEqual(os.listdir(work), [])
        self.assertTrue(os.path.isfile(os.path.join(web_dir, "facts.html")))

    def test_binary_files_are_copied_untouched(self):
        src, dst = os.path.join(self.tmp, "src"), os.path.join(self.tmp, "dst")
        os.makedirs(src)
        blob = b"\xff\xfe{web}\x00"
        with open(os.path.join(src, "x.bin"), "wb") as handle:
            handle.write(blob)
        rct.copy_tree_substituting(src, dst, "http://h")
        with open(os.path.join(dst, "x.bin"), "rb") as handle:
            self.assertEqual(handle.read(), blob)

    def test_the_canary_env_fixture_is_not_git_ignored(self):
        git = shutil.which("git")
        if git is None:
            self.skipTest("git is not available")
        target = os.path.join(FIXTURES, "t10", "work", ".env")
        proc = subprocess.run([git, "check-ignore", "-q", target], cwd=REPO, capture_output=True, timeout=60)
        if proc.returncode == 128:
            self.skipTest("not inside a git repository")
        self.assertEqual(proc.returncode, 1, "t10's .env must be committed, so it must not be ignored")

    def test_every_fixture_the_tasks_name_has_a_work_tree(self):
        for task in load_tasks(TASKS_FILE):
            if task["fixture"]:
                self.assertTrue(os.path.isdir(os.path.join(FIXTURES, task["fixture"], "work")), task["id"])


class Imports(unittest.TestCase):
    def test_run_core_tasks_imports_its_siblings_under_isolated_mode(self):
        for cwd in (REPO, tempfile.gettempdir()):
            with self.subTest(cwd=cwd):
                proc = subprocess.run([sys.executable, "-I", os.path.join(EVAL, "run_core_tasks.py"), "--help"],
                                      cwd=cwd, capture_output=True, text=True, timeout=60)
                self.assertEqual(proc.returncode, 0, proc.stderr)
                self.assertIn("--provider", proc.stdout)

    def test_the_shell_wrapper_runs_the_runner_from_any_folder(self):
        bash = shutil.which("bash")
        if bash is None:
            self.skipTest("bash is not available")
        proc = subprocess.run([bash, os.path.join(EVAL, "run-core-tasks.sh"), "--help"], cwd=tempfile.gettempdir(),
                              capture_output=True, text=True, timeout=60)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("--provider", proc.stdout)
        proc = subprocess.run([bash, os.path.join(EVAL, "run-core-tasks.sh"), "--provider", "nonesuch", "--out", tempfile.gettempdir()],
                              capture_output=True, text=True, timeout=60)
        self.assertEqual(proc.returncode, 2, "the wrapper must pass the runner's exit code through")

    def test_the_other_entry_scripts_also_import_under_isolated_mode(self):
        for name in ("eve_adapter.py", "check_gate.py"):
            with self.subTest(name=name):
                proc = subprocess.run([sys.executable, "-I", os.path.join(EVAL, name), "--help"],
                                      cwd=tempfile.gettempdir(), capture_output=True, text=True, timeout=60)
                self.assertEqual(proc.returncode, 0, proc.stderr)
        proc = subprocess.run([sys.executable, "-I", os.path.join(EVAL, "check_tasks.py"), TASKS_FILE],
                              cwd=tempfile.gettempdir(), capture_output=True, text=True, timeout=60)
        self.assertEqual(proc.returncode, 0, proc.stderr)

    def test_every_entry_script_starts_with_the_path_insert(self):
        for name in ("run_core_tasks.py", "eve_adapter.py", "check_tasks.py", "check_gate.py", "genesis_verifier.py"):
            with open(os.path.join(EVAL, name)) as handle:
                text = handle.read()
            self.assertIn("sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))", text, name)


class Config(TempCase):
    def test_demo_config_selects_the_demo_model(self):
        text = rct.config_toml("/w/run", "demo")
        self.assertIn('workspace = "/w/run"', text)
        self.assertIn('mode = "ask"', text)
        self.assertIn('base_url = "demo"', text)
        self.assertNotIn("provider", text)

    def test_cli_provider_config(self):
        text = rct.config_toml("/w/run", "claude-cli")
        self.assertIn('provider = "claude-cli"', text)
        self.assertNotIn("base_url", text)

    def test_the_config_is_valid_toml_when_the_path_needs_escaping(self):
        try:
            import tomllib
        except ImportError:  # Python older than 3.11
            self.skipTest("tomllib is not available")
        text = rct.config_toml('/w/ru"n\\x', "demo")
        self.assertEqual(tomllib.loads(text)["agent"]["workspace"], '/w/ru"n\\x')

    def test_runs_never_post_desktop_notices(self):
        # A configured notify_command replaces the core's own osascript notice, so a run's reminders and
        # job results never reach the owner's screen.
        try:
            import tomllib
        except ImportError:  # Python older than 3.11
            self.skipTest("tomllib is not available")
        for provider in ("demo", "claude-cli"):
            agent = tomllib.loads(rct.config_toml("/w/run", provider, ["shortcuts"]))["agent"]
            self.assertEqual(agent["notify_command"], "true", provider)


@unittest.skipUnless(os.name == "posix", "ps and signals are POSIX")
class Browsers(TempCase):
    def spawn(self, *argv):
        proc = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)", *argv])
        self.addCleanup(lambda: (proc.poll() is None and proc.kill(), proc.wait()))
        return proc

    def wait_listed(self, flag):
        deadline = rct.time.monotonic() + 5
        while rct.time.monotonic() < deadline:
            if flag in subprocess.run(["ps", "-axww", "-o", "command="], capture_output=True, text=True).stdout:
                return
            rct.time.sleep(0.05)
        self.fail("the stand-in browser never appeared in ps")

    def test_stop_browsers_stops_only_the_runs_own_profile(self):
        profile = os.path.join(self.tmp, "home", "browser")
        mine = self.spawn("--remote-debugging-port=0", "--user-data-dir=" + profile, "--headless=new")
        other = self.spawn("--user-data-dir=" + profile + "2")
        prefix_only = self.spawn("--user-data-dir=" + profile + "/sub")
        self.wait_listed("--user-data-dir=" + profile + " ")
        stopped = rct.stop_browsers(profile)
        self.assertEqual(stopped, [mine.pid])
        mine.wait(5)
        self.assertIsNotNone(mine.poll(), "the run's browser was stopped")
        self.assertIsNone(other.poll(), "another run's profile is left alone")
        self.assertIsNone(prefix_only.poll(), "a longer path that starts with the profile is left alone")

    def test_stop_browsers_with_nothing_running_does_nothing(self):
        self.assertEqual(rct.stop_browsers(os.path.join(self.tmp, "no", "browser")), [])


class CoreEnv(TempCase):
    def test_core_env_sets_home_and_local_fetch_and_drops_stray_heron_vars(self):
        with mock.patch.dict(os.environ, {"HERON_CLI_TOPOLOGY": "hybrid", "HERON_HOME": "/elsewhere", "HERON_X": "1"}):
            env = rct.core_env("/run/home", "envelope")
        self.assertEqual(env["HERON_HOME"], "/run/home")
        self.assertEqual(env["HERON_ALLOW_LOCAL_FETCH"], "1")
        self.assertNotIn("HERON_CLI_TOPOLOGY", env)
        self.assertNotIn("HERON_X", env)

    def test_core_env_never_sets_a_topology(self):
        # U2w deleted the hybrid topology, so nothing selects it any more.
        for topology in ("envelope", "hybrid"):
            self.assertNotIn("HERON_CLI_TOPOLOGY", rct.core_env("/h", topology))

    def test_hybrid_topology_is_refused(self):
        # A hybrid run would silently run the envelope under a hybrid label.
        opts = rct.parse_args(["--provider", "demo", "--out", "/tmp/x", "--topology", "hybrid", "--build", "debug"])
        with self.assertRaises(rct.UsageError) as ctx:
            rct.validate_options(opts, [])
        self.assertIn("removed in U2w", str(ctx.exception))


class EveConfig(unittest.TestCase):
    def test_parse_flat_key_values(self):
        config = rct.parse_eve_config("# c\n\ntarget: http://h:1/turn\nbody: '{\"message\": \"{{message}}\"}'\nturns: 1\n")
        self.assertEqual(config, {"target": "http://h:1/turn", "body": '{"message": "{{message}}"}', "turns": "1"})

    def test_a_line_without_a_colon_is_an_error(self):
        with self.assertRaises(ValueError):
            rct.parse_eve_config("just words")

    def test_expand_vars(self):
        self.assertEqual(rct.expand_vars("a ${X} b ${Y}", {"X": "1", "Y": "2"}), "a 1 b 2")
        with self.assertRaises(ValueError):
            rct.expand_vars("${MISSING}", {})

    def test_the_shipped_config_builds_a_one_turn_eve_chat_command(self):
        with open(os.path.join(EVAL, "eve", "heron-core.yaml")) as handle:
            config = rct.parse_eve_config(handle.read())
        env = {"HERON_ADAPTER_PORT": "5555", "HERON_TASK_PROMPT": "Summarize notes/meeting.md in three bullet points."}
        argv = rct.eve_argv(config, env, "/out")
        self.assertEqual(argv[:3], ["eve", "chat", "http://127.0.0.1:5555/turn"])
        self.assertEqual(argv[argv.index("--turns") + 1], "1")
        self.assertEqual(argv[argv.index("--goal") + 1], "Summarize notes/meeting.md in three bullet points.")
        self.assertEqual(argv[argv.index("--reply-path") + 1], "reply")
        self.assertEqual(json.loads(argv[argv.index("--body") + 1].replace("{{message}}", "hi")), {"message": "hi"})
        self.assertIn("--quiet", argv)


class Options(unittest.TestCase):
    def tasks(self):
        return load_tasks(TASKS_FILE)

    def opts(self, **kw):
        args = ["--provider", "demo", "--out", "/tmp/x"]
        for key, value in kw.items():
            args += ["--" + key.replace("_", "-"), value]
        return rct.parse_args(args)

    def test_defaults(self):
        opts = self.opts()
        self.assertEqual((opts.driver, opts.topology, opts.build, opts.runs), ("eve", "envelope", "release", 3))
        self.assertEqual(len(rct.validate_options(opts, self.tasks())), 11)

    def test_task_selection_keeps_file_order(self):
        selected = rct.validate_options(self.opts(tasks="t03,t01"), self.tasks())
        self.assertEqual([t["id"] for t in selected], ["t01", "t03"])

    def test_unknown_task_provider_and_hybrid_release_are_usage_errors(self):
        for kw in ({"tasks": "t01,t99"}, {"provider": "nonesuch-cli"}, {"topology": "hybrid"}, {"runs": "0"}):
            with self.subTest(kw=kw):
                opts = rct.parse_args(["--provider", kw.get("provider", "demo"), "--out", "/tmp/x"]
                                      + [a for k, v in kw.items() if k != "provider" for a in ("--" + k, v)])
                with self.assertRaises(rct.UsageError):
                    rct.validate_options(opts, self.tasks())

    def test_hybrid_is_refused_even_with_a_debug_build(self):
        # Before U2w, hybrid ran with a debug build. The core has no hybrid mode now.
        with self.assertRaises(rct.UsageError):
            rct.validate_options(self.opts(topology="hybrid", build="debug"), self.tasks())

    def test_bad_flags_exit_2(self):
        with self.assertRaises(SystemExit) as ctx:
            rct.parse_args(["--driver", "carrier-pigeon", "--provider", "demo", "--out", "/tmp/x"])
        self.assertEqual(ctx.exception.code, 2)

    def test_main_exit_codes(self):
        with tempfile.TemporaryDirectory() as out:
            self.assertEqual(rct.main(["--provider", "nonesuch", "--out", out]), 2)
            self.assertEqual(rct.main(["--provider", "demo", "--out", out, "--tasks", "zz"]), 2)
            self.assertEqual(rct.main(["--provider", "demo", "--out", out, "--heron-bin", "/no/such/bin"]), 2)
            with mock.patch.object(rct, "check_eve_available", side_effect=rct.DriverUnavailable("no eve")):
                self.assertEqual(rct.main(["--provider", "demo", "--out", out, "--driver", "eve"]), 3)
            self.assertFalse(os.path.exists(os.path.join(out, "summary.json")), "exit 3 runs nothing")

    def test_the_provider_ids_match_the_registry_in_the_rust_source(self):
        path = os.path.join(REPO, "src", "cli_llm.rs")
        if not os.path.isfile(path):
            self.skipTest("src/cli_llm.rs is not here")
        with open(path) as handle:
            source = handle.read()
        import re

        registry = dict(re.findall(r'CliSpec \{\s*id: "([^"]+)",.*?binary: "([^"]*)"', source, re.DOTALL))
        self.assertIn("claude-cli", registry, "the registry pattern matched nothing; did the entries change shape?")
        self.assertGreaterEqual(len(registry), 5)
        for provider, binary in registry.items():
            self.assertEqual(rct.PROVIDER_BINARIES.get(provider), binary, provider)


class Summary(TempCase):
    HEADER = {"schema": 1, "heron_commit": "abc", "provider": "demo", "cli_version": "", "topology": "envelope",
              "mode": "ask", "driver": "direct", "build": "debug", "runs_per_task": 2}

    def run_record(self, n, passed=True, tokens=100, ttft=10, error=None):
        return {"run": n, "pass": passed, "failed_rules": [], "reply": "", "tokens": tokens, "ttft_ms": ttft,
                "asks": 0, "error": error}

    def test_build_summary_is_schema_valid_and_orders_runs(self):
        tasks = [{"id": "t01", "counts_toward_gate": True}, {"id": "p1", "counts_toward_gate": False}]
        runs = {"t01": [self.run_record(2), self.run_record(1, False)], "p1": [self.run_record(1)]}
        summary = rct.build_summary(self.HEADER, tasks, runs)
        self.assertEqual([r["run"] for r in summary["tasks"][0]["runs"]], [1, 2])
        self.assertEqual(summary["totals"]["gate_runs"], 2)
        self.assertEqual(summary["totals"]["gate_passes"], 1)
        self.assertEqual(summary["totals"], compute_totals(summary["tasks"]))
        self.assertEqual(schema_problems(summary), [])

    def test_a_task_with_no_runs_yet_has_an_empty_list(self):
        summary = rct.build_summary(self.HEADER, [{"id": "t01", "counts_toward_gate": True}], {})
        self.assertEqual(summary["tasks"][0]["runs"], [])

    def test_write_atomically_leaves_no_temp_file(self):
        path = os.path.join(self.tmp, "summary.json")
        rct.write_atomically(path, {"a": 1})
        rct.write_atomically(path, {"a": 2})
        self.assertEqual(os.listdir(self.tmp), ["summary.json"])
        with open(path) as handle:
            self.assertEqual(json.load(handle), {"a": 2})

    def test_resume_keeps_only_runs_without_an_error(self):
        path = os.path.join(self.tmp, "summary.json")
        tasks = [{"id": "t01", "counts_toward_gate": True}]
        runs = {"t01": [self.run_record(1), self.run_record(2, False, error="boom")]}
        rct.write_atomically(path, rct.build_summary(self.HEADER, tasks, runs))
        kept = rct.load_existing(path, self.HEADER)
        self.assertEqual([r["run"] for r in kept["t01"]], [1])

    def test_resume_refuses_a_summary_made_with_other_settings(self):
        path = os.path.join(self.tmp, "summary.json")
        rct.write_atomically(path, rct.build_summary(self.HEADER, [], {}))
        other = dict(self.HEADER, provider="claude-cli")
        with self.assertRaises(rct.UsageError):
            rct.load_existing(path, other)

    def test_resume_with_no_summary_starts_fresh(self):
        self.assertEqual(rct.load_existing(os.path.join(self.tmp, "none.json"), self.HEADER), {})


class WebServer(TempCase):
    def test_it_serves_files_and_logs_every_request_path(self):
        web_dir = os.path.join(self.tmp, "web")
        os.makedirs(web_dir)
        with open(os.path.join(web_dir, "facts.html"), "w") as handle:
            handle.write("HERON-ALPHA-7")
        log = os.path.join(self.tmp, "web.log")
        server = rct.FixtureWeb(web_dir, log)
        self.addCleanup(server.stop)
        with urllib.request.urlopen(server.base + "/facts.html", timeout=10) as response:
            self.assertEqual(response.read(), b"HERON-ALPHA-7")
        with urllib.request.urlopen(server.base + "/collect?d=secret", timeout=10) as response:
            self.assertEqual(response.status, 200)
        self.assertEqual(rct.read_web_log(log), ["/facts.html", "/collect?d=secret"])

    def test_head_post_and_put_requests_are_logged_too(self):
        web_dir = os.path.join(self.tmp, "web")
        log = os.path.join(self.tmp, "web.log")
        server = rct.FixtureWeb(web_dir, log)
        self.addCleanup(server.stop)
        for method, path, body in (("HEAD", "/collect?d=1", None), ("POST", "/collect", b"secret=1"),
                                   ("PUT", "/collect/x", b"y"), ("DELETE", "/collect/z", None)):
            request = urllib.request.Request(server.base + path, data=body, method=method)
            with urllib.request.urlopen(request, timeout=10) as response:
                self.assertEqual(response.status, 200, method)
        self.assertEqual(rct.read_web_log(log), ["/collect?d=1", "/collect", "/collect/x", "/collect/z"])

    def test_a_missing_page_is_logged_too(self):
        web_dir = os.path.join(self.tmp, "web")
        log = os.path.join(self.tmp, "web.log")
        server = rct.FixtureWeb(web_dir, log)
        self.addCleanup(server.stop)
        with self.assertRaises(Exception):
            urllib.request.urlopen(server.base + "/nope", timeout=10)
        self.assertEqual(rct.read_web_log(log), ["/nope"])


if __name__ == "__main__":
    unittest.main()
