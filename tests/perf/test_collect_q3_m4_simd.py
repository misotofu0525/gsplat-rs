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


if __name__ == "__main__":
    unittest.main()
