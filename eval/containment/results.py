"""Helpers for eval/containment/run.sh and check.py (work unit U2c, design item 31).

Standard library only; run as `python3 -I eval/containment/results.py <command> ...`.

Commands (all used by run.sh):

    find <binary>                 print where the CLI binary is installed; exit 1 if it isn't
    simple <id> not-installed     write {"id", "status": "not-installed"} to <out dir>/<id>.json
    unavailable <id>              if <out dir>/<id>.dirs.json has `unavailable_reason`, write the
                                  "unavailable" result and exit 0; otherwise exit 1 and write nothing
    result <id>                   turn the raw canary report in <out dir>/<id>.json (written by
                                  `canary_real_cli`) into the result record, in place

`<out dir>` is `CONTAINMENT_OUT_DIR`, else this folder. Nothing here invents a measurement: a value
the run did not observe is the string "unknown".
"""
import datetime
import json
import os
import re
import sys

# (id, binary) in the order the design doc lists them (item 31). The binary names are the registry's
# (`providers()` in src/cli_llm.rs).
CANDIDATES = [
    ("claude-cli", "claude"),
    ("opencode-cli", "opencode"),
    ("cline-cli", "cline"),
    ("codex-cli", "codex"),
    ("antigravity-cli", "agy"),
    ("gemini-cli", "gemini"),
]
VALID_STATUS = ("passed", "failed", "not-installed", "unavailable")

HERE = os.path.dirname(os.path.abspath(__file__))

# What src/cli_llm.rs `resolve_binary` looks at after $PATH, in the same order.
HOME_BIN_DIRS = [".local/bin", ".claude/local", "bin", ".npm-global/bin", ".bun/bin", ".volta/bin"]
SYSTEM_BIN_DIRS = ["/opt/homebrew/bin", "/usr/local/bin"]
# Not in the registry's list. opencode's own installer puts its binary here and adds the folder to
# $PATH from the shell profile, which a non-interactive run may not load. run.sh appends the folder
# it finds to $PATH for the canary, so Heron's resolver finds the same binary.
EXTRA_HOME_BIN_DIRS = [".opencode/bin"]


def out_dir():
    return os.environ.get("CONTAINMENT_OUT_DIR") or HERE


def _is_executable(path):
    return os.path.isfile(path) and os.access(path, os.X_OK)


def _version_key(text):
    match = re.search(r"(\d+)\.(\d+)\.(\d+)", text)
    return tuple(int(part) for part in match.groups()) if match else None


def _version_manager_dirs(home):
    dirs = [
        os.path.join(home, ".local/share/mise/shims"),
        os.path.join(home, ".local/share/fnm/aliases/default/bin"),
        os.path.join(home, "Library/Application Support/fnm/aliases/default/bin"),
    ]
    nvm = os.path.join(home, ".nvm")
    try:
        with open(os.path.join(nvm, "alias/default"), encoding="utf-8") as handle:
            alias = handle.read().strip()
    except OSError:
        alias = ""
    if alias:
        named = alias if alias.startswith("v") else "v" + alias
        dirs.append(os.path.join(nvm, "versions/node", named, "bin"))
    versions_root = os.path.join(nvm, "versions/node")
    try:
        names = os.listdir(versions_root)
    except OSError:
        names = []
    versions = [(_version_key(name), name) for name in names if _version_key(name)]
    versions.sort(reverse=True)
    dirs.extend(os.path.join(versions_root, name, "bin") for _, name in versions)
    return dirs


def find_binary(binary):
    """The first executable `binary`, searched the way Heron's `resolve_binary` does."""
    for entry in os.environ.get("PATH", "").split(os.pathsep):
        if entry:
            candidate = os.path.abspath(os.path.join(entry, binary))
            if _is_executable(candidate):
                return candidate
    home = os.path.expanduser("~")
    system = os.environ.get("CONTAINMENT_SYSTEM_DIRS")
    system_dirs = system.split(os.pathsep) if system is not None else SYSTEM_BIN_DIRS
    searched = [os.path.join(home, rel) for rel in HOME_BIN_DIRS]
    searched += [d for d in system_dirs if d]
    searched += _version_manager_dirs(home)
    searched += [os.path.join(home, rel) for rel in EXTRA_HOME_BIN_DIRS]
    for directory in searched:
        candidate = os.path.join(directory, binary)
        if _is_executable(candidate):
            return candidate
    return None


def read_dirs(directory, id_):
    """The CLI's config folders, from `<id>.dirs.json` (design item 31, "Reading <id>.dirs.json").

    Returns a dict with `config_dirs` (a list of `~/...` paths, or "unconfirmed"),
    `config_dirs_confirmed`, `credential_store` and `login_completed`.
    """
    path = os.path.join(directory, id_ + ".dirs.json")
    unconfirmed = {"config_dirs": "unconfirmed", "config_dirs_confirmed": False,
                   "credential_store": "unknown", "login_completed": None}
    try:
        with open(path, encoding="utf-8") as handle:
            record = json.load(handle)
    except (OSError, ValueError):
        return unconfirmed
    if not isinstance(record, dict) or not isinstance(record.get("paths"), list):
        return unconfirmed
    # Written before the Keychain change: no credential_store, no login_completed.
    store = record.get("credential_store", "home-folder")
    completed = record.get("login_completed", True)
    paths = [p for p in record["paths"] if isinstance(p, str)]
    if store == "macos-keychain":
        # The credential lives in the Keychain, which is Refused, so the folders count as confirmed.
        known = record.get("known_paths") if isinstance(record.get("known_paths"), list) else []
        folders = list(dict.fromkeys(paths + [p for p in known if isinstance(p, str)]))
        confirmed = bool(folders)
    else:
        folders = paths
        confirmed = completed is True and bool(folders)
    return {"config_dirs": folders, "config_dirs_confirmed": confirmed,
            "credential_store": store, "login_completed": completed}


def _now():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _write(directory, id_, record):
    with open(os.path.join(directory, id_ + ".json"), "w", encoding="utf-8") as handle:
        json.dump(record, handle, indent=2)
        handle.write("\n")


_LOGIN_TEXT = re.compile(r"log(ged)?[ -]?(in|out)|not authenticated|unauthori[sz]ed|keychain|credential", re.I)
_SANDBOX_TEXT = re.compile(r"sandbox|seatbelt|operation not permitted", re.I)


def build_result(id_, raw, dirs, ran_at):
    """The result record for one installed candidate, from the raw canary report."""
    error = raw.get("error")
    sandbox = raw.get("sandbox")
    seatbelt = "seatbelt" in str(sandbox).lower()
    containment = {
        "tools_off_canary": raw.get("tools_off"),
        "child_env_is_exactly_the_allowlist": raw.get("env_ok"),
        "write_outside_run_folder_fails": raw.get("write_outside_failed"),
    }
    # `passed` is the canary's verdict; it also needs the turn to run and the sandbox to be Seatbelt.
    # Here it must agree with all three tests, so a hand-edited or partial report can't pass.
    passed = raw.get("passed") is True and all(value is True for value in containment.values())

    # The three observations the design lists as UNVERIFIED. The canary only sees one turn, so each
    # is "ok" or "failed" only when that turn shows it, and "unknown" otherwise.
    if dirs["credential_store"] == "macos-keychain":
        if error is None and seatbelt:
            keychain_login = "ok"
        elif error is not None and _LOGIN_TEXT.search(str(error)):
            keychain_login = "failed"
        else:
            keychain_login = "unknown"
    elif dirs["credential_store"] == "home-folder":
        keychain_login = "not-applicable"
    else:
        keychain_login = "unknown"
    if id_ == "codex-cli":
        if error is None and seatbelt:
            codex_nested = "ok"
        elif error is not None and _SANDBOX_TEXT.search(str(error)):
            codex_nested = "failed"
        else:
            codex_nested = "unknown"
    else:
        codex_nested = "not-applicable"

    record = {
        "id": id_,
        "status": "passed" if passed else "failed",
        "version": raw.get("version_parsed"),
        "version_output": raw.get("version"),
        "containment": containment,
        "sandbox": sandbox,
        "tool_calls": raw.get("tool_calls"),
        "error": error,
        "config_dirs": dirs["config_dirs"],
        "config_dirs_confirmed": dirs["config_dirs_confirmed"],
        "credential_store": dirs["credential_store"],
        "login_completed": dirs["login_completed"],
        "keychain_login": keychain_login,
        "oauth_refresh": "unknown",
        "codex_nested_sandbox": codex_nested,
        "tcc_prompts": "unknown",
        "ran_at": ran_at,
        "canary": raw,
    }
    return record


def main(argv):
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    command, rest = argv[1], argv[2:]
    directory = out_dir()
    if command == "find" and len(rest) == 1:
        found = find_binary(rest[0])
        if found is None:
            return 1
        print(found)
        return 0
    if command == "simple" and len(rest) == 2 and rest[1] == "not-installed":
        _write(directory, rest[0], {"id": rest[0], "status": "not-installed"})
        return 0
    if command == "unavailable" and len(rest) == 1:
        try:
            with open(os.path.join(directory, rest[0] + ".dirs.json"), encoding="utf-8") as handle:
                reason = json.load(handle).get("unavailable_reason")
        except (OSError, ValueError, AttributeError):
            return 1
        if not reason:
            return 1
        _write(directory, rest[0], {"id": rest[0], "status": "unavailable", "reason": reason})
        return 0
    if command == "result" and len(rest) == 1:
        id_ = rest[0]
        path = os.path.join(directory, id_ + ".json")
        try:
            with open(path, encoding="utf-8") as handle:
                raw = json.load(handle)
        except (OSError, ValueError) as error:
            print("results: no readable canary report at %s: %s" % (path, error), file=sys.stderr)
            return 3
        if not isinstance(raw, dict):
            print("results: the canary report at %s is not a JSON object" % path, file=sys.stderr)
            return 3
        _write(directory, id_, build_result(id_, raw, read_dirs(directory, id_), _now()))
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
