import { test, expect, bootApp, nav } from './_shared'

// B18 D3 — the card's consent badge was fetched only on mount and never
// re-checked, so it could go stale and contradict the health line (the
// reported screenshot: "Consent active" beside "needs your consent").
// consentBadge() now checks the health reason first. Browser-preview mode
// cannot reproduce the exact reported combination (stale active:true — the
// consentActive flag is backend-driven and check_consent is a no-op without
// Tauri, so it is always unknown/null here); this proves the more general
// property the fix guarantees: the badge is never absent or wrong when
// health already says consent is required, regardless of consentActive.
//
// Uses the browser-preview-only `?intg=<id>:<state>` hint (useIntegrations.ts).

test.describe('pawns-consent-badge', () => {
  test('the consent badge agrees with the health reason', async ({ page }) => {
    await page.goto('/?intg=pawns:needsConsent', { waitUntil: 'domcontentloaded' })
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible({ timeout: 15_000 })

    await nav(page, 'Integrations')
    await expect(page.locator('.ic').first()).toBeVisible({ timeout: 15_000 })

    const consentBadge = page.getByTestId('consent-pawns')
    await expect(consentBadge).toBeVisible()
    await expect(consentBadge).toContainText('Consent required')
    await expect(consentBadge).not.toContainText('Consent active')
  })
})
