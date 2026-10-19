import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'

// G5 (rc39b): the Sentinel card's docs link (docs.sentinel.co) broke mid-word
// because its parent span sets overflowWrap: 'anywhere'. The fix is scoped to
// the Sentinel card only: the link itself never wraps. Source-guard idiom
// (vitest env is `node`, no jsdom).

const SOURCE = readFileSync(fileURLToPath(new URL('./IntCard.tsx', import.meta.url)), 'utf-8')

const link = (() => {
  const at = SOURCE.indexOf('href={SETUP_GUIDANCE[id].url}')
  return SOURCE.slice(at, SOURCE.indexOf('</a>', at))
})()

describe('Sentinel docs link wrapping', () => {
  it('stops the inherited overflow-wrap: anywhere from splitting the link mid-word, for the Sentinel card', () => {
    expect(link).toContain("id === 'sentinel'")
    expect(link).toContain("overflowWrap: 'normal'")
    expect(link).toContain("wordBreak: 'normal'")
  })

  it('leaves the other cards (iagon, storj) on the unchanged link', () => {
    expect(link).toMatch(/id === 'sentinel' \? \{[^}]*\} : undefined/)
  })
})
