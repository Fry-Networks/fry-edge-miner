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

describe('App.tsx wires the hardening banner to a real retry', () => {
  it("listens for the backend's elevation-required event", () => {
    expect(appTsxSource).toContain("listen('elevation-required'")
  })

  it('invokes retry_hardening — the UserClick hardening command', () => {
    expect(appTsxSource).toContain("invoke('retry_hardening')")
  })

  // BUG LOOP 2 (BLOCKING): the boot pass's Automatic refusal publishes into
  // the backend's elevation gate microseconds after setup — long before this
  // component mounts and the listener above registers. The live listener
  // alone can never see that: `emit()` is fire-and-forget with no replay.
  // The mount-time pull (get_hardening_status, called once per mount) is
  // what recovers a block that already happened by the time we exist.
  it('pulls the current hardening status on mount, not just the live event', () => {
    expect(appTsxSource).toContain("invoke<string | null>('get_hardening_status')")
  })

  it('decodes the pulled status with hardeningWarningFromStatus, feeding the same banner state', () => {
    expect(appTsxSource).toContain('hardeningWarningFromStatus(')
    expect(appTsxSource).toContain('setHardeningWarning(warning)')
  })
})
