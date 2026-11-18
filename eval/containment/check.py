"""Judge the recorded containment results (work unit U2c, design item 31).

    python3 -I eval/containment/check.py --recorded     U2c's check: every candidate has a result
                                                        file with a valid `status`
    python3 -I eval/containment/check.py                G4, U2w's check and `verify` step 8:
                                                        claude-cli passed and at least one other
                                                        candidate passed
    python3 -I eval/containment/check.py --list-tier1   one id per line for each candidate that
                                                        meets U2w's Tier 1 rule (item 39)

`--dir <folder>` reads the results from somewhere else (the tests use it). Exit 0 on success, 1 when
the check fails, 2 on bad arguments. Standard library only.
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from results import CANDIDATES, HERE, VALID_STATUS  # noqa: E402

IDS = [id_ for id_, _ in CANDIDATES]


def load(directory):
    """(records, problems): the result of each candidate, or None when it has no valid record."""
    records, problems = {}, []
    for id_ in IDS:
        path = os.path.join(directory, id_ + ".json")
        try:
            with open(path, encoding="utf-8") as handle:
                record = json.load(handle)
        except OSError:
            problems.append("%s: no result file (%s)" % (id_, path))
            records[id_] = None
            continue
        except ValueError as error:
            problems.append("%s: %s is not valid JSON: %s" % (id_, path, error))
            records[id_] = None
            continue
        if not isinstance(record, dict):
            problems.append("%s: %s is not a JSON object" % (id_, path))
            records[id_] = None
        elif record.get("id") != id_:
            problems.append("%s: %s has id %r" % (id_, path, record.get("id")))
            records[id_] = None
        elif record.get("status") not in VALID_STATUS:
            problems.append("%s: status %r is not one of %s" % (id_, record.get("status"), ", ".join(VALID_STATUS)))
            records[id_] = None
        else:
            records[id_] = record
    return records, problems


def status_of(records, id_):
    record = records.get(id_)
    return record["status"] if record else None


def tier1(records):
    """The ids that meet U2w's Tier 1 rule, in candidate order.

    A candidate is Tier 1 when it passed and its config folders are confirmed. gemini-cli also needs
    antigravity-cli to have failed or to be missing (G4); an `unavailable` gemini-cli never passed.
    """
    chosen = []
    for id_ in IDS:
        record = records.get(id_)
        if not record or record["status"] != "passed":
            continue
        folders = record.get("config_dirs")
        if record.get("config_dirs_confirmed") is not True or not isinstance(folders, list) or not folders:
            continue
        if id_ == "gemini-cli" and status_of(records, "antigravity-cli") == "passed":
            continue
        chosen.append(id_)
    return chosen


def main(argv):
    args = argv[1:]
    directory = HERE
    if "--dir" in args:
        at = args.index("--dir")
        if at + 1 >= len(args):
            print("check: --dir needs a folder", file=sys.stderr)
            return 2
        directory = args[at + 1]
        del args[at:at + 2]
    if len(args) > 1 or (args and args[0] not in ("--recorded", "--list-tier1")):
        print(__doc__, file=sys.stderr)
        return 2
    mode = args[0] if args else "g4"

    records, problems = load(directory)
    if mode == "--recorded":
        for problem in problems:
            print("check: " + problem, file=sys.stderr)
        return 1 if problems else 0

    if mode == "--list-tier1":
        for id_ in tier1(records):
            print(id_)
        return 0

    for id_ in IDS:
        print("%-16s %s" % (id_, status_of(records, id_) or "no valid result"))
    others = [id_ for id_ in IDS if id_ != "claude-cli" and status_of(records, id_) == "passed"]
    if status_of(records, "claude-cli") != "passed":
        print("check: G4 fails: claude-cli did not pass its containment check", file=sys.stderr)
        return 1
    if not others:
        print("check: G4 fails: no other candidate passed its containment check", file=sys.stderr)
        return 1
    print("G4 holds: claude-cli and %s passed" % ", ".join(others))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
