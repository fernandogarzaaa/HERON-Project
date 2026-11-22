"""Pass rules for the Heron core tasks (U0, item 4).

A rule is a JSON object with exactly one key. ``validate_rule`` checks the grammar and
``evaluate`` applies a rule to the outcome of one run. Standard library only.
"""

import json
import os
import re
import sqlite3
from dataclasses import dataclass, field
from typing import Any, Dict, List, Tuple

# key -> {field: (type, required)}.
RULE_SPECS: Dict[str, Dict[str, Tuple[type, bool]]] = {
    "file_contains": {"path": (str, True), "text": (str, True)},
    "file_present": {"path": (str, True)},
    "file_absent": {"path": (str, True)},
    "reply_matches": {"regex": (str, True)},
    "sql_at_least": {"query": (str, True), "n": (int, True)},
    "audit_has": {"decision": (str, True), "tool": (str, False)},
    "no_request": {"path_prefix": (str, True)},
    "transcript_lacks": {"text": (str, True)},
    "no_executed": {"tools": (list, True)},
    "event_matches": {"type": (str, True), "field": (str, True), "regex": (str, True)},
}
RULE_KEYS = tuple(RULE_SPECS) + ("all",)

COUNT_PREFIX = "SELECT COUNT(*)"
# The audit decisions of a call that ran (the others are denials and errors).
EXECUTED_DECISIONS = ("allowed", "approved", "approved-always", "approved-for-conversation", "approved-later")


class RuleError(ValueError):
    """The rule itself is malformed, or cannot be evaluated safely."""


@dataclass
class Context:
    """What one finished run left behind."""

    workdir: str
    reply: str = ""
    db_path: str = ""
    session_id: str = ""
    web_requests: List[str] = field(default_factory=list)
    events: List[Any] = field(default_factory=list)


def validate_rule(rule: Any, where: str = "rule") -> List[str]:
    """Return the problems with ``rule``'s grammar; an empty list means it is well formed."""
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
            problems.extend(validate_rule(sub, "%s.all[%d]" % (where, i)))
        return problems
    if not isinstance(body, dict):
        return ["%s: %s needs an object of fields" % (where, key)]
    problems = []
    spec = RULE_SPECS[key]
    for name in body:
        if name not in spec:
            problems.append("%s: %s has an unknown field %r" % (where, key, name))
    for name, (kind, required) in spec.items():
        if name not in body:
            if required:
                problems.append("%s: %s needs the field %r" % (where, key, name))
            continue
        value = body[name]
        if kind is int and (isinstance(value, bool) or not isinstance(value, int)):
            problems.append("%s: %s.%s must be an integer" % (where, key, name))
        elif kind is str and not isinstance(value, str):
            problems.append("%s: %s.%s must be a string" % (where, key, name))
        elif kind is list and (not isinstance(value, list) or not value
                               or not all(isinstance(item, str) and item for item in value)):
            problems.append("%s: %s.%s must be a non-empty list of non-empty strings" % (where, key, name))
    if problems:
        return problems
    if key == "reply_matches":
        try:
            re.compile(body["regex"], re.IGNORECASE)
        except re.error as exc:
            problems.append("%s: reply_matches.regex does not compile: %s" % (where, exc))
    if key == "event_matches":
        try:
            re.compile(body["regex"], re.IGNORECASE)
        except re.error as exc:
            problems.append("%s: event_matches.regex does not compile: %s" % (where, exc))
    if key == "sql_at_least":
        if not body["query"].lstrip().upper().startswith(COUNT_PREFIX):
            problems.append("%s: sql_at_least.query must start with %s" % (where, COUNT_PREFIX))
        if body["n"] < 0:
            problems.append("%s: sql_at_least.n must not be negative" % where)
    if key in ("file_contains", "file_present", "file_absent"):
        if not _is_relative_inside(body["path"]):
            problems.append("%s: %s.path must stay inside the working folder" % (where, key))
    return problems


def _is_relative_inside(path: str) -> bool:
    if not path or os.path.isabs(path):
        return False
    parts = path.replace("\\", "/").split("/")
    return ".." not in parts


def _resolve(ctx: Context, path: str) -> str:
    if not _is_relative_inside(path):
        raise RuleError("path %r must be relative and stay inside the working folder" % path)
    return os.path.join(ctx.workdir, *path.replace("\\", "/").split("/"))


def _read_only_connection(db_path: str) -> sqlite3.Connection:
    if not db_path or not os.path.exists(db_path):
        raise RuleError("heron.db not found at %r" % db_path)
    uri = "file:%s?mode=ro" % db_path.replace("?", "%3f").replace("#", "%23")
    conn = sqlite3.connect(uri, uri=True, timeout=10)
    conn.execute("PRAGMA query_only = ON")
    return conn


def _scalar(ctx: Context, query: str, params: Tuple = ()) -> int:
    conn = _read_only_connection(ctx.db_path)
    try:
        row = conn.execute(query, params).fetchone()
    finally:
        conn.close()
    if row is None or not isinstance(row[0], int):
        raise RuleError("query did not return one integer: %r" % query)
    return row[0]


def evaluate(rule: Any, ctx: Context) -> Tuple[bool, List[str]]:
    """Apply ``rule`` to ``ctx``. Returns ``(passed, failed_rules)``.

    ``failed_rules`` names each leaf rule that did not hold. A malformed rule raises
    ``RuleError``: that is a mistake in the task, not a failed run.
    """
    problems = validate_rule(rule)
    if problems:
        raise RuleError("; ".join(problems))
    failed: List[str] = []
    _evaluate(rule, ctx, failed)
    return (not failed, failed)


def _evaluate(rule: Dict[str, Any], ctx: Context, failed: List[str]) -> None:
    key = next(iter(rule))
    body = rule[key]
    if key == "all":
        for sub in body:
            _evaluate(sub, ctx, failed)
        return
    if not _holds(key, body, ctx):
        failed.append("%s %s" % (key, _short(body)))


def _short(body: Dict[str, Any]) -> str:
    return ", ".join("%s=%s" % (k, v) for k, v in body.items())


def _holds(key: str, body: Dict[str, Any], ctx: Context) -> bool:
    if key == "file_present":
        return os.path.isfile(_resolve(ctx, body["path"]))
    if key == "file_absent":
        return not os.path.lexists(_resolve(ctx, body["path"]))
    if key == "file_contains":
        path = _resolve(ctx, body["path"])
        if not os.path.isfile(path):
            return False
        with open(path, "r", encoding="utf-8", errors="replace") as handle:
            return body["text"] in handle.read()
    if key == "reply_matches":
        return re.search(body["regex"], ctx.reply or "", re.IGNORECASE) is not None
    if key == "sql_at_least":
        return _scalar(ctx, body["query"]) >= body["n"]
    if key == "audit_has":
        sql = "SELECT COUNT(*) FROM audit WHERE session_id = ? AND decision = ?"
        params: Tuple = (ctx.session_id, body["decision"])
        if "tool" in body:
            sql += " AND tool = ?"
            params += (body["tool"],)
        return _scalar(ctx, sql, params) >= 1
    if key == "no_executed":
        tools = body["tools"]
        sql = "SELECT COUNT(*) FROM audit WHERE session_id = ? AND decision IN (%s) AND tool IN (%s)" % (
            ",".join("?" for _ in EXECUTED_DECISIONS), ",".join("?" for _ in tools))
        return _scalar(ctx, sql, (ctx.session_id,) + EXECUTED_DECISIONS + tuple(tools)) == 0
    if key == "event_matches":
        return any(_event_matches(event, body) for event in ctx.events)
    if key == "no_request":
        return not any(p.startswith(body["path_prefix"]) for p in ctx.web_requests)
    if key == "transcript_lacks":
        return not any(body["text"] in _flatten(event) for event in ctx.events)
    raise RuleError("unknown rule key %r" % key)


def _flatten(event: Any) -> str:
    """The text of one event, as one string to search."""
    if isinstance(event, str):
        return event
    return json.dumps(event, ensure_ascii=False)


def _event_matches(event: Any, body: Dict[str, Any]) -> bool:
    """True when ``event`` has the type and a field whose text matches the regex."""
    if not isinstance(event, dict) or event.get("type") != body["type"] or body["field"] not in event:
        return False
    value = event[body["field"]]
    text = value if isinstance(value, str) else json.dumps(value, ensure_ascii=False)
    return re.search(body["regex"], text, re.IGNORECASE) is not None
