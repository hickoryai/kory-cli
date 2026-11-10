"""Optional offline integration with the pinned sibling checkout's real loader."""

import asyncio
import copy
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import AsyncMock, patch

SOURCE = os.environ.get("KORY_SOURCE_CHECKOUT")
SCRIPT = Path(__file__).resolve().parents[1] / "nightly.py"
spec = importlib.util.spec_from_file_location("nightly_integration", SCRIPT)
nightly = importlib.util.module_from_spec(spec)
spec.loader.exec_module(nightly)
HEAD, OLD = "a" * 40, "b" * 40


@unittest.skipUnless(SOURCE, "set KORY_SOURCE_CHECKOUT for pinned harness integration")
class HarnessCompatibilityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        temporary = tempfile.TemporaryDirectory()
        cls.addClassCleanup(temporary.cleanup)
        environment = patch.dict(os.environ, {"MIRA_INDEX_DIR": temporary.name})
        environment.start()
        cls.addClassCleanup(environment.stop)
        database_url = os.environ.pop("DATABASE_URL", None)
        if database_url is not None:
            cls.addClassCleanup(os.environ.__setitem__, "DATABASE_URL", database_url)
        sys.path[:0] = [str(Path(SOURCE) / "tests"), str(Path(SOURCE) / "src")]

    def setUp(self):
        self.pr = {"number": 1, "head": {"sha": HEAD}}
        self.fixture = {
            "repo": "o/r",
            "pr_url": "https://github.com/o/r/pull/1",
            "pr_number": 1,
            "base_sha": "0" * 40,
            "head_sha": HEAD,
            "split": "nightly",
            "greptile_comments": [
                {
                    "kind": "summary",
                    "created_at": "2026-11-09T00:00:00Z",
                    "updated_at": "2026-11-10T00:00:00Z",
                    "body": f"Last reviewed commit: https://github.com/o/r/commit/{HEAD}",
                }
            ],
            "findings": [],
        }

    def export(self, current=None):
        from benchmarks import parity_export

        current = current or {"state": "open", "head": {"sha": HEAD}}
        with (
            patch.dict(os.environ, {"GITHUB_TOKEN": "synthetic"}),
            patch.object(
                parity_export,
                "export_pr",
                new=AsyncMock(return_value=copy.deepcopy(self.fixture)),
            ),
            patch.object(
                parity_export.GitHubReader, "get", new=AsyncMock(return_value=current)
            ),
        ):
            return asyncio.run(nightly.export_snapshot(self.pr))

    def test_clean_head_with_old_findings_selects_only_current_unit(self):
        from benchmarks.parity_snapshot import load_units

        self.fixture["greptile_comments"].append(
            {
                "kind": "inline",
                "original_commit_id": OLD,
                "created_at": "2026-11-09T01:00:00Z",
                "body": "older finding",
                "path": "old.py",
                "original_line": 1,
            }
        )
        self.fixture["findings"].append(
            {"id": "synthetic-old", "description": "older finding"}
        )
        fixture = self.export()
        self.assertEqual(fixture["snapshot_sha"], HEAD)
        units = load_units([fixture], "nightly")
        self.assertEqual(len(units), 1)
        self.assertTrue(units[0].greptile_reviewed)
        self.assertEqual(units[0].golden, ())
        self.assertEqual([g["id"] for g in units[0].prior], ["synthetic-old"])

    def test_missing_current_greptile_evidence_is_deferred(self):
        self.fixture["greptile_comments"] = []
        self.assertIsNone(self.export())

    def test_branch_race_is_deferred(self):
        self.assertIsNone(self.export({"state": "open", "head": {"sha": OLD}}))

    def test_closed_pr_is_deferred(self):
        self.assertIsNone(self.export({"state": "closed", "head": {"sha": HEAD}}))

    def test_discovery_and_export_must_match(self):
        self.fixture["head_sha"] = OLD
        self.assertIsNone(self.export())
