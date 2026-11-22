"""Reader for ``eval/core-tasks.md``: one ``## <id>: <title>`` section per task, each holding
exactly one fenced ``json`` block. Standard library only."""

import json
import os
import re
from typing import Any, Dict, List

TASK_KEYS = ("id", "title", "counts_toward_gate", "prompt", "fixture", "seed_sql", "approve", "pass")
# Optional keys of a feature task (eval/features/<slug>.md); see eval/features/README.md.
FEATURE_KEYS = ("features", "config", "context", "bins", "injection")
HEADING = re.compile(r"^## (\S+): (.+?)\s*$")
FENCE_OPEN = re.compile(r"^```json\s*$")
FENCE_CLOSE = re.compile(r"^```\s*$")


class TasksError(ValueError):
    """The tasks file is not in the documented format."""


def parse_tasks(text: str) -> List[Dict[str, Any]]:
    """Return the task blocks of ``text``, in file order."""
    sections: List[Dict[str, Any]] = []
    current = None
    in_block = False
    for number, line in enumerate(text.splitlines(), 1):
        if not in_block:
            heading = HEADING.match(line)
            if heading:
                current = {"heading_id": heading.group(1), "title": heading.group(2),
                           "line": number, "blocks": []}
                sections.append(current)
                continue
            if current is not None and FENCE_OPEN.match(line):
                in_block = True
                current["blocks"].append([])
            continue
        if FENCE_CLOSE.match(line):
            in_block = False
        else:
            current["blocks"][-1].append(line)
    if in_block:
        raise TasksError("a json block is never closed")
    tasks = []
    for section in sections:
        where = "section '%s' (line %d)" % (section["heading_id"], section["line"])
        if len(section["blocks"]) != 1:
            raise TasksError("%s needs exactly one json block, found %d" % (where, len(section["blocks"])))
        try:
            task = json.loads("\n".join(section["blocks"][0]))
        except ValueError as exc:
            raise TasksError("%s: the json block does not parse: %s" % (where, exc))
        if not isinstance(task, dict):
            raise TasksError("%s: the json block must be an object" % where)
        task["_heading"] = "%s: %s" % (section["heading_id"], section["title"])
        tasks.append(task)
    return tasks


def load_tasks(path: str) -> List[Dict[str, Any]]:
    with open(path, "r", encoding="utf-8") as handle:
        return parse_tasks(handle.read())


def fixtures_root_for(tasks_path: str) -> str:
    """The folder a tasks file's ``fixture`` names live in: ``fixtures/`` next to the file, or, for
    a file in ``eval/features/``, the ``eval/fixtures/`` one level up."""
    here = os.path.dirname(os.path.abspath(tasks_path))
    for candidate in (os.path.join(here, "fixtures"), os.path.join(os.path.dirname(here), "fixtures")):
        if os.path.isdir(candidate):
            return candidate
    return os.path.join(here, "fixtures")
