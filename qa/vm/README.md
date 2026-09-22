# FEM QA VM matrix — M4 / M5

Playwright specs that drive the release candidate **inside a real Windows guest**, over CDP,
proving the wiring end to end in a way browser-preview mode (`tests/e2e/**`) structurally cannot.

- `card-tile-parity.spec.ts` — B14: every integration reads the same status on the
  Integrations card and the Dashboard tile.
- `rewards-consistency.spec.ts` — B22: staking multiplier / daily estimate / boost
  label agree between Rewards and Dashboard, anchored to the production server payload
  (`GET /versions/FEM`: server `base_reward` 59.52, `reward_amount` 14.88, `stake_tiers`
  unregistered 0 / none 1 / 24h 1.5 / 6mo 3), and the duplicated-word ("stake stake") copy defect
  is gone. **The UI's own "Base reward" / "Full Day Est." figure is anchored to 14.88, not 59.52**
  — `commands/rewards.rs:172-173` sets FEM's `RewardSummary.base_reward` from the server payload's
  `reward_amount` field when config is present, not from the server payload's own differently-
  scoped `base_reward` field; the spec's header comment has the full trace, verified directly
  against the landed Rust source, not assumed. `estimated_daily = (this) base_reward *
  integration_multiplier * stake_multiplier`.

Both reuse `harness.ts`, which connects to an already-running app over CDP rather than launching
anything itself — see "Prerequisites" below. `harness.ts`'s own connect/context/page mechanics
and its `readStatCard`/`readBreakdownRow` DOM helpers were verified locally against a real CDP
endpoint (a throwaway headless Chromium instance and synthetic markup matching StatCard.tsx's and
Dashboard.tsx's exact JSX shape) before this file was written — see the run's fix-log entry for
this item. **The two specs themselves are UNRUN against the actual VM/RC** — that requires the
release candidate installed in the guest, which does not exist yet at the time this was written.

## AppShell access — RESOLVED design (lead's call), T1-owned guest side

Both specs need `AppShell` (the sidebar + Dashboard/Integrations/Rewards/Settings/Updates pages).
`src/App.tsx` renders the registration **Wizard**, not `AppShell`, whenever `device.registered` is
false, and `device.registered` only flips true after `Wizard`'s `Step3Install` calls the real
`register_device` Tauri command (`src/hooks/useDevice.ts`). There is no skip/dev bypass anywhere
in the wizard or `App.tsx`. The run's hard rule keeps QA VMs unregistered (no real
`register_device` call, no production device record), so this can't be solved by registering one.

The lead's resolution (T1 owns implementing this on the guest side; this worktree does not touch
it): make the app *believe* it is registered without ever telling production anything.

1. Write a synthetic miner key straight into the guest's `%APPDATA%\com.frynetworks.fem\
   fem_config.json` — registration state comes from config, so `AppShell` renders without the
   wizard ever calling `register_device`.
2. A guest `hosts` entry pointing `hardwareapi.frynetworks.com` at `127.0.0.1` — belt-and-braces so
   no code path can reach production even if something calls it unexpectedly.
3. A tiny in-guest stub on that loopback address serving the captured production payload for
   `GET /versions/FEM` (see the anchor numbers above), so M5 runs against real values.

**Discovered while fitting M5 to this design, flagged for T1/the lead — not resolved here:**
`commands/rewards.rs`'s `stake_data_ready` is `verified_present || !has_key`. The synthetic key
in step 1 makes `has_key` true, which *removes* the `!has_key` escape hatch that would otherwise
make an unregistered device's stake data "ready" for free — so `stake_data_ready` now depends
entirely on `verified_present`, which only becomes true once `GET /credentials/{miner_key}
/verified` (`src-tauri/src/api/credentials.rs`) has succeeded at least once. That path is
blackholed by step 2 and is **not** the endpoint step 3 stubs. Net effect: unless the stub (or a
second one) also answers `GET /credentials/{synthetic-key}/verified` — response shape is
`VerifiedStatus { miner_key: string, verified: bool, staked: Option<StakedInfo> }`
(`src-tauri/src/api/types.rs:32-45`); `{ "miner_key": "<the synthetic key>", "verified": true,
"staked": null }` is a reasonable synthetic answer and resolves to the "None"/1.0×/"No stake" tier
per `rewards.rs`'s existing `(Some(tiers), None)` branch — `isSummaryReady` (`config_ready &&
stake_data_ready`) will never become true, and M5 will only ever exercise its cold-cache/placeholder
branch (a real check, but not the full arithmetic proof the anchor numbers above are for). This
spec doesn't need anything else changed to handle either outcome — it branches on whichever
actually happens — but the fuller M5 proof depends on this second endpoint being covered too.

## Prerequisites

1. The RC is installed in the guest and its FEM `.exe` path is known.
2. The VM is up (`vm-reset w11 up` / `vm-reset w10 up`) and SSH-reachable.
3. The app is launched under CDP via the existing harness — **do not invent a launch path**:

   ```bash
   vm-launch-fem w11 'C:\Program Files\Fry Edge Miner\Fry Edge Miner.exe'
   vm-cdp w11   # confirm it answers before running specs
   ```

4. Whatever the "Known blocker" above resolves to (device registered, or AppShell reachable some
   other sanctioned way) has happened, so the app is actually showing `AppShell`.

## Running

From the `fry-edge-miner` worktree root (this file's `../../`):

```bash
# Target w11 (default) or w10:
FEMQA_VM=w11 PLAYWRIGHT_BROWSERS_PATH=/home/fry/.cache/ms-playwright \
  npx playwright test --config=qa/vm/playwright.vm.config.ts qa/vm/card-tile-parity.spec.ts

FEMQA_VM=w11 PLAYWRIGHT_BROWSERS_PATH=/home/fry/.cache/ms-playwright \
  npx playwright test --config=qa/vm/playwright.vm.config.ts qa/vm/rewards-consistency.spec.ts

# Or both:
FEMQA_VM=w11 PLAYWRIGHT_BROWSERS_PATH=/home/fry/.cache/ms-playwright \
  npx playwright test --config=qa/vm/playwright.vm.config.ts qa/vm/

# w10 instead:
FEMQA_VM=w10 PLAYWRIGHT_BROWSERS_PATH=/home/fry/.cache/ms-playwright \
  npx playwright test --config=qa/vm/playwright.vm.config.ts qa/vm/
```

`FEMQA_CDP_URL` overrides the whole endpoint (e.g. for a non-standard port mapping) instead of
deriving it from `FEMQA_VM`.

This is a **separate** Playwright config from `../../playwright.config.ts` (browser-preview mode
against the local vite dev server) — it has no `webServer` and no `baseURL`; navigation happens by
clicking the app's own sidebar (`nav()` in `harness.ts`), not `page.goto()`. Re-runnable for future
releases: repoint `FEMQA_VM`/the exe path and re-run.

## Evidence

Each spec writes its read values (and, on `M4`, every mismatch found) to
`~/femqa/evidence/vm/<matrix>/<vm>/<timestamp>/*.json` via `harness.ts`'s `writeEvidence()`,
matching `vm-shot`/`vm-popups`/etc.'s existing convention (`~/femqa/vm/HARNESS.md` § Evidence) —
`<matrix>` is `B14-card-tile-parity` for M4 and `B22-rewards-dashboard-consistency` for M5. Every
failed `expect()` also names that evidence path in its message.
