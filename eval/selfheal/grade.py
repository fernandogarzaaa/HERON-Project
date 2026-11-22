"""The replay suite's grader (item 20): did an agent's repair of one replay case earn a point?

``grade`` is pure. ``run_replay.py`` collects the outcome of one case and calls it; ``genesis.yaml``
audits it through ``replay_verifier.py``. Standard library only.
"""

OUTCOME_KEYS = ("patch_empty", "hidden_test_deleted", "hidden_failed_on_base", "hidden_passed_on_fix", "suite_passed")

# What each outcome field must be for a point. An empty patch passes every test that already passed,
# and a deleted hidden test can't fail, so both are refused by name.
PASSING = {
    "patch_empty": False,
    "hidden_test_deleted": False,
    "hidden_failed_on_base": True,
    "hidden_passed_on_fix": True,
    "suite_passed": True,
}


def grade(outcome):
    """True only when the patch is not empty, did not delete the hidden test, the hidden test failed
    on the base and passes with the patch, and the whole suite passes. Anything missing or not a real
    boolean is False: a truthy string from a half-written record earns nothing."""
    if not isinstance(outcome, dict):
        return False
    return all(outcome.get(key) is want for key, want in PASSING.items())
