import { describe, it, expect } from 'vitest'
import { isUnavailable, toggleAllTargets, categoryCounts } from './availability'

const intg = (id: string, enabled: boolean, unavailable_reason: string | null = null) => ({
  id,
  enabled,
  unavailable_reason
})

describe('isUnavailable', () => {
  it('is false when the backend reported no reason', () => {
    expect(isUnavailable(intg('mysterium', true))).toBe(false)
    expect(isUnavailable({ id: 'x', enabled: false })).toBe(false)
  })

  it('is true when a reason is present', () => {
    expect(isUnavailable(intg('iagon', false, 'Iagon requires at least 900 GB'))).toBe(true)
  })

  it('treats an empty reason as available rather than unavailable', () => {
    // An empty string would otherwise disable a card with no explanation.
    expect(isUnavailable(intg('iagon', false, ''))).toBe(false)
  })
})

describe('toggleAllTargets', () => {
  const members = [
    intg('storj', false),
    intg('space_acres', true),
    intg('iagon', false, 'Iagon requires at least 900 GB of free storage')
  ]

  it('skips integrations this machine cannot run when enabling', () => {
    const ids = toggleAllTargets(members, true).map((i) => i.id)
    expect(ids).toEqual(['storj'])
    expect(ids).not.toContain('iagon')
  })

  it('skips them when disabling too', () => {
    const ids = toggleAllTargets(members, false).map((i) => i.id)
    expect(ids).toEqual(['space_acres'])
  })

  it('returns nothing when every member is already in the target state', () => {
    expect(toggleAllTargets([intg('a', true), intg('b', true)], true)).toEqual([])
  })

  it('still turns off an unavailable member that is running', () => {
    // An integration can become unavailable while it is enabled — Diiisco does
    // exactly this once its device wallet stops resolving. Excluding it here
    // left no way to stop it from the category slider. The backend's disable
    // path applies no requirements gate, so the stop is always accepted.
    const running = [intg('iagon', true, 'needs 900 GB')]
    expect(toggleAllTargets(running, false).map((i) => i.id)).toEqual(['iagon'])
  })

  it('still refuses to turn an unavailable member on', () => {
    const stopped = [intg('iagon', false, 'needs 900 GB')]
    expect(toggleAllTargets(stopped, true)).toEqual([])
  })
})

describe('categoryCounts', () => {
  it('excludes unavailable members from the fillable total', () => {
    const counts = categoryCounts([
      intg('storj', true),
      intg('space_acres', false),
      intg('titan', true),
      intg('iagon', false, 'needs 900 GB')
    ])
    expect(counts).toEqual({ activeCount: 2, availableTotal: 3, unavailableCount: 1 })
  })

  it('reports a zero fillable total when nothing can run', () => {
    const counts = categoryCounts([intg('iagon', false, 'needs 900 GB')])
    expect(counts).toEqual({ activeCount: 0, availableTotal: 0, unavailableCount: 1 })
  })

  it('is unchanged for a category with no unavailable members', () => {
    const counts = categoryCounts([intg('a', true), intg('b', false)])
    expect(counts).toEqual({ activeCount: 1, availableTotal: 2, unavailableCount: 0 })
  })
})
