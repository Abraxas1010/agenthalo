#!/usr/bin/env python3
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import living_agent_hive_publisher as publisher


GOOD_REPORT = {
    "paper_sha256": "abc123",
    "composite": {"passed": True, "score": 0.8, "details": {}},
}


class TestLivingAgentHivePublisher(unittest.TestCase):
    def test_dedup_blocks_pending_and_published_entries(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "hive_publication_log.jsonl"
            publisher.append_jsonl(
                log_path,
                {
                    "paper_sha256": "same",
                    "publish_result": {"status": "pending_publication"},
                },
            )
            self.assertTrue(publisher.dedup(log_path, "same"))
            publisher.append_jsonl(
                log_path,
                {
                    "paper_sha256": "same",
                    "publish_result": {"status": "published"},
                },
            )
            self.assertTrue(publisher.dedup(log_path, "same"))

    def test_update_shared_discoveries_records_status_column(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            living_agent_root = Path(tmpdir)
            publisher.update_shared_discoveries(
                living_agent_root,
                cycle=7,
                title="Bridge Result",
                sns=0.61,
                trace="R0_C0 -> R1_C0",
                status="dry_run",
            )
            text = (living_agent_root / "memories" / "hive" / "shared_discoveries.md").read_text(
                encoding="utf-8"
            )
        self.assertIn("| Cycle | Title | SNS | Status | Trace |", text)
        self.assertIn("| 7 | Bridge Result | 0.610 | dry_run | R0_C0 -> R1_C0 |", text)

    def test_bridge_rejection_routes_paper_to_rejected_memory(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            archive_dir = Path(tmpdir) / "artifacts"
            living_agent_root = Path(tmpdir) / "living-agent"
            archive_dir.mkdir(parents=True, exist_ok=True)
            living_agent_root.mkdir(parents=True, exist_ok=True)
            paper_text = "# Rejected by Bridge\n\n**Abstract:** Real content.\n"
            argv = [
                "living_agent_hive_publisher.py",
                "--text",
                paper_text,
                "--cycle",
                "9",
                "--trace",
                "[0,0] -> [1,0]",
                "--trace-cells",
                "R0_C0,R1_C0",
                "--archive-dir",
                str(archive_dir),
                "--living-agent-root",
                str(living_agent_root),
                "--json",
            ]
            with patch.object(
                publisher, "Encoder", return_value=object()
            ), patch.object(
                publisher, "score_text", return_value={"sns": 0.42}
            ), patch.object(
                publisher, "verify_paper", return_value=GOOD_REPORT
            ), patch.object(
                publisher, "write_report_payload", return_value=archive_dir / "verification_reports" / "abc123.json"
            ), patch.object(
                publisher,
                "publish_via_agenthalo",
                return_value={"status": "rejected", "surface": "agenthalo-cli:p2pclaw-bridge-publish-paper"},
            ):
                with patch("sys.argv", argv):
                    rc = publisher.main()
            self.assertEqual(rc, 0)
            rejected_path = living_agent_root / "memories" / "rejected" / "paper_9.md"
            semantic_path = living_agent_root / "memories" / "semantic" / "paper_9.md"
            self.assertTrue(rejected_path.exists())
            self.assertFalse(semantic_path.exists())
            rows = publisher.load_jsonl(archive_dir / "hive_publication_log.jsonl")
            self.assertEqual(rows[0]["publish_result"]["status"], "rejected")
            self.assertEqual(rows[0]["trace_cells"], ["R0_C0", "R1_C0"])


if __name__ == "__main__":
    unittest.main()
