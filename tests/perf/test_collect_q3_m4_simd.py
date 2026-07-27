import importlib.util
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("collect-q3-m4-simd.py")
SPEC = importlib.util.spec_from_file_location("collect_q3_m4_simd", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
COLLECTOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COLLECTOR)


class Q3M4SimdCollectorTests(unittest.TestCase):
    def test_only_a_native_apple_m4_is_admitted(self):
        self.assertIsNone(COLLECTOR.host_deferral("Darwin", "arm64", "Apple M4"))
        self.assertIsNone(
            COLLECTOR.host_deferral("Darwin", "arm64", "Apple M4 Max")
        )
        self.assertEqual(
            COLLECTOR.host_deferral("Darwin", "x86_64", "Apple M4"),
            "requires_native_aarch64_not_rosetta",
        )
        self.assertEqual(
            COLLECTOR.host_deferral("Darwin", "arm64", "Apple M3"),
            "requires_physical_apple_m4_cpu",
        )
        self.assertEqual(
            COLLECTOR.host_deferral("Darwin", "arm64", "Apple M40"),
            "requires_physical_apple_m4_cpu",
        )

    def test_receipt_rejects_a_fourth_machine_state(self):
        receipt = {
            "schema": COLLECTOR.SCHEMA,
            "cell": COLLECTOR.CELL,
            "decision": "Ready",
            "whole_plan_promotion": False,
        }
        with self.assertRaisesRegex(ValueError, "Accepted, Rejected or Deferred"):
            COLLECTOR.validate_receipt(receipt)

    def test_microbenchmark_cannot_claim_whole_plan_promotion(self):
        receipt = {
            "schema": COLLECTOR.SCHEMA,
            "cell": COLLECTOR.CELL,
            "decision": "Accepted",
            "whole_plan_promotion": True,
        }
        with self.assertRaisesRegex(ValueError, "cannot promote a whole plan"):
            COLLECTOR.validate_receipt(receipt)

    def test_unavailable_hardware_is_a_deferred_cell(self):
        receipt = COLLECTOR.terminal_cell(
            "Deferred", "requires_physical_apple_m4_cpu", brand="Apple M3"
        )
        COLLECTOR.validate_receipt(receipt)
        self.assertEqual(receipt["decision"], "Deferred")
        self.assertFalse(receipt["whole_plan_promotion"])

    def test_output_is_fresh_and_published_once(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "nested" / "cell.json"
            COLLECTOR.require_fresh_output(output)
            COLLECTOR.publish_output(output, "{}")
            self.assertEqual(output.read_text(encoding="utf-8"), "{}\n")
            with self.assertRaisesRegex(ValueError, "already exists"):
                COLLECTOR.require_fresh_output(output)

    def test_git_receipt_hashes_the_exact_status_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            import subprocess

            subprocess.run(["git", "init", "-q"], cwd=root, check=True)
            subprocess.run(
                ["git", "config", "user.email", "q3@example.invalid"],
                cwd=root,
                check=True,
            )
            subprocess.run(
                ["git", "config", "user.name", "Q3 Test"], cwd=root, check=True
            )
            tracked = root / "tracked.txt"
            tracked.write_text("fixed\n", encoding="utf-8")
            subprocess.run(["git", "add", "tracked.txt"], cwd=root, check=True)
            subprocess.run(["git", "commit", "-qm", "fixture"], cwd=root, check=True)

            clean = COLLECTOR.git_receipt(root)
            self.assertFalse(clean["dirty"])
            self.assertEqual(clean["status_porcelain_sha256"], "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")

            tracked.write_text("changed\n", encoding="utf-8")
            dirty = COLLECTOR.git_receipt(root)
            self.assertTrue(dirty["dirty"])
            self.assertEqual(dirty["commit"], clean["commit"])
            self.assertNotEqual(
                dirty["status_porcelain_sha256"], clean["status_porcelain_sha256"]
            )


if __name__ == "__main__":
    unittest.main()
