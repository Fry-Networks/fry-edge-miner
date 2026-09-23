import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'

// B17 D6: in-app entry for the Iagon node_token, which the Done-when
// requires. The backend half (commands/partner_secret.rs — set_partner_secret,
// registered in main.rs's generate_handler!) has landed (T2, confirmed
// directly rather than assumed — see the cross-boundary contract test below,
// and runlog/fix-log.md for the compiled-build + cargo-test verification
// performed against a merged checkout). Source-guard idiom
// (IntCardWarnings.test.ts) for the frontend half, since the vitest env is
// `node` with no jsdom/testing-library; the cross-boundary test mirrors
// setupRequiredParity.test.ts's pattern (parses the real Rust source rather
// than grepping this file's own claims about it) and is RED in this
// worktree alone — src-tauri/** is out of fix/ui's fence — until fix/ui
// merges alongside the backend, same as that test.
//
// G4 pre-release review finding 4/12 (the reason for the checks above): a
// source-scan test previously passed while set_partner_secret did not exist
// anywhere in the backend, so the masked field could only ever render
// "Command set_partner_secret not found". Finding 9 (fixed below): the
// original version of this file asserted that a successful save re-ran
// onToggle(id) as INTENDED behaviour. It was backwards — the input only
// renders while enabled === true, so onToggle(id) DISABLES the integration
// instead of restarting it.

const SOURCE = readFileSync(fileURLToPath(new URL('./IntCard.tsx', import.meta.url)), 'utf-8')

describe('IntCard partner secret input (B17 D6)', () => {
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

// Cross-boundary contract with the real backend (T2-owned,
// src-tauri/src/commands/partner_secret.rs) — same idiom as
// setupRequiredParity.test.ts: read the ACTUAL Rust source rather than
// trusting a claim about it, so a signature or registration drift on either
// side shows up here instead of as a silent "Command not found" at runtime.
describe('set_partner_secret backend contract (cross-boundary, mirrors setupRequiredParity.test.ts)', () => {
  let RUST_SRC: string | null = null
  let MAIN_SRC: string | null = null
  try {
    RUST_SRC = readFileSync(
      fileURLToPath(new URL('../../src-tauri/src/commands/partner_secret.rs', import.meta.url)),
      'utf-8'
    )
    MAIN_SRC = readFileSync(fileURLToPath(new URL('../../src-tauri/src/main.rs', import.meta.url)), 'utf-8')
  } catch {
    // Expected in this worktree alone: src-tauri/** is out of fix/ui's
    // fence, so the backend file isn't present here until merge. The tests
    // below report that plainly rather than silently skipping.
  }

  it('the backend command file is present (RED in this worktree alone until merge)', () => {
    expect(RUST_SRC, 'src-tauri/src/commands/partner_secret.rs not found in this worktree').not.toBeNull()
  })

  it('the command signature takes exactly (id: String, value: String) — matches the JS call shape', () => {
    if (!RUST_SRC) return
    expect(RUST_SRC).toMatch(/pub\s+async\s+fn\s+set_partner_secret\s*\(\s*id:\s*String,\s*value:\s*String\s*\)/)
  })

  it('the command is registered in the invoke handler', () => {
    if (!MAIN_SRC) return
    expect(MAIN_SRC).toContain('commands::partner_secret::set_partner_secret')
  })

  it('writes to the key iagon.rs actually reads (node_token, under the resolved partners root)', () => {
    if (!RUST_SRC) return
    expect(RUST_SRC).toContain('"node_token"')
    expect(RUST_SRC).toContain('partners_base_dir()')
  })
})
