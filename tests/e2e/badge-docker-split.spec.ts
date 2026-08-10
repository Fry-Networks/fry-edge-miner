import { test, expect, bootApp } from './_shared'

// Matrix D-S — v0.4.8 badge/docker split (fix for the v0.4.7 field reports
// where a stopped Docker Desktop showed a permanent "Degraded" badge and
// users read it as a broken connection to Fry).
// Uses the browser-only ?docker=<kind> hint in useIntegrations.fetchSystem.

test.describe('badge-docker-split', () => {
  test('D-S1 docker daemon_stopped: badge stays Connected, Docker chip shown', async ({ page }) => {
    await page.goto('/?docker=daemon_stopped', { waitUntil: 'domcontentloaded' })
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible({ timeout: 15_000 })

    await expect(page.getByText('Connected', { exact: true })).toBeVisible()
    await expect(page.getByText('Degraded', { exact: true })).not.toBeVisible()

    const chip = page.getByTestId('docker-chip')
    await expect(chip).toBeVisible()
    await expect(chip).toHaveText('Docker stopped')
  })

  test('D-S2 docker not_installed: badge stays Connected, chip labels install state', async ({ page }) => {
    await page.goto('/?docker=not_installed', { waitUntil: 'domcontentloaded' })
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible({ timeout: 15_000 })

    await expect(page.getByText('Connected', { exact: true })).toBeVisible()
    await expect(page.getByTestId('docker-chip')).toHaveText('Docker not installed')
  })

  test('D-S3 default browser mode: Connected, no Docker chip', async ({ page }) => {
    await bootApp(page)
    await expect(page.getByText('Connected', { exact: true })).toBeVisible()
    await expect(page.getByTestId('docker-chip')).toHaveCount(0)
  })

  test('D-S4 docker ready via hint: Connected, no chip', async ({ page }) => {
    await page.goto('/?docker=ready', { waitUntil: 'domcontentloaded' })
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('Connected', { exact: true })).toBeVisible()
    await expect(page.getByTestId('docker-chip')).toHaveCount(0)
  })
})
