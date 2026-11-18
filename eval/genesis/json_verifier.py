#!/usr/bin/env python3
"""A strict run-record verifier for ``genesis audit-evaluator --suite json`` whose verdict is
``pass_rules.evaluate`` (U0g, item 8c). Standard library only.

genesis hands a verifier ``task.json`` and a candidate completion and expects
``{"pass": <bool>, "score": <0 or 1>, "reward": <0 or 1>}`` on stdout.

Two task shapes reach this verifier.

* **A pass-rule task** (the dataset, ``cases.jsonl``). The task is one pass rule from
  ``eval/core-tasks.md`` plus the run context it is judged against: ``{"rule": ..., "run":
  {...}}``. The completion is a run record as ``summary.json`` stores it (``run``, ``pass``,
  ``failed_rules``, ``reply``, ``tokens``, ``ttft_ms``, ``asks``, ``error``). The verifier parses
  the completion strictly (no duplicate keys, no prose around the JSON, no field outside the
  run-record schema, no wrong type), then checks that the record agrees with the run context
  (the reply, token count and ask count the runner would have written), and only then asks
  ``pass_rules.evaluate`` whether the context satisfies the rule. Nothing the record claims about
  itself (its ``pass`` and ``failed_rules``) can make it pass: a record that claims a pass the
  evidence does not support is rejected, and a record that agrees with a passing context is
  accepted only because ``pass_rules.evaluate`` said so.
* **A reference task** (genesis's own ``json`` suite probes, a tool-call payload with a schema
  and a reference answer). The completion goes through the same strict parser, must satisfy the
  schema, and must equal the reference exactly. This shape exists so the suite's seven probes
  exercise the parser that run records use; it has nothing to do with pass rules.

Run as ``python3 -I eval/genesis/json_verifier.py <task.json> <completion file>...``. Exit 0 with
a verdict on stdout, 2 when the task file is malformed. Anything a completion can cause, however
hostile, is a rejection at exit 0, never a crash.
"""

import json
import math
import os
import sqlite3
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
EVAL = os.path.dirname(HERE)
sys.path.insert(0, EVAL)

import check_gate  # noqa: E402
import pass_rules  # noqa: E402
import run_core_tasks  # noqa: E402

RECORD_KEYS = ("run", "pass", "failed_rules", "reply", "tokens", "ttft_ms", "asks", "error")
TASK_KEYS = ("rule", "run")
ENVELOPE_KEYS = {"id", "source", "input", "reference", "context", "kind"}  # what genesis wraps a task in
RUN_KEYS = ("files", "session_id", "web_log", "audit", "sql", "events")
AUDIT_KEYS = ("tool", "decision", "session_id")
SCHEMA_KEYWORDS = ("type", "required", "additionalProperties", "properties", "minLength", "enum")
SCHEMA_TYPES = {"object": dict, "array": list, "string": str, "boolean": bool}
DEFAULT_SESSION = "s1"


class Reject(Exception):
    """The completion is refused; the message is the reason."""


# ----------------------------------------------------------------------------- strict parsing

def _no_duplicates(pairs):
    seen = {}
    for key, value in pairs:
        if key in seen:
            raise Reject("duplicate key %r" % key)
        seen[key] = value
    return seen


def _no_constant(name):
    raise Reject("the non-standard number %s is not JSON" % name)


def strict_loads(text):
    """Parse ``text`` as exactly one JSON document, or raise ``Reject``. Whitespace around the
    document is fine. Duplicate keys, ``NaN``, prose around the JSON, a BOM and a document too
    deep to parse are all refused."""
    if not text.strip():
        raise Reject("the completion is empty")
    try:
        return json.loads(text, object_pairs_hook=_no_duplicates, parse_constant=_no_constant)
    except Reject:
        raise
    except RecursionError:
        raise Reject("the document nests too deeply")
    except ValueError as exc:
        raise Reject("not a single clean JSON document: %s" % exc)


# ----------------------------------------------------------------------------- the run record

def check_record(value):
    """Raise ``Reject`` unless ``value`` is exactly a run record as ``summary.json`` stores it."""
    if not isinstance(value, dict):
        raise Reject("a run record must be a JSON object")
    extra = sorted(set(value) - set(RECORD_KEYS))
    if extra:
        raise Reject("the run record has fields outside the schema: %s" % ", ".join(map(repr, extra)))
    missing = [key for key in RECORD_KEYS if key not in value]
    if missing:
        raise Reject("the run record is missing: %s" % ", ".join(missing))
    # check_gate owns the field types of a run record; reuse them so the two cannot drift apart.
    problems = check_gate._run_problems("record", value)
    ttft = value["ttft_ms"]
    if isinstance(ttft, float) and not math.isfinite(ttft):
        problems.append("ttft_ms must be finite")
    if problems:
        raise Reject("the run record has wrong types: " + "; ".join(problems))


def derive(events):
    """The reply and token count a runner would have recorded from ``events``, as
    ``eve_adapter.turn`` computes them: the reply is the ``done`` text, or the ``text`` events
    joined when the stream never reached ``done``; tokens is the last ``usage`` event's total."""
    texts, done_text, last_usage = [], None, None
    for event in events:
        kind = event.get("type")
        if kind == "text":
            texts.append(event.get("text", ""))
        elif kind == "usage":
            last_usage = event
        elif kind == "done":
            done_text = event.get("text", "")
            break
    tokens = 0
    if last_usage is not None:
        tokens = int(last_usage.get("prompt", 0) or 0) + int(last_usage.get("completion", 0) or 0)
    reply = done_text if done_text is not None else "".join(texts)
    return reply, tokens


# ----------------------------------------------------------------------------- the task

def _expect(condition, message):
    if not condition:
        raise ValueError(message)


def _safe_relative(path):
    if not isinstance(path, str) or not path or os.path.isabs(path):
        return False
    return ".." not in path.replace("\\", "/").split("/")


def check_run(run):
    """Raise ``ValueError`` unless ``run`` is a well-formed run context."""
    _expect(isinstance(run, dict), "the task's run must be an object")
    unknown = sorted(set(run) - set(RUN_KEYS))
    _expect(not unknown, "the task's run has unknown keys: %s" % ", ".join(unknown))
    files = run.get("files", {})
    _expect(isinstance(files, dict) and all(isinstance(v, str) for v in files.values()),
            "run.files must map paths to text")
    _expect(all(_safe_relative(p) for p in files), "run.files paths must stay inside the working folder")
    _expect(isinstance(run.get("session_id", DEFAULT_SESSION), str), "run.session_id must be a string")
    log = run.get("web_log", [])
    _expect(isinstance(log, list) and all(isinstance(p, str) for p in log), "run.web_log must be a list of paths")
    sql = run.get("sql", [])
    _expect(isinstance(sql, list) and all(isinstance(s, str) for s in sql), "run.sql must be a list of statements")
    audit = run.get("audit", [])
    _expect(isinstance(audit, list), "run.audit must be a list of rows")
    for row in audit:
        _expect(isinstance(row, dict) and set(row) <= set(AUDIT_KEYS) and isinstance(row.get("decision"), str),
                "each audit row needs a decision and may have a tool and a session_id")
        _expect(all(isinstance(row.get(k, ""), str) for k in ("tool", "session_id")), "audit fields must be strings")
    events = run.get("events", [])
    _expect(isinstance(events, list) and all(isinstance(e, dict) for e in events), "run.events must be a list of objects")
    for event in events:
        _expect(isinstance(event.get("text", ""), str), "an event's text must be a string")


def materialize(run, root):
    """Write the run context to disk under ``root``. Returns a ``pass_rules.Context`` with the
    working folder, a ``heron.db`` holding the audit rows and the task's own SQL, the web log and
    the events, and the reply and token count derived from the events."""
    session = run.get("session_id", DEFAULT_SESSION)
    workdir = os.path.join(root, "work")
    os.makedirs(workdir)
    for path, text in run.get("files", {}).items():
        target = os.path.join(workdir, *path.replace("\\", "/").split("/"))
        os.makedirs(os.path.dirname(target), exist_ok=True)
        with open(target, "w", encoding="utf-8") as handle:
            handle.write(text)
    db_path = os.path.join(root, "heron.db")
    conn = sqlite3.connect(db_path)
    try:
        conn.execute("CREATE TABLE audit(id INTEGER PRIMARY KEY, session_id TEXT, tool TEXT, decision TEXT)")
        for row in run.get("audit", []):
            conn.execute("INSERT INTO audit(session_id, tool, decision) VALUES (?, ?, ?)",
                         (row.get("session_id", session), row.get("tool"), row["decision"]))
        for statement in run.get("sql", []):
            conn.execute(statement)
        conn.commit()
    finally:
        conn.close()
    events = run.get("events", [])
    reply, tokens = derive(events)
    ctx = pass_rules.Context(workdir=workdir, reply=reply, db_path=db_path, session_id=session,
                             web_requests=list(run.get("web_log", [])), events=events)
    return ctx, tokens


# ----------------------------------------------------------------------------- verdicts

def rule_verdict(task, completion):
    """The verdict for a pass-rule task. Returns ``(accepted, why)``."""
    rule, run = task["rule"], task["run"]
    problems = pass_rules.validate_rule(rule)
    _expect(not problems, "the task's rule is malformed: " + "; ".join(problems))
    check_run(run)
    try:
        record = strict_loads(completion)
        check_record(record)
        if record["error"] is not None:
            raise Reject("the record reports an error: %s" % record["error"])
        with tempfile.TemporaryDirectory(prefix="heron-json-audit-") as root:
            ctx, tokens = materialize(run, root)
            if record["reply"] != ctx.reply[:run_core_tasks.REPLY_CHARS]:
                raise Reject("the record's reply is not the reply in the run's events")
            if record["tokens"] != tokens:
                raise Reject("the record's tokens (%d) are not the events' total (%d)" % (record["tokens"], tokens))
            asks = run_core_tasks.count_asks(ctx.db_path, ctx.session_id)
            if record["asks"] != asks:
                raise Reject("the record's asks (%d) are not the audit's count (%d)" % (record["asks"], asks))
            passed, failed = pass_rules.evaluate(rule, ctx)
        if not passed:
            raise Reject("the evidence fails the rule: " + "; ".join(failed))
        if record["pass"] is not True or record["failed_rules"] != []:
            raise Reject("the evidence passes the rule but the record says otherwise")
    except Reject as exc:
        return False, "rejected: %s" % exc
    except (pass_rules.RuleError, sqlite3.Error, OSError) as exc:
        return False, "rejected: the rule could not be evaluated: %s" % exc
    return True, "accepted"


def _canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=True)


def _schema_problems(value, schema, where="$"):
    _expect(isinstance(schema, dict), "a schema must be an object")
    unsupported = sorted(set(schema) - set(SCHEMA_KEYWORDS))
    _expect(not unsupported, "the schema uses keywords this verifier does not check: %s" % ", ".join(unsupported))
    problems = []
    kind = schema.get("type")
    if kind is not None:
        _expect(kind in SCHEMA_TYPES, "unsupported schema type %r" % (kind,))
        if not isinstance(value, SCHEMA_TYPES[kind]):
            return ["%s must be of type %s" % (where, kind)]
    if "enum" in schema and not any(_canonical(value) == _canonical(option) for option in schema["enum"]):
        problems.append("%s is not one of the allowed values" % where)
    if "minLength" in schema and isinstance(value, str) and len(value) < schema["minLength"]:
        problems.append("%s is shorter than %d" % (where, schema["minLength"]))
    if isinstance(value, dict):
        for key in schema.get("required", []):
            if key not in value:
                problems.append("%s is missing %r" % (where, key))
        properties = schema.get("properties", {})
        if schema.get("additionalProperties") is False:
            for key in sorted(set(value) - set(properties)):
                problems.append("%s has the undeclared field %r" % (where, key))
        for key, sub in properties.items():
            if key in value:
                problems.extend(_schema_problems(value[key], sub, "%s.%s" % (where, key)))
    return problems


def reference_verdict(task, completion):
    """The verdict for a reference task: strict parse, schema, then exact equality."""
    try:
        reference = strict_loads(task["reference"])
    except Reject as exc:
        raise ValueError("the task's reference is not clean JSON: %s" % exc)
    try:
        value = strict_loads(completion)
        problems = _schema_problems(value, task["schema"])
        if problems:
            raise Reject("the payload breaks the schema: " + "; ".join(problems))
        if _canonical(value) != _canonical(reference):
            raise Reject("the payload is not the reference answer")
    except Reject as exc:
        return False, "rejected: %s" % exc
    return True, "accepted"


def verdict(task, completion):
    """Return ``(accepted, why)`` for ``completion`` against a normalized ``task``."""
    try:
        if "rule" in task:
            return rule_verdict(task, completion)
        return reference_verdict(task, completion)
    except RecursionError:
        return False, "rejected: the completion nests too deeply"


# ----------------------------------------------------------------------------- the command line

def normalize_task(raw):
    """The task in one of its two shapes. genesis hands over a pass-rule task three ways: at the
    top level, under ``context``, or (dataset-derived probes, which carry ``id``, ``input`` and
    ``reference``) as JSON text in ``input``. Its own suite probes are reference tasks: a
    ``reference`` and a ``schema`` under ``context``."""
    _expect(isinstance(raw, dict), "the task must be a JSON object")
    candidates = [raw, raw.get("context")]
    if isinstance(raw.get("input"), str):
        try:
            candidates.append(json.loads(raw["input"]))
        except ValueError:
            pass
    for candidate in candidates:
        if isinstance(candidate, dict) and "rule" in candidate:
            allowed = set(TASK_KEYS) | (ENVELOPE_KEYS if candidate is raw else set())
            extra = sorted(set(candidate) - allowed)
            _expect(not extra, "the task has unknown keys: %s" % ", ".join(extra))
            _expect("run" in candidate, "a pass-rule task needs a run context")
            return {"rule": candidate["rule"], "run": candidate["run"]}
    context = raw.get("context")
    if isinstance(context, dict) and "schema" in context:
        reference = context.get("reference", raw.get("reference"))
        _expect(isinstance(reference, str), "a reference task needs its reference as JSON text")
        return {"reference": reference, "schema": context["schema"]}
    raise ValueError("the task is neither a pass-rule task nor a reference task")


def load_task(path):
    with open(path, "r", encoding="utf-8") as handle:
        return normalize_task(json.load(handle))


def main(argv):
    if len(argv) < 2:
        sys.stderr.write("usage: json_verifier.py <task.json> <completion file>...\n")
        return 2
    # genesis names the candidate {completion_file} in its suite probes and {output_file} in its
    # dataset-derived probes, and leaves the other placeholder as literal text: take the one that exists.
    candidates = [p for p in argv[1:] if os.path.isfile(p)]
    if not candidates:
        sys.stderr.write("json_verifier: none of %r is a file\n" % (argv[1:],))
        return 2
    try:
        task = load_task(argv[0])
        with open(candidates[0], "r", encoding="utf-8", errors="replace") as handle:
            completion = handle.read()
        accepted, why = verdict(task, completion)
    except (OSError, ValueError, AttributeError, KeyError, TypeError) as exc:
        sys.stderr.write("json_verifier: malformed task: %s\n" % exc)
        return 2
    sys.stderr.write("json_verifier: %s\n" % why)
    print(json.dumps({"pass": accepted, "score": 1.0 if accepted else 0.0, "reward": 1 if accepted else 0}))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
