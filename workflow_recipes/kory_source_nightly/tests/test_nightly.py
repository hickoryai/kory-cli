import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "nightly.py"
spec = importlib.util.spec_from_file_location("nightly", SCRIPT)
nightly = importlib.util.module_from_spec(spec)
spec.loader.exec_module(nightly)


def pr(number=1, sha="a" * 40):
    return {"number": number, "head": {"sha": sha}, "updated_at": "2026-11-10"}


class NightlyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.storage = Path(self.temp.name)
        self.calls = []
        self.prs = [pr()]

    async def export(self, snapshot):
        return {"pr_number": snapshot["number"], "head_sha": snapshot["head"]["sha"]}

    def run_harness(self, argv, source, output):
        self.calls.append(argv)
        nightly.save(
            output.with_suffix(".json"), {"finished_at": "now", "records": [{}]}
        )
        return 0

    def compare(self, **kwargs):
        return nightly.compare(
            self.storage,
            Path("/source"),
            kwargs.pop("budget", 128),
            kwargs.pop("replay", False),
            discover=lambda: self.prs,
            recheck=kwargs.pop("recheck", lambda _: True),
            export=self.export,
            run=kwargs.pop("run", self.run_harness),
            day=kwargs.pop("day", "2026-11-10"),
            **kwargs,
        )

    def state(self):
        return json.loads((self.storage / "cursor.json").read_text())

    def test_skip_unchanged_but_compare_changed_head(self):
        self.compare()
        self.assertEqual(self.compare()["skipped"], 1)
        self.prs = [pr(sha="b" * 40)]
        self.compare()
        self.assertEqual(len(self.calls), 2)
        self.assertEqual(len(self.state()["attempts"]), 2)

    def test_replay_is_explicit_and_does_not_reset_daily_budget(self):
        self.compare()
        self.assertEqual(self.compare(replay=True)["completed"], 1)
        self.assertEqual(self.compare(replay=True)["deferred"], 1)
        self.assertEqual(len(self.calls), 2)

    def test_interrupted_after_artifact_recovers_without_comparing_twice(self):
        def interrupted(argv, source, output):
            self.run_harness(argv, source, output)
            raise KeyboardInterrupt()

        with self.assertRaises(KeyboardInterrupt):
            self.compare(run=interrupted)
        self.assertEqual(self.state()["attempts"][0]["status"], "running")
        self.assertEqual(self.compare()["skipped"], 1)
        self.assertEqual(self.state()["attempts"][0]["status"], "completed")
        self.assertEqual(len(self.calls), 1)

    def test_unknown_interruption_is_not_resent_and_missed_work_recovers(self):
        def interrupted(argv, source, output):
            self.calls.append(argv)
            raise KeyboardInterrupt()

        with self.assertRaises(KeyboardInterrupt):
            self.compare(run=interrupted)
        self.prs.append(pr(2))
        self.compare()
        self.assertEqual(len(self.calls), 2)
        self.assertEqual(
            [a["status"] for a in self.state()["attempts"]],
            ["interrupted", "completed"],
        )
        self.assertEqual(self.compare(day="2026-11-12", replay=True)["completed"], 2)

    def test_daily_cap_includes_preflight_and_survives_restart(self):
        self.prs = [pr(n) for n in range(1, 6)]
        self.compare(budget=100)
        allocations = [
            int(c[c.index("--seat-budget") + 1]) + nightly.PREFLIGHT_RESERVE
            for c in self.calls
        ]
        self.assertEqual(sum(allocations), 100)
        self.assertTrue(all(a <= nightly.SNAPSHOT_BUDGET for a in allocations))
        self.compare(budget=1000)
        self.assertEqual(len(self.calls), 2)
        self.assertEqual(self.state()["days"]["2026-11-10"]["reserved"], 100)
        self.compare(day="2026-11-12", budget=100)
        self.assertEqual(len(self.calls), 4)

    def test_unreviewed_or_racing_head_remains_pending(self):
        async def missing(_):
            return None

        self.export = missing
        self.assertEqual(self.compare()["deferred"], 1)
        self.assertEqual(self.state()["attempts"], [])
        self.assertEqual(self.state()["days"]["2026-11-10"]["reserved"], 0)

    def test_partial_artifact_cannot_advance_completed_cursor(self):
        def partial(argv, source, output):
            output.with_suffix(".json").write_text('{"records":')
            return 1

        self.assertEqual(self.compare(run=partial)["interrupted"], 1)
        self.assertEqual(self.compare()["skipped"], 1)

    def test_head_change_at_launch_does_not_reserve_or_consume_cursor(self):
        self.assertEqual(self.compare(recheck=lambda _: False)["deferred"], 1)
        self.assertEqual(self.state()["attempts"], [])
        self.assertEqual(self.state()["days"]["2026-11-10"]["reserved"], 0)
        self.assertEqual(self.calls, [])

    def test_model_pins_and_single_current_unit(self):
        self.compare()
        argv = self.calls[0]
        self.assertEqual(argv[argv.index("--reviewer-model") + 1], nightly.REVIEWER)
        self.assertEqual(argv[argv.index("--judge-model") + 1], nightly.JUDGE)
        self.assertEqual(argv[argv.index("--only") + 1], "1@aaaaaaaaaaaa")
        self.assertEqual(argv[argv.index("--arms") + 1], "registry_off")

    def test_model_pins_can_match_the_local_router_accounts(self):
        reviewer = "grok@local/grok-4.7"
        judge = "claude@local/opus"
        self.compare(reviewer=reviewer, judge=judge)
        argv = self.calls[0]
        self.assertEqual(argv[argv.index("--reviewer-model") + 1], reviewer)
        self.assertEqual(argv[argv.index("--judge-model") + 1], judge)

    def test_private_outputs_and_lock_inherited_by_harness(self):
        lock = self.storage / "lock"
        old_mask = os.umask(0o077)
        try:
            with lock.open("a") as handle:
                output = self.storage / "private"
                nightly.launch(
                    [
                        "python3",
                        "-c",
                        f"import os; os.fstat({handle.fileno()}); print('private finding'); "
                        "print(os.environ['KORY_PARITY_RELAY_LOG'])",
                    ],
                    self.storage,
                    output,
                    lock_fd=handle.fileno(),
                )
            self.assertEqual(output.with_suffix(".log").stat().st_mode & 0o777, 0o600)
            self.assertIn("private finding", output.with_suffix(".log").read_text())
            self.assertIn(
                str(output.with_suffix(".relay.log")),
                output.with_suffix(".log").read_text(),
            )
        finally:
            os.umask(old_mask)

    def test_open_pr_discovery_is_paginated_read_only(self):
        with patch.object(subprocess, "run") as run:
            run.return_value.stdout = json.dumps([[pr(2)], [pr(1)]])
            self.assertEqual([p["number"] for p in nightly.open_prs()], [1, 2])
            self.assertIn("--paginate", run.call_args.args[0])
            self.assertIn("state=open", run.call_args.args[0][-1])


if __name__ == "__main__":
    unittest.main()
