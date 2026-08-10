import { describe, expect, test } from 'vitest'
import { shouldShowSavedMinerKey } from './settingsView'

describe('shouldShowSavedMinerKey', () => {
  test('unregistered device with a saved key shows it', () => {
    expect(shouldShowSavedMinerKey(false, 'FEM-abc123')).toBe(true)
  })

  test('registered device uses the normal registered view instead', () => {
    expect(shouldShowSavedMinerKey(true, 'FEM-abc123')).toBe(false)
  })

  test('no saved key: nothing to show', () => {
    expect(shouldShowSavedMinerKey(false, null)).toBe(false)
    expect(shouldShowSavedMinerKey(false, undefined)).toBe(false)
    expect(shouldShowSavedMinerKey(false, '')).toBe(false)
  })
})
