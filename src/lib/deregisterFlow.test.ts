import { describe, it, expect } from 'vitest'
import {
  deregisterFailureState,
  IDLE_DEREGISTER_STATE,
  DEREGISTER_CONFIRM,
  DEREGISTER_FORCE_CONFIRM
} from './deregisterFlow'

// JuggaCrypto's report: clicking Deregister showed the confirm and then
// appeared to do nothing, and reinstalling FEM brought the device back
// registered to the same key. The server call was failing; its rejection was
// never shown next to the button, and the local clear only ran on success.

describe('deregisterFailureState', () => {
  it('surfaces the failure reason instead of swallowing it', () => {
    const state = deregisterFailureState(new Error('API deregistration failed: HTTP 500'), false)
    expect(state.error).toContain('deregistration failed')
  })

  it('offers the local-only clear after a normal attempt fails', () => {
    // Without this the device can never be detached — an uninstall leaves
    // %APPDATA% state behind, so it reinstalls still registered.
    expect(deregisterFailureState(new Error('boom'), false).offerForce).toBe(true)
  })

  it('does not re-offer the escape hatch when the forced attempt itself failed', () => {
    expect(deregisterFailureState(new Error('boom'), true).offerForce).toBe(false)
  })

  it('handles a non-Error rejection (Tauri rejects with a string)', () => {
    const state = deregisterFailureState('API deregistration failed: timeout', false)
    expect(state.error).toContain('timeout')
    expect(state.offerForce).toBe(true)
  })

  it('starts clean', () => {
    expect(IDLE_DEREGISTER_STATE).toEqual({ error: null, offerForce: false })
  })
})

describe('confirmation copy', () => {
  it('warns that the forced clear only affects local state', () => {
    // The user must understand the server may still list the device.
    expect(DEREGISTER_FORCE_CONFIRM.toLowerCase()).toContain('locally')
    expect(DEREGISTER_FORCE_CONFIRM.toLowerCase()).toContain('server')
  })

  it('keeps the original confirm for the normal path', () => {
    expect(DEREGISTER_CONFIRM).toBe('Deregister this device? This cannot be undone.')
  })
})
