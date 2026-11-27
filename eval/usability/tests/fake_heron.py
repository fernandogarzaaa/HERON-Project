#!/usr/bin/env python3
"""A fake Heron core for the usability runner's tests. Standard library only.

    fake_heron.py serve --help
    fake_heron.py serve --no-open --port 0 [--print-endpoint]

It serves the routes the usability tasks use, in two shapes chosen by ``FAKE_HERON_U3``:

- unset: today's core (the banner URL line, no ``/api/risk-notice``, no ``/api/events``, a run
  that ends with its ``/api/chat`` stream);
- ``1``: the U3 contract as ``docs/designs/heron-v1.md`` section 3 describes it (the endpoint JSON
  line with ``--print-endpoint``, ``seq`` and ``run_id`` on every event, ``run_end``, runs that outlive
  their stream, ``/api/events?since=``, the risk notice and 428 until it is accepted).

Chat behaves like Heron's demo model for the messages the tasks send: a message that mentions
"save" writes ``plan.md`` in the working folder (asking first in ``ask`` mode, blocked in
``readonly``, without asking in ``auto``); anything else gets a canned reply. Audit rows go to
``$HERON_HOME/heron.db``. The process exits when its stdin closes.
"""

import json
import os
import re
import secrets
import sqlite3
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

U3 = os.environ.get("FAKE_HERON_U3") == "1"
HOME = os.environ.get("HERON_HOME", ".")
TOKEN = secrets.token_hex(16)
LOCK = threading.Condition()
RING = []  # every event, U3 only
STATE = {"mode": "ask", "accepted": None, "seq": 0, "session": 0}
APPROVALS = {}  # id -> [threading.Event, decision]
CANNED = "This is a fake Heron core. Try asking it to save a plan as plan.md."


def workspace():
    with open(os.path.join(HOME, "config.toml"), encoding="utf-8") as handle:
        return json.loads(re.search(r"^workspace = (.+)$", handle.read(), re.M).group(1))


def audit(session, tool, decision):
    conn = sqlite3.connect(os.path.join(HOME, "heron.db"))
    try:
        conn.execute("CREATE TABLE IF NOT EXISTS audit(id INTEGER PRIMARY KEY, session_id TEXT, tool TEXT, decision TEXT)")
        conn.execute("INSERT INTO audit(session_id, tool, decision) VALUES (?, ?, ?)", (session, tool, decision))
        conn.commit()
    finally:
        conn.close()


def run_events(session, run_id, message, emit):
    """Play one run, calling ``emit(event)`` for each event."""
    emit({"type": "session", "id": session, "run_id": run_id})
    if "save" not in message.lower():
        for word in CANNED.split(" "):
            emit({"type": "text", "text": word + " "})
        emit({"type": "done", "text": CANNED})
        return
    call = {"id": "call_1", "name": "write_file"}
    mode = STATE["mode"]
    if mode == "readonly":
        emit(dict(call, type="tool_start"))
        emit(dict(call, type="tool_end", ok=False, preview="blocked: read-only mode"))
        audit(session, "write_file", "denied-by-policy")
        reply = "That was not allowed, so I left everything as it was."
    else:
        decision = "allowed"
        if mode == "ask":
            approval_id = secrets.token_hex(8)
            waiter = [threading.Event(), None]
            APPROVALS[approval_id] = waiter
            emit({"type": "approval", "id": approval_id, "tool": "write_file", "summary": "plan.md", "risk": "write",
                  "detail": "Create plan.md", "always_rule": "write_file:*", "can_always": True, "can_session": True})
            waiter[0].wait(20)
            answer = waiter[1] or "deny"
            if U3:
                emit({"type": "approval_resolved", "id": approval_id, "decision": answer})
            decision = "denied-by-user" if answer == "deny" else "approved"
        emit(dict(call, type="tool_start"))
        if decision == "denied-by-user":
            emit(dict(call, type="tool_end", ok=False, preview="Permission denied"))
            reply = "That was not allowed, so I left everything as it was."
        else:
            with open(os.path.join(workspace(), "plan.md"), "w", encoding="utf-8") as handle:
                handle.write("# Plan\n\n1. Connect a real model\n")
            emit(dict(call, type="tool_end", ok=True, preview="wrote plan.md"))
            reply = "Done. Wrote plan.md."
        audit(session, "write_file", decision)
    emit({"type": "text", "text": reply})
    emit({"type": "done", "text": reply})


def publish(session, run_id, event):
    with LOCK:
        STATE["seq"] += 1
        event = dict(event, seq=STATE["seq"], session_id=session, run_id=run_id)
        RING.append(event)
        LOCK.notify_all()


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def _json(self, code, payload=None):
        data = b"" if payload is None else json.dumps(payload).encode()
        self.send_response(code)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def _sse_start(self):
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("connection", "close")
        self.end_headers()
        self.close_connection = True

    def _send_event(self, event):
        self.wfile.write(("id: %s\ndata: %s\n\n" % (event.get("seq", 0), json.dumps(event))).encode())
        self.wfile.flush()

    def _authorized(self):
        if self.headers.get("x-heron-token") != TOKEN:
            self._json(401, {"error": "bad token"})
            return False
        return True

    def _body(self):
        length = int(self.headers.get("content-length", "0") or 0)
        return json.loads(self.rfile.read(length) or b"{}")

    def _stream_ring(self, since, run_id=None):
        """Stream ring events after ``since`` (of ``run_id`` if given) until that run's run_end."""
        position = since
        while True:
            with LOCK:
                pending = [e for e in RING if e["seq"] > position]
                if not pending:
                    LOCK.wait(10)
                    continue
            for event in pending:
                position = event["seq"]
                if run_id is not None and event["run_id"] != run_id:
                    continue
                self._send_event(event)
                if event["type"] == "run_end" and (run_id is None or event["run_id"] == run_id):
                    return

    def do_GET(self):
        if not self._authorized():
            return
        path, _, query = self.path.partition("?")
        if path == "/api/state":
            return self._json(200, {"onboarded": False, "sandbox": True, "risk_notice_accepted": STATE["accepted"] == 1})
        if path == "/api/telegram":
            return self._json(200, {"configured": False, "paired": False, "code": None})
        if U3 and path == "/api/risk-notice":
            return self._json(200, {"version": 1, "text": "Version: 1\n\n## C35-8\n\n## R-1\n",
                                    "accepted_version": STATE["accepted"]})
        if U3 and path == "/api/events":
            since = int(dict(p.split("=") for p in query.split("&") if "=" in p).get("since", "0"))
            self._sse_start()
            try:
                self._stream_ring(since)
            except OSError:
                pass
            return
        self._json(404, {"error": "not found"})

    def do_POST(self):
        if not self._authorized():
            return
        body = self._body()
        if self.path == "/api/mode":
            STATE["mode"] = body["mode"]
            return self._json(200, {"mode": body["mode"]})
        if self.path == "/api/telegram":
            return self._json(400, {"error": "That does not look like a bot token. Create a bot with @BotFather."})
        if self.path == "/api/approve":
            waiter = APPROVALS.get(body.get("id"))
            if waiter:
                waiter[1] = body.get("decision")
                waiter[0].set()
            return self._json(200, {"ok": True})
        if U3 and self.path == "/api/risk-notice":
            if body.get("version") != 1:
                return self._json(409, {"error": "stale"})
            STATE["accepted"] = 1
            return self._json(204)
        if self.path == "/api/chat":
            if U3 and STATE["accepted"] != 1:
                return self._json(428, {"error": "risk_notice_not_accepted"})
            STATE["session"] += 1
            session = body.get("session_id") or "s%d" % STATE["session"]
            run_id = secrets.token_hex(8)
            self._sse_start()
            if U3:
                with LOCK:
                    since = STATE["seq"]

                def worker():
                    run_events(session, run_id, body["message"], lambda e: publish(session, run_id, e))
                    publish(session, run_id, {"type": "run_end"})
                threading.Thread(target=worker, daemon=True).start()  # the run outlives this stream
                try:
                    self._stream_ring(since, run_id)
                except OSError:
                    pass
                return
            try:
                run_events(session, run_id, body["message"], self._send_event)
            except OSError:
                pass  # today's core: a dropped stream ends the run
            return
        self._json(404, {"error": "not found"})


def main(argv):
    if argv[:2] == ["serve", "--help"]:
        print("Usage: heron serve [--no-open] [--port N]%s" % (" [--print-endpoint]" if U3 else ""))
        return 0
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    port = server.server_address[1]
    threading.Thread(target=server.serve_forever, daemon=True).start()
    if "--print-endpoint" in argv:
        print(json.dumps({"port": port, "token": TOKEN, "background_owner": True, "version": "fake"}), flush=True)
    else:
        print("Heron is running at:\n\n  http://127.0.0.1:%d/?token=%s\n" % (port, TOKEN), flush=True)
    sys.stderr.write("fake core started with token %s\n" % TOKEN)
    sys.stderr.flush()
    sys.stdin.read()
    time.sleep(0.05)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
