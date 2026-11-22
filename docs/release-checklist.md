# Release checklist: Heron v1 research preview

The owner runs these steps by hand, on their own Mac, before each research-preview
release. They are not build work units: each one needs the owner's login, judgement or
signature. Do them in order, and tick each box in the release pull request.

Release: `1.0.0-preview.1`. Tag: `v1.0.0-preview.1`.

Before you start, the second pull request from `feat/heron-v1` to `main` has passed CI
and review, and `bash scripts/verify-v1.sh` passed on its head commit without
`HERON_G9_PENDING`.

## 1. Drift run on the latest CLI releases

The supported ranges pin each Tier 1 CLI to one major version, but a minor release can
still change its flags or behaviour. Check them against today's releases.

- [ ] Update each installed candidate CLI to its latest release (for example
  `npm install -g @anthropic-ai/claude-code @openai/codex`, and OpenCode's own updater).
- [ ] Run the containment check against them:

  ```sh
  bash eval/containment/run.sh
  python3 -I eval/containment/check.py
  python3 -I eval/containment/check.py --list-tier1
  ```

  `check.py` must exit 0 (claude-cli and at least one other candidate passed), and
  `--list-tier1` must still print every CLI in the
  [Tier 1 table](cli-providers.md#tier-1-clis).
- [ ] For each id `--list-tier1` prints, run one quick smoke run on the version you just
  checked:

  ```sh
  bash eval/run-core-tasks.sh --provider <id> --runs 1 --out "${TMPDIR:-/tmp}/heron-smoke/<id>"
  ```

  This only shows early that the CLI still answers through Heron. It is not the task
  gate: the gate is step 5, with 3 runs per task on every Tier 1 CLI.
- [ ] If a CLI failed, or its latest version is outside its range, stop. Record the
  result, and open a `src/` fix (or narrow the [Tier 1 table](cli-providers.md#tier-1-clis))
  before releasing. Don't release with a Tier 1 CLI that fails containment.
- [ ] Commit any changed `eval/containment/*.json` records with the release.

## 2. Keychain check by hand (G8)

The upgrade test checks data, allow rules and the audit chain automatically. It never
touches Keychain secrets, so check them once, on a real Mac.

- [ ] On a Mac with v0.3.1 installed and at least one secret saved in its Settings (for
  example a model API key), note which secrets are set.
- [ ] Install the release candidate's DMG over it and open Heron.
- [ ] Confirm every secret is still there and works, without typing it again. In
  Keychain Access the entries are under the service name `Heron`.
- [ ] Note whether macOS asked to let the new app use those Keychain items, and put the
  answer in the release notes.

## 3. Terms decisions

- [ ] Read each Tier 1 vendor's current consumer terms.
- [ ] Fill in the "Terms decision" line for every Tier 1 provider in
  [cli-providers.md](cli-providers.md#terms-decisions). No Tier 1 line may still say
  "not yet recorded".

## 4. Accepted-risk signatures

- [ ] Confirm R-1 and C35-8 in [ACCEPTED_RISKS.md](ACCEPTED_RISKS.md) are signed (both were
  signed on 2026-11-18):

  ```sh
  ! grep -q "Awaiting the owner's signature" docs/ACCEPTED_RISKS.md
  test "$(grep -c '^## R-' docs/ACCEPTED_RISKS.md)" -le 3
  ```

- [ ] If an entry was added since the last release, sign it, and raise the `Version:` line
  at the top of the file by one, so every user sees the notice again.

## 5. The task gate on every Tier 1 CLI (Q5)

`scripts/verify-v1.sh` step 7 runs the full gate, one CLI at a time: for each id from
`python3 -I eval/containment/check.py --list-tier1`,

```sh
bash eval/run-core-tasks.sh --provider <id> --driver eve --runs 3 --out "${TMPDIR:-/tmp}/heron-verify-eval/<id>" --resume
python3 -I eval/check_gate.py "${TMPDIR:-/tmp}/heron-verify-eval/<id>/summary.json"
```

Each CLI must pass on its own: at least 26 of 30 runs, and no task below 2 of 3.

- [ ] Every Tier 1 CLI passed.
- [ ] Paste the table `check_gate.py` prints for each CLI into the release notes, under
  the CLI's id and version. The table includes the median time to first token.

Release-notes template:

```markdown
### Task gate

#### <provider id> <version>

<table from check_gate.py>

check_gate: G7 <passed|FAILED> (<passes> of <runs> runs)
```

## 6. Version bump

- [ ] In `Cargo.toml`, set `version = "1.0.0-preview.1"`, then run `cargo check` once
  (without `--locked`) so `Cargo.lock` records the new version.
- [ ] In `CHANGELOG.md`, rename `## Unreleased` to `## 1.0.0-preview.1 — <date>`, and
  list the changes: the security fixes, Tier 1 CLIs, the SwiftUI app, the risk notice,
  and "Reading a file outside the working folder now asks you first."
- [ ] Commit both on a branch, and merge it through the normal pull request.

## 7. Tag

- [ ] On `main`, at the merged version-bump commit:

  ```sh
  git tag v1.0.0-preview.1
  git push origin v1.0.0-preview.1
  ```

  The tag starts `.github/workflows/release.yml`, which builds, signs and notarizes the
  DMG and publishes a prerelease titled "Research preview".
- [ ] When the workflow finishes, download `Heron-macos-universal.dmg` on a clean Mac and
  follow [Getting started on a Mac](getting-started-mac.md) once, end to end.
- [ ] Add the task-gate tables (step 5) and the Keychain note (step 2) to the release
  notes.
