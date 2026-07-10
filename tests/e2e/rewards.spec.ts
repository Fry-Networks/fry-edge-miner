import { test, expect, bootApp, nav } from './_shared'

// Matrix E — Rewards page.

test.describe('rewards', () => {
  test.beforeEach(async ({ page }) => {
    await bootApp(page)
    await nav(page, 'Rewards')
    await expect(page.getByText('Reward History', { exact: true })).toBeVisible({ timeout: 15_000 })
  })

  test('E1 three StatCards render (Total Earned, Full Day Est., Staking Tier)', async ({ page }) => {
    await expect(page.getByText('Total Earned', { exact: true })).toBeVisible()
    await expect(page.getByText('Full Day Est.', { exact: true })).toBeVisible()
    await expect(page.getByText('Staking Tier', { exact: true })).toBeVisible()
  })

  test('E2 Reward History table renders with all 5 column headers', async ({ page }) => {
    for (const h of ['Date', 'Reward', 'Slots', 'Factor', 'Status']) {
      await expect(page.getByText(h, { exact: true }).first()).toBeVisible()
    }
  })

  test('E3 Epoch badge shows "Epoch 1 live"', async ({ page }) => {
    await expect(page.getByText('Epoch 1 live', { exact: true })).toBeVisible()
  })

  test('E4 PoC heatmap section renders gate labels', async ({ page }) => {
    await expect(page.getByText('PoC Heatmap — 6 gates × 24 hours', { exact: true })).toBeVisible()
    // 6 gates: data, online, mac, pol, poi, poa (per src/lib/integrationMeta.ts GATES)
    for (const g of ['data', 'online', 'mac', 'pol', 'poi', 'poa']) {
      // "mac" appears in heatmap left column labels + in the gate list summary line
      await expect(page.getByText(g, { exact: true }).first()).toBeVisible()
    }
  })

  test('E5 heatmap hour ticks render at 00h, 06h, 12h, 18h', async ({ page }) => {
    for (const t of ['00h', '06h', '12h', '18h']) {
      await expect(page.getByText(t, { exact: true })).toBeVisible()
    }
  })

  test('E6 legend renders pass + pending', async ({ page }) => {
    await expect(page.getByText('pass', { exact: true })).toBeVisible()
    await expect(page.getByText('pending', { exact: true })).toBeVisible()
  })

  test('E7 no unexpected browser errors on rewards', async ({ page, errors }) => {
    // Same tolerance for expected Tauri IPC fails from useRewards/useDevice
    const unexpected = errors.filter(
      (e) => !/window\.__TAURI|get_reward_summary|get_poc_slots|get_device_info|get_integrations|get_system_status/i.test(e)
    )
    expect(unexpected, `rewards errors:\n${unexpected.join('\n')}`).toEqual([])
  })
})
