// Shared integration-count formatting. Both the active/total label and the
// reward-proportion percent derive from the real integration total so the UI
// never drifts when integrations are added or removed (E2E QA v0.2.29 fix).

import { REQUIRED_INTEGRATIONS, BOOST_RATE, isRequiredIntegration } from './integrationMeta'

/** Minimal shape the required-count needs; the real type is FrontendIntegration. */
export interface RequiredCountable {
  id: string
  enabled: boolean
}

/**
 * How many of the REQUIRED integrations (Fry dVPN + Olostep) are enabled.
 * The headline stat and the sidebar summary both read this, out of
 * REQUIRED_INTEGRATIONS.length — 2/2 already means full base reward, so an
 * official-partner denominator ("3/5") undersold a fully-earning device.
 */
export function requiredActiveCount(members: RequiredCountable[]): number {
  return members.filter((m) => m.enabled && isRequiredIntegration(m.id)).length
}

export function activeFraction(active: number, total: number): string {
  return `${active}/${total}`
}

export function proportionPct(active: number, total: number): number {
  return total > 0 ? Math.round((active / total) * 100) : 0
}

// F18: the reward proportion is now driven by the REQUIRED integrations only
// (Fry dVPN + Olostep). Boost integrations add a flat +5% each on top.
export function requiredProportionPct(requiredActive: number): number {
  const denom = REQUIRED_INTEGRATIONS.length
  return denom > 0 ? Math.round((Math.min(requiredActive, denom) / denom) * 100) : 0
}

export function boostPct(boostActive: number): number {
  return Math.round(BOOST_RATE * 100 * boostActive)
}
