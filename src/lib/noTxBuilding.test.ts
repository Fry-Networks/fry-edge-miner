import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve, join } from 'node:path'

// P0 (c) CHARACTERIZATION GUARD, expected to pass on the pre-P0 code too:
// FEM builds and signs no transactions, so no funding copy may imply FEM
// moves money. Only the bundled frynode binary builds the registration payment.

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const walk = (d: string): string[] =>
  readdirSync(d).flatMap((n) => {
    const p = join(d, n)
    return statSync(p).isDirectory() ? walk(p) : [p]
  })

describe('no transaction building or signing in FEM', () => {
  it('has no Algorand SDK dependency in package.json or Cargo.toml', () => {
    const pkg = readFileSync(join(root, 'package.json'), 'utf-8')
    const cargo = readFileSync(join(root, 'src-tauri/Cargo.toml'), 'utf-8')
    for (const text of [pkg, cargo]) {
      expect(text).not.toMatch(/algosdk|algonaut|algorand/i)
    }
  })

  it('has no payment / asset-transfer builders in src', () => {
    const files = walk(join(root, 'src')).filter(
      (f) => /\.(ts|tsx)$/.test(f) && !/noTxBuilding\.test\.ts$/.test(f)
    )
    expect(files.length).toBeGreaterThan(10)
    for (const f of files) {
      expect(readFileSync(f, 'utf-8'), f).not.toMatch(/makePayment|makeAssetTransfer/)
    }
  })
})
