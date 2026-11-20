import json
import os
import tempfile
import unittest

import g7

EVAL = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SAMPLES = os.path.join(EVAL, "fixtures", "g7_limit_samples.json")


def samples():
    with open(SAMPLES, "r", encoding="utf-8") as handle:
        return json.load(handle)["samples"]


class LimitClassifier(unittest.TestCase):
    def test_every_sample_gets_its_recorded_class(self):
        rows = samples()
        self.assertEqual(len(rows), 19)
        self.assertTrue(any(r["limit"] for r in rows) and any(not r["limit"] for r in rows))
        for row in rows:
            with self.subTest(text=row["text"]):
                self.assertIs(g7.is_limit(row["text"]), row["limit"])

    def test_empty_and_missing_text_is_not_a_limit(self):
        self.assertFalse(g7.is_limit(""))
        self.assertFalse(g7.is_limit(None))


class LimitStopListing(unittest.TestCase):
    PART = {"tasks": [{"id": "t01", "runs": [
        {"pass": True, "error": None},
        {"pass": False, "error": "OpenCode refused the request: provider.quota: quota exceeded.\nBearer abcdefabcdefabcdefabcdefabcdefabcdef"},
        {"pass": False, "error": "Disk quota exceeded (os error 69)"},
    ]}]}

    def test_every_limit_run_is_listed_with_its_redacted_raw_line(self):
        lines = g7.limit_runs(self.PART, "opencode-cli")
        self.assertEqual(len(lines), 1, lines)
        self.assertIn("limit stop t01 run2", lines[0])
        self.assertIn("quota exceeded", lines[0])
        self.assertNotIn("abcdefabcdef", lines[0])

    def test_scan_stops_the_cli_and_report_counts_the_stops(self):
        with tempfile.TemporaryDirectory() as out:
            os.makedirs(os.path.join(out, "parts"))
            with open(os.path.join(out, "parts", "t01.json"), "w", encoding="utf-8") as handle:
                json.dump(self.PART, handle)
            with self.assertRaises(SystemExit) as stop:
                g7.main(["scan", "opencode-cli", out, "t01"])
            self.assertEqual(stop.exception.code, 10)


if __name__ == "__main__":
    unittest.main()
