import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'

const here = dirname(fileURLToPath(import.meta.url))
const settingsPage = readFileSync(resolve(here, '../pages/SettingsPage.tsx'), 'utf-8')
const useDevice = readFileSync(resolve(here, '../hooks/useDevice.ts'), 'utf-8')

describe('FEM-DISCORD-DUPLICATE-KEY-CONFIG: Settings exposes config path and local key switch', () => {
  it('SettingsPage shows the primary config file path in Device Identity', () => {
    expect(settingsPage).toContain('Config file')
    expect(settingsPage).toContain('data-testid="config-file-path"')
  })

  it('SettingsPage and useDevice provide a local-only key switch/reset action', () => {
    expect(useDevice).toContain('switch_miner_key_local')
    expect(settingsPage).toContain('data-testid="switch-miner-key-local"')
    expect(settingsPage).toContain('Use a different key on this device')
  })
})
