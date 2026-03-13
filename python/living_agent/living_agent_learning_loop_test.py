#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import tempfile
import unittest
from pathlib import Path

import living_agent_learning_loop as loop


SOUL_TEMPLATE = """# SOUL OF AGENT ZERO

## IDENTITY (immutable)
Goal: Discover intersections between biological computing and physics.

## GENERATION
Current Cycle: 1
Total Papers Published: 0
Highest SNS Score: 0.0
"""


class LivingAgentLearningLoopTest(unittest.TestCase):
    def test_update_preserves_soul_and_records_report_hash(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            soul_path = tmp / "soul.md"
            report_path = tmp / "report.json"
            soul_path.write_text(SOUL_TEMPLATE, encoding="utf-8")
            report_path.write_text('{"composite":{"passed":true}}', encoding="utf-8")

            priority = loop.CellPriority.load_from_soul(soul_path.read_text(encoding="utf-8"))
            report_sha256 = hashlib.sha256(report_path.read_bytes()).hexdigest()
            priority.update(
                ["R0_C0", "R1_C1"],
                True,
                verification_report_sha256=report_sha256,
                verification_report_path=str(report_path),
            )
            updated = priority.save_to_soul(soul_path.read_text(encoding="utf-8"))

            self.assertIn("Current Cycle: 1", updated)
            self.assertIn(loop.SECTION_HEADER, updated)
            self.assertIn(loop.EVIDENCE_HEADER, updated)
            reloaded = loop.CellPriority.load_from_soul(updated)
            self.assertEqual(reloaded.query("R0_C0"), 0.55)
            self.assertEqual(
                reloaded.evidence["R0_C0"]["verification_report_sha256"],
                report_sha256,
            )

    def test_prompt_fragment_emits_high_and_low_labels(self) -> None:
        priority = loop.CellPriority()
        for _ in range(4):
            priority.update(["R0_C0"], True)
            priority.update(["R1_C1"], False)
        fragment = priority.inject_into_prompt(["R0_C0", "R1_C1", "R2_C2"])
        self.assertIn("R0_C0: HIGH PRIORITY", fragment)
        self.assertIn("R1_C1: LOW PRIORITY", fragment)
        self.assertIn("R2_C2: NEUTRAL PRIORITY", fragment)

    def test_simulation_converges_and_stays_bounded(self) -> None:
        payload = loop.simulate_priorities(
            cycles=50,
            grid_size=256,
            verify_rate=0.3,
            alpha=0.1,
            seed=7,
        )
        self.assertGreater(payload["verified_mean_priority"], 0.6)
        self.assertLess(payload["rejected_mean_priority"], 0.4)
        self.assertGreater(payload["neutral_mean_priority"], 0.4)
        self.assertLess(payload["neutral_mean_priority"], 0.6)
        self.assertLessEqual(payload["priority_map_size"], 256)


if __name__ == "__main__":
    unittest.main()
