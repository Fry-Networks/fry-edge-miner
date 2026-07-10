import { test, expect, bootApp, nav, NAV_LABELS } from './_shared'

// Matrix I — cross-cutting concerns: console, network, responsive, contrast, keyboard.

// Filter allows the known Tauri-IPC failures triggered by browser-mode hooks
// (they all invoke get_settings / get_reward_summary / get_poc_slots and log
// via console.warn — but the @tauri-apps/api may surface as console.error too).
const KNOWN_IPC_NOISE =
  /window\.__TAURI|get_device_info|get_integrations|get_system_status|get_settings|save_settings|get_reward_summary|get_poc_slots|check_updates|install_update|register_device|deregister_device|toggle_integration/i

test.describe('cross-cutting', () => {
  test('I1+I2 no unexpected console / network errors across all 5 pages', async ({ page, errors }) => {
    await bootApp(page)
    for (const label of NAV_LABELS) {
      await nav(page, label)
      await page.waitForTimeout(500) // let effects flush
    }
    const unexpected = errors.filter((e) => !KNOWN_IPC_NOISE.test(e))
    expect(unexpected, `unexpected errors:\n${unexpected.join('\n')}`).toEqual([])
  })

  test('I3 responsive at 1920x1080, 1366x768, 1024x768 — no horizontal scroll on Dashboard', async ({ page }) => {
    await bootApp(page)
    for (const [w, h] of [[1920, 1080], [1366, 768], [1024, 768]] as const) {
      await page.setViewportSize({ width: w, height: h })
      await nav(page, 'Dashboard')
      // Body should not overflow horizontally
      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth
      )
      expect(overflow, `${w}x${h}: horizontal overflow=${overflow}px`).toBeLessThanOrEqual(2)
    }
  })

  test('I4 dark theme — Sidebar background is not white', async ({ page }) => {
    await bootApp(page)
    // Sidebar has `background: 'var(--s0)'` — must resolve to non-white RGB.
    const sidebarBg = await page.evaluate(() => {
      const buttons = Array.from(document.querySelectorAll('button'))
      const dashboardBtn = buttons.find((b) => b.textContent?.trim() === 'Dashboard')
      if (!dashboardBtn) return null
      const sidebar = dashboardBtn.closest('div[style*="width: 216px"]')
      if (!sidebar) return null
      return getComputedStyle(sidebar).backgroundColor
    })
    expect(sidebarBg, `sidebar bg=${sidebarBg}`).not.toBeNull()
    expect(sidebarBg).not.toBe('rgb(255, 255, 255)')
    // Parse rgb and verify each channel is dark (< 60).
    const m = sidebarBg?.match(/rgb\((\d+),\s*(\d+),\s*(\d+)/)
    if (m) {
      const [r, g, b] = [Number(m[1]), Number(m[2]), Number(m[3])]
      expect(r + g + b, `sidebar rgb=${sidebarBg} sum=${r + g + b}`).toBeLessThan(180)
    }
  })

  test('I5 refresh (F5) on each page recovers app-shell state', async ({ page }) => {
    await bootApp(page)
    for (const label of NAV_LABELS) {
      await nav(page, label)
      await page.reload({ waitUntil: 'domcontentloaded' })
      // App always resets to Dashboard on reload — App.tsx `page` state defaults to 'dashboard'
      await expect(page.getByText('Active Integrations', { exact: true })).toBeVisible({ timeout: 15_000 })
    }
  })

  test('I6 invalid URL query does not break app shell', async ({ page }) => {
    await page.goto('/?bogus=1&foo=bar', { waitUntil: 'domcontentloaded' })
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible({ timeout: 15_000 })
  })

  test('I9 keyboard tab navigation reaches all 5 nav buttons', async ({ page }) => {
    await bootApp(page)
    // Tab through elements; sidebar buttons should be focusable.
    const focused: string[] = []
    for (let i = 0; i < 30 && focused.length < 5; i++) {
      await page.keyboard.press('Tab')
      const label = await page.evaluate(() => {
        const el = document.activeElement as HTMLElement | null
        return el?.textContent?.trim() || ''
      })
      if (['Dashboard', 'Integrations', 'Rewards', 'Settings', 'Updates'].includes(label) && !focused.includes(label)) {
        focused.push(label)
      }
    }
    expect(focused, `focused set: ${focused.join(', ')}`).toEqual(
      expect.arrayContaining(['Dashboard', 'Integrations', 'Rewards', 'Settings', 'Updates'])
    )
  })
})
