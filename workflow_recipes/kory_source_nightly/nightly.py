#!/usr/bin/env python3
"""Private, at-most-once open-head parity monitoring (HAI-11608)."""

from __future__ import annotations

import argparse
import asyncio
import datetime as dt
import fcntl
import functools
import json
import os
from pathlib import Path
import subprocess
import sys
import uuid

HARNESS_SHA = "f29d30d4ea0fb635ae59c85a299b96572635d62a"
REVIEWER = "grok@59aa6fc18f3f4b929dd9d59a6f3bfc11/grok-4.7"
JUDGE = "claude@22e7810507d5403e83727a419617c51a/opus"
REPO = "hickoryai/hickory-ai"
# The pinned harness preflight is outside its SeatMeter. Reserve more than its
# worst case: two JSON completions x two final-answer turns x three attempts, plus a
# conversation open (three attempts) and kept turn (one, never retried) = 16.
PREFLIGHT_RESERVE = 24
SNAPSHOT_BUDGET = 64
MIN_RUN_BUDGET = 7


def save(path: Path, value: dict | list) -> None:
    temporary = path.with_suffix(".tmp")
    with temporary.open("w") as stream:
        json.dump(value, stream, indent=2)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(path)
    descriptor = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def key(pr: dict) -> str:
    return f"{REPO}#{pr['number']}@{pr['head']['sha']}"


def open_prs() -> list[dict]:
    result = subprocess.run(
        [
            "gh",
            "api",
            "--paginate",
            "--slurp",
            f"repos/{REPO}/pulls?state=open&per_page=100",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    return sorted(
        [pr for page in json.loads(result.stdout) for pr in page],
        key=lambda pr: (pr["updated_at"], pr["number"]),
    )


def still_current(pr: dict) -> bool:
    result = subprocess.run(
        ["gh", "api", f"repos/{REPO}/pulls/{pr['number']}"],
        check=True,
        capture_output=True,
        text=True,
    )
    current = json.loads(result.stdout)
    return current["state"] == "open" and current["head"]["sha"] == pr["head"]["sha"]


async def export_snapshot(pr: dict) -> dict | None:
    from benchmarks.parity_export import GitHubReader, export_pr
    from benchmarks.parity_snapshot import load_units
    import httpx

    async with httpx.AsyncClient(timeout=60) as client:
        gh = GitHubReader(os.environ["GITHUB_TOKEN"], client)
        fixture = await export_pr(gh, REPO, pr["number"], "nightly")
        current = await gh.get(f"/repos/{REPO}/pulls/{pr['number']}")
    # A branch can advance while comments are fetched. Defer it until discovery
    # sees the same head; never consume its cursor under the old head.
    if (
        fixture["head_sha"] != pr["head"]["sha"]
        or current["head"]["sha"] != fixture["head_sha"]
        or current["state"] != "open"
    ):
        return None
    fixture["snapshot_sha"] = fixture["head_sha"]
    unit_id = f"{pr['number']}@{fixture['head_sha'][:12]}"
    units = load_units([fixture], "nightly")
    if not any(u.unit_id == unit_id and u.greptile_reviewed for u in units):
        return None
    return fixture


def command(
    source: Path, manifest: Path, output: Path, pr: dict, allocation: int
) -> list[str]:
    return [
        sys.executable,
        str(source / "tests/benchmarks/fixtures/hickory/run_parity.py"),
        "run",
        "--manifest",
        str(manifest),
        "--split",
        "nightly",
        "--only",
        f"{pr['number']}@{pr['head']['sha'][:12]}",
        "--arms",
        "registry_off",
        "--runs",
        "1",
        "--reviewer-model",
        REVIEWER,
        "--judge-provider",
        "router-acp",
        "--judge-model",
        JUDGE,
        "--seed",
        "11607",
        "--seat-budget",
        str(allocation - PREFLIGHT_RESERVE),
        "--calls-per-review",
        "6.0",
        "--review-timeout-seconds",
        "1800",
        "--conversations",
        "--concurrency",
        "1",
        "--out",
        str(output),
    ]


def launch(argv: list[str], source: Path, output: Path, *, lock_fd: int) -> int:
    # Harness stdout includes private findings; never relay it into a Goose
    # transcript or scheduler log.
    with output.with_suffix(".log").open("w") as log:
        environment = dict(
            os.environ, KORY_PARITY_RELAY_LOG=str(output.with_suffix(".relay.log"))
        )
        return subprocess.run(
            argv,
            cwd=source,
            stdout=log,
            stderr=subprocess.STDOUT,
            pass_fds=(lock_fd,),
            env=environment,
        ).returncode


def recover(state: dict) -> None:
    for attempt in state["attempts"]:
        if attempt["status"] != "running":
            continue
        artifact = Path(attempt["output"]).with_suffix(".json")
        try:
            data = json.loads(artifact.read_text())
            complete = bool(data.get("finished_at")) and bool(data.get("records"))
        except (OSError, ValueError):
            complete = False
        # No artifact cannot prove no seat call happened. Never resend that
        # snapshot automatically; replay is the explicit operator escape hatch.
        attempt["status"] = "completed" if complete else "interrupted"


def compare(
    state_dir: Path,
    source: Path,
    budget: int,
    replay: bool,
    *,
    discover=open_prs,
    recheck=still_current,
    export=export_snapshot,
    run=launch,
    day: str | None = None,
) -> dict:
    day = day or dt.datetime.now(dt.timezone.utc).date().isoformat()
    cursor = state_dir / "cursor.json"
    state = (
        json.loads(cursor.read_text())
        if cursor.exists()
        else {"version": 1, "attempts": [], "days": {}}
    )
    recover(state)
    ledger = state["days"].setdefault(day, {"cap": budget, "reserved": 0})
    # A restart (or replay) cannot silently increase today's approved cap.
    cap = min(budget, ledger["cap"])
    ledger["cap"] = cap
    save(cursor, state)
    seen = {a["key"] for a in state["attempts"]}
    summary = {"completed": 0, "skipped": 0, "deferred": 0, "interrupted": 0}
    for pr in discover():
        snapshot_key = key(pr)
        if snapshot_key in seen and not replay:
            summary["skipped"] += 1
            continue
        allocation = min(SNAPSHOT_BUDGET, cap - ledger["reserved"])
        if allocation < PREFLIGHT_RESERVE + MIN_RUN_BUDGET:
            summary["deferred"] += 1
            continue
        fixture = asyncio.run(export(pr))
        if fixture is None:
            summary["deferred"] += 1
            continue
        attempt_dir = state_dir / uuid.uuid4().hex
        attempt_dir.mkdir(mode=0o700)
        manifest, output = attempt_dir / "manifest.json", attempt_dir / "parity"
        save(manifest, [fixture])
        if not recheck(pr):
            summary["deferred"] += 1
            continue
        attempt = {
            "key": snapshot_key,
            "day": day,
            "status": "running",
            "reserved": allocation,
            "output": str(output),
        }
        state["attempts"].append(attempt)
        ledger["reserved"] += allocation
        # Charge before launch. Ambiguous/failed attempts keep their entire
        # reservation across restarts, so interrupted calls cannot overspend.
        save(cursor, state)
        seen.add(snapshot_key)
        code = run(command(source, manifest, output, pr, allocation), source, output)
        attempt["exit_code"] = code
        recover(state)
        save(cursor, state)
        summary[attempt["status"]] += 1
    return summary


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--source", type=Path, required=True, help="pinned Kory Source checkout"
    )
    parser.add_argument(
        "--state-dir",
        type=Path,
        required=True,
        help="private local directory outside any git checkout",
    )
    parser.add_argument(
        "--seat-budget",
        type=int,
        default=128,
        help="approved daily harness cap, including preflight",
    )
    parser.add_argument(
        "--replay",
        action="store_true",
        help="explicitly repeat current heads, within today's remaining cap",
    )
    args = parser.parse_args()
    if args.seat_budget < PREFLIGHT_RESERVE + MIN_RUN_BUDGET:
        parser.error(
            f"--seat-budget must be at least {PREFLIGHT_RESERVE + MIN_RUN_BUDGET}"
        )
    source = args.source.resolve()
    if (
        subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=source, text=True
        ).strip()
        != HARNESS_SHA
    ):
        parser.error(
            f"Kory Source must be at {HARNESS_SHA}; re-audit preflight bounds before updating"
        )
    if subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=no"], cwd=source, text=True
    ).strip():
        parser.error("Kory Source checkout must have no tracked modifications")
    state_dir = args.state_dir.expanduser().resolve()
    if any((parent / ".git").exists() for parent in [state_dir, *state_dir.parents]):
        parser.error("--state-dir must be outside git checkouts")
    os.umask(0o077)
    state_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    state_dir.chmod(0o700)
    # The harness creates a shared loopback port. Serialize the entire nightly,
    # and let the child inherit the lock if the parent is interrupted.
    with (state_dir / "nightly.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            parser.exit(0, "nightly already running\n")
        os.set_inheritable(lock.fileno(), True)
        if not os.environ.get("GITHUB_TOKEN"):
            os.environ["GITHUB_TOKEN"] = subprocess.check_output(
                ["gh", "auth", "token"], text=True
            ).strip()
        # Importing the exporter also imports Mira configuration. Isolate it
        # before imports so discovery cannot initialize a deployment database.
        os.environ.pop("DATABASE_URL", None)
        os.environ["MIRA_INDEX_DIR"] = str(state_dir / "export-index")
        sys.path[:0] = [str(source / "tests"), str(source / "src")]
        print(
            json.dumps(
                compare(
                    state_dir,
                    source,
                    args.seat_budget,
                    args.replay,
                    run=functools.partial(launch, lock_fd=lock.fileno()),
                )
            )
        )


if __name__ == "__main__":
    main()
