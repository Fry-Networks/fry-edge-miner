import { test, expect, nav, assertInAppShell, readStatCard, readBreakdownRow, writeEvidence, vmName, cdpUrl } from './harness'

/**
 * M5 — Rewards vs Dashboard numeric consistency (B22), against the REAL
 * backend inside the Windows guest.
 *
 * CONNECTION: the `page` fixture below comes from harness.ts, which calls
 * Playwright's `chromium.connectOverCDP()` against the guest's forwarded
 * CDP port — 127.0.0.1:9223 for W11, 127.0.0.1:9224 for W10 (the ports
 * ~/femqa/vm/HARNESS.md publishes; the guest side is always :9222, wired
 * by vm-launch-fem's netsh portproxy). See harness.ts's cdpUrl() for the
 * exact FEMQA_VM / FEMQA_CDP_URL selection logic.
 *
 * Anchored to the production server payload captured for this run
 * (GET /versions/FEM?platform=windows): server base_reward 59.52,
 * reward_amount 14.88, stake_tiers { unregistered: 0, none: 1, "24h": 1.5,
 * "6mo": 3 }. IMPORTANT: what the UI shows as "Base reward" / "Full Day
 * Est." is FEM's OWN RewardSummary.base_reward field, which
 * commands/rewards.rs:172-173 sets to the server payload's `reward_amount`
 * (14.88) when config is present — NOT the server payload's own
 * differently-scoped `base_reward` field (59.52). Verified directly
 * against T2/T3's landed rewards.rs (`let base_reward = if config.is_some()
 * { reward_amount } else { ... }`) rather than assumed — the anchor below
 * is 14.88 for exactly this reason, and the arithmetic in B22's own bug
 * report (14.88 × 1.25 × 3.0 = 55.80, 14.88 × 1.40 × 3.0 = 62.50) only
 * checks out with 14.88, never with 59.52. estimated_daily = (this)
 * base_reward * integration_multiplier * stake_multiplier
 * (src-tauri/src/commands/rewards.rs:237, T2/T3-owned, not touched here).
 *
 * UNREGISTERED-DEVICE NOTE: same precondition gap as M4 — AppShell (and
 * therefore both Rewards and Dashboard) requires a registered device.
 * See README.md's "Known blocker". Beyond that gate, this spec does not
 * assume the reward summary has resolved (isSummaryReady, src/lib/
 * rewardReadiness.ts) — an unregistered device may never see live numbers,
 * in which case every StatCard reads "—". Both branches are handled and
 * both branches still prove something: the "—" branch proves the two pages
 * agree on "not ready" (no divergence), and the live branch proves the
 * full arithmetic + copy fixes.
 *
 * PREREQUISITE: vm-launch-fem <vm> '<path to the RC's FEM exe>' has been
 * run and vm-cdp <vm> answers. See README.md.
 */

const DASH = '—'

function parseLeadingNumber(s: string): number | null {
  const m = s.match(/-?\d+(\.\d+)?/)
  return m ? Number(m[0]) : null
}

test.describe('M5 — Rewards vs Dashboard numeric consistency (real backend)', () => {
  test('staking multiplier, daily estimate and boost label agree; duplicated-word copy is gone', async ({ page }) => {
    await assertInAppShell(page)

    await nav(page, 'Dashboard')
    const dailyEstimate = await readStatCard(page, 'Daily Estimate')
    const baseRewardRow = await readBreakdownRow(page, 'Base reward')
    const stakingMultRow = await readBreakdownRow(page, 'Staking mult')
    const requiredPctRow = await readBreakdownRow(page, 'Required proportion')
    const boostRow = await readBreakdownRow(page, 'Boost')

    await nav(page, 'Rewards')
    const fullDayEst = await readStatCard(page, 'Full Day Est.')
    const stakingTier = await readStatCard(page, 'Staking Tier')

    const evidence = {
      vm: vmName(),
      cdpUrl: cdpUrl(),
      dashboard: { dailyEstimate, baseRewardRow, stakingMultRow, requiredPctRow, boostRow },
      rewards: { fullDayEst, stakingTier },
    }
    const evidencePath = writeEvidence('B22-rewards-dashboard-consistency', 'm5-result.json', evidence)
    test.info().annotations.push({ type: 'evidence', description: evidencePath })

    // --- B22 D1: duplicated-word copy check. Independent of whether the
    // summary is ready — this is a pure string check on whatever renders. ---
    expect(stakingTier.sub ?? '', `Staking Tier sub-line (evidence: ${evidencePath})`).not.toMatch(/stake\s+stake/i)
    const bodyText = (await page.locator('body').innerText()).toLowerCase()
    expect(bodyText, `full-page scan for the duplicated-word defect (evidence: ${evidencePath})`).not.toMatch(/stake stake/)

    // --- B22 D3 (settled question): "Staking Tier" (Rewards) and "Staking
    // mult" (Dashboard) are the SAME field — must be string-identical.
    // This is the literal 3.0x-vs-1.0x reported defect. ---
    expect(stakingTier.value, `Staking Tier (Rewards) vs Staking mult (Dashboard) (evidence: ${evidencePath})`).toBe(
      stakingMultRow
    )

    if (dailyEstimate.value === DASH || fullDayEst.value === DASH) {
      // Cold-cache / not-ready branch (isSummaryReady false — plausible on
      // an unregistered device that never resolves stake data). Both pages
      // must agree it is not ready; nothing more to compare.
      expect(dailyEstimate.value, `both pages must show the placeholder together (evidence: ${evidencePath})`).toBe(DASH)
      expect(fullDayEst.value, `both pages must show the placeholder together (evidence: ${evidencePath})`).toBe(DASH)
      test.info().annotations.push({
        type: 'note',
        description: 'Reward summary not ready (both pages show "—") — arithmetic assertions skipped, copy/parity assertions above still applied.',
      })
      return
    }

    // --- Live-numbers branch ---

    // B22 D3: "Full Day Est." (Rewards) is base_reward, unmultiplied — must
    // equal Dashboard's own "Base reward" row.
    const baseRewardNumeric = parseLeadingNumber(baseRewardRow)
    const fullDayEstNumeric = parseLeadingNumber(fullDayEst.value)
    expect(baseRewardNumeric, `Base reward row did not parse as a number: "${baseRewardRow}"`).not.toBeNull()
    expect(fullDayEstNumeric, `Full Day Est. did not parse as a number: "${fullDayEst.value}"`).not.toBeNull()
    expect(fullDayEstNumeric, `Full Day Est. (Rewards) vs Base reward (Dashboard) (evidence: ${evidencePath})`).toBeCloseTo(
      baseRewardNumeric as number,
      2
    )

    // Hard anchor to the captured production payload's reward_amount
    // (14.88 — see the file header for why it's this field, not the
    // server payload's own differently-scoped base_reward 59.52). This is
    // a real requirement, not a soft note: per the lead's design (T1-owned
    // guest side), the in-guest stub serves GET /versions/FEM with this
    // EXACT verbatim payload and production is blackholed via a hosts
    // entry, so there is no legitimate way for this run's QA VM to observe
    // a different value. If it does, that is either a stub wiring problem
    // or a real bug in how FEM reads reward_amount — either way it should
    // fail loudly, not log a note.
    expect(
      baseRewardNumeric,
      `Base reward (${baseRewardRow}) should equal the guest stub's reward_amount, 14.88 (evidence: ${evidencePath})`
    ).toBeCloseTo(14.88, 2)

    // B22 D4 / arithmetic identity: estimated_daily = base_reward *
    // integration_multiplier * stake_multiplier, reconstructed from
    // Dashboard's own visible breakdown (Required proportion + Boost =
    // integration_multiplier; Staking mult = stake_multiplier) and
    // cross-checked against the Daily Estimate StatCard's own value.
    const requiredPct = parseLeadingNumber(requiredPctRow)
    const boostPct = parseLeadingNumber(boostRow)
    const stakeMult = parseLeadingNumber(stakingMultRow)
    const dailyEstimateNumeric = parseLeadingNumber(dailyEstimate.value)
    expect(requiredPct, `Required proportion did not parse: "${requiredPctRow}"`).not.toBeNull()
    expect(boostPct, `Boost did not parse: "${boostRow}"`).not.toBeNull()
    expect(stakeMult, `Staking mult did not parse: "${stakingMultRow}"`).not.toBeNull()
    expect(dailyEstimateNumeric, `Daily Estimate did not parse: "${dailyEstimate.value}"`).not.toBeNull()

    const integrationMultiplier = (requiredPct as number) / 100 + (boostPct as number) / 100
    const reconstructed = (baseRewardNumeric as number) * integrationMultiplier * (stakeMult as number)
    expect(
      dailyEstimateNumeric,
      `Daily Estimate (${dailyEstimate.value}) should equal base_reward(${baseRewardNumeric}) * ` +
        `integration_multiplier(${integrationMultiplier}) * stake_multiplier(${stakeMult}) = ` +
        `${reconstructed.toFixed(2)} (evidence: ${evidencePath})`
    ).toBeCloseTo(reconstructed, 1)

    // Cross-page: Rewards' "Now: … at Y% of full" sub2 line (B22 D3) must
    // name the SAME daily figure the Dashboard StatCard shows.
    if (fullDayEst.sub2) {
      const nowNumeric = parseLeadingNumber(fullDayEst.sub2.replace(/^Now:\s*/, ''))
      expect(nowNumeric, `Rewards sub2 "Now: …" did not parse a number: "${fullDayEst.sub2}"`).not.toBeNull()
      expect(
        nowNumeric,
        `Rewards' "Now:" figure (${fullDayEst.sub2}) vs Dashboard's Daily Estimate (${dailyEstimate.value}) (evidence: ${evidencePath})`
      ).toBeCloseTo(dailyEstimateNumeric as number, 2)
    }
  })
})
