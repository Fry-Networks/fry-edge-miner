import { test, expect, bootApp, NAV_LABELS } from './_shared'

test.describe('smoke — dev server + browser mode boot', () => {
  test('app shell renders without console/network errors', async ({ page, errors }) => {
    await bootApp(page)
    // The 5 sidebar nav buttons render
    for (const label of NAV_LABELS) {
      await expect(page.getByRole('button', { name: label })).toBeVisible()
    }
    // FRY / EDGE MINER brand
    await expect(page.getByText('FRY', { exact: true })).toBeVisible()
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible()
    // No unexpected browser errors while booting
    expect(errors, `boot errors:\n${errors.join('\n')}`).toEqual([])
  })
})
