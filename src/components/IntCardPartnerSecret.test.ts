import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'

// B17 D6: no in-app way to supply the Iagon node_token, which the Done-when
// explicitly requires. Frontend half only — the set_partner_secret /
// has_partner_secret Tauri commands and their main.rs/commands/mod.rs wiring
// are T2/T3-owned (src-tauri/**, out of this worktree's fence). Source-guard
// idiom (IntCardWarnings.test.ts) since the vitest env is `node`.
//
// G4 pre-release review finding 9 (fixed here): the original version of
// this file asserted that a successful save re-ran onToggle(id) as
// INTENDED behaviour. It was backwards — the input only renders while
// enabled === true, so onToggle(id) DISABLES the integration instead of
// restarting it. The two cases below were changed from pinning that
// ordering to forbidding it.

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

  it('never calls onToggle after a successful save (G4 review finding 9)', () => {
    // The input only renders while stLbl === 'Setup required', which
    // integrationBadge.ts's ladder can only reach past the !enabled arm —
    // so enabled is always true on the one path that reaches handleSaveSecret,
    // and onToggle(id) would DISABLE the integration instead of restarting
    // it. Scope the check to handleSaveSecret's own body so an unrelated
    // onToggle call elsewhere in the file (the card's own toggle switch)
    // doesn't produce a false pass.
    const fnStart = SOURCE.indexOf('const handleSaveSecret = async () => {')
    expect(fnStart).toBeGreaterThan(-1)
    const fnEnd = SOURCE.indexOf('\n  }', fnStart)
    const fnBody = SOURCE.slice(fnStart, fnEnd)
    expect(fnBody).toContain("safeInvoke('set_partner_secret'")
    expect(fnBody).not.toContain('onToggle(')
  })

  it('confirms the save so the user gets feedback instead of silence', () => {
    expect(SOURCE).toContain('secretSaved')
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
