import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'

// B17 D6: no in-app way to supply the Iagon node_token, which the Done-when
// explicitly requires. Frontend half only — the set_partner_secret /
// has_partner_secret Tauri commands and their main.rs/commands/mod.rs wiring
// are T2/T3-owned (src-tauri/**, out of this worktree's fence). Source-guard
// idiom (IntCardWarnings.test.ts) since the vitest env is `node`.

const SOURCE = readFileSync(fileURLToPath(new URL('./IntCard.tsx', import.meta.url)), 'utf-8')

describe('IntCard partner secret input (B17 D6, frontend half)', () => {
  it('the input is masked', () => {
    expect(SOURCE).toMatch(/<input\s+type="password"/)
  })

  it('saving calls the set_partner_secret command with the id and the typed value', () => {
    expect(SOURCE).toContain("safeInvoke('set_partner_secret', { id, value: secretValue })")
  })

  it('only Iagon (the backend allow-list today) gets the input', () => {
    expect(SOURCE).toContain("const canSetSecret = id === 'iagon'")
  })

  it('re-runs the toggle after a successful save so the backend restarts with the fresh token', () => {
    // Must appear after the safeInvoke call, inside the same try block.
    const saveIdx = SOURCE.indexOf("safeInvoke('set_partner_secret'")
    const toggleIdx = SOURCE.indexOf('onToggle(id)', saveIdx)
    expect(saveIdx).toBeGreaterThan(-1)
    expect(toggleIdx).toBeGreaterThan(saveIdx)
  })

  it('never logs or otherwise surfaces the raw secret value outside the input itself', () => {
    // Only the controlled <input>'s value prop may reference secretValue;
    // no console.*, no title=, no other rendered text node.
    const refs = SOURCE.match(/secretValue/g) ?? []
    // useState decl + setSecretValue calls (2) + trim() check + invoke arg +
    // the input's value prop + onChange target read = a small fixed set;
    // guard against a stray extra reference (e.g. a title or log) creeping in.
    expect(refs.length).toBeLessThanOrEqual(6)
    expect(SOURCE).not.toMatch(/console\.\w+\([^)]*secretValue/)
    expect(SOURCE).not.toMatch(/title=\{[^}]*secretValue/)
  })
})
