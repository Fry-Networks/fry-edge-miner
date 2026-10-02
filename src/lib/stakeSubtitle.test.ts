import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, it, expect } from 'vitest'
import { stakeSubtitle } from './rewardReadiness'

// B22 D1: Rewards' "Staking Tier" sub-line printed "No stake stake active"
// (duplicated word) — the de-dup guard already exists at SettingsPage.tsx's
// stake line but was never applied to Rewards.tsx.

describe('stakeSubtitle', () => {
  // Vacuity audit (runlog/vacuity-audit.md): the previous version of this
  // case compared two compile-time string literals to each other — a
  // tautology that referenced no production symbol and would pass with
  // src/ deleted entirely. Fixed by tying the recorded pre-fix defect to
  // the function actually under test via `.not.toBe()`, so this case goes
  // red if stakeSubtitle ever regresses to the naive template.
  it('reproduces the pre-fix duplication the old inline template produced, and stakeSubtitle avoids it', () => {
    const preFixTemplate = (l: string) => `${l} stake active`
    expect(preFixTemplate('No stake')).toBe('No stake stake active') // the reported defect, from the naive template
    expect(stakeSubtitle('No stake')).not.toBe(preFixTemplate('No stake'))
    expect(stakeSubtitle('No stake')).toBe('No stake active')
  })

  it('does not duplicate "stake" when the label already contains it', () => {
    expect(stakeSubtitle('No stake')).toBe('No stake active')
  })

  it('appends " stake active" when the label has no "stake" word', () => {
    expect(stakeSubtitle('Bronze')).toBe('Bronze stake active')
    expect(stakeSubtitle('Silver')).toBe('Silver stake active')
    expect(stakeSubtitle('Gold')).toBe('Gold stake active')
  })

  it('passes the placeholder through unchanged', () => {
    expect(stakeSubtitle('—')).toBe('—')
  })

  it('matches the guard SettingsPage.tsx already applies', () => {
    for (const l of ['No stake', 'Bronze']) {
      expect(stakeSubtitle(l)).toBe(`${l}${l.toLowerCase().includes('stake') ? '' : ' stake'} active`)
    }
  })
})

describe('Rewards.tsx no longer builds the duplicated-word template inline', () => {
  const SOURCE = readFileSync(fileURLToPath(new URL('../pages/Rewards.tsx', import.meta.url)), 'utf-8')

  it('does not contain the raw pre-fix template', () => {
    expect(SOURCE).not.toContain('${stakeLabel} stake active')
  })
})
