import { test, expect, bootApp, nav } from './_shared'

// Matrix G — Updates page.

test.describe('updates', () => {
  test.beforeEach(async ({ page }) => {
    await bootApp(page)
    await nav(page, 'Updates')
    // After loading spinner completes, "Application" section header renders.
    await expect(page.getByText('Application', { exact: true })).toBeVisible({ timeout: 15_000 })
  })

  test('G1 Application + Partner Software section headers', async ({ page }) => {
    await expect(page.getByText('Application', { exact: true })).toBeVisible()
    await expect(page.getByText('Partner Software', { exact: true })).toBeVisible()
  })

  test('G2 Check-for-updates button visible + clickable', async ({ page }) => {
    const btn = page.getByRole('button', { name: /Check for updates|Checking…/ })
    await expect(btn).toBeVisible()
    await btn.click()
    // Optimistic "Checking…" or refresh completes; final state is a "Last checked ..." or "Not checked yet"
    await expect(page.getByText(/Last checked|Not checked yet/).first()).toBeVisible({ timeout: 10_000 })
  })

  test('G3 FEM app card renders with the current version (browser mock)', async ({ page }) => {
    // Browser fallback appUpdate: name='Fry Edge Miner', current=APP_VERSION
    await expect(page.getByText('Fry Edge Miner', { exact: true })).toBeVisible()
  })

  test('G4 all 5 partner integration cards render on the Updates page', async ({ page }) => {
    for (const n of ['Mysterium', 'Presearch', 'Diiisco', 'SpaceAcres', 'Olostep']) {
      await expect(page.getByText(n, { exact: true }).first()).toBeVisible()
    }
  })

  test('G5 footer info line renders', async ({ page }) => {
    await expect(page.getByText(/FEM checks partner software versions/i)).toBeVisible()
  })
})
