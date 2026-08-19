// B2: reward amounts arrive as IEEE-754 doubles summed server-side, so a
// value that is conceptually 13.392 reaches the UI as 13.392000000000001.
// Rendering it raw put a 17-digit number in the reward history table and made
// users ask whether their payout was wrong.
//
// Six decimals is the resolution that matters: below that is float noise, not
// reward. Trailing zeros are trimmed so a whole number reads as "5", not
// "5.000000".

const MAX_DECIMALS = 6

export function formatReward(value: number, maxDecimals: number = MAX_DECIMALS): string {
  if (!Number.isFinite(value)) return '0'
  // toFixed rounds away the float tail; the regex then removes the padding it
  // adds (and the now-bare decimal point) without touching significant zeros.
  const fixed = value.toFixed(maxDecimals)
  const trimmed = fixed.includes('.') ? fixed.replace(/\.?0+$/, '') : fixed
  // A negative value smaller than the last decimal rounds to "-0.000000",
  // which trims to "-0". Nobody was ever paid negative nothing.
  if (trimmed === '' || Number(trimmed) === 0) return '0'
  return trimmed
}

/** Reward plus its token symbol, as shown in the history table. */
export function formatRewardWithToken(value: number, token: string): string {
  return `${formatReward(value)} ${token}`
}
