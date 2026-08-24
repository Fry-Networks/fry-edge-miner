import { describe, expect, test } from 'vitest'
import {
  BOOST_INTEGRATIONS,
  BOOST_RATE,
  REQUIRED_INTEGRATIONS,
  isRequiredIntegration
} from './integrationMeta'
import { boostPct, requiredProportionPct } from './integrationCount'

// F17/F18: reward tiering. Only Fry dVPN + Olostep are Required; every other
// integration is a +5% boost. These lists must stay in lockstep with the
// hardcoded lists in dbrewards (server-side reward math).

describe('reward-role membership', () => {
  test('only fryvpn and aem are required', () => {
    expect([...REQUIRED_INTEGRATIONS].sort()).toEqual(['aem', 'fryvpn'])
  })

  test('the other eight integrations are boost', () => {
    expect([...BOOST_INTEGRATIONS].sort()).toEqual(
      ['diiisco', 'iagon', 'mysterium', 'pawns', 'sentinel', 'space_acres', 'storj', 'titan']
    )
  })

  test('required and boost lists are disjoint and cover all ten', () => {
    const overlap = BOOST_INTEGRATIONS.filter((id) =>
      (REQUIRED_INTEGRATIONS as readonly string[]).includes(id)
    )
    expect(overlap).toEqual([])
    expect(REQUIRED_INTEGRATIONS.length + BOOST_INTEGRATIONS.length).toBe(10)
  })

  test('isRequiredIntegration is true only for the required set', () => {
    expect(isRequiredIntegration('fryvpn')).toBe(true)
    expect(isRequiredIntegration('aem')).toBe(true)
    expect(isRequiredIntegration('mysterium')).toBe(false)
    expect(isRequiredIntegration('pawns')).toBe(false)
    expect(isRequiredIntegration('unknown')).toBe(false)
  })
})

describe('reward proportion + boost math', () => {
  test('required proportion is 0/50/100 for 0/1/2 active required', () => {
    expect(requiredProportionPct(0)).toBe(0)
    expect(requiredProportionPct(1)).toBe(50)
    expect(requiredProportionPct(2)).toBe(100)
  })

  test('required proportion never exceeds 100', () => {
    expect(requiredProportionPct(3)).toBe(100)
  })

  test('boost is 5% per active boost integration', () => {
    expect(boostPct(0)).toBe(0)
    expect(boostPct(1)).toBe(5)
    expect(boostPct(3)).toBe(15)
    expect(boostPct(8)).toBe(40)
  })

  test('boost rate constant is 0.05', () => {
    expect(BOOST_RATE).toBe(0.05)
  })
})
