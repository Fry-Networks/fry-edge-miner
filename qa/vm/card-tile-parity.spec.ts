import { test, expect, nav, assertInAppShell, writeEvidence, vmName, cdpUrl } from './harness'

/**
 * M4 (this item's slice of the VM matrix) — card == tile parity for every
 * integration, against the REAL backend inside the Windows guest.
 *
 * CONNECTION: the `page` fixture below comes from harness.ts, which calls
 * Playwright's `chromium.connectOverCDP()` against the guest's forwarded
 * CDP port — 127.0.0.1:9223 for W11, 127.0.0.1:9224 for W10 (the ports
 * ~/femqa/vm/HARNESS.md publishes; the guest side is always :9222, wired
 * by vm-launch-fem's netsh portproxy). See harness.ts's cdpUrl() for the
 * exact FEMQA_VM / FEMQA_CDP_URL selection logic.
 *
 * tests/e2e/card-tile-parity.spec.ts (browser-preview mode) already proves
 * the pure function (integrationBadge()) is correct for every state it can
 * synthesize via the `?intg=<id>:<state>` hint. That is necessary but not
 * sufficient: it cannot prove the WIRING end to end — that
 * useIntegrations.ts's real IPC poll, the real backend's health/lifecycle
 * reporting, and the real Integrations/Dashboard pages agree with each
 * other for whatever states the actual partner processes are actually in.
 * This spec is that end-to-end proof; it reuses the same data-testid hooks
 * (status-<id> on the Integrations card, tile-status-<id> on the Dashboard
 * tile — see src/components/IntCard.tsx and src/pages/Dashboard.tsx) so a
 * mismatch here can only be a real wiring bug, not a selector drift.
 *
 * UNREGISTERED-DEVICE NOTE: reaching the Integrations/Dashboard pages at
 * all requires AppShell, which requires a registered device (App.tsx —
 * !registered renders the Wizard, not AppShell). See README.md's "Known
 * blocker" section — flagged to the lead, not resolved here. This spec
 * does not itself register anything; assertInAppShell() fails fast with a
 * clear message if the precondition isn't met, rather than a confusing
 * "no such element" timeout.
 *
 * PREREQUISITE: vm-launch-fem <vm> '<path to the RC's FEM exe>' has been
 * run and vm-cdp <vm> answers. See README.md.
 */

test.describe('M4 — card/tile parity (real backend)', () => {
  test('every integration reads the same status on the Integrations card and the Dashboard tile', async ({ page }) => {
    await assertInAppShell(page)

    await nav(page, 'Integrations')
    // Discover the live id set from the real backend rather than assuming
    // a fixed list — a real VM may have a different partner set than the
    // browser-preview mock's INTEGRATION_META.
    await page.locator('[data-testid^="status-"]').first().waitFor({ timeout: 15_000 })
    const statusTestIds = await page.locator('[data-testid^="status-"]').evaluateAll((els) =>
      els.map((el) => el.getAttribute('data-testid') ?? '')
    )
    const ids = statusTestIds
      .map((t) => t.replace(/^status-/, ''))
      .filter((id) => id.length > 0)

    expect(ids.length, 'no integration cards found — is the Integrations page actually populated?').toBeGreaterThan(0)

    const cardLabels: Record<string, string> = {}
    for (const id of ids) {
      const text = await page.getByTestId(`status-${id}`).textContent()
      cardLabels[id] = (text ?? '').trim()
    }

    await nav(page, 'Dashboard')

    const mismatches: { id: string; card: string; tile: string }[] = []
    const tileLabels: Record<string, string> = {}
    for (const id of ids) {
      const tileEl = page.getByTestId(`tile-status-${id}`)
      const present = await tileEl.isVisible({ timeout: 5_000 }).catch(() => false)
      if (!present) {
        // A community/SDK integration that isn't in either required/partner
        // grid section for some reason, or genuinely not rendered as a
        // tile — record it as evidence rather than silently skipping.
        tileLabels[id] = '(tile not found)'
        mismatches.push({ id, card: cardLabels[id], tile: '(tile not found)' })
        continue
      }
      const text = (await tileEl.textContent()) ?? ''
      tileLabels[id] = text.trim()
      if (tileLabels[id] !== cardLabels[id]) {
        mismatches.push({ id, card: cardLabels[id], tile: tileLabels[id] })
      }
    }

    const evidencePath = writeEvidence('B14-card-tile-parity', 'm4-result.json', {
      vm: vmName(),
      cdpUrl: cdpUrl(),
      ids,
      cardLabels,
      tileLabels,
      mismatches,
    })
    test.info().annotations.push({ type: 'evidence', description: evidencePath })

    expect(mismatches, `card/tile mismatches (evidence: ${evidencePath}):\n${JSON.stringify(mismatches, null, 2)}`).toEqual([])
  })
})
