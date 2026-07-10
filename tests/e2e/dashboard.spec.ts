import { test, expect, bootApp, nav } from './_shared'

// Matrix C — Dashboard page.

test.describe('dashboard', () => {
  test.beforeEach(async ({ page }) => {
    await bootApp(page)
  })

  test('C1 three StatCards render with labels', async ({ page }) => {
    await expect(page.getByText('Active Integrations', { exact: true })).toBeVisible()
    await expect(page.getByText('Daily Estimate', { exact: true })).toBeVisible()
    await expect(page.getByText('PoC Score', { exact: true })).toBeVisible()
  })

  test('C2 StatCard values are non-empty placeholders', async ({ page }) => {
    // Active Integrations should render "N / 5" (browser mock: enabled=false → 0/5)
    await expect(page.getByText(/^\d+\s*\/\s*5$/).first()).toBeVisible()
    // Daily Estimate renders a number
    await expect(page.getByText(/^\d+\.\d{2}$/).first()).toBeVisible()
    // PoC Score renders a fraction like 0.000
    await expect(page.getByText(/^\d\.\d{3}$/).first()).toBeVisible()
  })

  test('C3 integration health overview section renders', async ({ page }) => {
    await expect(page.getByText('Integration Status', { exact: true })).toBeVisible()
  })

  test('C4 reward breakdown card renders with all label rows', async ({ page }) => {
    await expect(page.getByText('Reward Breakdown', { exact: true })).toBeVisible()
    await expect(page.getByText('Base reward', { exact: true })).toBeVisible()
    await expect(page.getByText('Staking mult', { exact: true })).toBeVisible()
    await expect(page.getByText('BYOD factor', { exact: true })).toBeVisible()
    await expect(page.getByText('Yesterday', { exact: true })).toBeVisible()
  })

  test('C5 PoC Slots card renders 144-slot grid + hour labels', async ({ page }) => {
    await expect(page.getByText('PoC Slots', { exact: true })).toBeVisible()
    await expect(page.getByText('Today · 144 slots', { exact: true })).toBeVisible()
    // Hour tick labels
    for (const t of ['00:00', '06:00', '12:00', '18:00', '23:50']) {
      await expect(page.getByText(t, { exact: true })).toBeVisible()
    }
  })

  test('C6 slot-hits text renders as N/144', async ({ page }) => {
    await expect(page.getByText(/^\d+\/144$/).first()).toBeVisible()
  })

  test('C7 no browser errors while dashboard rendered', async ({ page, errors }) => {
    await nav(page, 'Integrations')
    await nav(page, 'Dashboard')
    await expect(page.getByText('Active Integrations', { exact: true })).toBeVisible()
    expect(errors, `dashboard errors:\n${errors.join('\n')}`).toEqual([])
  })
})
