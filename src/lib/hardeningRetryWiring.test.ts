import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'

// FAIL-11 (row 6): at 5b7c8f5, App.tsx had NO listener for the backend's
// "elevation-required" event at all — hardening's blocked state never
// reached the UI, only a log line. This reads the real App.tsx source via
// fs (not a static import — the point is to prove the SOURCE really wires
// the listener to the retry command, the same property the Rust-side
// security_setup_user_click_hardening_tests.rs proves for the backend half).

const __dirname = dirname(fileURLToPath(import.meta.url))
const appTsxSource = readFileSync(resolve(__dirname, '../App.tsx'), 'utf-8')
const hardeningStatusEffectSource = readFileSync(resolve(__dirname, './hardeningStatusEffect.ts'), 'utf-8')

describe('App.tsx wires the hardening banner to a real retry', () => {
  // BUG LOOP 3 (NB): the live listen() call moved into
  // subscribeToHardeningStatus alongside the pull it was originally paired
  // with here (see the extraction note below) — it is no longer literal
  // text in App.tsx itself.
  it("listens for the backend's elevation-required event", () => {
    expect(hardeningStatusEffectSource).toContain("listen('elevation-required'")
  })

  it('invokes retry_hardening — the UserClick hardening command', () => {
    expect(appTsxSource).toContain("invoke('retry_hardening')")
  })

  // BUG LOOP 3 (NB): the mount-time pull + listen -> banner-state wiring
  // used to live inline in App.tsx and was pinned ONLY by raw source
  // substrings here, which two mutants defeated while leaving every
  // substring check green (delete `if (warning) setHardeningWarning(warning)`
  // from the pull's `.then`, since the live listener a few lines below
  // contains the same literal text; or comment the whole pull out, since
  // `toContain` matches inside comments too). It has since been extracted
  // into `subscribeToHardeningStatus` (src/lib/hardeningStatusEffect.ts),
  // which hardeningStatusEffect.test.ts pins BEHAVIOURALLY with mocked
  // invoke/listen/setWarning — that file is the real guarantee now. These
  // two checks only confirm App.tsx still mounts that extracted function
  // wired to the real invoke/listen/setWarning, not some inert stand-in,
  // and that the extraction didn't silently drop the pull or its decoder.
  it('mounts the extracted hardening-status subscription with the real invoke/listen/setWarning', () => {
    expect(appTsxSource).toContain('subscribeToHardeningStatus({ invoke, listen, setWarning: setHardeningWarning })')
  })

  it('the extracted subscription pulls get_hardening_status and decodes it with hardeningWarningFromStatus', () => {
    expect(hardeningStatusEffectSource).toContain("invoke<string | null>('get_hardening_status')")
    expect(hardeningStatusEffectSource).toContain('hardeningWarningFromStatus(')
  })
})
