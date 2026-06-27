/**
 * Safely extract a human-readable error message from any thrown value.
 * Handles Tauri v2 invoke rejections (plain strings, Error objects,
 * and objects with message/error/detail fields).
 * Never JSON-stringifies objects to avoid leaking secrets.
 */
export function extractErrorMessage(err: unknown): string {
  if (typeof err === 'string') {
    return err.replace(/^Error:\s*/, '')
  }
  if (err instanceof Error) {
    return err.message
  }
  if (err !== null && typeof err === 'object') {
    const obj = err as Record<string, unknown>
    if (typeof obj.message === 'string' && obj.message.length > 0) {
      return obj.message
    }
    if (typeof obj.error === 'string' && obj.error.length > 0) {
      return obj.error
    }
    if (typeof obj.detail === 'string' && obj.detail.length > 0) {
      return obj.detail
    }
  }
  return 'Registration failed'
}
