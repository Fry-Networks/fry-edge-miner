import {
  Globe,
  Search,
  Cpu,
  HardDrive,
  Eye,
  Shield,
  type LucideIcon
} from 'lucide-react'

export interface IntegrationMeta {
  id: string
  name: string
  tag: string
  desc: string
  Icon: LucideIcon
  col: string
  uptime: number
}

export const INTEGRATION_META: IntegrationMeta[] = [
  {
    id: 'mysterium',
    name: 'Mysterium',
    tag: 'VPN NODE',
    desc: 'Share bandwidth via the MystNodes VPN network',
    Icon: Globe,
    col: '#4a9eff',
    uptime: 99.2
  },
  {
    id: 'presearch',
    name: 'Presearch',
    tag: 'SEARCH NODE',
    desc: 'Serve queries on the decentralized search network',
    Icon: Search,
    col: '#a855f7',
    uptime: 97.8
  },
  {
    id: 'diiisco',
    name: 'Diiisco',
    tag: 'AI NODE',
    desc: 'Run local AI models that contribute to a decentralized network of shared inference',
    Icon: Cpu,
    col: '#f0a500',
    uptime: 0
  },
  {
    id: 'space_acres',
    name: 'SpaceAcres',
    tag: 'STORAGE NODE',
    desc: 'Provide decentralized storage to the Autonomys Network.',
    Icon: HardDrive,
    col: '#22c55e',
    uptime: 98.4
  },
  {
    id: 'aem',
    name: 'Olostep',
    tag: 'SCRAPE NODE',
    desc: 'Browser-based web scraping and data collection',
    Icon: Eye,
    col: '#00c49a',
    uptime: 96.1
  },
  {
    id: 'fryvpn',
    name: 'Fry dVPN',
    tag: 'VPN NODE',
    desc: 'Provide bandwidth to the Fry decentralized VPN network',
    Icon: Shield,
    col: '#ef4444',
    uptime: 0
  }
]

export const GATES = ['data', 'online', 'mac', 'pol', 'poi', 'poa']
