import json
import os
import socket
import stat
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HERE = os.path.dirname(os.path.abspath(__file__))
USABILITY = os.path.dirname(HERE)
EVAL = os.path.dirname(USABILITY)
for path in (USABILITY, EVAL):
    if path not in sys.path:
        sys.path.insert(0, path)

import client  # noqa: E402

TOKEN = "tok3n5ecret0123456789abcdef"

FAKE_CORE = r'''#!{python}
import json, sys, time
args = sys.argv[1:]
if args[:2] == ["serve", "--help"]:
    print("Usage: heron serve [--no-open] [--port N]{help_extra}")
    sys.exit(0)
mode = {mode!r}
if mode == "json":
    assert "--print-endpoint" in args, args
    sys.stderr.write("starting with token {token}\n"); sys.stderr.flush()
    print(json.dumps({{"port": 4321, "token": "{token}", "background_owner": False, "version": "1.0.0"}}), flush=True)
    sys.stderr.write("after {token}\n"); sys.stderr.flush()
    sys.stdin.read()  # a --print-endpoint core exits on stdin EOF
    print("bye {token}", flush=True)
elif mode == "url":
    assert "--print-endpoint" not in args, args
    print("Heron is running at:\n\n  http://127.0.0.1:5555/?token={token}\n", flush=True)
    sys.stdin.read()
elif mode == "silent-exit":
    sys.exit(4)
'''


def fake_core(folder, mode, help_extra=""):
    path = os.path.join(folder, "heron-%s" % mode)
    with open(path, "w") as handle:
        handle.write(FAKE_CORE.format(python=sys.executable, mode=mode, token=TOKEN, help_extra=help_extra))
    os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR)
    return path


class EndpointTests(unittest.TestCase):
    def test_json_line(self):
        ep = client.parse_endpoint('{"port": 4321, "token": "abc", "background_owner": false, "version": "1.0"}\n')
        self.assertEqual(ep, {"port": 4321, "token": "abc", "background_owner": False, "version": "1.0",
                              "source": "print-endpoint"})

    def test_url_line(self):
        ep = client.parse_endpoint("  http://127.0.0.1:5555/?token=Ab_c-9\n")
        self.assertEqual((ep["port"], ep["token"], ep["source"]), (5555, "Ab_c-9", "url"))

    def test_not_an_endpoint(self):
        for line in ("", "Heron is running at:", "{not json", '{"port": "1", "token": "x"}', '{"port": 0, "token": "x"}',
                     '{"port": 65536, "token": "x"}', '{"port": true, "token": "x"}', '{"port": 80, "token": ""}',
                     '{"port": 80}', "[1, 2]", "http://localhost:5555/?token=x", "http://127.0.0.1:5555/"):
            self.assertIsNone(client.parse_endpoint(line), line)

    def test_port_bounds(self):
        self.assertEqual(client.parse_endpoint('{"port": 1, "token": "x"}')["port"], 1)
        self.assertEqual(client.parse_endpoint('{"port": 65535, "token": "x"}')["port"], 65535)


class DecideTests(unittest.TestCase):
    def test_policy(self):
        policy = [{"tool": "write_file", "summary_regex": "^notes/"}, {"tool": "shell", "decision": "session"},
                  {"tool": "edit_file", "decision": "deny"}]
        self.assertEqual(client.decide(policy, {"tool": "write_file", "summary": "notes/a.md"}), ("once", True))
        self.assertEqual(client.decide(policy, {"tool": "write_file", "summary": "other.md"}), ("deny", False))
        self.assertEqual(client.decide(policy, {"tool": "shell", "summary": "ls"}), ("session", True))
        self.assertEqual(client.decide(policy, {"tool": "edit_file"}), ("deny", True))
        self.assertEqual(client.decide(policy, {"tool": "delete_file"}), ("deny", False))
        self.assertEqual(client.decide([], {"tool": "x"}), ("deny", False))

    def test_unknown_decision_is_a_script_error(self):
        with self.assertRaises(ValueError):
            client.decide([{"tool": "x", "decision": "forever"}], {"tool": "x"})


class RedactTests(unittest.TestCase):
    def test_redactor(self):
        r = client.Redactor()
        self.assertEqual(r("a secret"), "a secret")
        r.add("secret")
        r.add("secret")
        r.add("")
        r.add(None)
        self.assertEqual(r.secrets, ["secret"])
        self.assertEqual(r("a secret, secret"), "a <redacted>, <redacted>")

    def test_scan_for_secret(self):
        with tempfile.TemporaryDirectory() as root:
            os.makedirs(os.path.join(root, "a"))
            with open(os.path.join(root, "a", "x.log"), "w") as handle:
                handle.write("token=" + TOKEN)
            with open(os.path.join(root, "clean.log"), "w") as handle:
                handle.write("nothing")
            self.assertEqual(client.scan_for_secret(root, TOKEN), [os.path.join(root, "a", "x.log")])
            self.assertEqual(client.scan_for_secret(root, "absent"), [])

    def test_core_env(self):
        os.environ["HERON_STRAY"] = "1"
        try:
            env = client.core_env("/h", {"X": "y"})
        finally:
            del os.environ["HERON_STRAY"]
        self.assertEqual((env["HERON_HOME"], env["HERON_ALLOW_LOCAL_FETCH"], env["X"]), ("/h", "1", "y"))
        self.assertNotIn("HERON_STRAY", env)


class CoreTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)
        self.logs = os.path.join(self.dir.name, "logs")

    def read_logs(self):
        out = ""
        for name in ("core.out.log", "core.err.log"):
            with open(os.path.join(self.logs, name)) as handle:
                out += handle.read()
        return out

    def test_print_endpoint_core(self):
        binary = fake_core(self.dir.name, "json", help_extra=" [--print-endpoint]")
        self.assertTrue(client.supports_print_endpoint(binary))
        core = client.Core(binary, os.path.join(self.dir.name, "home"), self.logs)
        self.assertEqual((core.base, core.token, core.endpoint["source"]), ("http://127.0.0.1:4321", TOKEN, "print-endpoint"))
        self.assertIs(core.endpoint["background_owner"], False)
        self.assertTrue(core.alive())
        core.stop()  # closing stdin is what stops it
        self.assertEqual(core.proc.returncode, 0)
        logs = self.read_logs()
        self.assertNotIn(TOKEN, logs)
        self.assertIn("starting with token <redacted>", logs)  # held until the token was known
        self.assertIn("bye <redacted>", logs)

    def test_url_core(self):
        binary = fake_core(self.dir.name, "url")
        self.assertFalse(client.supports_print_endpoint(binary))
        core = client.Core(binary, os.path.join(self.dir.name, "home"), self.logs)
        self.assertEqual((core.endpoint["port"], core.endpoint["source"]), (5555, "url"))
        core.stop()
        self.assertNotIn(TOKEN, self.read_logs())
        self.assertIn("?token=<redacted>", self.read_logs())

    def test_core_that_prints_nothing(self):
        binary = fake_core(self.dir.name, "silent-exit")
        with self.assertRaises(client.ApiError) as caught:
            client.Core(binary, os.path.join(self.dir.name, "home"), self.logs, print_endpoint=False, start_secs=10)
        self.assertIn("status 4", str(caught.exception))

    def test_missing_binary(self):
        self.assertFalse(client.supports_print_endpoint(os.path.join(self.dir.name, "nope")))


def can_bind():
    try:
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
        return True
    except OSError:
        return False


class FakeHeron:
    """A tiny stand-in for the core's /api/chat, /api/approve and /api/events (U3 shapes)."""

    def __init__(self, chat_events, resume_events=(), chat_status=200):
        self.approvals = []
        self.seen_tokens = []
        self.paths = []
        fake = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *a):
                pass

            def _sse(self, events):
                self.send_response(200)
                self.send_header("content-type", "text/event-stream")
                self.end_headers()
                self.wfile.write(b": keep-alive\n\n")
                for e in events:
                    self.wfile.write(("id: %s\ndata: %s\n\n" % (e.get("seq", 0), json.dumps(e))).encode())
                    self.wfile.flush()

            def do_POST(self):
                fake.seen_tokens.append(self.headers.get("x-heron-token"))
                fake.paths.append(self.path)
                body = json.loads(self.rfile.read(int(self.headers.get("content-length", 0))) or b"{}")
                if self.path == "/api/approve":
                    fake.approvals.append(body)
                    self.send_response(200)
                    self.end_headers()
                    self.wfile.write(b"{}")
                elif self.path == "/api/chat":
                    if chat_status != 200:
                        self.send_response(chat_status)
                        self.end_headers()
                        self.wfile.write(b'{"error":"risk_notice_not_accepted"}')
                        return
                    fake.chat_body = body
                    self._sse(chat_events)
                else:
                    self.send_response(404)
                    self.end_headers()

            def do_GET(self):
                fake.paths.append(self.path)
                if self.path.startswith("/api/events"):
                    self._sse(resume_events)
                elif self.path == "/api/state":
                    self.send_response(200)
                    self.end_headers()
                    self.wfile.write(b'{"ok":true}')
                else:
                    self.send_response(404)
                    self.end_headers()
                    self.wfile.write(b"nope")

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.base = "http://127.0.0.1:%d" % self.server.server_address[1]
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()


@unittest.skipUnless(can_bind(), "this sandbox blocks binding a loopback port")
class ApiTests(unittest.TestCase):
    R = "run-1"

    def events(self):
        return [{"seq": 1, "run_id": self.R, "type": "session", "id": "s9"},
                {"seq": 2, "run_id": self.R, "type": "approval", "id": "a1", "tool": "write_file", "summary": "plan.md"},
                {"seq": 3, "run_id": self.R, "type": "text", "text": "Do"},
                {"seq": 4, "run_id": self.R, "type": "text", "text": "ne"},
                {"seq": 5, "run_id": self.R, "type": "done", "text": "Done."},
                {"seq": 6, "run_id": self.R, "type": "run_end"}]

    def test_chat_answers_approvals_and_reads_the_reply(self):
        fake = FakeHeron(self.events())
        self.addCleanup(fake.close)
        api = client.Api(fake.base, TOKEN)
        out = api.chat("hi", [{"tool": "write_file", "decision": "session"}])
        self.assertEqual((out["status"], out["reply"], out["end"]), (200, "Done.", "done"))
        self.assertEqual(fake.approvals, [{"id": "a1", "decision": "session"}])
        self.assertEqual(out["decisions"], [{"tool": "write_file", "decision": "session", "planned": True}])
        self.assertEqual((api.session_id, api.run_id, api.last_seq), ("s9", self.R, 5))
        self.assertIsNotNone(out["ttft_ms"])
        self.assertEqual(set(fake.seen_tokens), {TOKEN})
        self.assertEqual(fake.chat_body, {"message": "hi"})
        out = api.chat("again", [])
        self.assertEqual(fake.chat_body, {"message": "again", "session_id": "s9"})

    def test_until_run_end_reads_past_done(self):
        fake = FakeHeron(self.events())
        self.addCleanup(fake.close)
        api = client.Api(fake.base, TOKEN, until_run_end=True)
        out = api.chat("hi", [])
        self.assertEqual(out["end"], "run_end")
        self.assertEqual(out["events"][-1]["type"], "run_end")
        self.assertEqual(api.last_seq, 6)
        self.assertEqual(out["decisions"], [{"tool": "write_file", "decision": "deny", "planned": False}])

    def test_disconnect_then_resume_skips_other_runs_and_repeats(self):
        resume = [{"seq": 1, "run_id": self.R, "type": "session", "id": "s9"},
                  {"seq": 2, "run_id": "other", "type": "text", "text": "not ours"}] + self.events()[1:]
        fake = FakeHeron(self.events(), resume_events=resume)
        self.addCleanup(fake.close)
        api = client.Api(fake.base, TOKEN)
        first = api.chat("hi", [], disconnect_after=1)
        self.assertEqual((first["end"], len(first["events"])), ("disconnected", 1))
        self.assertEqual(fake.approvals, [])
        out = api.resume([{"tool": "write_file"}])
        self.assertIn("/api/events?since=1", fake.paths)
        self.assertEqual([e["seq"] for e in out["events"]], [1, 2, 3, 4, 5, 6])
        self.assertNotIn("not ours", json.dumps(out["events"]))
        self.assertEqual((out["end"], out["reply"]), ("run_end", "Done."))
        self.assertEqual(fake.approvals, [{"id": "a1", "decision": "once"}])

    def test_chat_http_error_is_a_record_not_an_exception(self):
        fake = FakeHeron([], chat_status=428)
        self.addCleanup(fake.close)
        out = client.Api(fake.base, TOKEN).chat("hi", [])
        self.assertEqual((out["status"], out["end"], out["reply"]), (428, "http", ""))
        self.assertIn("risk_notice_not_accepted", out["body"])

    def test_request(self):
        fake = FakeHeron([])
        self.addCleanup(fake.close)
        api = client.Api(fake.base + "/", TOKEN)
        self.assertEqual(api.request("GET", "/api/state"), (200, '{"ok":true}'))
        self.assertEqual(api.request("GET", "/api/nope"), (404, "nope"))

    def test_unreachable_core(self):
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            port = s.getsockname()[1]
        api = client.Api("http://127.0.0.1:%d" % port, TOKEN)
        with self.assertRaises(client.ApiError):
            api.request("GET", "/api/state")
        with self.assertRaises(client.ApiError):
            api.chat("hi", [])


if __name__ == "__main__":
    unittest.main()
