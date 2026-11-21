"""Talking to a Heron core for the usability runner. Standard library only.

- ``parse_endpoint`` reads the line a core prints when it starts: the U3 ``--print-endpoint``
  JSON line (``{"port", "token", "background_owner", "version"}``) or, on a core from before U3,
  the ``http://127.0.0.1:<port>/?token=<t>`` banner line.
- ``Core`` starts ``heron serve --no-open --port 0`` (plus ``--print-endpoint`` when the binary
  has it) with a disposable ``HERON_HOME``. It holds the core's stdin open, because a
  ``--print-endpoint`` core exits when its stdin reaches EOF, and writes the core's output to log
  files only after the token is replaced by ``<redacted>``.
- ``Api`` sends requests with the ``x-heron-token`` header, runs one chat turn with an approval
  policy (``once``, ``session``, ``always`` or ``deny`` per tool), can drop the stream on purpose,
  and resumes a run through ``GET /api/events?since=<seq>``.

The token is never put on a command line, in an environment variable of another process, or in
a file. ``scan_for_secret`` checks an output folder for it after a run.
"""

import json
import os
import re
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from typing import Any, Dict, List, Optional, Tuple

HERE = os.path.dirname(os.path.abspath(__file__))
EVAL = os.path.dirname(HERE)
if EVAL not in sys.path:
    sys.path.insert(0, EVAL)

from eve_adapter import _sse_events  # noqa: E402  (the SSE reader U0 already tests)

URL_LINE = re.compile(r"http://127\.0\.0\.1:(\d+)/\?token=([A-Za-z0-9_\-]+)")
CORE_START_SECS = 60
TURN_SECS = 600
READ_SECS = 60
AFTER_END_SECS = 5
REDACTED = "<redacted>"
DECISIONS = ("once", "session", "always", "deny")


class ApiError(RuntimeError):
    """The core could not be reached."""


# ----------------------------------------------------------------------------- the endpoint

def parse_endpoint(line: str) -> Optional[Dict[str, Any]]:
    """The endpoint in one line of core output, or ``None``."""
    text = line.strip()
    if text.startswith("{"):
        try:
            value = json.loads(text)
        except ValueError:
            value = None
        if (isinstance(value, dict) and isinstance(value.get("port"), int) and not isinstance(value.get("port"), bool)
                and 0 < value["port"] < 65536 and isinstance(value.get("token"), str) and value["token"]):
            return {"port": value["port"], "token": value["token"],
                    "background_owner": value.get("background_owner"), "version": value.get("version"),
                    "source": "print-endpoint"}
        return None
    match = URL_LINE.search(text)
    if match:
        return {"port": int(match.group(1)), "token": match.group(2), "background_owner": None,
                "version": None, "source": "url"}
    return None


def supports_print_endpoint(binary: str) -> bool:
    """Whether ``heron serve --help`` lists ``--print-endpoint`` (U3)."""
    try:
        done = subprocess.run([binary, "serve", "--help"], capture_output=True, text=True, timeout=30,
                              stdin=subprocess.DEVNULL)
    except (OSError, subprocess.TimeoutExpired):
        return False
    return "--print-endpoint" in (done.stdout or "") + (done.stderr or "")


class Redactor:
    """Replaces every known secret in a text with ``<redacted>``."""

    def __init__(self) -> None:
        self.secrets: List[str] = []

    def add(self, secret: Optional[str]) -> None:
        if secret and secret not in self.secrets:
            self.secrets.append(secret)

    def __call__(self, text: str) -> str:
        for secret in self.secrets:
            text = text.replace(secret, REDACTED)
        return text


def scan_for_secret(root: str, secret: str) -> List[str]:
    """Every file under ``root`` whose bytes contain ``secret``."""
    hits = []
    needle = secret.encode("utf-8")
    for folder, _dirs, files in os.walk(root):
        for name in files:
            path = os.path.join(folder, name)
            try:
                with open(path, "rb") as handle:
                    if needle in handle.read():
                        hits.append(path)
            except OSError:
                continue
    return hits


def core_env(home: str, extra: Optional[Dict[str, str]] = None) -> Dict[str, str]:
    env = {k: v for k, v in os.environ.items() if not k.startswith("HERON_")}
    env["HERON_HOME"] = home
    env["HERON_ALLOW_LOCAL_FETCH"] = "1"  # the fixture web server is on 127.0.0.1
    env.update(extra or {})
    return env


class Core:
    """A running ``heron serve`` and the endpoint it printed."""

    def __init__(self, binary: str, home: str, log_dir: str, print_endpoint: Optional[bool] = None,
                 env_extra: Optional[Dict[str, str]] = None, start_secs: int = CORE_START_SECS) -> None:
        if print_endpoint is None:
            print_endpoint = supports_print_endpoint(binary)
        argv = [binary, "serve", "--no-open", "--port", "0"] + (["--print-endpoint"] if print_endpoint else [])
        self.redact = Redactor()
        self.endpoint: Optional[Dict[str, Any]] = None
        self._found = threading.Event()
        os.makedirs(log_dir, exist_ok=True)
        self._out_path = os.path.join(log_dir, "core.out.log")
        self._err_path = os.path.join(log_dir, "core.err.log")
        self.proc = subprocess.Popen(argv, env=core_env(home, env_extra), stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
        self._threads = [threading.Thread(target=self._pump_out, daemon=True),
                         threading.Thread(target=self._pump_err, daemon=True)]
        for thread in self._threads:
            thread.start()
        if not self._found.wait(start_secs) or self.endpoint is None:
            code = self.proc.poll()
            self.stop()
            raise ApiError("the core printed no endpoint within %ds%s" % (
                start_secs, "" if code is None else " (it exited with status %s)" % code))
        assert self.endpoint is not None
        self.base = "http://127.0.0.1:%d" % self.endpoint["port"]
        self.token = self.endpoint["token"]

    def _pump_out(self) -> None:
        assert self.proc.stdout is not None
        with open(self._out_path, "w", encoding="utf-8") as log:
            for line in self.proc.stdout:
                if not self._found.is_set():
                    endpoint = parse_endpoint(line)
                    if endpoint:
                        self.redact.add(endpoint["token"])
                        self.endpoint = endpoint
                        self._found.set()
                log.write(self.redact(line))
                log.flush()
        self._found.set()  # the core closed stdout: stop waiting, endpoint or not

    def _pump_err(self) -> None:
        assert self.proc.stderr is not None
        pending: List[str] = []
        with open(self._err_path, "w", encoding="utf-8") as log:
            for line in self.proc.stderr:
                if not self._found.is_set():
                    pending.append(line)  # the token isn't known yet: hold the line until it is
                    continue
                for held in pending:
                    log.write(self.redact(held))
                pending = []
                log.write(self.redact(line))
                log.flush()
            for held in pending:
                log.write(self.redact(held) if self.endpoint else "<held line dropped: no endpoint>\n")

    def alive(self) -> bool:
        return self.proc.poll() is None

    def stop(self) -> None:
        if self.proc.stdin and not self.proc.stdin.closed:
            try:
                self.proc.stdin.close()  # a --print-endpoint core exits on stdin EOF
            except OSError:
                pass
        try:
            self.proc.wait(5)
        except subprocess.TimeoutExpired:
            self.proc.terminate()
            try:
                self.proc.wait(10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
        for thread in self._threads:
            thread.join(5)
        for stream in (self.proc.stdout, self.proc.stderr):
            if stream is not None:
                stream.close()


# ----------------------------------------------------------------------------- approvals

def decide(policy: List[Dict[str, Any]], approval: Dict[str, Any]) -> Tuple[str, bool]:
    """``(decision, planned)`` for one approval. The first entry whose ``tool`` matches (and whose
    ``summary_regex`` matches, if it has one) answers with its ``decision`` (default ``once``).
    An ask nothing covers is denied, and ``planned`` is ``False``."""
    tool = approval.get("tool")
    summary = approval.get("summary") or ""
    for entry in policy or []:
        if entry.get("tool") != tool:
            continue
        regex = entry.get("summary_regex")
        if regex is None or re.search(regex, summary):
            decision = entry.get("decision", "once")
            if decision not in DECISIONS:
                raise ValueError("unknown approval decision %r" % decision)
            return decision, True
    return "deny", False


# ----------------------------------------------------------------------------- the API

class Api:
    """Requests against one core, carrying the session and the last ``seq`` and ``run_id`` seen."""

    def __init__(self, base: str, token: str, until_run_end: bool = False) -> None:
        self.base = base.rstrip("/")
        self._token = token
        self.until_run_end = until_run_end  # U3: /api/chat ends on run_end, which follows done
        self.session_id: Optional[str] = None
        self.run_id: Optional[str] = None
        self.last_seq: Optional[int] = None

    def _open(self, method: str, path: str, body: Any = None, timeout: float = 30):
        data = None if body is None else json.dumps(body).encode("utf-8")
        request = urllib.request.Request(self.base + path, data=data, method=method,
                                         headers={"x-heron-token": self._token, "content-type": "application/json"})
        return urllib.request.urlopen(request, timeout=timeout)

    def request(self, method: str, path: str, body: Any = None) -> Tuple[int, str]:
        try:
            with self._open(method, path, body) as response:
                return response.status, response.read().decode("utf-8", errors="replace")
        except urllib.error.HTTPError as exc:
            try:
                return exc.code, exc.read().decode("utf-8", errors="replace")
            finally:
                exc.close()
        except (urllib.error.URLError, OSError) as exc:
            raise ApiError("%s %s failed: %s" % (method, path, exc)) from exc

    def _note(self, event: Dict[str, Any]) -> None:
        seq = event.get("seq")
        if isinstance(seq, int) and not isinstance(seq, bool) and (self.last_seq is None or seq > self.last_seq):
            self.last_seq = seq
        if event.get("type") == "session":
            self.session_id = event.get("id", self.session_id)
            self.run_id = event.get("run_id", self.run_id)

    def _answer(self, event: Dict[str, Any], policy: List[Dict[str, Any]], decisions: List[Dict[str, Any]]) -> None:
        decision, planned = decide(policy, event)
        decisions.append({"tool": event.get("tool"), "decision": decision, "planned": planned})
        try:
            self._open("POST", "/api/approve", {"id": event.get("id"), "decision": decision}).close()
        except (urllib.error.URLError, OSError):
            pass  # it may have timed out already; the stream says what happened

    def _consume(self, response, policy, deadline, disconnect_after=None, run_filter=None, until_run_end=False):
        events: List[Dict[str, Any]] = []
        decisions: List[Dict[str, Any]] = []
        texts: List[str] = []
        done_text = None
        ttft = None
        started = time.monotonic()
        end = "closed"
        error = None
        ended_at = None
        try:
            for event in _sse_events(response, deadline):
                if run_filter is not None and event.get("run_id") not in (None, run_filter):
                    continue
                events.append(event)
                self._note(event)
                kind = event.get("type")
                if kind == "text":
                    if ttft is None:
                        ttft = int(round((time.monotonic() - started) * 1000))
                    texts.append(event.get("text", ""))
                elif kind == "approval":
                    self._answer(event, policy, decisions)
                elif kind in ("done", "cancelled", "error"):
                    if kind == "done":
                        done_text = event.get("text", "")
                    if kind == "error":
                        error = event.get("message", "")
                    end = kind
                    if not until_run_end:
                        break
                    ended_at = ended_at or time.monotonic()
                elif kind == "run_end":
                    end = "run_end" if end in ("closed", "done") else end
                    break
                if disconnect_after is not None and len(events) >= disconnect_after:
                    end = "disconnected"
                    break
                if ended_at is not None and time.monotonic() - ended_at > AFTER_END_SECS:
                    break
            else:
                if time.monotonic() >= deadline:
                    end = "timeout"
        except OSError as exc:
            end = "timeout" if "timed out" in str(exc) else "closed"
            error = str(exc)
        finally:
            response.close()
        reply = done_text if done_text is not None else "".join(texts)
        return {"events": events, "decisions": decisions, "reply": reply, "ttft_ms": ttft, "end": end, "error": error}

    def chat(self, message: str, policy: List[Dict[str, Any]], disconnect_after: Optional[int] = None,
             timeout: float = TURN_SECS) -> Dict[str, Any]:
        """One ``POST /api/chat``. Returns a step record (without ``kind`` and ``ms``)."""
        body: Dict[str, Any] = {"message": message}
        if self.session_id:
            body["session_id"] = self.session_id
        deadline = time.monotonic() + timeout
        try:
            response = self._open("POST", "/api/chat", body, timeout=READ_SECS)
        except urllib.error.HTTPError as exc:
            try:
                text = exc.read().decode("utf-8", errors="replace")
            finally:
                exc.close()
            return {"status": exc.code, "body": text[:2000], "events": [], "decisions": [], "reply": "",
                    "ttft_ms": None, "end": "http", "error": None}
        except (urllib.error.URLError, OSError) as exc:
            raise ApiError("POST /api/chat failed: %s" % exc) from exc
        status = response.status
        out = self._consume(response, policy, deadline, disconnect_after=disconnect_after,
                            until_run_end=self.until_run_end)
        out.update({"status": status, "body": ""})
        return out

    def resume(self, policy: List[Dict[str, Any]], timeout: float = TURN_SECS) -> Dict[str, Any]:
        """``GET /api/events?since=<last seq>`` until the last run's ``run_end``."""
        since = self.last_seq if self.last_seq is not None else 0
        deadline = time.monotonic() + timeout
        try:
            response = self._open("GET", "/api/events?since=%d" % since, timeout=READ_SECS)
        except urllib.error.HTTPError as exc:
            try:
                text = exc.read().decode("utf-8", errors="replace")
            finally:
                exc.close()
            return {"status": exc.code, "body": text[:2000], "events": [], "decisions": [], "reply": "",
                    "ttft_ms": None, "end": "http", "error": None}
        except (urllib.error.URLError, OSError) as exc:
            raise ApiError("GET /api/events failed: %s" % exc) from exc
        status = response.status
        out = self._consume(response, policy, deadline, run_filter=self.run_id, until_run_end=True)
        # a resumed stream can repeat the event at ``since``; keep each seq once
        seen, unique = set(), []
        for event in out["events"]:
            seq = event.get("seq")
            if seq is not None and seq in seen:
                continue
            seen.add(seq)
            unique.append(event)
        out["events"] = unique
        out.update({"status": status, "body": ""})
        return out
