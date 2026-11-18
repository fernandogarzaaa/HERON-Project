#!/usr/bin/env python3
"""The Heron v1 usability runner. Standard library only; run as ``python3 -I`` (``run.sh``).

    bash eval/usability/run.sh                    # replay: score the recorded fixtures (no core, no ports)
    bash eval/usability/run.sh --live             # start a core per task and run the scripts against it
    bash eval/usability/run.sh --check            # validate tasks.md and personas.json only

See ``eval/usability/README.md`` for the options, the report format and what each mode proves.
"""

import argparse
import datetime
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Dict, List, Optional, Tuple

HERE = os.path.dirname(os.path.abspath(__file__))
EVAL = os.path.dirname(HERE)
REPO = os.path.dirname(EVAL)
for path in (HERE, EVAL):
    if path not in sys.path:
        sys.path.insert(0, path)

import client  # noqa: E402
import pass_rules  # noqa: E402
import rules  # noqa: E402
import run_core_tasks  # noqa: E402
from tasks_md import TasksError, load_tasks  # noqa: E402

TASKS_PATH = os.path.join(HERE, "tasks.md")
PERSONAS_PATH = os.path.join(HERE, "personas.json")
FIXTURES = os.path.join(HERE, "fixtures")
CORE_FIXTURES = os.path.join(EVAL, "fixtures")
REQUIREMENTS = ("u3", "real_model")
STATUSES = ("ran-live", "replayed-recorded", "replayed-synthetic", "skipped-blocked", "harness-error")
BODY_CHARS = 2000
REPLY_CHARS = 2000
SEEN_TOKENS: List[str] = []  # every core token this process saw, for the leak check
VAR = re.compile(r"\{\{([A-Za-z_][A-Za-z0-9_]*)\}\}")


class UsageError(Exception):
    pass


# ----------------------------------------------------------------------------- loading and checks

def load_personas(path: str = PERSONAS_PATH) -> Dict[str, Dict[str, Any]]:
    with open(path, "r", encoding="utf-8") as handle:
        data = json.load(handle)
    return {p["id"]: p for p in data["personas"]}


def check_all(tasks: List[Dict[str, Any]], personas: Dict[str, Dict[str, Any]],
              core: Dict[str, Dict[str, Any]]) -> List[str]:
    """Every structural problem with the task scripts, as one sentence each."""
    problems: List[str] = []
    seen = set()
    for task in tasks:
        tid = task.get("id")
        where = "task %s" % tid
        if not isinstance(tid, str) or not re.match(r"^u\d\d$", tid):
            problems.append("%s: id must look like u01" % where)
        if tid in seen:
            problems.append("%s: duplicate id" % where)
        seen.add(tid)
        if task.get("_heading", "").split(":")[0] != tid:
            problems.append("%s: the heading id and the block id differ" % where)
        for key in ("title", "criteria", "metrics", "steps", "success", "personas", "requires", "mac"):
            if key not in task:
                problems.append("%s: missing %r" % (where, key))
        if problems and any(p.startswith(where + ": missing") for p in problems):
            continue
        if task.get("reuses") is not None and task["reuses"] not in core:
            problems.append("%s: reuses unknown core task %r" % (where, task["reuses"]))
        if not task["personas"] or any(p not in personas for p in task["personas"]):
            problems.append("%s: personas must be non-empty ids from personas.json" % where)
        if any(r not in REQUIREMENTS for r in task["requires"]):
            problems.append("%s: requires must be a subset of %s" % (where, ", ".join(REQUIREMENTS)))
        if sorted(task["metrics"]) != sorted(["completion", "time", "errors", "confusion"]):
            problems.append("%s: metrics must be completion, time, errors, confusion" % where)
        if not task["criteria"] or not all(isinstance(c, str) and c for c in task["criteria"]):
            problems.append("%s: criteria must be non-empty sentences" % where)
        variants = [("", task["steps"], task["success"])]
        if "demo" in task:
            variants.append((" (demo)", task["demo"].get("steps"), task["demo"].get("success")))
        for label, steps, success in variants:
            problems.extend(_check_steps("%s%s" % (where, label), steps, core))
            if isinstance(steps, list):
                problems.extend(rules.validate_rule(success, "%s%s success" % (where, label), len(steps)))
                problems.extend(_check_core_refs("%s%s success" % (where, label), success, core))
    return problems


def _check_steps(where: str, steps: Any, core: Dict[str, Dict[str, Any]]) -> List[str]:
    if not isinstance(steps, list) or not steps:
        return ["%s: steps must be a non-empty list" % where]
    problems = []
    for i, step in enumerate(steps):
        at = "%s step %d" % (where, i)
        kinds = [k for k in ("say", "say_from", "api", "resume", "plant") if k in step]
        if len(kinds) != 1:
            problems.append("%s: needs exactly one of say, say_from, api, resume, plant" % at)
            continue
        kind = kinds[0]
        if kind == "say_from" and step["say_from"] not in core:
            problems.append("%s: say_from names unknown core task %r" % (at, step["say_from"]))
        if "approve_from" in step and step["approve_from"] not in core:
            problems.append("%s: approve_from names unknown core task %r" % (at, step["approve_from"]))
        if "approve" in step and "approve_from" in step:
            problems.append("%s: use approve or approve_from, not both" % at)
        for entry in step.get("approve", []):
            if entry.get("decision", "once") not in client.DECISIONS:
                problems.append("%s: unknown decision %r" % (at, entry.get("decision")))
        if kind == "api" and not re.match(r"^(GET|POST|DELETE) /api/\S*$", step["api"]):
            problems.append("%s: api must be 'GET|POST|DELETE /api/...'" % at)
        if kind == "plant":
            target = step["plant"].get("path", "")
            if not re.match(r"^\{(home|work)\}/[^/]", target) or ".." in target.split("/"):
                problems.append("%s: plant.path must start with {home}/ or {work}/ and stay inside" % at)
    return problems


def _check_core_refs(where: str, rule: Any, core: Dict[str, Dict[str, Any]]) -> List[str]:
    if not isinstance(rule, dict) or len(rule) != 1:
        return []
    key, body = next(iter(rule.items()))
    if key == "all" and isinstance(body, list):
        return [p for sub in body for p in _check_core_refs(where, sub, core)]
    if key == "core_pass" and isinstance(body, dict) and body.get("task") not in core:
        return ["%s: core_pass names unknown core task %r" % (where, body.get("task"))]
    return []


def script_for(task: Dict[str, Any], provider: str) -> Tuple[List[Dict[str, Any]], Dict[str, Any], str]:
    """``(steps, success, variant)``: the demo variant when the provider is ``demo`` and the task has one."""
    if provider == "demo" and "demo" in task:
        return task["demo"]["steps"], task["demo"]["success"], "demo"
    return task["steps"], task["success"], "default"


def expand_step(step: Dict[str, Any], core: Dict[str, Dict[str, Any]]) -> Dict[str, Any]:
    """Resolve ``say_from`` and ``approve_from`` against ``eval/core-tasks.md``."""
    out = dict(step)
    if "say_from" in out:
        out["say"] = core[out.pop("say_from")]["prompt"]
    if "approve_from" in out:
        out["approve"] = [dict(e, decision=e.get("decision", "once")) for e in core[out.pop("approve_from")].get("approve", [])]
    return out


def substitute(value: Any, places: Dict[str, str], saved: Dict[str, Any]) -> Any:
    """Replace ``{web}``, ``{home}``, ``{work}`` and ``{{saved}}`` in strings, recursively. A string
    that is exactly ``{{name}}`` becomes the saved value itself, so a number stays a number."""
    if isinstance(value, dict):
        return {k: substitute(v, places, saved) for k, v in value.items()}
    if isinstance(value, list):
        return [substitute(v, places, saved) for v in value]
    if not isinstance(value, str):
        return value
    whole = VAR.fullmatch(value)
    if whole:
        if whole.group(1) not in saved:
            raise UsageError("{{%s}} is used before a step saved it" % whole.group(1))
        return saved[whole.group(1)]
    for name, text in places.items():
        value = value.replace("{%s}" % name, text)

    def repl(match):
        if match.group(1) not in saved:
            raise UsageError("{{%s}} is used before a step saved it" % match.group(1))
        return str(saved[match.group(1)])
    return VAR.sub(repl, value)


def dig(value: Any, dotted: str) -> Any:
    for part in dotted.split("."):
        if isinstance(value, list) and part.isdigit():
            value = value[int(part)] if int(part) < len(value) else None
        elif isinstance(value, dict):
            value = value.get(part)
        else:
            return None
    return value


# ----------------------------------------------------------------------------- scoring

def last_reply(steps: List[Dict[str, Any]]) -> str:
    for record in reversed(steps):
        if record.get("kind") in ("say", "resume"):
            return record.get("reply") or ""
    return ""


def all_events(steps: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    return [e for record in steps for e in (record.get("events") or []) if isinstance(e, dict)]


def score_fixture(fixture: Dict[str, Any], success: Dict[str, Any]) -> Dict[str, Any]:
    """Score a recorded or synthetic run: write its files and audit rows to a scratch folder,
    then apply the task's rule."""
    if os.path.join(EVAL, "genesis") not in sys.path:
        # appended, not inserted: eval/genesis/build_cases.py must not shadow eval/usability/genesis's
        sys.path.append(os.path.join(EVAL, "genesis"))
    import json_verifier  # U0g's materializer: files to a working folder, audit rows to heron.db

    steps = fixture["steps"]
    run = {"files": fixture.get("files", {}), "audit": fixture.get("audit", []),
           "session_id": fixture.get("session_id", "s1"), "web_log": fixture.get("web_log", []),
           "events": all_events(steps)}
    with tempfile.TemporaryDirectory(prefix="heron-usability-") as root:
        ctx, _tokens = json_verifier.materialize(run, root)
        ctx.reply = last_reply(steps)
        return rules.score(success, rules.Evidence(ctx=ctx, steps=steps))


# ----------------------------------------------------------------------------- reports

def write_json(path: str, data: Any) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as handle:
        json.dump(data, handle, indent=2, sort_keys=True)
        handle.write("\n")
    os.replace(tmp, path)


def run_report_md(entry: Dict[str, Any], task: Dict[str, Any]) -> str:
    lines = ["# %s: %s" % (task["id"], task["title"]), "",
             "- Persona: %s (%s)" % (entry["persona"], entry.get("persona_note") or "api run"),
             "- Status: **%s**%s" % (entry["status"], " (%s)" % entry["reason"] if entry.get("reason") else ""),
             "- Variant: %s" % entry.get("variant", "-")]
    if entry["status"] in ("skipped-blocked", "harness-error"):
        return "\n".join(lines) + "\n"
    lines += ["- Completed: **%s**" % ("yes" if entry["completed"] else "no"),
              "- Time: %d ms (first text after %s ms), %d approval(s) asked" % (
                  entry["time_ms"], entry["ttft_ms"] if entry["ttft_ms"] is not None else "-", entry["asks"]),
              "", "## Success criteria", ""]
    lines += ["- %s" % c for c in task["criteria"]]
    lines += ["", "## Failed checks", ""] + (["- `%s`" % f for f in entry["failed_checks"]] or ["- none"])
    lines += ["", "## Errors", ""] + (["- %s" % e for e in entry["errors"]] or ["- none"])
    lines += ["", "## Confusion points", ""] + (
        ["- step %s, %s: %s" % (c["step"], c["kind"], c["detail"]) for c in entry["confusion_points"]] or ["- none"])
    return "\n".join(lines) + "\n"


def summary_md(summary: Dict[str, Any]) -> str:
    lines = ["# Usability run, %s" % summary["started"], "",
             "Mode `%s`, provider `%s`, driver `%s`, Heron commit `%s`." % (
                 summary["mode"], summary["provider"], summary["driver"], summary["heron_commit"]), ""]
    if summary.get("capabilities"):
        lines += ["Core capabilities: " + ", ".join("%s=%s" % kv for kv in sorted(summary["capabilities"].items())), ""]
    lines += ["| task | persona | status | completed | time ms | asks | errors | confusion |",
              "|---|---|---|---|---|---|---|---|"]
    for e in summary["runs"]:
        done = e.get("completed")
        lines.append("| %s | %s | %s%s | %s | %s | %s | %s | %s |" % (
            e["task"], e["persona"] + (" (api-only)" if e.get("persona_note") else ""), e["status"],
            " (%s)" % e["reason"] if e.get("reason") else "",
            "-" if done is None else ("yes" if done else "**no**"), e.get("time_ms", "-"), e.get("asks", "-"),
            len(e.get("errors") or []) if done is not None else "-",
            len(e.get("confusion_points") or []) if done is not None else "-"))
    lines += ["", "Time, errors and confusion points are measurements, not gates. Completion comes from "
              "the task's success rule (`rules.py`), which `genesis/` audits. Rows marked api-only for an "
              "assistive persona say nothing about keyboard or VoiceOver use; that needs the Mac app run."]
    return "\n".join(lines) + "\n"


# ----------------------------------------------------------------------------- replay

def fixture_for(task_id: str) -> Tuple[Optional[str], Optional[Dict[str, Any]]]:
    for kind in ("recorded", "synthetic"):
        path = os.path.join(FIXTURES, kind, "%s.json" % task_id)
        if os.path.isfile(path):
            with open(path, "r", encoding="utf-8") as handle:
                return kind, json.load(handle)
    return None, None


def replay_task(task: Dict[str, Any], persona_id: str, personas) -> Dict[str, Any]:
    kind, fixture = fixture_for(task["id"])
    entry = base_entry(task, persona_id, personas)
    if fixture is None:
        entry.update(status="skipped-blocked", reason="no recorded or synthetic fixture yet")
        return entry
    steps, success, variant = script_for(task, fixture.get("provider", "demo"))
    if len(fixture["steps"]) != len(steps):
        entry.update(status="harness-error", reason="the fixture has %d steps, the script %d" % (
            len(fixture["steps"]), len(steps)))
        return entry
    entry.update(status="replayed-%s" % kind, variant=variant, fixture_provider=fixture.get("provider"),
                 fixture_commit=fixture.get("heron_commit"))
    entry.update(score_fixture(fixture, success))
    return entry


def base_entry(task, persona_id, personas) -> Dict[str, Any]:
    persona = personas[persona_id]
    note = None
    if persona.get("assistive"):
        note = "api-only: no %s coverage; needs the Mac app run" % persona["assistive"]
    return {"task": task["id"], "persona": persona_id, "surface": "api", "persona_note": note,
            "status": None, "reason": None, "completed": None}


# ----------------------------------------------------------------------------- live

class EveBridge:
    """``POST /turn {"message"}`` on 127.0.0.1 for ``eve chat``, relayed to ``Api.chat`` in this
    process, so the token never reaches EVE's command line."""

    def __init__(self, api: client.Api, policy: List[Dict[str, Any]]) -> None:
        self.records: List[Dict[str, Any]] = []
        bridge = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, fmt, *args):
                pass

            def do_POST(self):
                try:
                    length = int(self.headers.get("content-length", "0"))
                    message = json.loads(self.rfile.read(length).decode("utf-8")).get("message")
                except (ValueError, AttributeError):
                    message = None
                if not isinstance(message, str) or not message.strip():
                    return self._send(400, {"error": "expected {\"message\": \"...\"}"})
                started = time.monotonic()
                try:
                    record = api.chat(message, policy)
                except client.ApiError as exc:
                    record = {"status": None, "body": "", "events": [], "decisions": [], "reply": "",
                              "ttft_ms": None, "end": "error", "error": str(exc)}
                record["ms"] = int(round((time.monotonic() - started) * 1000))
                record["message"] = message
                bridge.records.append(record)
                self._send(200, {"reply": record["reply"]})

            def _send(self, code, payload):
                data = json.dumps(payload).encode("utf-8")
                try:
                    self.send_response(code)
                    self.send_header("content-type", "application/json")
                    self.send_header("content-length", str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)
                except OSError:
                    pass  # EVE stops waiting after 30 s; the record is kept anyway

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.url = "http://127.0.0.1:%d/turn" % self.server.server_address[1]
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()


def eve_say(api, text, policy, persona, turns, out_dir) -> Dict[str, Any]:
    """Let EVE's persona open with ``text`` and carry on for up to ``turns`` turns."""
    bridge = EveBridge(api, policy)
    argv = ["eve", "chat", bridge.url, "--kind", "assistant", "--persona", persona["eve_persona"],
            "--goal", text, "--turns", str(turns), "--reply-path", "reply",
            "--body", '{"message": "{{message}}"}', "--out", out_dir, "--json", "--quiet"]
    if persona.get("eve_profession"):
        argv += ["--profession", persona["eve_profession"]]
    started = time.monotonic()
    try:
        done = subprocess.run(argv, capture_output=True, text=True, timeout=client.TURN_SECS * max(1, turns))
    finally:
        bridge.close()
    analysis = None
    try:
        analysis = json.loads(done.stdout)
    except ValueError:
        pass
    records = bridge.records
    record = {"events": [e for r in records for e in r["events"]],
              "decisions": [d for r in records for d in r["decisions"]],
              "reply": records[-1]["reply"] if records else "", "ttft_ms": records[0]["ttft_ms"] if records else None,
              "status": records[-1]["status"] if records else None, "body": "",
              "end": records[-1]["end"] if records else "closed",
              "error": None if done.returncode != 2 else "eve chat rejected its arguments: %s" % done.stderr[:300],
              "turns": [{"message": r["message"], "reply": r["reply"][:REPLY_CHARS]} for r in records],
              "ms": int(round((time.monotonic() - started) * 1000))}
    if analysis:
        record["eve"] = {k: analysis.get(k) for k in ("understanding", "grounding", "recovery", "operatorTurns",
                                                    "repairAttempts", "admittedMisses", "silentMisses")}
        findings = list(analysis.get("findings") or [])
        if analysis.get("repairAttempts"):
            findings.append("the persona rephrased %d time(s)" % analysis["repairAttempts"])
        record["eve_findings"] = findings
    if not records and record["error"] is None:
        record["error"] = "eve chat sent no message (exit %d): %s" % (done.returncode, done.stderr[:300])
    return record


def probe_capabilities(api: client.Api, core: client.Core) -> Dict[str, Any]:
    caps = {"print_endpoint": core.endpoint["source"] == "print-endpoint",
            "background_owner": core.endpoint.get("background_owner")}
    status, _ = api.request("GET", "/api/risk-notice")
    caps["risk_notice_route"] = status != 404
    try:
        with api._open("GET", "/api/events?since=0", timeout=3) as response:
            caps["events_route"] = response.status == 200
    except client.urllib.error.HTTPError as exc:
        caps["events_route"] = exc.code != 404
        exc.close()
    except OSError:
        caps["events_route"] = False
    caps["u3"] = bool(caps["risk_notice_route"] and caps["events_route"])
    return caps


def unmet(task, provider, caps) -> List[str]:
    missing = []
    for need in task["requires"]:
        if need == "u3" and not caps.get("u3"):
            missing.append("U3 (core API contract) not on this core")
        if need == "real_model" and provider == "demo":
            missing.append("a real model (the demo model can't do this script)")
    return missing


def live_task(task, persona_id, personas, core_tasks, opts, out_dir, caps_cache) -> Dict[str, Any]:
    entry = base_entry(task, persona_id, personas)
    steps_spec, success, variant = script_for(task, opts.provider)
    entry["variant"] = variant
    if "real_model" in task["requires"] and opts.provider == "demo":
        entry.update(status="skipped-blocked", reason="needs a real model (the demo model can't do this script)")
        return entry
    run_dir = os.path.join(out_dir, "runs", "%s-%s" % (task["id"], persona_id))
    if os.path.exists(run_dir):
        shutil.rmtree(run_dir)
    home, work, web_dir = (os.path.join(run_dir, d) for d in ("home", "work", "web"))
    for folder in (home, work):
        os.makedirs(folder)
    web = None
    core = None
    try:
        web = run_core_tasks.FixtureWeb(web_dir, os.path.join(run_dir, "web.log"))
        core_task = core_tasks.get(task.get("reuses")) or {"fixture": None}
        run_core_tasks.prepare_fixture(core_task, CORE_FIXTURES, work, web_dir, web.base)
        with open(os.path.join(home, "config.toml"), "w", encoding="utf-8") as handle:
            handle.write(run_core_tasks.config_toml(work, opts.provider))
        core = client.Core(opts.heron_bin, home, os.path.join(run_dir, "logs"))
        SEEN_TOKENS.append(core.token)
        api = client.Api(core.base, core.token)
        caps = probe_capabilities(api, core)
        caps_cache.update(caps)
        api.until_run_end = caps["u3"]
        missing = unmet(task, opts.provider, caps)
        if missing:
            entry.update(status="skipped-blocked", reason="; ".join(missing))
            return entry
        if caps["risk_notice_route"] and task.get("accept_risk_notice", True):
            run_core_tasks.accept_risk_notice(core.base, core.token)
        places = {"web": web.base, "home": home, "work": work}
        saved: Dict[str, Any] = {}
        records: List[Dict[str, Any]] = []
        persona = personas[persona_id]
        for index, raw in enumerate(steps_spec):
            step = substitute(expand_step(raw, core_tasks), places, saved)
            started = time.monotonic()
            record: Dict[str, Any] = {"index": index}
            if "plant" in step:
                target = step["plant"]["path"]
                os.makedirs(os.path.dirname(target), exist_ok=True)
                with open(target, "w", encoding="utf-8") as handle:
                    handle.write(step["plant"]["text"])
                record.update(kind="plant", status=None, events=[], reply="", error=None)
            elif "api" in step:
                method, path = step["api"].split(" ", 1)
                status, body = api.request(method, path, step.get("body"))
                record.update(kind="api", request=step["api"], status=status, body=body[:BODY_CHARS],
                              events=[], reply="", error=None)
                for name, dotted in (step.get("save") or {}).items():
                    try:
                        saved[name] = dig(json.loads(body), dotted)
                    except ValueError:
                        saved[name] = None
            elif "resume" in step:
                record.update(kind="resume", **api.resume(step.get("approve", [])))
            elif opts.driver == "eve":
                record.update(kind="say", message=step["say"],
                              **eve_say(api, step["say"], step.get("approve", []), persona,
                                        int(step.get("eve_turns", 1)), os.path.join(run_dir, "eve-%d" % index)))
            else:
                record.update(kind="say", message=step["say"],
                              **api.chat(step["say"], step.get("approve", []), step.get("disconnect_after")))
            record.setdefault("ms", int(round((time.monotonic() - started) * 1000)))
            record["reply"] = (record.get("reply") or "")[:REPLY_CHARS]
            records.append(record)
        web_requests = run_core_tasks.read_web_log(os.path.join(run_dir, "web.log"))
        ctx = pass_rules.Context(workdir=work, reply=last_reply(records), db_path=os.path.join(home, "heron.db"),
                                 session_id=api.session_id or "", web_requests=web_requests, events=all_events(records))
        entry.update(status="ran-live", **rules.score(success, rules.Evidence(ctx=ctx, steps=records)))
        live = {"task": task["id"], "persona": persona_id, "provider": opts.provider, "variant": variant,
                "driver": opts.driver, "heron_commit": run_core_tasks.git_commit(), "session_id": api.session_id or "",
                "steps": records, "web_log": web_requests,
                "files": snapshot_files(success, work), "audit": audit_rows(ctx)}
        write_json(os.path.join(run_dir, "record.json"), redact_tree(live, core.redact))
        if opts.record_fixtures:
            fixture = scrub(dict(live, kind="recorded",
                                 _provenance="Recorded by `run.sh --live --record-fixtures` against a real core; "
                                             "absolute paths, ports and the token are replaced by placeholders."),
                            places, core.redact)
            write_json(os.path.join(opts.record_fixtures, "%s.json" % task["id"]), fixture)
        return entry
    except (client.ApiError, run_core_tasks.RunError, UsageError, OSError) as exc:
        entry.update(status="harness-error", reason=str(exc)[:300] if core is None else core.redact(str(exc))[:300])
        return entry
    finally:
        if core is not None:
            core.stop()
        if web is not None:
            web.stop()


def leaf_paths(rule: Any) -> List[str]:
    if not isinstance(rule, dict) or len(rule) != 1:
        return []
    key, body = next(iter(rule.items()))
    if key == "all":
        return [p for sub in body for p in leaf_paths(sub)]
    if key in ("file_contains", "file_present") and isinstance(body, dict):
        return [body["path"]]
    if key == "core_pass":
        return leaf_paths(rules.core_task(body["task"])["pass"])
    return []


def snapshot_files(rule, work) -> Dict[str, str]:
    out = {}
    for rel in leaf_paths(rule):
        path = os.path.join(work, rel)
        if os.path.isfile(path):
            with open(path, "r", encoding="utf-8", errors="replace") as handle:
                out[rel] = handle.read()[:20000]
    return out


def audit_rows(ctx) -> List[Dict[str, str]]:
    try:
        conn = pass_rules._read_only_connection(ctx.db_path)
    except pass_rules.RuleError:
        return []
    try:
        rows = conn.execute("SELECT tool, decision FROM audit WHERE session_id = ? ORDER BY id",
                            (ctx.session_id,)).fetchall()
    except Exception:  # noqa: BLE001  (an older schema: record nothing rather than fail the run)
        rows = []
    finally:
        conn.close()
    return [{"tool": t or "", "decision": d} for t, d in rows]


def redact_tree(value: Any, redact) -> Any:
    if isinstance(value, dict):
        return {k: redact_tree(v, redact) for k, v in value.items()}
    if isinstance(value, list):
        return [redact_tree(v, redact) for v in value]
    if isinstance(value, str):
        return redact(value)
    return value


def scrub(value: Any, places: Dict[str, str], redact) -> Any:
    """Placeholders for this run's paths and web base, ``~`` for the owner's home, and no user name."""
    user_home = os.path.expanduser("~")
    user = os.path.basename(user_home)
    ordered = sorted(places.items(), key=lambda kv: -len(kv[1]))

    def one(text: str) -> str:
        text = redact(text)
        for name, path in ordered:
            text = text.replace(os.path.realpath(path), "{%s}" % name).replace(path, "{%s}" % name)
        text = text.replace(user_home, "~")
        return re.sub(r"\b%s\b" % re.escape(user), "<user>", text) if user else text

    if isinstance(value, dict):
        return {k: scrub(v, places, redact) for k, v in value.items()}
    if isinstance(value, list):
        return [scrub(v, places, redact) for v in value]
    if isinstance(value, str):
        return one(value)
    return value


# ----------------------------------------------------------------------------- main

def parse_args(argv):
    p = argparse.ArgumentParser(description="Run Heron's usability tasks (replay by default).")
    p.add_argument("--live", action="store_true", help="start a core per task instead of replaying fixtures")
    p.add_argument("--check", action="store_true", help="validate tasks.md and personas.json, then exit")
    p.add_argument("--provider", default="demo", help="demo (default) or a CLI provider id such as claude-cli")
    p.add_argument("--driver", choices=("direct", "eve"), default="direct",
                   help="direct: the runner sends each message; eve: an EVE persona opens with it (live only)")
    p.add_argument("--heron-bin", help="the heron binary (default: target/debug/heron, then target/release/heron)")
    p.add_argument("--tasks", help="comma-separated task ids (default: all)")
    p.add_argument("--all-personas", action="store_true", help="run each task once per listed persona")
    p.add_argument("--out", help="report folder (default: eval/usability/out/<time>-<mode>)")
    p.add_argument("--record-fixtures", metavar="DIR", help="live: also write scrubbed fixtures for replay here")
    p.add_argument("--strict", action="store_true", help="exit 1 when any run that ran did not complete")
    return p.parse_args(argv)


def default_binary() -> Optional[str]:
    for build in ("debug", "release"):
        path = os.path.join(REPO, "target", build, "heron")
        if os.access(path, os.X_OK):
            return path
    return None


def main(argv) -> int:
    opts = parse_args(argv)
    try:
        tasks = load_tasks(TASKS_PATH)
        core_tasks = {t["id"]: t for t in load_tasks(rules.CORE_TASKS)}
        personas = load_personas()
    except (OSError, ValueError, KeyError, TasksError) as exc:
        print("usability: cannot load the task files: %s" % exc, file=sys.stderr)
        return 2
    problems = check_all(tasks, personas, core_tasks)
    if problems:
        for problem in problems:
            print("usability: %s" % problem, file=sys.stderr)
        return 2
    if opts.check:
        print("usability: %d tasks and %d personas are well formed" % (len(tasks), len(personas)))
        return 0
    if opts.tasks:
        wanted = opts.tasks.split(",")
        unknown = [t for t in wanted if t not in {t["id"] for t in tasks}]
        if unknown:
            print("usability: unknown task id(s): %s" % ", ".join(unknown), file=sys.stderr)
            return 2
        tasks = [t for t in tasks if t["id"] in wanted]
    if opts.driver == "eve" and not opts.live:
        print("usability: --driver eve needs --live", file=sys.stderr)
        return 2
    if opts.live:
        opts.heron_bin = opts.heron_bin or default_binary()
        if not opts.heron_bin or not os.access(opts.heron_bin, os.X_OK):
            print("usability: no heron binary; build one with `cargo build --locked` or pass --heron-bin",
                  file=sys.stderr)
            return 2
        if opts.driver == "eve" and shutil.which("eve") is None:
            print("usability: --driver eve needs `eve` on PATH", file=sys.stderr)
            return 3
    mode = "live" if opts.live else "replay"
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out_dir = os.path.abspath(opts.out or os.path.join(HERE, "out", "%s-%s" % (stamp, mode)))
    os.makedirs(out_dir, exist_ok=True)
    caps: Dict[str, Any] = {}
    entries = []
    for task in tasks:
        for persona_id in (task["personas"] if opts.all_personas else task["personas"][:1]):
            if opts.live:
                entry = live_task(task, persona_id, personas, core_tasks, opts, out_dir, caps)
            else:
                entry = replay_task(task, persona_id, personas)
            entries.append(entry)
            write_json(os.path.join(out_dir, "runs", "%s-%s" % (task["id"], persona_id), "report.json"), entry)
            with open(os.path.join(out_dir, "runs", "%s-%s" % (task["id"], persona_id), "report.md"), "w",
                      encoding="utf-8") as handle:
                handle.write(run_report_md(entry, task))
            print("usability: %s %-14s %-19s %s" % (task["id"], persona_id, entry["status"],
                                                   "" if entry.get("completed") is None
                                                   else ("completed" if entry["completed"] else "NOT completed: "
                                                         + "; ".join(entry["failed_checks"])[:200])))
    summary = {"schema": 1, "started": stamp, "mode": mode, "provider": opts.provider if opts.live else "fixtures",
               "driver": opts.driver if opts.live else "replay", "heron_commit": run_core_tasks.git_commit(),
               "capabilities": caps if opts.live else None, "runs": entries}
    write_json(os.path.join(out_dir, "summary.json"), summary)
    with open(os.path.join(out_dir, "summary.md"), "w", encoding="utf-8") as handle:
        handle.write(summary_md(summary))
    leaks = check_leaks(out_dir)
    if leaks:
        print("usability: a token-shaped secret was found in: %s" % ", ".join(leaks), file=sys.stderr)
        return 1
    print("usability: report in %s" % out_dir)
    if any(e["status"] == "harness-error" for e in entries):
        return 1
    if opts.strict and any(e.get("completed") is False for e in entries):
        return 1
    return 0


def check_leaks(out_dir: str, tokens: Optional[List[str]] = None) -> List[str]:
    """Report files under ``out_dir`` that hold a core token: one of ``tokens`` verbatim, a
    ``?token=`` URL, or a long ``"token":`` value. The disposable ``home/`` folders are the core's
    own state and are skipped."""
    pattern = re.compile(rb"\?token=[A-Za-z0-9_\-]{8,}|\"token\"\s*:\s*\"[A-Za-z0-9_\-]{16,}\"")
    needles = [t.encode("utf-8") for t in (SEEN_TOKENS if tokens is None else tokens) if t]
    hits = []
    for folder, _dirs, files in os.walk(out_dir):
        if "home" in os.path.relpath(folder, out_dir).split(os.sep):
            continue  # the disposable HERON_HOME is the core's own state, not a report
        for name in files:
            path = os.path.join(folder, name)
            try:
                with open(path, "rb") as handle:
                    data = handle.read()
                    if pattern.search(data) or any(n in data for n in needles):
                        hits.append(os.path.relpath(path, out_dir))
            except OSError:
                continue
    return hits


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
