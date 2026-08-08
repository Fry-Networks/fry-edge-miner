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

/**
 * Reduce a multi-line backend error to the one line worth putting on a card.
 *
 * Tool output buries the cause at the bottom — `docker compose up` prints a
 * dozen "Container X Creating" lines before the line that actually explains the
 * failure. The card truncates from the front, so showing the first line hides
 * the answer. Prefer the last line that reads like a failure; fall back to the
 * first non-empty line. Single-line messages are returned unchanged.
 */
export function condenseError(message: string): string {
  const lines = message
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean)
  if (lines.length <= 1) return message.trim()

  const failureLike = /error|failed|cannot|denied|not available|not found|refused|timed out|already/i
  for (let i = lines.length - 1; i >= 0; i--) {
    if (failureLike.test(lines[i])) return lines[i]
  }
  return lines[0]
}
