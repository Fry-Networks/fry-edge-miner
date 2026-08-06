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
    id: 'storj',
    name: 'Storj',
    tag: 'STORAGE NODE',
    desc: 'Provide decentralized storage to the Storj Network',
    Icon: Cloud,
    col: '#0066ff',
    uptime: 0
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
  },
  {
    id: 'titan',
    name: 'Titan Network',
    tag: 'EDGE NODE',
    desc: 'Storage, bandwidth & IP contribution via Titan edge nodes',
    Icon: Network,
    col: '#e8452c',
    uptime: 0
  },
  {
    id: 'sentinel',
    name: 'Sentinel dVPN',
    tag: 'VPN NODE',
    desc: 'Decentralized VPN bandwidth node on Cosmos',
    Icon: Lock,
    col: '#0fa0ce',
    uptime: 0
  },
  {
    id: 'iagon',
    name: 'Iagon Storage',
    tag: 'STORAGE NODE',
    desc: 'Decentralized storage on Cardano',
    Icon: Server,
    col: '#7b3fe4',
    uptime: 0
  },
  {
    id: 'pawns',
    name: 'Pawns.app',
    tag: 'BANDWIDTH NODE',
    desc: 'Residential bandwidth sharing ($0.20/GB)',
    Icon: Wifi,
    col: '#f5a623',
    uptime: 0
  }
]

export const GATES = ['data', 'online', 'mac', 'pol', 'poi', 'poa']
