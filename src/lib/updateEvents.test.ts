import { describe, it, expect } from 'vitest'
import {
  updateButtonState,
  formatUpdateBlockedMsi,
  formatUpdateFailed,
  UPDATE_HANDOFF_NOTICE,
} from './updateEvents'

describe('updateButtonState', () => {
  it('disables the button for the whole time an install is in flight', () => {
    // The old page had an isUpdating flag that gated nothing, so the button
    // stayed clickable while the app was being replaced underneath it.
    for (const phase of ['preparing', 'handing-off'] as const) {
      expect(updateButtonState({ phase, available: true }).disabled).toBe(true)
    }
  })

  it('disables the button when the update is blocked', () => {
    expect(updateButtonState({ phase: 'blocked', available: true }).disabled).toBe(true)
  })

  it('enables it only when an update is genuinely available and idle', () => {
    expect(updateButtonState({ phase: 'idle', available: true })).toEqual({
      disabled: false,
      label: 'Update',
    })
    expect(updateButtonState({ phase: 'idle', available: false }).disabled).toBe(true)
  })

  it('labels every phase distinctly so the user can tell what is happening', () => {
    const labels = (['idle', 'preparing', 'handing-off', 'blocked'] as const).map(
      (phase) => updateButtonState({ phase, available: true }).label
    )
    expect(new Set(labels).size).toBe(4)
  })
})

describe('UPDATE_HANDOFF_NOTICE', () => {
  it('warns that the app itself is about to close', () => {
    // On Windows the promise never resolves because the process exits, so this
    // notice is the only honest feedback available.
    expect(UPDATE_HANDOFF_NOTICE.toLowerCase()).toContain('close')
    expect(UPDATE_HANDOFF_NOTICE.toLowerCase()).toContain('reopen')
  })
})

describe('formatUpdateBlockedMsi', () => {
  it('renders an MSI-owned install with the uninstall command the user needs', () => {
    const out = formatUpdateBlockedMsi({
      reason: 'msi-present',
      uninstallString: 'MsiExec.exe /X{ABC} /qn /norestart REBOOT=ReallySuppress',
      message: 'This copy was installed from the .msi package.',
    })
    expect(out.title).toBe('Update blocked')
    expect(out.command).toContain('/X{ABC}')
    expect(out.command).toContain('REBOOT=ReallySuppress')
  })

  it('treats an inconclusive probe as a pause, not a failure', () => {
    // Fail-closed must not read as "your install is broken".
    const out = formatUpdateBlockedMsi({ reason: 'probe-inconclusive', detail: 'timed out' })
    expect(out.title).toBe('Update paused')
    expect(out.detail.toLowerCase()).toContain('try again')
    expect(out.command).toBeNull()
  })
})

describe('formatUpdateFailed', () => {
  it('names both versions on a mismatch so the user can see what happened', () => {
    const out = formatUpdateFailed({ expected: '0.4.30', actual: '0.4.29' })
    expect(out.detail).toContain('0.4.30')
    expect(out.detail).toContain('0.4.29')
    expect(out.detail.toLowerCase()).toContain('previous version was kept')
  })

  it('still produces something actionable without version details', () => {
    const out = formatUpdateFailed({})
    expect(out.title).toBeTruthy()
    expect(out.detail.length).toBeGreaterThan(20)
  })
})
