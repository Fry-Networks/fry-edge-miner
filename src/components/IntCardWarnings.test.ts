import { readFileSync } from 'node:fs'
import { describe, it, expect } from 'vitest'

// E8: warning text on an integration card was clipped to one line
// ("this device has 185 GB availabl", "contact su"), with the full string
// reachable only through a `title` tooltip — invisible on touch and to a
// screen reader on a role="alert" span.
//
// This is a source-level guard rather than a rendering test on purpose: the
// vitest environment here is `node` and the project ships no jsdom or
// testing-library, so a component cannot be mounted without adding
// dependencies. It still fails if anyone reintroduces the clipping quartet.
const SOURCE = readFileSync(new URL('./IntCard.tsx', import.meta.url), 'utf-8')

describe('IntCard warning text', () => {
  it('never clips a warning to a single line', () => {
    expect(SOURCE).not.toContain("whiteSpace: 'nowrap'")
    expect(SOURCE).not.toContain("textOverflow: 'ellipsis'")
  })

  it('wraps every warning block instead', () => {
    const wrapping = SOURCE.match(/overflowWrap: 'anywhere'/g) ?? []
    // startError, unhealthy reason, dockerNote, unavailableReason.
    expect(wrapping.length).toBeGreaterThanOrEqual(4)
  })

  it('keeps the full text available as a tooltip as well', () => {
    expect(SOURCE).toContain('title={lastError ?? undefined}')
    expect(SOURCE).toContain('title={reason}')
    expect(SOURCE).toContain('title={dockerNote}')
    expect(SOURCE).toContain('title={unavailableReason ?? undefined}')
  })
})
