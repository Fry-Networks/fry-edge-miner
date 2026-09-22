import { test, expect, bootApp, nav } from './_shared'

// B17 D3 + D5 — a setup-blocked partner (Storj/Iagon/Sentinel waiting on a
// step only the user can do) read "Unhealthy" on the Dashboard tile while
// its card read "SETUP REQUIRED" (D3), and once D1/D2 widened the predicate
// the reason block's gate (`st === 'err'`) would have gone false and hidden
// Sentinel's funding address entirely had D5 not widened it too. Uses the
// browser-preview-only `?intg=<id>:<state>` hint (useIntegrations.ts).
//
// Does not edit any existing spec.

test.describe('setup-required', () => {
  test('card badge, tile status, and Sentinel funding block all agree', async ({ page }) => {
    await page.goto('/?intg=storj:setupRequired&intg=iagon:setupRequired&intg=sentinel:setupRequiredFunding', {
      waitUntil: 'domcontentloaded'
    })
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible({ timeout: 15_000 })

    await nav(page, 'Integrations')
    await expect(page.locator('.ic').first()).toBeVisible({ timeout: 15_000 })

    for (const id of ['storj', 'iagon', 'sentinel']) {
      const cardStatus = (await page.getByTestId(`status-${id}`).textContent())?.trim()
      expect(cardStatus, `card status for ${id}`).toBe('Setup required')
    }

    // Sentinel keeps its funding block — the sent1 address stays visible and
    // copyable in the state that needs it most.
    const fundingBlock = page.getByTestId('sentinel-fund-sentinel')
    await expect(fundingBlock).toBeVisible()
    await expect(fundingBlock).toContainText('sent1qqqqexample')

    // Storj and Iagon show the new guidance line: what to do + a link.
    for (const [id, urlLabel] of [
      ['storj', 'storj.io'],
      ['iagon', 'app.iagon.com']
    ] as const) {
      const card = page.locator('.ic').filter({ has: page.getByTestId(`status-${id}`) })
      await expect(card.getByRole('note')).toBeVisible()
      const link = card.getByRole('link', { name: urlLabel })
      await expect(link).toBeVisible()
    }

    await nav(page, 'Dashboard')
    for (const id of ['storj', 'iagon', 'sentinel']) {
      const tileStatus = (await page.getByTestId(`tile-status-${id}`).textContent())?.trim()
      expect(tileStatus, `tile status for ${id}`).toBe('Setup required')
    }

    // D4: a setup-blocked integration must not paint the sidebar amber
    // (Sidebar.tsx: color is var(--amb) #f0a500 when hasUnhealthy, else
    // var(--teal) #00c49a). All three fixtures above are setup-blocked, not
    // genuine failures, so this must read teal.
    // Sidebar.tsx's own counter span, not Dashboard's unrelated "Required
    // active" breakdown-row label — match the "N/M required active" shape.
    await expect(page.getByText(/^\d+\/\d+ required active$/)).toHaveCSS('color', 'rgb(0, 196, 154)')
  })
})
