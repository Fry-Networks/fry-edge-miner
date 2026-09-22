import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'
import { AWAITING_MARKERS } from './setupRequired'

// B17 D2: cross-language drift guard. The Rust supervisor
// (src-tauri/src/integrations/mod.rs, T3-owned per this item's split) and
// this TS predicate must recognise exactly the same set of "waiting on the
// user" reason substrings, or the backend keeps restart-looping a partner
// the frontend has already stopped painting red. Parses the Rust array
// literal out of the source and deep-equals it against the TS export.
//
// NOTE: this test only goes fully green once B17 D1 (T3, mod.rs) lands
// alongside this D2 change — see runlog/fix-log.md.

const RUST_SRC = readFileSync(
  fileURLToPath(new URL('../../src-tauri/src/integrations/mod.rs', import.meta.url)),
  'utf-8'
)

function parseRustMarkers(src: string): string[] {
  const m = src.match(/const AWAITING_MARKERS: \[&str; \d+\] = \[([\s\S]*?)\];/)
  if (!m) throw new Error('AWAITING_MARKERS array literal not found in mod.rs')
  return Array.from(m[1].matchAll(/"((?:[^"\\]|\\.)*)"/g)).map((mm) => mm[1])
}

describe('setup-required marker parity with src-tauri (B17 D2)', () => {
  it('the TS AWAITING_MARKERS list matches the Rust one exactly', () => {
    const rustMarkers = parseRustMarkers(RUST_SRC)
    expect([...AWAITING_MARKERS]).toEqual(rustMarkers)
  })
})
