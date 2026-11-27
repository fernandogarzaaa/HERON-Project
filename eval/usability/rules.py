"""Usability rules and metrics for ``eval/usability/tasks.md``. Standard library only.

A usability rule is a JSON object with exactly one key. The leaves of ``eval/pass_rules.py`` are
used as they are, through ``pass_rules.evaluate`` (that file is U0's and is not changed here).
This module adds the leaves a multi-step usability script needs:

- ``core_pass`` (``task``): the pass rule of that core task in ``eval/core-tasks.md`` holds.
- ``event_seen`` / ``event_absent`` (``type``, optional ``step``, optional ``where``): an event of
  that type was (or was not) seen, in that step or in any step. ``where`` maps field names to
  regexes, matched with ``re.search`` against the field's value; a non-string value is matched as
  its JSON text (``true``, ``false``, ``3``). A missing field never matches.
- ``http_status`` (``step``, ``status``, optional ``body_regex``): that step answered with that
  HTTP status, and its body matches.
- ``step_reply_matches`` (``step``, ``regex``): that step's reply matches, case-insensitively.
- ``all``: a non-empty list of rules, every one of which holds.

The verdict (``completed``) is what ``genesis/`` audits. The metrics (time, errors, confusion
points) are measurements: they are reported, never gated, and not audited.
"""

import json
import os
import re
import sys
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Tuple

HERE = os.path.dirname(os.path.abspath(__file__))
EVAL = os.path.dirname(HERE)
if EVAL not in sys.path:
    sys.path.insert(0, EVAL)

import pass_rules  # noqa: E402
from tasks_md import load_tasks  # noqa: E402

CORE_TASKS = os.path.join(EVAL, "core-tasks.md")
STEP_KINDS = ("say", "api", "resume", "plant")
WHERE_FIELD = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")

# key -> {field: (type, required)}. dict means "an object of regex strings".
LEAF_SPECS: Dict[str, Dict[str, Tuple[type, bool]]] = {
    "core_pass": {"task": (str, True)},
    "event_seen": {"type": (str, True), "step": (int, False), "where": (dict, False)},
    "event_absent": {"type": (str, True), "step": (int, False), "where": (dict, False)},
    "http_status": {"step": (int, True), "status": (int, True), "body_regex": (str, False)},
    "step_reply_matches": {"step": (int, True), "regex": (str, True)},
}
RULE_KEYS = tuple(LEAF_SPECS) + tuple(pass_rules.RULE_SPECS) + ("all",)

# Measurement heuristics (not gated, not audited).
CONFUSED_REPLY = re.compile(
    r"\b(i'?m not sure what you mean|could you clarify|can you clarify|i don'?t understand|"
    r"which (file|folder) do you mean|please specify|could you be more specific)\b"
    r"|demo mode\*\*: my replies are canned",
    re.IGNORECASE,
)

RuleError = pass_rules.RuleError


@dataclass
class Evidence:
    """What one usability run left behind.

    ``ctx`` is the ``pass_rules.Context`` for the pass-rule leaves: its ``reply`` is the last
    ``say`` step's reply and its ``events`` are every step's events. ``steps`` is the list of step
    records the runner wrote, in script order.
    """

    ctx: pass_rules.Context
    steps: List[Dict[str, Any]] = field(default_factory=list)


# ----------------------------------------------------------------------------- grammar

def _type_ok(value: Any, kind: type) -> bool:
    if kind is int:
        return isinstance(value, int) and not isinstance(value, bool)
    return isinstance(value, kind)


def validate_rule(rule: Any, where: str = "rule", n_steps: Optional[int] = None) -> List[str]:
    """Problems with ``rule``'s grammar; empty means well formed. With ``n_steps``, every
    ``step`` index must name a step of the script."""
    if not isinstance(rule, dict):
        return ["%s: a rule must be a JSON object" % where]
    if len(rule) != 1:
        return ["%s: a rule must have exactly one key, got %d" % (where, len(rule))]
    key = next(iter(rule))
    body = rule[key]
    if key not in RULE_KEYS:
        return ["%s: unknown rule key %r" % (where, key)]
    if key == "all":
        if not isinstance(body, list) or not body:
            return ["%s: 'all' needs a non-empty list of rules" % where]
        problems: List[str] = []
        for i, sub in enumerate(body):
            problems.extend(validate_rule(sub, "%s.all[%d]" % (where, i), n_steps))
        return problems
    if key in pass_rules.RULE_SPECS:
        return pass_rules.validate_rule(rule, where)
    if not isinstance(body, dict):
        return ["%s: %s needs an object of fields" % (where, key)]
    spec = LEAF_SPECS[key]
    problems = ["%s: %s has an unknown field %r" % (where, key, name) for name in body if name not in spec]
    for name, (kind, required) in spec.items():
        if name not in body:
            if required:
                problems.append("%s: %s needs the field %r" % (where, key, name))
        elif not _type_ok(body[name], kind):
            problems.append("%s: %s.%s has the wrong type" % (where, key, name))
    if problems:
        return problems
    if "step" in body:
        if body["step"] < 0:
            problems.append("%s: %s.step must not be negative" % (where, key))
        elif n_steps is not None and body["step"] >= n_steps:
            problems.append("%s: %s.step %d is past the script's %d steps" % (where, key, body["step"], n_steps))
    regexes: List[Tuple[str, Any]] = []
    if key in ("event_seen", "event_absent"):
        for name, regex in (body.get("where") or {}).items():
            if not WHERE_FIELD.match(name):
                problems.append("%s: %s.where field %r is not a plain name" % (where, key, name))
            regexes.append(("where.%s" % name, regex))
    if key == "http_status":
        if not 100 <= body["status"] <= 599:
            problems.append("%s: http_status.status must be an HTTP status" % where)
        if "body_regex" in body:
            regexes.append(("body_regex", body["body_regex"]))
    if key == "step_reply_matches":
        regexes.append(("regex", body["regex"]))
    for name, regex in regexes:
        if not isinstance(regex, str):
            problems.append("%s: %s.%s must be a string" % (where, key, name))
            continue
        try:
            re.compile(regex, re.IGNORECASE)
        except re.error as exc:
            problems.append("%s: %s.%s does not compile: %s" % (where, key, name, exc))
    return problems


# ----------------------------------------------------------------------------- evaluation

_CORE_CACHE: Dict[str, Dict[str, Any]] = {}


def core_task(task_id: str, path: str = CORE_TASKS) -> Dict[str, Any]:
    """The core task ``task_id`` from ``eval/core-tasks.md``."""
    if path not in _CORE_CACHE:
        _CORE_CACHE[path] = {t["id"]: t for t in load_tasks(path)}
    try:
        return _CORE_CACHE[path][task_id]
    except KeyError:
        raise RuleError("core_pass names an unknown core task %r" % task_id) from None


def evaluate(rule: Any, evidence: Evidence) -> Tuple[bool, List[str]]:
    """Apply ``rule`` to ``evidence``. Returns ``(passed, failed_leaves)``. A malformed rule
    raises ``RuleError``: that is a mistake in the task, not a failed run."""
    problems = validate_rule(rule)
    if problems:
        raise RuleError("; ".join(problems))
    failed: List[str] = []
    _evaluate(rule, evidence, failed)
    return (not failed, failed)


def _evaluate(rule: Dict[str, Any], evidence: Evidence, failed: List[str]) -> None:
    key = next(iter(rule))
    body = rule[key]
    if key == "all":
        for sub in body:
            _evaluate(sub, evidence, failed)
        return
    if key in pass_rules.RULE_SPECS:
        ok, leaf_failed = pass_rules.evaluate(rule, evidence.ctx)
        failed.extend(leaf_failed)
        return
    if not _holds(key, body, evidence):
        failed.append("%s %s" % (key, json.dumps(body, sort_keys=True)))


def _step(evidence: Evidence, index: int) -> Optional[Dict[str, Any]]:
    if 0 <= index < len(evidence.steps):
        return evidence.steps[index]
    return None


def _events_of(evidence: Evidence, step: Optional[int]) -> List[Dict[str, Any]]:
    if step is None:
        out: List[Dict[str, Any]] = []
        for record in evidence.steps:
            out.extend(e for e in record.get("events") or [] if isinstance(e, dict))
        return out
    record = _step(evidence, step)
    if record is None:
        return []
    return [e for e in record.get("events") or [] if isinstance(e, dict)]


def _field_text(value: Any) -> str:
    return value if isinstance(value, str) else json.dumps(value)


def event_matches(event: Dict[str, Any], kind: str, where: Dict[str, str]) -> bool:
    if event.get("type") != kind:
        return False
    for name, regex in where.items():
        if name not in event:
            return False
        if re.search(regex, _field_text(event[name]), re.IGNORECASE) is None:
            return False
    return True


def _holds(key: str, body: Dict[str, Any], evidence: Evidence) -> bool:
    if key == "core_pass":
        rule = core_task(body["task"])["pass"]
        ok, _ = pass_rules.evaluate(rule, evidence.ctx)
        return ok
    if key in ("event_seen", "event_absent"):
        where = body.get("where") or {}
        seen = any(event_matches(e, body["type"], where) for e in _events_of(evidence, body.get("step")))
        return seen if key == "event_seen" else not seen
    record = _step(evidence, body["step"])
    if record is None:
        return False
    if key == "http_status":
        if record.get("status") != body["status"]:
            return False
        regex = body.get("body_regex")
        return regex is None or re.search(regex, record.get("body") or "", re.IGNORECASE) is not None
    if key == "step_reply_matches":
        return re.search(body["regex"], record.get("reply") or "", re.IGNORECASE) is not None
    raise RuleError("unknown rule key %r" % key)


# ----------------------------------------------------------------------------- metrics

def expected_statuses(rule: Any) -> Dict[int, int]:
    """step -> status for every ``http_status`` leaf, so an expected 4xx isn't counted as an error."""
    out: Dict[int, int] = {}
    if isinstance(rule, dict) and len(rule) == 1:
        key, body = next(iter(rule.items()))
        if key == "all" and isinstance(body, list):
            for sub in body:
                out.update(expected_statuses(sub))
        elif key == "http_status" and isinstance(body, dict):
            out[body.get("step")] = body.get("status")
    return out


def metrics(rule: Any, steps: List[Dict[str, Any]]) -> Dict[str, Any]:
    """Time, errors and confusion points for one run. Measurements only."""
    expected = expected_statuses(rule)
    errors: List[str] = []
    confusion: List[Dict[str, Any]] = []
    time_ms = 0
    ttft_ms = None
    asks = 0
    for i, record in enumerate(steps):
        time_ms += int(record.get("ms") or 0)
        if record.get("error"):
            errors.append("step %d: %s" % (i, record["error"]))
        status = record.get("status")
        if isinstance(status, int) and status >= 400 and expected.get(i) != status:
            errors.append("step %d: HTTP %d" % (i, status))
        for event in record.get("events") or []:
            if not isinstance(event, dict):
                continue
            if event.get("type") == "error":
                errors.append("step %d: error event %s: %s" % (i, event.get("code"), event.get("message")))
            if event.get("type") == "approval":
                asks += 1
                if not (event.get("summary") or "").strip() or not (event.get("detail") or "").strip():
                    confusion.append({"step": i, "kind": "approval_unclear",
                                      "detail": "an approval for %s has no summary or no detail" % event.get("tool")})
        for decision in record.get("decisions") or []:
            if decision.get("planned") is False:
                confusion.append({"step": i, "kind": "unexpected_ask",
                                  "detail": "Heron asked about %s, which the script didn't expect" % decision.get("tool")})
        if record.get("kind") in ("say", "resume"):
            if ttft_ms is None and record.get("ttft_ms") is not None:
                ttft_ms = record["ttft_ms"]
            reply = record.get("reply") or ""
            if record.get("status") in (None, 200) and not reply.strip() and record.get("end") != "disconnected":
                confusion.append({"step": i, "kind": "no_reply", "detail": "the step ended with no reply text"})
            if CONFUSED_REPLY.search(reply):
                confusion.append({"step": i, "kind": "confused_reply", "detail": reply[:160]})
            if record.get("end") not in (None, "done", "disconnected", "run_end", "http"):
                confusion.append({"step": i, "kind": "cut_off", "detail": "the stream ended with %s" % record.get("end")})
        for finding in record.get("eve_findings") or []:
            text = finding.get("title") or finding.get("description") if isinstance(finding, dict) else finding
            confusion.append({"step": i, "kind": "eve", "detail": str(text)[:200]})
    return {"time_ms": time_ms, "ttft_ms": ttft_ms, "asks": asks, "errors": errors, "confusion_points": confusion}


def score(rule: Any, evidence: Evidence) -> Dict[str, Any]:
    """The verdict and the metrics for one run."""
    completed, failed = evaluate(rule, evidence)
    out = {"completed": completed, "failed_checks": failed}
    out.update(metrics(rule, evidence.steps))
    return out
