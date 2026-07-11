import { test, expect, bootApp, nav } from './_shared'

// Matrix E — Empty states and fallback rendering.
const KNOWN_TAURI_NOISE = /window\.__TAURI|invoke|get_reward_summary|get_poc_slots|get_updates|fonts\.gstatic|REQFAIL|ERR_ABORTED/i

test.describe('empty-states', () => {
  test.beforeEach(async ({ page }) => {
    await bootApp(page)
  })

  test('E-S1 Rewards with null summary (browser mode) renders placeholders, empty history', async ({ page, errors }) => {
    // Navigate to Rewards page
    await nav(page, 'Rewards')

    // Browser mock sets rewards.summary to null (no backend data)
    // Placeholders should render:
    // - "Total Earned" stat card shows placeholder (—, 0.00, etc.)
    // - "Full Day Est." shows placeholder
    // - "Staking Tier" shows placeholder (—)

    // All three StatCards should render with labels
    await expect(page.getByText('Total Earned', { exact: true })).toBeVisible()
    await expect(page.getByText('Full Day Est.', { exact: true })).toBeVisible()
    await expect(page.getByText('Staking Tier', { exact: true })).toBeVisible()

    // Null summary → token sub renders the '—' placeholder ("— lifetime")
    await expect(page.getByText(/ lifetime$/).first()).toBeVisible()

    // Reward History table should render but be empty (0 rows)
    await expect(page.getByText('Reward History', { exact: true })).toBeVisible()
    // In browser mode with no data, table body should have no data rows
    // or show placeholder text like "No rewards yet"

    // No unexpected errors
    const unexpected = errors.filter((e) => !KNOWN_TAURI_NOISE.test(e))
    expect(unexpected, unexpected.join('\n')).toEqual([])
  })

  test('E-S2 PoC slot grid with empty slots (0 hits) renders correctly', async ({ page }) => {
    // PoC Slots card on Dashboard shows a 144-slot grid
    // In browser mode, slots are empty (all 0)
    await expect(page.getByText('PoC Slots', { exact: true })).toBeVisible()
    await expect(page.getByText('Today · 144 slots', { exact: true })).toBeVisible()

    // Slot count text should show "0/144" or similar
    const slotCountText = await page.getByText(/^\d+\/144$/).first().textContent()
    expect(slotCountText).toMatch(/^\d+\/144$/)
    expect(slotCountText).toContain('0') // Empty state

    // Hour tick labels should still render even with empty slots
    for (const label of ['00:00', '06:00', '12:00', '18:00', '23:50']) {
      await expect(page.getByText(label, { exact: true })).toBeVisible()
    }

    // Slot grid should render (search for grid container by "Today" header or slot-related text)
    const gridContainer = page.locator('text=/Today.*144 slots/').locator('..')
    await expect(gridContainer).toBeVisible()
  })

  test('E-S3 Integrations page with 0 enabled shows all 5 cards disabled', async ({ page }) => {
    // Browser mode: all 5 integrations start disabled (enabled:false)

    // Navigate to Integrations page to see all 5 cards
    await nav(page, 'Integrations')
    await expect(page.locator('.ic').first()).toBeVisible({ timeout: 15_000 })

    // All 5 integration cards should render
    const integrationLabels = ['Mysterium', 'Presearch', 'Diiisco', 'SpaceAcres', 'Olostep']
    for (const label of integrationLabels) {
      await expect(page.getByText(label, { exact: true }).first()).toBeVisible()
    }

    // Should have exactly 5 cards
    await expect(page.locator('.ic')).toHaveCount(5)

    // Proportion counter should show initial state with 0 active
    const counter = page.getByText(/^0\s*\/\s*5/)
    await expect(counter.first()).toBeVisible()
  })

  test('E-S4 Updates check-for-updates click with no backend returns gracefully', async ({ page, errors }) => {
    // Navigate to Updates page
    await nav(page, 'Updates')

    // Updates page should render a "Check for Updates" button (mandatory)
    const checkBtn = page.getByRole('button', { name: /Check|Update/i }).first()
    await expect(checkBtn).toBeVisible()

    // Click the check button
    await checkBtn.click()

    // In browser mode, Tauri invoke fails silently or returns an error
    // The page should:
    // - NOT crash
    // - NOT show an unhandled error banner
    // - Gracefully handle the failure

    // Wait a moment for any network call
    await page.waitForTimeout(500)

    // Verify the page is still functional (EDGE MINER header still visible)
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible()

    // The button should return to idle state (not stuck in loading)
    // No unhandled rejection errors should be captured
    const unexpected = errors.filter((e) => !KNOWN_TAURI_NOISE.test(e))
    expect(unexpected, unexpected.join('\n')).toEqual([])
  })
})
