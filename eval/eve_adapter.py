#!/usr/bin/env python3
"""Bridge between EVE and Heron's API (U0, item 3). Standard library only.

EVE's HTTP backend posts one JSON request and reads the reply at a path. Heron's
``/api/chat`` answers with a server-sent-event stream, and a live approval's id travels only
inside that stream. This adapter reads the stream, answers each approval from the task's
``approve`` list, and hands back one reply.

As a library: ``turn(base, token, message, policy, session_id=None)``.

As a server: ``python3 -I eval/eve_adapter.py --heron <base> --token <t> --task <id> --port 0
--record <file>`` serves ``POST /turn {"message"}`` on 127.0.0.1, prints ``port=<n>`` on its
first line, and appends each ``turn()`` result to ``<file>`` as one JSON line.
"""

import argparse
import json
import os
import re
import sys
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

TURN_TIMEOUT_SECS = 600
READ_TIMEOUT_SECS = 60  # Heron sends a keep-alive every 15 seconds, so silence this long is a fault.
HERE = os.path.dirname(os.path.abspath(__file__))


class TurnError(RuntimeError):
    """Heron refused the turn (a non-200 answer) or could not be reached."""


def decide(policy, approval):
    """``"once"`` when the task's ``approve`` list covers this ask, ``"deny"`` otherwise."""
    tool = approval.get("tool")
    summary = approval.get("summary") or ""
    for entry in policy or []:
        if entry.get("tool") != tool:
            continue
        regex = entry.get("summary_regex")
        if regex is None or re.search(regex, summary):
            return "once"
    return "deny"


def _post_json(base, token, path, body, timeout=30):
    request = urllib.request.Request(
        base.rstrip("/") + path,
        data=json.dumps(body).encode("utf-8"),
        headers={"content-type": "application/json", "x-heron-token": token},
        method="POST",
    )
    return urllib.request.urlopen(request, timeout=timeout)


def _sse_events(response, deadline):
    """Yield each ``data:`` payload of an SSE stream as a dict, until it ends or time is up."""
    data_lines = []
    while time.monotonic() < deadline:
        raw = response.readline()
        if not raw:
            break
        line = raw.decode("utf-8", errors="replace").rstrip("\r\n")
        if line == "":
            if data_lines:
                payload = "\n".join(data_lines)
                data_lines = []
                try:
                    yield json.loads(payload)
                except ValueError:
                    yield {"type": "malformed", "raw": payload}
            continue
        if line.startswith(":"):
            continue
        if line.startswith("data:"):
            data_lines.append(line[5:].lstrip(" "))
    if data_lines:
        try:
            yield json.loads("\n".join(data_lines))
        except ValueError:
            pass


def turn(base, token, message, policy, session_id=None, timeout=TURN_TIMEOUT_SECS):
    """Run one user message through ``POST /api/chat`` and return what happened.

    Returns ``{reply, session_id, events, ttft_ms, tokens, end, error}``. ``end`` is the event
    that stopped the stream: ``done``, ``cancelled``, ``error``, ``closed`` (the stream ended
    with none of those) or ``timeout``. ``tokens`` is ``prompt + completion`` from the last
    ``usage`` event, because each ``usage`` event carries the running total of the run.
    """
    started = time.monotonic()
    deadline = started + timeout
    body = {"message": message}
    if session_id:
        body["session_id"] = session_id
    try:
        response = _post_json(base, token, "/api/chat", body, timeout=READ_TIMEOUT_SECS)
    except urllib.error.HTTPError as exc:
        try:
            detail = exc.read().decode("utf-8", errors="replace")[:500]
        except OSError:
            detail = "(the error body could not be read)"
        finally:
            exc.close()
        raise TurnError("POST /api/chat answered %d: %s" % (exc.code, detail))
    except (urllib.error.URLError, OSError) as exc:
        raise TurnError("POST /api/chat failed: %s" % exc)

    events = []
    texts = []
    done_text = None
    ttft_ms = None
    last_usage = None
    end = "closed"
    error = None
    sid = session_id
    try:
        for event in _sse_events(response, deadline):
            events.append(event)
            kind = event.get("type")
            if kind == "session":
                sid = event.get("id", sid)
            elif kind == "text":
                if ttft_ms is None:
                    ttft_ms = int(round((time.monotonic() - started) * 1000))
                texts.append(event.get("text", ""))
            elif kind == "usage":
                last_usage = event
            elif kind == "approval":
                decision = decide(policy, event)
                try:
                    _post_json(base, token, "/api/approve", {"id": event.get("id"), "decision": decision}).close()
                except (urllib.error.URLError, OSError):
                    pass  # the ask may already have timed out; the stream says what happened
            elif kind == "done":
                done_text = event.get("text", "")
                end = "done"
                break
            elif kind == "cancelled":
                end = "cancelled"
                break
            elif kind == "error":
                end = "error"
                error = event.get("message", "")
                break
        else:
            if time.monotonic() >= deadline:
                end = "timeout"
    except OSError as exc:  # a read timeout or a reset connection
        end = "timeout" if "timed out" in str(exc) else "closed"
        error = str(exc)
    finally:
        response.close()

    tokens = 0
    if last_usage is not None:
        tokens = int(last_usage.get("prompt", 0) or 0) + int(last_usage.get("completion", 0) or 0)
    reply = done_text if done_text is not None else "".join(texts)
    return {"reply": reply, "session_id": sid, "events": events, "ttft_ms": ttft_ms,
            "tokens": tokens, "end": end, "error": error}


def load_policy(tasks_path, task_id):
    from tasks_md import load_tasks

    for task in load_tasks(tasks_path):
        if task.get("id") == task_id:
            return task.get("approve", [])
    raise SystemExit("eve_adapter: unknown task id %r in %s" % (task_id, tasks_path))


def make_handler(state):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, fmt, *args):  # keep stdout clean: the first line is port=<n>
            pass

        def _answer(self, code, payload):
            data = json.dumps(payload).encode("utf-8")
            try:
                self.send_response(code)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
            except OSError:
                pass  # EVE gave up waiting (its own timeout is 30 s); the record is already written

        def do_POST(self):
            if self.path.split("?")[0] != "/turn":
                return self._answer(404, {"error": "not found"})
            try:
                length = int(self.headers.get("content-length", "0"))
                message = json.loads(self.rfile.read(length).decode("utf-8")).get("message")
            except (ValueError, AttributeError):
                return self._answer(400, {"error": "expected {\"message\": \"...\"}"})
            if not isinstance(message, str) or not message.strip():
                return self._answer(400, {"error": "expected {\"message\": \"...\"}"})
            with state["lock"]:
                open(state["record"] + ".started", "a").close()  # lets the runner tell "never asked" from "still working"
                try:
                    result = turn(state["heron"], state["token"], message, state["policy"], state["session_id"])
                except TurnError as exc:
                    result = {"reply": "", "session_id": state["session_id"], "events": [], "ttft_ms": None,
                              "tokens": 0, "end": "error", "error": str(exc)}
                state["session_id"] = result.get("session_id") or state["session_id"]
                result["message"] = message
                with open(state["record"], "a", encoding="utf-8") as handle:
                    handle.write(json.dumps(result) + "\n")
            if result["error"] and not result["reply"]:
                return self._answer(502, {"reply": "", "error": result["error"]})
            self._answer(200, {"reply": result["reply"]})

    return Handler


def serve(args):
    state = {"heron": args.heron, "token": args.token, "record": args.record, "session_id": None,
             "policy": load_policy(args.tasks, args.task), "lock": threading.Lock()}
    server = ThreadingHTTPServer(("127.0.0.1", args.port), make_handler(state))
    server.daemon_threads = True
    print("port=%d" % server.server_address[1], flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


def main(argv):
    parser = argparse.ArgumentParser(description="Serve POST /turn for EVE and relay it to Heron's /api/chat.")
    parser.add_argument("--heron", required=True, help="base URL of the Heron core, e.g. http://127.0.0.1:4000")
    parser.add_argument("--token", required=True)
    parser.add_argument("--task", required=True, help="task id whose approve list answers the asks")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--record", required=True, help="file that gets one JSON line per turn")
    parser.add_argument("--tasks", default=os.path.join(HERE, "core-tasks.md"))
    return serve(parser.parse_args(argv))


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
