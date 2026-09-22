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
 * (GET /versions/FEM?platform=windows): base_reward 59.52, reward_amount
 * 14.88, stake_tiers { unregistered: 0, none: 1, "24h": 1.5, "6mo": 3 },
 * and the backend formula estimated_daily = base_reward *
 * integration_multiplier * stake_multiplier
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

    // Anchor to the captured production payload, when it's the live value
    // (a QA VM's actual server config may differ — record and only assert
    // equality when it matches the anchor, otherwise record the observed
    // triple instead of failing on an environment difference the Done-when
    // doesn't require).
    if (Math.abs((baseRewardNumeric as number) - 59.52) < 0.005) {
      test.info().annotations.push({ type: 'note', description: 'base_reward matches the captured production anchor (59.52).' })
    } else {
      test.info().annotations.push({
        type: 'note',
        description: `base_reward observed as ${baseRewardNumeric}, not the captured anchor 59.52 — recorded, not failed (server config may legitimately differ for this run).`,
      })
    }

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
