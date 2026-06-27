import { extractErrorMessage } from './error'

function assertEqual(actual: string, expected: string, label: string) {
  if (actual !== expected) {
    throw new Error(`FAIL ${label}: expected "${expected}", got "${actual}"`)
  }
  console.log(`PASS ${label}`)
}

// plain string
assertEqual(extractErrorMessage('something went wrong'), 'something went wrong', 'plain string')

// string with "Error:" prefix
assertEqual(extractErrorMessage('Error: backend rejected key'), 'backend rejected key', 'Error: prefix stripped')

// Error object
assertEqual(extractErrorMessage(new Error('object message')), 'object message', 'Error object')

// object with message field
assertEqual(extractErrorMessage({ message: 'obj msg' }), 'obj msg', 'object message field')

// object with error field
assertEqual(extractErrorMessage({ error: 'err field' }), 'err field', 'object error field')

// object with detail field
assertEqual(extractErrorMessage({ detail: 'det field' }), 'det field', 'object detail field')

// null
assertEqual(extractErrorMessage(null), 'Registration failed', 'null fallback')

// undefined
assertEqual(extractErrorMessage(undefined), 'Registration failed', 'undefined fallback')

// number
assertEqual(extractErrorMessage(42), 'Registration failed', 'number fallback')

// object without known fields (must NOT stringify secrets)
assertEqual(extractErrorMessage({ foo: 'bar' }), 'Registration failed', 'unknown object fallback')

// object with empty message
assertEqual(extractErrorMessage({ message: '' }), 'Registration failed', 'empty message fallback')

console.log('All tests passed')
