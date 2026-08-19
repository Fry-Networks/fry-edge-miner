import {
  Globe,
  Cpu,
  HardDrive,
  Eye,
  Shield,
  Cloud,
  Network,
  Lock,
  Server,
  Wifi,
  type LucideIcon
} from 'lucide-react'

/** Groupings shown on the Integrations screen, in display order. */
export const CATEGORIES = ['VPN & Bandwidth', 'Storage & Farming', 'AI & Data'] as const
export type IntegrationCategory = (typeof CATEGORIES)[number]

/**
 * Who stands behind an integration. `official` partners are contracted by Fry
 * Networks and carry the base reward proportion; `sdk` ones are community
 * builds on the partner SDK — experimental, and a bonus on top rather than a
 * requirement. Mirrors `IntegrationTier` in src-tauri/src/integrations/mod.rs;
 * the two lists must agree (see integrationTier.test.ts).
 */
export type IntegrationTier = 'official' | 'sdk'

export interface IntegrationMeta {
  id: string
  name: string
  tag: string
  desc: string
  Icon: LucideIcon
  col: string
  uptime: number
  category: IntegrationCategory
  tier: IntegrationTier
}

export const INTEGRATION_META: IntegrationMeta[] = [
  {
    id: 'mysterium',
    name: 'Mysterium',
    tag: 'VPN NODE',
    desc: 'Share bandwidth via the MystNodes VPN network',
    Icon: Globe,
    col: '#4a9eff',
    uptime: 99.2,
    category: 'VPN & Bandwidth',
    tier: 'official'
  },
  {
    id: 'storj',
    name: 'Storj',
    tag: 'STORAGE NODE',
    desc: 'Provide decentralized storage to the Storj Network',
    Icon: Cloud,
    col: '#0066ff',
    uptime: 0,
    category: 'Storage & Farming',
    tier: 'sdk'
  },
  {
    id: 'diiisco',
    name: 'Diiisco',
    tag: 'AI NODE',
    desc: 'Run local AI models that contribute to a decentralized network of shared inference',
    Icon: Cpu,
    col: '#f0a500',
    uptime: 0,
    category: 'AI & Data',
    tier: 'official'
  },
  {
    id: 'space_acres',
    name: 'SpaceAcres',
    tag: 'STORAGE NODE',
    desc: 'Provide decentralized storage to the Autonomys Network.',
    Icon: HardDrive,
    col: '#22c55e',
    uptime: 98.4,
    category: 'Storage & Farming',
    tier: 'official'
  },
  {
    id: 'aem',
    name: 'Olostep',
    tag: 'SCRAPE NODE',
    desc: 'Browser-based web scraping and data collection',
    Icon: Eye,
    col: '#00c49a',
    uptime: 96.1,
    category: 'AI & Data',
    tier: 'official'
  },
  {
    id: 'fryvpn',
    name: 'Fry dVPN',
    tag: 'VPN NODE',
    desc: 'Provide bandwidth to the Fry decentralized VPN network',
    Icon: Shield,
    col: '#ef4444',
    uptime: 0,
    category: 'VPN & Bandwidth',
    tier: 'official'
  },
  {
    id: 'titan',
    name: 'Titan Network',
    tag: 'EDGE NODE',
    desc: 'Storage, bandwidth & IP contribution via Titan edge nodes',
    Icon: Network,
    col: '#e8452c',
    uptime: 0,
    category: 'Storage & Farming',
    tier: 'sdk'
  },
  {
    id: 'sentinel',
    name: 'Sentinel dVPN',
    tag: 'VPN NODE',
    desc: 'Decentralized VPN bandwidth node on Cosmos',
    Icon: Lock,
    col: '#0fa0ce',
    uptime: 0,
    category: 'VPN & Bandwidth',
    tier: 'sdk'
  },
  {
    id: 'iagon',
    name: 'Iagon Storage',
    tag: 'STORAGE NODE',
    desc: 'Decentralized storage on Cardano',
    Icon: Server,
    col: '#7b3fe4',
    uptime: 0,
    category: 'Storage & Farming',
    tier: 'sdk'
  },
  {
    id: 'pawns',
    name: 'Pawns.app',
    tag: 'BANDWIDTH NODE',
    desc: 'Residential bandwidth sharing ($0.20/GB)',
    Icon: Wifi,
    col: '#f5a623',
    uptime: 0,
    category: 'VPN & Bandwidth',
    tier: 'sdk'
  }
]

export const GATES = ['data', 'online', 'mac', 'pol', 'poi', 'poa']
