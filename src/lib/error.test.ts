import { describe, it, expect } from 'vitest'
import { extractErrorMessage } from './error'

describe('extractErrorMessage', () => {
  it('returns plain string as-is', () => {
    expect(extractErrorMessage('something went wrong')).toBe('something went wrong')
  })

  it('strips "Error:" prefix', () => {
    expect(extractErrorMessage('Error: backend rejected key')).toBe('backend rejected key')
  })

  it('extracts message from Error object', () => {
    expect(extractErrorMessage(new Error('object message'))).toBe('object message')
  })

  it('extracts message field from object', () => {
    expect(extractErrorMessage({ message: 'obj msg' })).toBe('obj msg')
  })

  it('extracts error field from object', () => {
    expect(extractErrorMessage({ error: 'err field' })).toBe('err field')
  })

  it('extracts detail field from object', () => {
    expect(extractErrorMessage({ detail: 'det field' })).toBe('det field')
  })

  it('returns fallback for null', () => {
    expect(extractErrorMessage(null)).toBe('Registration failed')
  })

  it('returns fallback for undefined', () => {
    expect(extractErrorMessage(undefined)).toBe('Registration failed')
  })

  it('returns fallback for number', () => {
    expect(extractErrorMessage(42)).toBe('Registration failed')
  })

  it('returns fallback for unknown object', () => {
    expect(extractErrorMessage({ foo: 'bar' })).toBe('Registration failed')
  })

  it('returns fallback for empty message', () => {
    expect(extractErrorMessage({ message: '' })).toBe('Registration failed')
  })
})
