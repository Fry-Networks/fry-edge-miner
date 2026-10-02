import { test, expect, bootApp, nav } from './_shared'

// B14 D2 — card and tile had no shared state source: the Integrations card
// badge and the Dashboard tile status line could disagree for the same
// integration (the reported case: card "Not installed", tile "Running").
// Both now derive from ../src/lib/integrationBadge.ts. Drives every
// integration through a range of badge states via the browser-preview-only
// `?intg=<id>:<state>` hint (useIntegrations.ts, D-12) and asserts the card
// Tag text and the Dashboard tile status line are string-equal for each id.
//
// Does not touch tests/e2e/integrations.spec.ts or badge-docker-split.spec.ts.

const STATES = [
  'running',
  'starting',
  'disabled',
  'notInstalled',
  'unavailable',
  'installing',
  'setupRequired',
  'unhealthy'
] as const

// One integration per state, all in a single mock so the app renders every
// state at once — real ids from src/lib/integrationMeta.ts.
const IDS = ['mysterium', 'storj', 'diiisco', 'space_acres', 'aem', 'fryvpn', 'titan', 'sentinel'] as const

const EXPECTED_LABEL: Record<(typeof STATES)[number], string> = {
  running: 'Running',
  starting: 'Starting',
  disabled: 'Disabled',
  notInstalled: 'Not installed',
  unavailable: 'Unavailable',
  installing: 'Installing',
  setupRequired: 'Setup required',
  unhealthy: 'Unhealthy'
}

function hintQuery() {
  return IDS.map((id, i) => `intg=${id}:${STATES[i]}`).join('&')
}

test.describe('card-tile-parity', () => {
  test('every sampled integration reads the same status on the card and the tile', async ({ page }) => {
    await page.goto(`/?${hintQuery()}`, { waitUntil: 'domcontentloaded' })
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible({ timeout: 15_000 })

    await nav(page, 'Integrations')
    await expect(page.locator('.ic').first()).toBeVisible({ timeout: 15_000 })

    // textContent (not innerText) — the Tag chip is styled text-transform:
    // uppercase for visual flair; the underlying label string is what must
    // match between the two pages, not its CSS rendering.
    const cardLabels: Record<string, string> = {}
    for (const id of IDS) {
      const text = await page.getByTestId(`status-${id}`).textContent()
      cardLabels[id] = (text ?? '').trim()
    }

    await nav(page, 'Dashboard')
    await expect(page.getByTestId(`tile-status-${IDS[0]}`)).toBeVisible({ timeout: 15_000 })

    for (let i = 0; i < IDS.length; i++) {
      const id = IDS[i]
      const state = STATES[i]
      const tileText = ((await page.getByTestId(`tile-status-${id}`).textContent()) ?? '').trim()
      expect(cardLabels[id], `card label for ${id} (${state})`).toBe(EXPECTED_LABEL[state])
      expect(tileText, `tile label for ${id} (${state})`).toBe(cardLabels[id])
    }
  })

  // G4 review finding 15: the eight states above never set dockerBlocked
  // (integrationBadge.ts's `!inst && dockerBlocked` arm), so a docker-
  // requiring, uninstalled, disabled integration could read 'Unavailable'
  // on the card and 'Not installed' on the tile — the parity spec's own
  // blind spot the review found. Combines the `?intg=` hint with the
  // existing `?docker=<kind>` hint (mirrors badge-docker-split.spec.ts,
  // not edited) to actually exercise it.
  test('a docker-blocked integration reads Unavailable on the card and the tile alike', async ({ page }) => {
    await page.goto('/?docker=daemon_stopped&intg=diiisco:notInstalled', { waitUntil: 'domcontentloaded' })
    await expect(page.getByText('EDGE MINER', { exact: true })).toBeVisible({ timeout: 15_000 })

    await nav(page, 'Integrations')
    const cardLabel = ((await page.getByTestId('status-diiisco').textContent()) ?? '').trim()
    expect(cardLabel, 'card label for diiisco under a docker-blocked, uninstalled, disabled state').toBe('Unavailable')

    await nav(page, 'Dashboard')
    const tileLabel = ((await page.getByTestId('tile-status-diiisco').textContent()) ?? '').trim()
    expect(tileLabel, 'tile label for diiisco under the same state').toBe(cardLabel)
  })
})
