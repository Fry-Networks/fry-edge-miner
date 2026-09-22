import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'
import { storageFallbackMessage } from './storageLocation'

// B4 D3 + B13 D5 (consolidated — both items independently proposed a
// storage-root-fallback banner for the same underlying condition; landed as
// one selector/banner rather than two overlapping ones):
//
// After a silent startup storage-root fallback (download.rs, T2-owned), the
// Settings page reported the CONFIGURED root as "where partner data is
// stored" while files actually go to the default location, with the only
// visible signal a "Restart Fry Edge Miner" banner that is FALSE — a
// restart cannot fix an unavailable drive. This manufactured "fry edge
// miner folder empty" reports with nothing ever deleted.
//
// storageLocation.ts / storageLocation.test.ts (existing pendingRestart
// tests) are not touched.

describe('storageFallbackMessage', () => {
  it('names the configured path, the reason, and the active path, and says nothing was deleted (B4 D3 shape)', () => {
    const msg = storageFallbackMessage({
      path: 'D:\\fry_storage\\FryEdgeMiner\\partners',
      active_path: 'C:\\Users\\x\\AppData\\Roaming\\FryEdgeMiner\\partners',
      pending_restart: true,
      fallback_reason: 'D:\\fry_storage\\FryEdgeMiner\\partners — access is denied'
    })
    expect(msg).not.toBeNull()
    expect(msg).toContain('D:\\fry_storage\\FryEdgeMiner\\partners')
    expect(msg).toContain('access is denied')
    expect(msg).toContain('C:\\Users\\x\\AppData\\Roaming\\FryEdgeMiner\\partners')
    expect(msg).toContain('have not been deleted')
    // Must not also carry the false "restart will fix it" advice.
    expect(msg).not.toContain('Restart Fry Edge Miner to start using this location.')
  })

  it('falls back to a generic message when only the boolean signal is present (B13 D5 shape)', () => {
    const msg = storageFallbackMessage({
      path: 'D:\\fry_storage\\FryEdgeMiner\\partners',
      pending_restart: true,
      fell_back: true
    })
    expect(msg).not.toBeNull()
    expect(msg).toContain('D:\\fry_storage\\FryEdgeMiner\\partners')
    expect(msg).toMatch(/default location/i)
  })

  it('is null when there is no fallback (the genuine pending-restart case, or nothing pending)', () => {
    expect(storageFallbackMessage({ path: 'D:\\x', pending_restart: true })).toBeNull()
    expect(storageFallbackMessage({ path: 'D:\\x', pending_restart: false })).toBeNull()
    expect(storageFallbackMessage(null)).toBeNull()
    expect(storageFallbackMessage(undefined)).toBeNull()
  })

  it('fallback_reason takes precedence when both signals are present', () => {
    const msg = storageFallbackMessage({
      path: 'D:\\x',
      active_path: 'C:\\y',
      pending_restart: true,
      fell_back: true,
      fallback_reason: 'the specific reason'
    })
    expect(msg).toContain('the specific reason')
  })
})

describe('SettingsPage.tsx renders the fallback message instead of the restart banner', () => {
  const SRC = readFileSync(fileURLToPath(new URL('../pages/SettingsPage.tsx', import.meta.url)), 'utf-8')

  it('imports storageFallbackMessage', () => {
    expect(SRC).toContain('storageFallbackMessage')
  })

  it('keeps the existing restart sentence for the genuine pending_restart case', () => {
    expect(SRC).toContain('Restart Fry Edge Miner to start using this location.')
  })
})
