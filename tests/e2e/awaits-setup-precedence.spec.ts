import { test, expect, bootApp, nav } from './_shared'

// Cross-team fix requested by T3 (via lead) for B7 D4 / B8 D3-D4: on v0.4.33
// (before any B14/B17 work on this branch) an amber "Setup required" card
// could render with NO body text — hiding the funding amount/address, and
// (independently) the Storj setup instruction — in two situations:
//
//   1. A stale `error` from an earlier failed toggle attempt outranked a
//      live awaitsUserSetup(health) reason (IntCard.tsx's startError gate).
//   2. The reason-explanation render gate could, in a ladder-precedence
//      edge case, stay closed even though awaitsUserSetup(health) was true.
//
// B17 D5 (earlier on this branch) already closed the Storj case for the
// no-stale-error path; test 2 below is a non-regression control for that,
// not new evidence. Test 1 is the new fix: it fails RED without today's
// change (confirmed against commit 346c018, the branch tip immediately
// before this fix).
//
// Uses the browser-preview-only `?intg=<id>:<state>` hint (useIntegrations.ts).

test.describe('awaits-setup-precedence', () => {
  test('a live awaiting-setup reason still shows its body text despite a stale start error', async ({ page }) => {
    await page.goto('/?intg=fryvpn:setupRequiredWithStaleError', { waitUntil: 'domcontentloaded' })
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible({ timeout: 15_000 })

    await nav(page, 'Integrations')
    await expect(page.locator('.ic').first()).toBeVisible({ timeout: 15_000 })

    // Badge still reads Setup required (the stale error must not flip it).
    const status = await page.getByTestId('status-fryvpn').textContent()
    expect(status?.trim()).toBe('Setup required')

    // The funding guidance body text must be visible — this is exactly what
    // was hidden before the fix (startError used to win).
    const card = page.locator('.ic').filter({ has: page.getByTestId('status-fryvpn') })
    await expect(card.getByText(/Awaiting fryDVPN funding/)).toBeVisible()

    // The stale error must NOT also be shown (it is suppressed, not merely
    // deprioritized) — there is exactly one explanation on the card.
    await expect(card.getByText(/a previous attempt timed out/)).toHaveCount(0)
  })

  test('non-regression control: Storj\'s setup instruction (no stale error involved) still renders', async ({ page }) => {
    await page.goto('/?intg=storj:setupRequired', { waitUntil: 'domcontentloaded' })
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible({ timeout: 15_000 })

    await nav(page, 'Integrations')
    const card = page.locator('.ic').filter({ has: page.getByTestId('status-storj') })
    await expect(card.getByRole('note')).toBeVisible()
  })
})
