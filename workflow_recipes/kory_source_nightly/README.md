# Private nightly parity monitoring

Implements [HAI-11608](https://linear.app/hickoryai/issue/HAI-11608).
This is post-rollout monitoring, independent of the HAI-12818 held-out check.
It does not enable automatic reviews or install a schedule.

## Run on the relay owner's private machine

Requires Python 3.11+, uv, authenticated read-only GitHub access, and a clean
Kory Source checkout at the `HARNESS_SHA` in `nightly.py`. Install that checkout's
normal uv dependencies first. The checkout needs the optional `snapshot_sha`
manifest hook described in [Kory Source #19](https://github.com/hickoryai/kory-source/pull/19).
Use the relay owner's loopback entry, never the production serving port:

```bash
export MIRA_ROUTER_ACP_RELAY_ENTRY=/absolute/path/to/loopback-relay.mjs
export KORY_COMPLETION_TIMEOUT_CAP_SECONDS=1800
# Optional if using an already-running loopback relay:
# export MIRA_ROUTER_ACP_URL=http://127.0.0.1:8799

goose run --recipe workflow_recipes/kory_source_nightly/recipe.yaml \
  --params source_checkout=/absolute/path/to/kory-source \
  --params private_storage=/private/local/kory-parity \
  --params seat_budget=128
```

For a deterministic trigger without an orchestration model call, the equivalent is:

```bash
uv run --project /absolute/path/to/kory-source python \
  /absolute/path/to/goose/workflow_recipes/kory_source_nightly/nightly.py \
  --source /absolute/path/to/kory-source --state-dir /private/local/kory-parity \
  --seat-budget 128
```

**No trigger is installed by this change.** After operator approval, run once
nightly (for example, 02:00 UTC) using the relay owner's local scheduler, under
that owner's account, with the same persistent private directory and environment.
Do not schedule it on shared CI or upload its output as CI artifacts. Missed
nights need no special date range: each invocation enumerates all currently
OPEN `hickoryai/hickory-ai` PRs, oldest updated first. It does not reconstruct
intermediate heads that disappeared between nights, or heads on now-closed PRs.

## Snapshots and recovery

Only the current `(repository, PR number, full head SHA)` is eligible. The existing
exporter fetches Greptile output using read-only APIs. The driver checks that the
PR is still open at the same head, sets the manifest's `snapshot_sha`, and passes
`--only <number>@<head-prefix>` to the harness. Older findings remain prior context
for the judge; they are never presented as current golden findings or shown to
the reviewer. A current head without matching Greptile output is deferred without
consuming a cursor or budget, so a later night can pick up a late Greptile review.

Each attempt has a UUID directory containing its frozen manifest, harness log,
and parity JSON/Markdown. Only counts go to stdout. A `0700` directory, `0600`
files, restrictive umask, and rejection of storage inside git checkouts keep
results local. Use private storage with appropriate host access controls; no
upload, comment, review, label, or production mutation is performed. The recipe
must not quote reports into its conversation. The private harness artifact keeps
its own rerun command and provenance; do not paste either into a PR.

An exclusive lock serializes runs using this storage, including a surviving
harness child after its parent dies. The cursor is atomically replaced and
fsynced **before** launching each attempt. Recovery consumes a finished artifact
if it exists. Without a final artifact, an attempt is marked `interrupted` and
never automatically resent: some seat calls may already have happened. This is
at-most-once recovery, not mid-review resumption. Other pending snapshots continue
within the remaining budget. Inspect ambiguous attempts privately; use `--replay`
(or recipe parameter `replay=true`) to explicitly repeat current heads, including
completed ones. Replay always keeps the old artifacts and reservations. Historical
heads that are no longer open/current are not replayed by this recipe.

Use one persistent directory for this monitor; deleting/changing it resets
history and the ledger. A corrupt cursor fails closed. Keep it with the artifacts
when backing up or migrating storage. A complete artifact can contain review,
judge, or budget failures; completion means the attempt was recorded, not that
parity passed. Small nightly samples normally give an inconclusive parity verdict.

## Budget and priority

`seat_budget` defaults to **128 relay requests per UTC day**, across invocations
and replays. This is a ceiling parameter, not authorization to spend seats:
operators must select an approved cap before enabling a trigger. The recipe's
orchestration turn, if used, is separate from the harness seat cap; the direct
trigger above needs no orchestration seat.

Before each snapshot the driver durably reserves up to 64 requests from that
cap. It subtracts 24 for the pinned harness's unmetered preflight (including
retries/JSON repairs/conversation probes; its worst case is at most 16). The
remainder is the harness's hard `--seat-budget`, which covers review/judge
requests and retries. A reservation below 31 is deferred, so no zero/unlimited
budget reaches the harness. Reservations are never refunded, even on preflight
failure or interruption. This conservative accounting keeps actual requests
below the approved cap when actual usage is unknown. A same-day restart cannot
raise the stored cap; new approval can take effect the following UTC day. There
is no catch-up burst for missed days. The cap is per invocation day: an attempt
crossing midnight stays charged to the day on which it started.

The harness reviewer remains pinned to **grok-4.7**, with the pinned Opus judge,
`registry_off`, one run, and concurrency one. Both request `priority=review`.
The existing loopback relay (`llm-complete.mjs`) pumps interactive requests before
queued review requests and has separate interactive/review lanes. **Gap:** this
is queue priority, not preemption of an active review. A kept conversation may
retain a seat until its review finishes. This recipe does not add preemption or
change the relay. Use a private off-hours trigger and the relay's existing review
lane limits; do not claim that active reviews are forcibly yielded to human work.

## Offline verification

```bash
python3 -m unittest discover -s workflow_recipes/kory_source_nightly/tests -v
# Also verify the real sibling loader, entirely offline:
KORY_SOURCE_CHECKOUT=/absolute/path/to/kory-source \
  uv run --project /absolute/path/to/kory-source python -m unittest discover \
  -s workflow_recipes/kory_source_nightly/tests -v
```

The tests use synthetic PRs and a fake harness; they make no model calls or
GitHub writes. They cover unchanged heads, changed heads, explicit replay,
interrupted runs with and without final artifacts, missed nights, daily cap
persistence including preflight reservations, deferred baselines, pagination,
model pins, and private logs. Real comparisons require separate seat approval.
