import { describe, it, expect } from 'vitest'
import { condenseError } from './error'

describe('condenseError', () => {
  it('returns a single-line message unchanged', () => {
    const m = 'Failed to spawn frynode: program not found'
    expect(condenseError(m)).toBe(m)
  })

  it('surfaces the real cause buried at the end of docker compose output', () => {
    // Verbatim shape of what diiisco actually returned on this machine: a dozen
    // progress lines, then the one line that explains the failure.
    const m = [
      'Failed to start Diiisco:  Network diiisco_default Creating',
      ' Network diiisco_default Created',
      ' Container diiisco-ollama Creating',
      ' Container diiisco-ollama Starting',
      'Error response from daemon: ports are not available: exposing port TCP 0.0.0.0:11434 -> 127.0.0.1:0: listen tcp 0.0.0.0:11434: bind: Only one usage of each socket address is normally permitted.'
    ].join('\n')
    expect(condenseError(m)).toMatch(/ports are not available/)
  })

  it('falls back to the first line when nothing looks like a failure', () => {
    expect(condenseError('step one\nstep two\nstep three')).toBe('step one')
  })

  it('ignores blank lines and trims', () => {
    expect(condenseError('\n\n  only line  \n\n')).toBe('only line')
  })

  it('keeps a storj-style single line intact', () => {
    const m = 'storagenode binary not found at C:\\Users\\x\\AppData\\Roaming\\FryEdgeMiner\\partners\\storj\\storagenode.exe'
    expect(condenseError(m)).toBe(m)
  })
})
