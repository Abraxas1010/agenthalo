#!/usr/bin/env python3
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import living_agent_verify as verify


GOOD_PAPER = """# Verified Grid Bridge

**Abstract:** This paper proposes a verified bridge between the Living Agent and Heyting artifacts. It shows how cell_R1_C2.md anchors the synthesis.

**Methodology:** We demonstrates a staged verification flow with explicit structural checks, semantic coverage, and formal snippet execution across the grid.

**Results:** The report establishes a machine-readable artifact and suggests a repeatable promotion gate for downstream publication.
"""


class TestLivingAgentVerify(unittest.TestCase):
    def test_structural_result_detects_bold_sections_and_claims(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            living_agent_root = Path(tmpdir)
            grid_cell = living_agent_root / "knowledge" / "grid" / "cell_R1_C2.md"
            grid_cell.parent.mkdir(parents=True, exist_ok=True)
            grid_cell.write_text("# cell\n", encoding="utf-8")
            result = verify.structural_result(GOOD_PAPER, living_agent_root)
        self.assertTrue(result.passed)
        self.assertGreaterEqual(result.details["section_count"], 2)
        self.assertGreaterEqual(result.details["claim_count"], 1)
        self.assertEqual(result.details["valid_references"], 1)

    def test_formal_result_fails_closed_without_typecheck_root(self) -> None:
        text = "```lean\n#check True\n```"
        with patch.object(verify, "resolve_typecheck_root", return_value=(None, "missing typecheck root")):
            result = verify.formal_result(text)
        self.assertFalse(result.passed)
        self.assertEqual(result.score, 0.0)
        self.assertTrue(result.details["fail_closed"])

    def test_write_report_payload_persists_machine_readable_json(self) -> None:
        payload = {
            "paper_sha256": "abc123",
            "schema_version": verify.SCHEMA_VERSION,
            "composite": {"score": 0.75, "passed": True, "details": {}},
        }
        with tempfile.TemporaryDirectory() as tmpdir:
            report_path = verify.write_report_payload(Path(tmpdir), payload)
            loaded = json.loads(report_path.read_text(encoding="utf-8"))
        self.assertEqual(report_path.name, "abc123.json")
        self.assertEqual(loaded["paper_sha256"], "abc123")


if __name__ == "__main__":
    unittest.main()
