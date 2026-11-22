#!/usr/bin/env python3
"""Run Heron's core tasks against a built core and write ``summary.json`` (U0, item 4).

Run through ``bash eval/run-core-tasks.sh``, which calls ``python3 -I eval/run_core_tasks.py``.
``--task-file <file>`` reads the tasks from another file (feature tasks, eval/features/README.md).
Exit codes: 0 every requested run executed; 1 at least one run still has an error after one
retry; 2 usage error; 3 the ``eve`` driver can't run at all. Standard library only.
"""

import argparse
import http.server
import json
import os
import re
import shutil
import signal
import sqlite3
import subprocess
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import eve_adapter  # noqa: E402
import pass_rules  # noqa: E402
from check_gate import compute_totals  # noqa: E402
from tasks_md import TasksError, fixtures_root_for, load_tasks  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(HERE)
SCHEMA = 1
URL_LINE = re.compile(r"http://127\.0\.0\.1:(\d+)/\?token=(\w+)")
CORE_START_SECS = 60
ADAPTER_START_SECS = 30
EVE_CLIENT_SECS = 900
ASK_DECISIONS = ("approved", "approved-always", "approved-for-conversation", "denied-by-user")
REPLY_CHARS = 2000

# The provider ids in `src/cli_llm.rs` (`providers()`) and the binary each one runs.
PROVIDER_BINARIES = {
    "claude-cli": "claude", "codex-cli": "codex", "cursor-cli": "cursor-agent", "grok-cli": "grok",
    "opencode-cli": "opencode", "antigravity-cli": "agy", "cline-cli": "cline", "copilot-cli": "copilot",
    "qwen-cli": "qwen", "gemini-cli": "gemini", "pi-cli": "pi", "fx-cli": "fx", "kiro-cli": "kiro-cli",
    "muse-cli": "muse", "goose-cli": "goose", "custom-cli": "",
}
SEMVER = re.compile(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?")


class UsageError(Exception):
    """Bad flags, an unknown task id or an unknown provider (exit 2)."""


class DriverUnavailable(Exception):
    """The ``eve`` driver can't run at all (exit 3)."""


class RunError(Exception):
    """One run failed for a reason that is not a failed pass rule."""


# ----------------------------------------------------------------------------- fixtures

def substitute_web(text, web_base):
    return text.replace("{web}", web_base)


def copy_tree_substituting(src, dst, web_base):
    """Copy ``src`` into ``dst``, replacing ``{web}`` in every text file."""
    for root, dirs, files in os.walk(src):
        rel = os.path.relpath(root, src)
        target = dst if rel == "." else os.path.join(dst, rel)
        os.makedirs(target, exist_ok=True)
        for name in files:
            source = os.path.join(root, name)
            with open(source, "rb") as handle:
                data = handle.read()
            try:
                data = substitute_web(data.decode("utf-8"), web_base).encode("utf-8")
            except UnicodeDecodeError:
                pass  # binary fixtures are copied as they are
            out = os.path.join(target, name)
            with open(out, "wb") as handle:
                handle.write(data)
            shutil.copymode(source, out)


def fixture_dir(task, fixtures_root):
    """The task's fixture folder (``core`` ones are ``t01``; feature ones ``features/<slug>``), or None."""
    fixture = task.get("fixture")
    if not fixture:
        return None
    root = os.path.realpath(fixtures_root)
    path = os.path.realpath(os.path.join(root, fixture))
    if path == root or not path.startswith(root + os.sep):
        raise RunError("fixture %r is outside the fixtures folder" % fixture)
    return path


def prepare_fixture(task, fixtures_root, workdir, web_dir, web_base):
    """Copy the task's ``work/`` tree and the web pages, with ``{web}`` replaced.

    The pages served are the shared ``<fixtures>/web/`` plus the task's own ``web/``, which wins
    when both hold the same file."""
    os.makedirs(workdir, exist_ok=True)
    fixture = fixture_dir(task, fixtures_root)
    if fixture:
        work = os.path.join(fixture, "work")
        if os.path.isdir(work):
            copy_tree_substituting(work, workdir, web_base)
    web = os.path.join(fixtures_root, "web")
    if os.path.isdir(web):
        copy_tree_substituting(web, web_dir, web_base)
    if fixture and os.path.isdir(os.path.join(fixture, "web")):
        copy_tree_substituting(os.path.join(fixture, "web"), web_dir, web_base)


def prepare_bins(task, fixtures_root, bin_dir, web_base):
    """Copy the task's ``bin/`` into ``bin_dir`` (executable) and return ``{env var: absolute path}``
    for the task's ``bins``."""
    bins = task.get("bins") or {}
    if not bins:
        return {}
    fixture = fixture_dir(task, fixtures_root)
    source = os.path.join(fixture, "bin") if fixture else None
    if not source or not os.path.isdir(source):
        raise RunError("the task has bins but its fixture has no bin/ folder")
    copy_tree_substituting(source, bin_dir, web_base)
    env = {}
    for var, name in bins.items():
        path = os.path.join(bin_dir, name)
        if not os.path.isfile(path):
            raise RunError("bins %s: %r is not a file in the fixture's bin/" % (var, name))
        os.chmod(path, os.stat(path).st_mode | 0o111)
        env[var] = os.path.abspath(path)
    return env


# ----------------------------------------------------------------------------- web server

class _WebHandler(http.server.SimpleHTTPRequestHandler):
    """Serves the web fixtures and logs the path of every request, whatever its method, so
    ``no_request`` can't be dodged with a HEAD or a POST."""

    def log_message(self, fmt, *args):
        pass

    def _log(self):
        with self.server.log_lock:
            with open(self.server.log_path, "a", encoding="utf-8") as handle:
                handle.write(self.path + "\n")

    def _bare_ok(self):
        """Answer a request that isn't a file fetch: drain any body and say 200."""
        length = int(self.headers.get("content-length", "0") or 0)
        if length:
            self.rfile.read(length)
        self.send_response(200)
        self.send_header("content-type", "text/plain")
        self.send_header("content-length", "3")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(b"ok\n")

    def do_GET(self):
        self._log()
        if self.path.split("?")[0].split("#")[0].startswith("/collect"):
            return self._bare_ok()
        super().do_GET()

    def do_HEAD(self):
        self._log()
        if self.path.split("?")[0].split("#")[0].startswith("/collect"):
            return self._bare_ok()
        super().do_HEAD()

    def do_POST(self):
        self._log()
        self._bare_ok()

    do_PUT = do_DELETE = do_PATCH = do_POST


class FixtureWeb:
    """The fixture web server: serves ``web_dir`` on 127.0.0.1 and logs each request path."""

    def __init__(self, web_dir, log_path):
        os.makedirs(web_dir, exist_ok=True)
        open(log_path, "a").close()
        handler = lambda *a, **k: _WebHandler(*a, directory=web_dir, **k)  # noqa: E731
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        self.server.daemon_threads = True
        self.server.log_path = log_path
        self.server.log_lock = threading.Lock()
        self.base = "http://127.0.0.1:%d" % self.server.server_address[1]
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def stop(self):
        self.server.shutdown()
        self.server.server_close()


def read_web_log(path):
    try:
        with open(path, "r", encoding="utf-8") as handle:
            return [line.rstrip("\n") for line in handle if line.strip()]
    except OSError:
        return []


# ----------------------------------------------------------------------------- the core

def toml_string(value):
    return json.dumps(value)


BARE_KEY = re.compile(r"^[A-Za-z0-9_-]+$")


def toml_value(value):
    """A TOML literal for a bool, an integer, a string or a list of those."""
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, str):
        return json.dumps(value)
    if isinstance(value, list):
        return "[" + ", ".join(toml_value(item) for item in value) + "]"
    raise ValueError("cannot write %r as a TOML value" % (value,))


def features_table(features, config):
    """The ``[features]`` entries of a task: each key of ``features`` on, then ``config`` over them."""
    entries = {}
    for key in features or []:
        entries[key] = True
    entries.update(config or {})
    for key in entries:
        if not isinstance(key, str) or not BARE_KEY.match(key):
            raise ValueError("%r is not a valid [features] key" % (key,))
    return entries


def config_toml(workdir, provider, features=None, config=None):
    # notify_command replaces the core's own desktop notice, so a run's reminders and job results never
    # pop up on the owner's screen.
    lines = ["[agent]", "workspace = %s" % toml_string(workdir), 'notify_command = "true"', "",
             "[permissions]", 'mode = "ask"', "", "[llm]"]
    if provider == "demo":
        lines.append('base_url = "demo"')
    else:
        lines.append("provider = %s" % toml_string(provider))
    entries = features_table(features, config)
    if entries:
        lines += ["", "[features]"] + ["%s = %s" % (key, toml_value(value)) for key, value in entries.items()]
    return "\n".join(lines) + "\n"


def core_env(home, topology, extra=None):
    """The core's environment. ``extra`` (a task's ``bins``) is added after every stray ``HERON_`` variable is dropped."""
    env = {k: v for k, v in os.environ.items() if not k.startswith("HERON_")}
    env["HERON_HOME"] = home
    env["HERON_ALLOW_LOCAL_FETCH"] = "1"
    env.update(extra or {})
    return env


class Core:
    """A running ``heron serve`` and the endpoint it printed."""

    def __init__(self, binary, home, topology, log_dir, extra_env=None):
        self.out_path = os.path.join(log_dir, "core.out.log")
        self.err_file = open(os.path.join(log_dir, "core.err.log"), "w")
        self.proc = subprocess.Popen([binary, "serve", "--no-open", "--port", "0"],
                                     env=core_env(home, topology, extra_env),
                                     stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=self.err_file,
                                     text=True, bufsize=1)
        self.port = None
        self.token = None
        self._found = threading.Event()
        self._reader = threading.Thread(target=self._pump, daemon=True)
        self._reader.start()
        if not self._found.wait(CORE_START_SECS):
            code = self.proc.poll()
            self.stop()
            raise RunError("the core did not print its URL within %ds%s" % (
                CORE_START_SECS, "" if code is None else " (it exited with status %s)" % code))
        self.base = "http://127.0.0.1:%s" % self.port

    def _pump(self):
        with open(self.out_path, "w") as log:
            for line in self.proc.stdout:
                log.write(line)
                log.flush()
                match = URL_LINE.search(line)
                if match and not self._found.is_set():
                    self.port, self.token = match.group(1), match.group(2)
                    self._found.set()

    def stop(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
        self._reader.join(5)
        self.err_file.close()


def http_request(base, token, method, path, body=None):
    import urllib.error
    import urllib.request

    data = None if body is None else json.dumps(body).encode("utf-8")
    request = urllib.request.Request(base + path, data=data, method=method,
                                     headers={"x-heron-token": token, "content-type": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.status, response.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as exc:
        return exc.code, exc.read().decode("utf-8", errors="replace")


def accept_risk_notice(base, token):
    """``GET /api/risk-notice`` then ``POST`` its version. A 404 (today's ``main``) is skipped."""
    status, text = http_request(base, token, "GET", "/api/risk-notice")
    if status == 404:
        return False
    if status != 200:
        raise RunError("GET /api/risk-notice answered %d" % status)
    try:
        version = json.loads(text)["version"]
    except (ValueError, KeyError, TypeError):
        raise RunError("GET /api/risk-notice did not return a version: %s" % text[:200])
    status, text = http_request(base, token, "POST", "/api/risk-notice", {"version": version})
    if status not in (200, 204):
        raise RunError("POST /api/risk-notice answered %d: %s" % (status, text[:200]))
    return True


def apply_seed_sql(db_path, statements):
    if not statements:
        return
    conn = sqlite3.connect(db_path, timeout=30)
    try:
        for statement in statements:
            conn.execute(statement)
        conn.commit()
    finally:
        conn.close()


def count_asks(db_path, session_id):
    marks = ",".join("?" for _ in ASK_DECISIONS)
    conn = sqlite3.connect("file:%s?mode=ro" % db_path, uri=True, timeout=10)
    try:
        row = conn.execute("SELECT COUNT(*) FROM audit WHERE session_id = ? AND decision IN (%s)" % marks,
                           (session_id,) + ASK_DECISIONS).fetchone()
    finally:
        conn.close()
    return row[0]


# ----------------------------------------------------------------------------- EVE driver

def parse_eve_config(text):
    """Read ``eval/eve/heron-core.yaml``: flat ``key: value`` lines, ``#`` comments, optional quotes."""
    config = {}
    for number, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if ":" not in line:
            raise ValueError("line %d is not 'key: value': %r" % (number, raw))
        key, value = line.split(":", 1)
        value = value.strip()
        if len(value) >= 2 and value[0] == value[-1] and value[0] in "'\"":
            value = value[1:-1]
        config[key.strip()] = value
    return config


def expand_vars(value, env):
    def repl(match):
        name = match.group(1)
        if name not in env:
            raise ValueError("%s is not set" % name)
        return env[name]

    return re.sub(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}", repl, value)


def eve_argv(config, env, out_dir):
    """The ``eve chat`` argv for one scripted turn, from the config file's keys."""
    cfg = {key: expand_vars(value, env) for key, value in config.items()}
    return ["eve", "chat", cfg["target"], "--kind", cfg["kind"], "--persona", cfg["persona"],
            "--goal", cfg["goal"], "--turns", cfg["turns"], "--reply-path", cfg["reply_path"],
            "--body", cfg["body"], "--out", out_dir, "--quiet"]


def check_eve_available():
    exe = shutil.which("eve")
    if exe is None:
        raise DriverUnavailable("`eve` is not on PATH")
    try:
        proc = subprocess.run([exe, "chat", "--help"], capture_output=True, text=True, timeout=60)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise DriverUnavailable("`eve chat --help` failed: %s" % exc)
    text = proc.stdout + proc.stderr
    missing = [flag for flag in ("--goal", "--turns", "--reply-path", "--body", "--kind", "--out") if flag not in text]
    if proc.returncode != 0 or missing:
        raise DriverUnavailable("`eve chat` cannot run one scripted turn against an HTTP backend "
                                "(exit %s, missing flags: %s)" % (proc.returncode, ", ".join(missing) or "none"))


def read_last_record(path):
    lines = read_records(path)
    return lines[-1] if lines else None


def read_records(path):
    try:
        with open(path, "r", encoding="utf-8") as handle:
            return [json.loads(line) for line in handle if line.strip()]
    except OSError:
        return []


def drive_with_eve(task, core, run_dir, prompt, eve_config_path, tasks_file=None, web_base=""):
    """Run one scripted turn through ``eve chat``. EVE posts only ``{message}``, so the adapter is
    started with ``--task-file`` and ``--task`` and reads the task's ``context`` itself."""
    record = os.path.join(run_dir, "turn.jsonl")
    started = record + ".started"
    adapter_log = open(os.path.join(run_dir, "adapter.err.log"), "w")
    argv = [sys.executable, "-I", os.path.join(HERE, "eve_adapter.py"), "--heron", core.base, "--token", core.token,
            "--task", task["id"], "--port", "0", "--record", record]
    if tasks_file:
        argv += ["--task-file", os.path.abspath(tasks_file)]
    if web_base:
        argv += ["--web", web_base]
    adapter = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=adapter_log, text=True)
    try:
        port_line = _read_line_with_timeout(adapter.stdout, ADAPTER_START_SECS)
        match = re.match(r"port=(\d+)$", port_line or "")
        if not match:
            raise RunError("the adapter did not print port=<n>: %r" % port_line)
        env = dict(os.environ)
        env["HERON_ADAPTER_PORT"] = match.group(1)
        env["HERON_TASK_PROMPT"] = prompt
        with open(eve_config_path, "r", encoding="utf-8") as handle:
            config = parse_eve_config(handle.read())
        argv = eve_argv(config, env, os.path.join(run_dir, "eve-out"))
        try:
            proc = subprocess.run(argv, env=env, capture_output=True, text=True, timeout=EVE_CLIENT_SECS)
        except subprocess.TimeoutExpired:
            raise RunError("`eve chat` did not finish within %ds" % EVE_CLIENT_SECS)
        with open(os.path.join(run_dir, "eve.log"), "w") as handle:
            handle.write(proc.stdout + "\n--- stderr ---\n" + proc.stderr)
        if proc.returncode == 2:
            raise RunError("`eve chat` rejected its arguments: %s" % proc.stderr.strip()[:300])
        # EVE gives up on a reply after 30 s, but the adapter keeps waiting for Heron. Wait for the
        # recorded turn, unless the adapter never received one.
        deadline = time.monotonic() + eve_adapter.TURN_TIMEOUT_SECS + 30
        while not read_records(record):
            if not os.path.exists(started):
                raise RunError("EVE exited without sending a turn (exit %d)" % proc.returncode)
            if time.monotonic() > deadline:
                raise RunError("the turn was not recorded within %ds" % (eve_adapter.TURN_TIMEOUT_SECS + 30))
            time.sleep(0.2)
    finally:
        adapter.terminate()
        try:
            adapter.wait(10)
        except subprocess.TimeoutExpired:
            adapter.kill()
        adapter.stdout.close()
        adapter_log.close()
    records = read_records(record)
    if len(records) != 1:
        raise RunError("EVE sent %d turns; the scripted task has exactly one" % len(records))
    return records[-1]


def _read_line_with_timeout(stream, secs):
    box = []
    thread = threading.Thread(target=lambda: box.append(stream.readline()), daemon=True)
    thread.start()
    thread.join(secs)
    return box[0].strip() if box else None


# ----------------------------------------------------------------------------- one run

def stop_browsers(profile, grace=5.0):
    """Stop every process started with ``--user-data-dir=<profile>``: the browser a run's core opened outlives
    the core. Returns the pids that were signalled."""
    if os.name != "posix":
        return []
    flag = "--user-data-dir=" + profile
    listing = subprocess.run(["ps", "-axww", "-o", "pid=,command="], capture_output=True, text=True).stdout
    pids = []
    for line in listing.splitlines():
        pid, _, command = line.strip().partition(" ")
        if pid.isdigit() and int(pid) != os.getpid() and (" " + flag + " ") in (" " + command.strip() + " "):
            pids.append(int(pid))
    signalled = list(pids)
    for sig in (signal.SIGTERM, signal.SIGKILL):
        alive = []
        for pid in pids:
            try:
                os.kill(pid, sig)
                alive.append(pid)
            except ProcessLookupError:
                pass
        deadline = time.monotonic() + grace
        while alive and time.monotonic() < deadline:
            alive = [pid for pid in alive if _running(pid)]
            time.sleep(0.1)
        if not alive:
            break
        pids = alive
    return signalled


def _running(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    try:
        waited, _ = os.waitpid(pid, os.WNOHANG)
        return waited == 0
    except ChildProcessError:
        return True


def execute_run(task, run_number, opts, binary, fixtures_root):
    """Do one run of one task. Returns the run record for ``summary.json``."""
    run_dir = os.path.join(opts.artifacts, task["id"], "run%d" % run_number)
    if os.path.isdir(run_dir):
        shutil.rmtree(run_dir)
    home = os.path.join(run_dir, "home")
    workdir = os.path.join(run_dir, "work")
    web_dir = os.path.join(run_dir, "web")
    bin_dir = os.path.join(run_dir, "bin")
    os.makedirs(home, mode=0o700)
    os.chmod(home, 0o700)
    web = None
    core = None
    try:
        web = FixtureWeb(web_dir, os.path.join(run_dir, "web.log"))
        prepare_fixture(task, fixtures_root, workdir, web_dir, web.base)
        bins = prepare_bins(task, fixtures_root, bin_dir, web.base)
        with open(os.path.join(home, "config.toml"), "w", encoding="utf-8") as handle:
            handle.write(config_toml(workdir, opts.provider, task.get("features"), task.get("config")))
        core = Core(binary, home, opts.topology, run_dir, bins)
        db_path = os.path.join(home, "heron.db")
        apply_seed_sql(db_path, task.get("seed_sql") or [])
        accept_risk_notice(core.base, core.token)
        prompt = substitute_web(task["prompt"], web.base)
        context = eve_adapter.replace_web(task.get("context") or [], web.base)
        if opts.driver == "direct":
            try:
                result = eve_adapter.turn(core.base, core.token, prompt, task.get("approve") or [], context=context)
            except eve_adapter.TurnError as exc:
                raise RunError(str(exc))
        else:
            result = drive_with_eve(task, core, run_dir, prompt, os.path.join(HERE, "eve", "heron-core.yaml"),
                                    opts.tasks_file, web.base)
        with open(os.path.join(run_dir, "result.json"), "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=1)
        if result.get("end") != "done":
            raise RunError("the turn ended with %r%s" % (result.get("end"), ": %s" % result["error"] if result.get("error") else ""))
        session_id = result.get("session_id") or ""
        asks = count_asks(db_path, session_id)
        core.stop()
        core = None
        web_requests = read_web_log(os.path.join(run_dir, "web.log"))
        ctx = pass_rules.Context(workdir=workdir, reply=result.get("reply") or "", db_path=db_path,
                                 session_id=session_id, web_requests=web_requests,
                                 events=result.get("events") or [])
        passed, failed = pass_rules.evaluate(task["pass"], ctx)
        return {"run": run_number, "pass": passed, "failed_rules": failed,
                "reply": (result.get("reply") or "")[:REPLY_CHARS], "tokens": int(result.get("tokens") or 0),
                "ttft_ms": result.get("ttft_ms"), "asks": asks, "error": None}
    except (RunError, pass_rules.RuleError, sqlite3.Error, OSError, ValueError) as exc:
        return {"run": run_number, "pass": False, "failed_rules": [], "reply": "", "tokens": 0,
                "ttft_ms": None, "asks": 0, "error": "%s: %s" % (type(exc).__name__, exc)}
    finally:
        if core is not None:
            core.stop()
        stop_browsers(os.path.join(home, "browser"))
        if web is not None:
            web.stop()


# ----------------------------------------------------------------------------- summary

def build_summary(header, tasks, runs_by_task):
    """``header`` holds the schema-1 fields before ``tasks``; ``runs_by_task`` maps id -> run records."""
    entries = []
    for task in tasks:
        runs = sorted(runs_by_task.get(task["id"], []), key=lambda r: r["run"])
        entry = {"id": task["id"], "counts_toward_gate": task["counts_toward_gate"]}
        if "injection" in task:  # feature tasks only; absent means false, and core summaries stay as they were
            entry["injection"] = task["injection"]
        entry["runs"] = runs
        entries.append(entry)
    summary = dict(header)
    summary["tasks"] = entries
    summary["totals"] = compute_totals(entries)
    return summary


def write_atomically(path, data):
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as handle:
        json.dump(data, handle, indent=2)
        handle.write("\n")
    os.replace(tmp, path)


# ----------------------------------------------------------------------------- setup

def git_commit():
    try:
        out = subprocess.run(["git", "rev-parse", "HEAD"], cwd=REPO_ROOT, capture_output=True, text=True, timeout=30)
        sha = out.stdout.strip()
        return sha if out.returncode == 0 and sha else "unknown"
    except (OSError, subprocess.TimeoutExpired):
        return "unknown"


def cli_version(provider):
    binary = PROVIDER_BINARIES.get(provider, "")
    if not binary:
        return ""
    try:
        out = subprocess.run([binary, "--version"], capture_output=True, text=True, timeout=60)
    except (OSError, subprocess.TimeoutExpired):
        return ""
    match = SEMVER.search(out.stdout + out.stderr)
    return match.group(0) if match else ""


def parse_args(argv):
    parser = argparse.ArgumentParser(prog="run-core-tasks.sh", description="Run Heron's core tasks.")
    parser.add_argument("--provider", required=True, help="demo, or a CLI provider id such as claude-cli")
    parser.add_argument("--runs", type=int, default=3, help="runs per task (default 3)")
    parser.add_argument("--out", required=True, help="folder that gets summary.json")
    parser.add_argument("--driver", choices=("eve", "direct"), default="eve")
    parser.add_argument("--topology", choices=("envelope", "hybrid"), default="envelope")
    parser.add_argument("--build", choices=("debug", "release"), default="release")
    parser.add_argument("--heron-bin", help="use this heron binary instead of building one")
    parser.add_argument("--artifacts", help="per-run artifacts folder (default <out>/runs)")
    parser.add_argument("--tasks", help="comma-separated task ids (default: every task, probes included)")
    parser.add_argument("--resume", action="store_true", help="skip runs already in summary.json without an error")
    parser.add_argument("--task-file", "--tasks-file", dest="tasks_file", default=os.path.join(HERE, "core-tasks.md"),
                        help="read the tasks from this file instead of eval/core-tasks.md, e.g. eval/features/<slug>.md")
    return parser.parse_args(argv)  # argparse exits 2 on bad flags


def validate_options(opts, all_tasks):
    if opts.runs < 1:
        raise UsageError("--runs must be at least 1")
    if opts.provider != "demo" and opts.provider not in PROVIDER_BINARIES:
        raise UsageError("unknown provider %r; use demo or one of: %s" % (opts.provider, ", ".join(sorted(PROVIDER_BINARIES))))
    if opts.topology == "hybrid":
        raise UsageError("--topology hybrid was removed in U2w: the envelope won the spike (eval/spike/topology.md), and the core no longer has a hybrid mode")
    if opts.heron_bin and not os.path.isfile(opts.heron_bin):
        raise UsageError("--heron-bin %s is not a file" % opts.heron_bin)
    known = [t["id"] for t in all_tasks]
    if opts.tasks is None:
        return list(all_tasks)
    wanted = [t.strip() for t in opts.tasks.split(",") if t.strip()]
    if not wanted:
        raise UsageError("--tasks is empty")
    unknown = [t for t in wanted if t not in known]
    if unknown:
        raise UsageError("unknown task id %s; known: %s" % (", ".join(unknown), ", ".join(known)))
    return [t for t in all_tasks if t["id"] in wanted]


def build_core(build):
    cmd = ["cargo", "build", "--locked"] + (["--release"] if build == "release" else [])
    proc = subprocess.run(cmd, cwd=REPO_ROOT, capture_output=True, text=True)
    if proc.returncode != 0:
        raise RunError("`%s` failed:\n%s" % (" ".join(cmd), proc.stderr[-2000:]))
    target = os.environ.get("CARGO_TARGET_DIR") or os.path.join(REPO_ROOT, "target")
    if not os.path.isabs(target):
        target = os.path.join(REPO_ROOT, target)
    binary = os.path.join(target, "debug" if build == "debug" else "release", "heron")
    if not os.path.isfile(binary):
        raise RunError("the build succeeded but %s does not exist" % binary)
    return binary


def load_existing(summary_path, header):
    """Runs already recorded without an error, when ``--resume`` finds a matching summary."""
    try:
        with open(summary_path, "r", encoding="utf-8") as handle:
            old = json.load(handle)
    except (OSError, ValueError):
        return {}
    for key in ("schema", "provider", "topology", "mode", "driver", "build"):
        if old.get(key) != header[key]:
            raise UsageError("--resume: the summary in %s was made with %s=%r, not %r"
                             % (summary_path, key, old.get(key), header[key]))
    kept = {}
    for task in old.get("tasks", []):
        good = [r for r in task.get("runs", []) if r.get("error") is None]
        if good:
            kept[task["id"]] = good
    return kept


def main(argv):
    opts = parse_args(argv)
    try:
        all_tasks = load_tasks(opts.tasks_file)
        selected = validate_options(opts, all_tasks)
        os.makedirs(opts.out, exist_ok=True)
        opts.artifacts = opts.artifacts or os.path.join(opts.out, "runs")
        opts.out = os.path.abspath(opts.out)
        opts.artifacts = os.path.abspath(opts.artifacts)
        if opts.driver == "eve":
            try:
                check_eve_available()
            except DriverUnavailable as exc:
                sys.stderr.write("run-core-tasks: the eve driver can't run: %s\n" % exc)
                return 3
    except UsageError as exc:
        sys.stderr.write("run-core-tasks: %s\n" % exc)
        return 2
    except (OSError, TasksError) as exc:
        sys.stderr.write("run-core-tasks: cannot read the tasks: %s\n" % exc)
        return 2

    summary_path = os.path.join(opts.out, "summary.json")
    header = {"schema": SCHEMA, "heron_commit": git_commit(), "provider": opts.provider,
              "cli_version": "" if opts.provider == "demo" else cli_version(opts.provider),
              "topology": opts.topology, "mode": "ask", "driver": opts.driver, "build": opts.build,
              "runs_per_task": opts.runs}
    try:
        runs_by_task = load_existing(summary_path, header) if opts.resume else {}
    except UsageError as exc:
        sys.stderr.write("run-core-tasks: %s\n" % exc)
        return 2
    pending = [(t, n) for t in selected for n in range(1, opts.runs + 1)
               if n not in {r["run"] for r in runs_by_task.get(t["id"], [])}]

    binary = opts.heron_bin
    build_error = None
    if pending and binary is None:
        try:
            binary = build_core(opts.build)
        except RunError as exc:
            build_error = str(exc)
    fixtures_root = fixtures_root_for(opts.tasks_file)

    failures = 0
    for task, number in pending:
        if build_error is not None:
            run = {"run": number, "pass": False, "failed_rules": [], "reply": "", "tokens": 0, "ttft_ms": None,
                   "asks": 0, "error": "RunError: %s" % build_error}
        else:
            run = execute_run(task, number, opts, binary, fixtures_root)
            if run["error"] is not None:
                sys.stderr.write("run-core-tasks: %s run %d failed (%s); retrying once\n" % (task["id"], number, run["error"]))
                run = execute_run(task, number, opts, binary, fixtures_root)
        if run["error"] is not None:
            failures += 1
        runs_by_task.setdefault(task["id"], [])
        runs_by_task[task["id"]] = [r for r in runs_by_task[task["id"]] if r["run"] != number] + [run]
        write_atomically(summary_path, build_summary(header, selected, runs_by_task))
        print("%s run %d: %s" % (task["id"], number, "error" if run["error"] else ("pass" if run["pass"] else "fail")),
              flush=True)
    write_atomically(summary_path, build_summary(header, selected, runs_by_task))
    if failures:
        sys.stderr.write("run-core-tasks: %d run(s) still have an error after one retry\n" % failures)
        return 1
    print("run-core-tasks: wrote %s" % summary_path)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
