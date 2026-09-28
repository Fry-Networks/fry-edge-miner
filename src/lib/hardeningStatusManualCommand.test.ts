import { describe, it, expect, vi } from 'vitest'
import { subscribeToHardeningStatus, type InvokeFn, type ListenFn } from './hardeningStatusEffect'

// c5 pin for lens-1 survivor RC13-HSE: the live `elevation-required`
// listener must hand the banner state the WHOLE decoded warning, including
// the manual PowerShell command the backend sends with it (main.rs and
// commands/hardening.rs emit `manualCommand`). The existing test fires a
// payload without one, and `toHaveBeenCalledWith` treats a missing property
// as equal to `undefined`, so `deps.setWarning({ reason: warning.reason })`
// passed every test while silently dropping the command.

const MANUAL =
  "Add-MpPreference -ExclusionPath 'C:\\Users\\femqa\\AppData\\Local\\Fry Edge Miner' ; " +
  "Add-MpPreference -ExclusionProcess 'fry-edge-miner.exe','titan-edge.exe'"

describe('subscribeToHardeningStatus live listener (RC13-HSE)', () => {
  it('passes the manual command through to the banner state', async () => {
    const invoke = (<T,>() => Promise.resolve(null as unknown as T)) as InvokeFn
    let fire: ((event: { payload: unknown }) => void) | undefined
    const listen: ListenFn = (_event, handler) => {
      fire = handler
      return Promise.resolve(() => {})
    }
    const setWarning = vi.fn()

    subscribeToHardeningStatus({ invoke, listen, setWarning })
    await Promise.resolve()
    await Promise.resolve()

    fire?.({ payload: { purpose: 'hardening', reason: 'Elevation declined', manualCommand: MANUAL } })
    expect(setWarning).toHaveBeenCalledTimes(1)
    expect(setWarning.mock.calls[0][0]).toStrictEqual({ reason: 'Elevation declined', manualCommand: MANUAL })
  })
})
