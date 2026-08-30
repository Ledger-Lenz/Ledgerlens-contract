import unittest
import os
import subprocess
import json
import tempfile

class TestTraceabilityReport(unittest.TestCase):

    def setUp(self):
        self.script_path = os.path.join(os.path.dirname(__file__), "../tools/replay/traceability_report.py")
        self.fixture_path = os.path.join(os.path.dirname(__file__), "../tools/replay/fixtures/sample_matrix.json")
        self.temp_dir = tempfile.TemporaryDirectory()

    def tearDown(self):
        self.temp_dir.cleanup()

    def run_cli(self, args):
        cmd = [self.script_path] + args
        return subprocess.run(cmd, capture_output=True, text=True)

    def write_json(self, name, data):
        path = os.path.join(self.temp_dir.name, name)
        with open(path, "w") as f:
            json.dump(data, f)
        return path

    def test_golden_path_json(self):
        out_path = os.path.join(self.temp_dir.name, "report.json")
        res = self.run_cli(["--input", self.fixture_path, "--output", out_path, "--format", "json"])
        self.assertEqual(res.returncode, 0)
        with open(out_path, "r") as f:
            data = json.load(f)
        self.assertEqual(data["version"], "1.0.0")
        self.assertEqual(data["summary"]["status"], "PASSED")
        self.assertEqual(data["summary"]["coverage_percentage"], 100.0)

    def test_golden_path_human(self):
        out_path = os.path.join(self.temp_dir.name, "report.txt")
        res = self.run_cli(["--input", self.fixture_path, "--output", out_path, "--format", "human"])
        self.assertEqual(res.returncode, 0)
        with open(out_path, "r") as f:
            text = f.read()
        self.assertIn("=== LedgerLens Traceability Report ===", text)
        self.assertIn("Status: PASSED", text)

    def test_corrupt_json_input(self):
        bad_input = os.path.join(self.temp_dir.name, "bad.json")
        with open(bad_input, "w") as f:
            f.write("{ invalid json structure")
        out_path = os.path.join(self.temp_dir.name, "report.json")
        res = self.run_cli(["--input", bad_input, "--output", out_path])
        self.assertEqual(res.returncode, 2)

    def test_truncated_json_input(self):
        bad_input = os.path.join(self.temp_dir.name, "truncated.json")
        with open(bad_input, "w") as f:
            f.write('[{"issue_id": "AA-1", "invariants": [')
        out_path = os.path.join(self.temp_dir.name, "report.json")
        res = self.run_cli(["--input", bad_input, "--output", out_path])
        self.assertEqual(res.returncode, 2)

    def test_duplicate_issue_id_rejected(self):
        data = [
            {"issue_id": "AA-1", "title": "one", "invariants": [{"invariant_id": "I1", "description": "d", "test_ids": ["t1"]}]},
            {"issue_id": "AA-1", "title": "two", "invariants": [{"invariant_id": "I2", "description": "d", "test_ids": ["t2"]}]}
        ]
        bad_input = self.write_json("dup.json", data)
        out_path = os.path.join(self.temp_dir.name, "report.json")
        res = self.run_cli(["--input", bad_input, "--output", out_path])
        self.assertEqual(res.returncode, 2)

    def test_version_mismatch_rejected(self):
        bad_input = self.write_json("versioned_bad.json", {"schema_version": "9.9.9", "issues": []})
        out_path = os.path.join(self.temp_dir.name, "report.json")
        res = self.run_cli(["--input", bad_input, "--output", out_path])
        self.assertEqual(res.returncode, 5)

    def test_huge_input_rejected(self):
        data = [{"issue_id": f"HH-{i}", "title": "t", "invariants": []} for i in range(5001)]
        bad_input = self.write_json("huge.json", data)
        out_path = os.path.join(self.temp_dir.name, "report.json")
        res = self.run_cli(["--input", bad_input, "--output", out_path])
        self.assertEqual(res.returncode, 2)

    def test_dry_run_no_file_creation(self):
        out_path = os.path.join(self.temp_dir.name, "dry_run_output.json")
        res = self.run_cli(["--input", self.fixture_path, "--output", out_path, "--dry-run"])
        self.assertEqual(res.returncode, 0)
        self.assertFalse(os.path.exists(out_path))

    def test_strict_mode_failure(self):
        data = [
            {
                "issue_id": "LL-200",
                "title": "Uncovered Invariant Test",
                "invariants": [
                    {
                        "invariant_id": "INV-99",
                        "description": "No test associated",
                        "test_ids": []
                    }
                ]
            }
        ]
        uncovered_fixture = self.write_json("uncovered.json", data)
        out_path = os.path.join(self.temp_dir.name, "strict_report.json")
        res = self.run_cli(["--input", uncovered_fixture, "--output", out_path, "--strict"])
        self.assertEqual(res.returncode, 3)

    def test_checkpoint_resume_equivalence(self):
        data = [
            {"issue_id": "AA-1", "title": "Alpha", "invariants": [{"invariant_id": "INV-A", "description": "d1", "test_ids": ["t1"]}]},
            {"issue_id": "BB-2", "title": "Bravo", "invariants": [{"invariant_id": "INV-B", "description": "d2", "test_ids": ["t2"]}]},
            {"issue_id": "CC-3", "title": "Charlie", "invariants": [{"invariant_id": "INV-C", "description": "d3", "test_ids": ["t3"]}]}
        ]
        fixture_path = self.write_json("checkpoint_fixture.json", data)

        baseline_path = os.path.join(self.temp_dir.name, "baseline.json")
        res_baseline = self.run_cli(["--input", fixture_path, "--output", baseline_path])
        self.assertEqual(res_baseline.returncode, 0)
        with open(baseline_path) as f:
            baseline = json.load(f)
        baseline.pop("timestamp")

        checkpoint_path = os.path.join(self.temp_dir.name, "manual_ckpt.json")
        checkpoint_content = {
            "last_index": 0,
            "processed_items": [
                {"issue_id": "AA-1", "title": "Alpha", "invariants": [{"invariant_id": "INV-A", "description": "d1", "test_ids": ["t1"], "verified": True}]}
            ]
        }
        with open(checkpoint_path, "w") as f:
            json.dump(checkpoint_content, f)

        resumed_path = os.path.join(self.temp_dir.name, "resumed.json")
        res_resumed = self.run_cli(["--input", fixture_path, "--output", resumed_path, "--resume-checkpoint", checkpoint_path])
        self.assertEqual(res_resumed.returncode, 0)
        self.assertFalse(os.path.exists(checkpoint_path))

        with open(resumed_path) as f:
            resumed = json.load(f)
        resumed.pop("timestamp")

        self.assertEqual(baseline, resumed)

    def test_checkpoint_mismatch_rejected(self):
        data = [
            {"issue_id": "AA-1", "title": "Alpha", "invariants": [{"invariant_id": "INV-A", "description": "d1", "test_ids": ["t1"]}]},
            {"issue_id": "BB-2", "title": "Bravo", "invariants": [{"invariant_id": "INV-B", "description": "d2", "test_ids": ["t2"]}]}
        ]
        fixture_path = self.write_json("checkpoint_fixture2.json", data)

        checkpoint_path = os.path.join(self.temp_dir.name, "bad_ckpt.json")
        checkpoint_content = {
            "last_index": 0,
            "processed_items": [
                {"issue_id": "ZZ-9", "title": "Wrong", "invariants": []}
            ]
        }
        with open(checkpoint_path, "w") as f:
            json.dump(checkpoint_content, f)

        out_path = os.path.join(self.temp_dir.name, "out.json")
        res = self.run_cli(["--input", fixture_path, "--output", out_path, "--resume-checkpoint", checkpoint_path])
        self.assertEqual(res.returncode, 4)

    def test_malformed_checkpoint_rejected(self):
        checkpoint_path = os.path.join(self.temp_dir.name, "malformed_ckpt.json")
        with open(checkpoint_path, "w") as f:
            f.write("{ not valid json")

        out_path = os.path.join(self.temp_dir.name, "out.json")
        res = self.run_cli(["--input", self.fixture_path, "--output", out_path, "--resume-checkpoint", checkpoint_path])
        self.assertEqual(res.returncode, 4)

if __name__ == "__main__":
    unittest.main()
