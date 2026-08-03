// Shared integration-count formatting. Both the active/total label and the
// reward-proportion percent derive from the real integration total so the UI
// never drifts when integrations are added or removed (E2E QA v0.2.29 fix).

export function activeFraction(active: number, total: number): string {
  return `${active}/${total}`
}

export function proportionPct(active: number, total: number): number {
  return total > 0 ? Math.round((active / total) * 100) : 0
}
