import { test, expect, bootApp, nav, NAV_LABELS } from './_shared'

// Matrix X — Cross-page consistency. Data should match across nav.
const KNOWN_TAURI_NOISE = /window\.__TAURI|invoke|get_device_info|get_reward_summary|get_poc_slots|check_updates|get_integrations|fonts\.gstatic|REQFAIL|ERR_ABORTED/i

test.describe('cross-page-consistency', () => {
  test.beforeEach(async ({ page }) => {
    await bootApp(page)
  })

  test('X-C1 integration active count consistent across Sidebar, Dashboard, Integrations pages', async ({ page }) => {
    // Sidebar shows "N/5 active" in the badge
    const sidebarBadge = page.getByText(/\d+\/5 active/)
    await expect(sidebarBadge).toBeVisible()
    const sidebarText = await sidebarBadge.textContent()
    const sidebarMatch = sidebarText?.match(/(\d+)\/5/)
    const sidebarCount = sidebarMatch ? parseInt(sidebarMatch[1], 10) : 0

    // Dashboard shows "Active Integrations" stat with "N / 5"
    await expect(page.getByText('Active Integrations', { exact: true })).toBeVisible()
    const dashboardStat = page.getByText(/^\d+\s*\/\s*5$/).first()
    const dashboardText = await dashboardStat.textContent()
    const dashboardMatch = dashboardText?.match(/(\d+)/)
    const dashboardCount = dashboardMatch ? parseInt(dashboardMatch[1], 10) : 0

    // Integrations page shows proportion counter "N / 5 · M%"
    await nav(page, 'Integrations')
    await expect(page.locator('.ic').first()).toBeVisible()
    const intCounterText = await page.getByText(/^\d+\s*\/\s*5/).first().textContent()
    const intMatch = intCounterText?.match(/(\d+)/)
    const intCount = intMatch ? parseInt(intMatch[1], 10) : 0

    // All three should match
    expect(sidebarCount).toBe(dashboardCount)
    expect(dashboardCount).toBe(intCount)
  })

  test('X-C2 sidebar truncated miner key derives from the device-state key', async ({ page }) => {
    // Browser mock key is 'FEM-BROWSER-PREVIEW-MODE' (useDevice.ts). Sidebar
    // truncates via truncateMinerKey: FEM- + first4.lower + … + last4.lower.
    // Full-key equality vs Settings CopyField is a desktop-only check —
    // browser mode has no get_config IPC, so Settings renders an empty field.
    const mockKey = 'FEM-BROWSER-PREVIEW-MODE'
    const body = mockKey.slice(4)
    const expected = `FEM-${body.slice(0, 4).toLowerCase()}…${body.slice(-4).toLowerCase()}`
    await expect(page.getByText(expected, { exact: true })).toBeVisible()

    // Settings still renders the Miner Key section (field itself empty in browser mode)
    await nav(page, 'Settings')
    await expect(page.getByText('Device configuration and preferences', { exact: true })).toBeVisible()
    await expect(page.getByText('Miner Key', { exact: true })).toBeVisible()
  })

  test('X-C3 device name identical in Sidebar and Settings content', async ({ page }) => {
    // Default device name prop is 'nimble-swift-wolf' in both components.
    // On Settings, the name must appear twice: sidebar footer + settings body.
    const sidebarDeviceName = 'nimble-swift-wolf'
    await expect(page.getByText(sidebarDeviceName, { exact: true })).toHaveCount(1)

    await nav(page, 'Settings')
    await expect(page.getByText('Device configuration and preferences', { exact: true })).toBeVisible()
    await expect(page.getByText(sidebarDeviceName, { exact: true })).toHaveCount(2)
  })

  test('X-C4 reward token name identical on Dashboard and Rewards cards', async ({ page }) => {
    // Dashboard Daily Estimate sub renders `${token} (ASA ${asa})`;
    // Rewards Total Earned sub renders `${token} lifetime`. Extract and compare.
    await expect(page.getByText('Daily Estimate', { exact: true })).toBeVisible()
    const dashSub = await page.getByText(/\(ASA .*\)$/).first().textContent()
    const dashToken = dashSub?.replace(/\s*\(ASA .*\)$/, '').trim()
    expect(dashToken, `unexpected dashboard sub: ${dashSub}`).toBeTruthy()

    await nav(page, 'Rewards')
    await expect(page.getByText('Total Earned', { exact: true })).toBeVisible()
    const rewSub = await page.getByText(/ lifetime$/).first().textContent()
    const rewToken = rewSub?.replace(/\s*lifetime$/, '').trim()

    expect(rewToken).toBe(dashToken)
  })

  test('X-C5 F5 reload returns app to functional state', async ({ page, errors }) => {
    // Start on Dashboard (bootApp defaults here)
    await expect(page.getByText('Active Integrations', { exact: true })).toBeVisible()

    // Navigate to another page first
    await nav(page, 'Rewards')
    await expect(page.getByText('Total Earned', { exact: true })).toBeVisible()

    // F5 refresh (should return to app and either Dashboard or current page)
    await page.reload({ waitUntil: 'domcontentloaded' })

    // Verify app is functional after reload (EDGE MINER header should be visible)
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible({ timeout: 20_000 })

    // Sidebar should be visible and functioning
    await expect(page.getByRole('button', { name: 'Dashboard' })).toBeVisible()

    const unexpected = errors.filter((e) => !KNOWN_TAURI_NOISE.test(e))
    expect(unexpected, unexpected.join('\n')).toEqual([])
  })
})
