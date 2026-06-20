import {
  Globe,
  Search,
  Cpu,
  HardDrive,
  Eye,
  type LucideIcon
} from 'lucide-react'
import type { HealthStatus, LifecycleState } from './types'

export interface MockIntegration {
  id: string
  name: string
  tag: string
  desc: string
  Icon: LucideIcon
  col: string
  enabled: boolean
  health: HealthStatus
  healthy: boolean
  lifecycle: LifecycleState
  version: string | null
  uptime: number
}

export const INTGS: MockIntegration[] = [
  {
    id: 'mysterium',
    name: 'Mysterium',
    tag: 'VPN NODE',
    desc: 'Share bandwidth via the MystNodes VPN network',
    Icon: Globe,
    col: '#4a9eff',
    enabled: true,
    health: 'Healthy',
    healthy: true,
    lifecycle: 'Running',
    version: '1.2.3',
    uptime: 99.2
  },
  {
    id: 'presearch',
    name: 'Presearch',
    tag: 'SEARCH NODE',
    desc: 'Serve queries on the decentralized search network',
    Icon: Search,
    col: '#a855f7',
    enabled: true,
    health: 'Healthy',
    healthy: true,
    lifecycle: 'Running',
    version: '0.9.1',
    uptime: 97.8
  },
  {
    id: 'diiisco',
    name: 'Diiisco',
    tag: 'AI NODE',
    desc: 'Run local AI models that contribute to a decentralized network of shared inference',
    Icon: Cpu,
    col: '#f0a500',
    enabled: false,
    health: 'Stopped',
    healthy: false,
    lifecycle: 'Disabled',
    version: '2.0.1',
    uptime: 0
  },
  {
    id: 'space_acres',
    name: 'SpaceAcres',
    tag: 'STORAGE NODE',
    desc: 'Contribute disk space for Chia / SpaceAcres farming',
    Icon: HardDrive,
    col: '#22c55e',
    enabled: true,
    health: 'Healthy',
    healthy: true,
    lifecycle: 'Running',
    version: '2.1.0',
    uptime: 98.4
  },
  {
    id: 'aem',
    name: 'AEM',
    tag: 'SCRAPE NODE',
    desc: 'Browser-based web scraping and data collection',
    Icon: Eye,
    col: '#00c49a',
    enabled: true,
    health: 'Healthy',
    healthy: true,
    lifecycle: 'Running',
    version: '1.0.0',
    uptime: 96.1
  }
]

export interface PocSlot {
  done: boolean
  pass: boolean | null
}

export const POC: PocSlot[] = Array.from({ length: 144 }, (_, i) => ({
  done: i < 62,
  pass: i < 62 ? Math.random() > 0.1 : null
}))

export interface RewardRow {
  date: string
  reward: number
  slots: number
  factor: number
  status: 'paid' | 'none'
}

export const RWDS: RewardRow[] = [
  { date: 'Jun 13', reward: 6.41, slots: 62, factor: 0.108, status: 'paid' },
  { date: 'Jun 12', reward: 0, slots: 0, factor: 0, status: 'none' },
  { date: 'Jun 11', reward: 0, slots: 0, factor: 0, status: 'none' },
  { date: 'Jun 10', reward: 5.88, slots: 58, factor: 0.099, status: 'paid' },
  { date: 'Jun 9', reward: 6.12, slots: 61, factor: 0.104, status: 'paid' }
]

export const GATES = ['data', 'online', 'mac', 'pol', 'poi', 'poa']

export interface InstallStage {
  label: string
  ms: number
}

export const STAGES: InstallStage[] = [
  { label: 'Validating miner key', ms: 900 },
  { label: 'Registering with Fry Networks', ms: 1100 },
  { label: 'Downloading partner software', ms: 2000 },
  { label: 'Verifying SHA256 signatures', ms: 700 },
  { label: 'Starting integration services', ms: 900 }
]
