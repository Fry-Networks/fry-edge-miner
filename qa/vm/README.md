# FEM QA VM matrix — M4 / M5

Playwright specs that drive the release candidate **inside a real Windows guest**, over CDP,
proving the wiring end to end in a way browser-preview mode (`tests/e2e/**`) structurally cannot.

- `m4-card-tile-parity.spec.ts` — B14: every integration reads the same status on the
  Integrations card and the Dashboard tile.
- `m5-rewards-dashboard-consistency.spec.ts` — B22: staking multiplier / daily estimate / boost
  label agree between Rewards and Dashboard, anchored to the production server payload
  (`base_reward` 59.52, `reward_amount` 14.88, `stake_tiers` unregistered 0 / none 1 / 24h 1.5 /
  6mo 3, `estimated_daily = base_reward * integration_multiplier * stake_multiplier`), and the
  duplicated-word ("stake stake") copy defect is gone.

Both reuse `harness.ts`, which connects to an already-running app over CDP rather than launching
anything itself — see "Prerequisites" below. `harness.ts`'s own connect/context/page mechanics
and its `readStatCard`/`readBreakdownRow` DOM helpers were verified locally against a real CDP
endpoint (a throwaway headless Chromium instance and synthetic markup matching StatCard.tsx's and
Dashboard.tsx's exact JSX shape) before this file was written — see the run's fix-log entry for
this item. **The two specs themselves are UNRUN against the actual VM/RC** — that requires the
release candidate installed in the guest, which does not exist yet at the time this was written.

## Known blocker — flagged to the lead, not resolved here

Both specs need `AppShell` (the sidebar + Dashboard/Integrations/Rewards/Settings/Updates pages).
`src/App.tsx` renders the registration **Wizard**, not `AppShell`, whenever
`device.registered` is false, and `device.registered` only flips true after `Wizard`'s
`Step3Install` calls the real `register_device` Tauri command (`src/hooks/useDevice.ts`). There is
no skip/dev bypass anywhere in the wizard or `App.tsx` (checked).

The run's own hard rule and `runlog/fix-log.md`'s R8 decision are explicit that QA VM instances
stay **unregistered** — no miner_key, no device record created. Taken literally, that means
`AppShell` — and therefore both `M4` and `M5` — is unreachable by the normal UI flow on these
guests. `harness.ts`'s `assertInAppShell()` fails fast with this explanation instead of a
confusing "no such element" timeout if run against a device still on the Wizard.

**This spec does not register a device and will not without the lead's sign-off** (per this run's
explicit instruction: "if a specific assertion genuinely needs a registered device, do not
register one — flag it to me and I will decide"). Options for the lead, none exercised here:

1. Approve a single throwaway QA-only device registration on one snapshot, used only to reach
   `AppShell` for this matrix (does it accrue real PoC/reward records the way R8 worried about? —
   needs confirming before doing this).
2. A dev/test bypass in the app itself that shows `AppShell` without a real `register_device` call
   — would be a new frontend change, out of this item's fence unless approved.
3. Some other sanctioned path the lead already has in mind.

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
  npx playwright test --config=qa/vm/playwright.vm.config.ts qa/vm/m4-card-tile-parity.spec.ts

FEMQA_VM=w11 PLAYWRIGHT_BROWSERS_PATH=/home/fry/.cache/ms-playwright \
  npx playwright test --config=qa/vm/playwright.vm.config.ts qa/vm/m5-rewards-dashboard-consistency.spec.ts

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
