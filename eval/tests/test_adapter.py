import json
import os
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import eve_adapter
from eve_adapter import TurnError, decide, turn

EVAL = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TOKEN = "tok123"


class FakeHeron:
    """A fake `/api/chat` that streams session, approval, text, two usage events and done."""

    def __init__(self, chat_status=200, delay=0.0):
        self.chat_status = chat_status
        self.delay = delay
        self.approvals = []  # {"id", "decision"} bodies posted to /api/approve
        self.chat_bodies = []
        self.bad_tokens = 0
        self._answered = threading.Event()
        owner = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.0"

            def log_message(self, fmt, *args):
                pass

            def _body(self):
                return json.loads(self.rfile.read(int(self.headers.get("content-length", "0"))) or b"{}")

            def do_POST(self):
                if self.headers.get("x-heron-token") != TOKEN:
                    owner.bad_tokens += 1
                    self.send_response(401)
                    self.end_headers()
                    return
                body = self._body()
                if self.path == "/api/approve":
                    owner.approvals.append(body)
                    self.send_response(200)
                    self.send_header("content-type", "application/json")
                    self.end_headers()
                    self.wfile.write(b'{"ok": true}')
                    owner._answered.set()
                    return
                if self.path != "/api/chat":
                    self.send_response(404)
                    self.end_headers()
                    return
                owner.chat_bodies.append(body)
                if owner.chat_status != 200:
                    self.send_response(owner.chat_status)
                    self.send_header("content-type", "application/json")
                    self.end_headers()
                    self.wfile.write(b'{"error": "busy"}')
                    return
                self.send_response(200)
                self.send_header("content-type", "text/event-stream")
                self.end_headers()
                tool = "shell" if "shell" in body["message"] else "write_file"

                def send(event):
                    self.wfile.write(("data: %s\n\n" % json.dumps(event)).encode())
                    self.wfile.flush()

                self.wfile.write(b": keep-alive\n\n")
                send({"type": "session", "id": "sess-1"})
                owner._answered.clear()
                send({"type": "approval", "id": "ap-1", "tool": tool, "summary": "do %s thing" % tool})
                owner._answered.wait(5)
                time.sleep(owner.delay)
                send({"type": "text", "text": "Hello "})
                send({"type": "text", "text": "world"})
                send({"type": "usage", "prompt": 10, "completion": 5})
                send({"type": "usage", "prompt": 30, "completion": 12})
                send({"type": "done", "text": "Hello world", "title": "t"})

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.base = "http://127.0.0.1:%d" % self.server.server_address[1]
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()


class Decide(unittest.TestCase):
    def test_a_listed_tool_is_approved_once(self):
        self.assertEqual(decide([{"tool": "write_file"}], {"tool": "write_file", "summary": "x"}), "once")

    def test_an_unlisted_tool_is_denied(self):
        self.assertEqual(decide([{"tool": "write_file"}], {"tool": "shell", "summary": "x"}), "deny")
        self.assertEqual(decide([], {"tool": "write_file"}), "deny")
        self.assertEqual(decide(None, {"tool": "write_file"}), "deny")

    def test_summary_regex_must_match_when_given(self):
        policy = [{"tool": "shell", "summary_regex": "^python3 -m unittest"}]
        self.assertEqual(decide(policy, {"tool": "shell", "summary": "python3 -m unittest discover"}), "once")
        self.assertEqual(decide(policy, {"tool": "shell", "summary": "rm -rf /"}), "deny")
        self.assertEqual(decide(policy, {"tool": "shell"}), "deny")

    def test_any_entry_can_approve(self):
        policy = [{"tool": "edit_file"}, {"tool": "write_file"}]
        self.assertEqual(decide(policy, {"tool": "write_file"}), "once")


class Turn(unittest.TestCase):
    def setUp(self):
        self.heron = FakeHeron()
        self.addCleanup(self.heron.close)

    def test_an_approved_tool_posts_once(self):
        result = turn(self.heron.base, TOKEN, "save it", [{"tool": "write_file"}])
        self.assertEqual(self.heron.approvals, [{"id": "ap-1", "decision": "once"}])
        self.assertEqual(result["end"], "done")

    def test_an_unlisted_tool_posts_deny(self):
        turn(self.heron.base, TOKEN, "run shell", [{"tool": "write_file"}])
        self.assertEqual(self.heron.approvals, [{"id": "ap-1", "decision": "deny"}])

    def test_tokens_are_the_last_usage_total_not_a_sum(self):
        result = turn(self.heron.base, TOKEN, "save it", [{"tool": "write_file"}])
        self.assertEqual(result["tokens"], 42)  # 30 + 12, not 15 + 42

    def test_reply_session_and_events(self):
        result = turn(self.heron.base, TOKEN, "save it", [{"tool": "write_file"}])
        self.assertEqual(result["reply"], "Hello world")
        self.assertEqual(result["session_id"], "sess-1")
        self.assertEqual([e["type"] for e in result["events"]],
                         ["session", "approval", "text", "text", "usage", "usage", "done"])
        self.assertIsNone(result["error"])

    def test_ttft_runs_to_the_first_text_event(self):
        result = turn(self.heron.base, TOKEN, "save it", [{"tool": "write_file"}])
        self.assertIsInstance(result["ttft_ms"], int)
        self.assertGreaterEqual(result["ttft_ms"], 0)

    def test_the_token_header_and_session_id_are_sent(self):
        turn(self.heron.base, TOKEN, "save it", [{"tool": "write_file"}], session_id="abc")
        self.assertEqual(self.heron.bad_tokens, 0)
        self.assertEqual(self.heron.chat_bodies, [{"message": "save it", "session_id": "abc"}])

    def test_no_session_id_is_not_sent_when_absent(self):
        turn(self.heron.base, TOKEN, "save it", [{"tool": "write_file"}])
        self.assertEqual(self.heron.chat_bodies, [{"message": "save it"}])

    def test_a_wrong_token_raises(self):
        with self.assertRaises(TurnError) as ctx:
            turn(self.heron.base, "wrong", "save it", [])
        self.assertIn("401", str(ctx.exception))

    def test_a_busy_session_raises_with_the_status(self):
        busy = FakeHeron(chat_status=409)
        self.addCleanup(busy.close)
        with self.assertRaises(TurnError) as ctx:
            turn(busy.base, TOKEN, "hi", [])
        self.assertIn("409", str(ctx.exception))

    def test_an_unreachable_core_raises(self):
        with self.assertRaises(TurnError):
            turn("http://127.0.0.1:1", TOKEN, "hi", [])


class AdapterServer(unittest.TestCase):
    def setUp(self):
        self.heron = FakeHeron()
        self.addCleanup(self.heron.close)
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.record = os.path.join(self._tmp.name, "turn.jsonl")
        self.proc = subprocess.Popen(
            [sys.executable, "-I", os.path.join(EVAL, "eve_adapter.py"), "--heron", self.heron.base, "--token", TOKEN,
             "--task", "t01", "--port", "0", "--record", self.record],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.addCleanup(self.stop)
        first = self.proc.stdout.readline().strip()
        self.assertRegex(first, r"^port=\d+$")
        self.port = int(first.split("=")[1])

    def stop(self):
        self.proc.terminate()
        self.proc.wait(10)
        self.proc.stdout.close()
        self.proc.stderr.close()

    def post(self, message, path="/turn"):
        request = urllib.request.Request("http://127.0.0.1:%d%s" % (self.port, path),
                                         data=json.dumps({"message": message}).encode(), method="POST",
                                         headers={"content-type": "application/json"})
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read())

    def records(self):
        with open(self.record) as handle:
            return [json.loads(line) for line in handle if line.strip()]

    def test_turn_returns_the_reply_for_eve(self):
        self.assertEqual(self.post("save it"), {"reply": "Hello world"})

    def test_record_gets_one_line_per_turn(self):
        self.post("save it")
        self.assertEqual(len(self.records()), 1)
        self.post("save it again")
        records = self.records()
        self.assertEqual(len(records), 2)
        self.assertEqual([r["message"] for r in records], ["save it", "save it again"])
        self.assertEqual(records[-1]["tokens"], 42)
        self.assertEqual(records[-1]["reply"], "Hello world")

    def test_the_task_policy_decides_approvals(self):
        # t01 approves write_file only: a shell ask is denied, a write_file ask is approved.
        self.post("please use shell")
        self.post("please save")
        self.assertEqual([a["decision"] for a in self.heron.approvals], ["deny", "once"])

    def test_the_second_turn_reuses_the_session(self):
        self.post("first")
        self.post("second")
        self.assertNotIn("session_id", self.heron.chat_bodies[0])
        self.assertEqual(self.heron.chat_bodies[1]["session_id"], "sess-1")

    def test_a_started_marker_appears_with_the_request(self):
        self.post("save it")
        self.assertTrue(os.path.exists(self.record + ".started"))

    def test_other_paths_and_bad_bodies_are_refused(self):
        for path, body in (("/nope", "x"), ("/turn", "")):
            with self.subTest(path=path):
                with self.assertRaises(urllib.error.HTTPError) as ctx:
                    self.post(body, path)
                self.assertIn(ctx.exception.code, (400, 404))

    def test_unknown_task_id_exits_non_zero(self):
        proc = subprocess.run([sys.executable, "-I", os.path.join(EVAL, "eve_adapter.py"), "--heron", "http://x",
                               "--token", "t", "--task", "zz9", "--port", "0", "--record", self.record + "2"],
                              capture_output=True, text=True, timeout=30)
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("zz9", proc.stderr)


if __name__ == "__main__":
    unittest.main()
