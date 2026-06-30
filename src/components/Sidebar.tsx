import {
  LayoutDashboard,
  Puzzle,
  Coins,
  Settings,
  RefreshCw,
  type LucideIcon
} from 'lucide-react'
import NetSVG from './NetSVG'
import Dot from './primitives/Dot'

export type NavPage = 'dashboard' | 'integrations' | 'rewards' | 'settings' | 'updates'

const NAV: { id: NavPage; label: string; Icon: LucideIcon }[] = [
  { id: 'dashboard', label: 'Dashboard', Icon: LayoutDashboard },
  { id: 'integrations', label: 'Integrations', Icon: Puzzle },
  { id: 'rewards', label: 'Rewards', Icon: Coins },
  { id: 'settings', label: 'Settings', Icon: Settings },
  { id: 'updates', label: 'Updates', Icon: RefreshCw }
]

interface SidebarProps {
  page: NavPage
  onNav: (id: NavPage) => void
  activeCount: number
  hasUnhealthy?: boolean
  deviceName?: string
}

export default function Sidebar({ page, onNav, activeCount, hasUnhealthy, deviceName = 'nimble-swift-wolf' }: SidebarProps) {
  return (
    <div
      style={{
        width: 216,
        minWidth: 216,
        background: 'var(--s0)',
        borderRight: '1px solid var(--b0)',
        display: 'flex',
        flexDirection: 'column',
        height: '100%'
      }}
    >
      <div
        style={{
          padding: '24px 18px 20px',
          background: 'linear-gradient(180deg,rgba(229,39,28,.07) 0%,transparent 100%)',
          borderBottom: '1px solid var(--b0)',
          position: 'relative',
          overflow: 'hidden'
        }}
      >
        <NetSVG />
        <div style={{ position: 'relative', zIndex: 1 }}>
          <div
            style={{
              fontFamily: 'var(--fl)',
              fontWeight: 700,
              fontSize: 22,
              color: 'var(--txt)',
              letterSpacing: '.04em',
              lineHeight: 1
            }}
          >
            FRY
          </div>
          <div
            style={{
              fontFamily: 'var(--fh)',
              fontSize: 9,
              letterSpacing: '.28em',
              color: 'var(--t2)',
              marginTop: 3
            }}
          >
            EDGE MINER
          </div>
          <div
            style={{
              marginTop: 9,
              display: 'inline-flex',
              alignItems: 'center',
              gap: 5,
              background: 'var(--s3)',
              borderRadius: 'var(--radsm)',
              padding: '2px 7px'
            }}
          >
            <Dot status={activeCount === 0 ? 'stopped' : hasUnhealthy ? 'warn' : 'run'} size={5} />
            <span style={{ fontFamily: 'var(--fm)', fontSize: 10, color: hasUnhealthy ? 'var(--amb)' : 'var(--teal)' }}>{activeCount}/5 active</span>
          </div>
        </div>
      </div>
      <nav style={{ flex: 1, padding: '6px 0' }}>
        {NAV.map(({ id, label, Icon }) => {
          const active = page === id
          return (
            <button
              key={id}
              className="nb"
              onClick={() => onNav(id)}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 9,
                width: '100%',
                padding: '9px 18px',
                border: 'none',
                cursor: 'pointer',
                background: active ? 'var(--s2)' : 'transparent',
                borderLeft: `3px solid ${active ? 'var(--red)' : 'transparent'}`,
                color: active ? 'var(--txt)' : 'var(--t1)',
                fontFamily: 'var(--fh)',
                fontWeight: active ? 700 : 500,
                fontSize: 13,
                letterSpacing: '.04em',
                transition: 'all .13s'
              }}
            >
              <Icon
                className="nico"
                size={15}
                color={active ? 'var(--red)' : 'var(--t1)'}
                style={{ transition: 'color .12s' }}
              />
              {label}
            </button>
          )
        })}
      </nav>
      <div style={{ padding: '12px 18px', borderTop: '1px solid var(--b0)' }}>
        <div
          style={{
            fontFamily: 'var(--fl)',
            fontWeight: 700,
            fontSize: 11,
            color: 'var(--teal)',
            letterSpacing: '.04em',
            marginBottom: 3,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap'
          }}
        >
          {deviceName}
        </div>
        <div
          style={{
            fontFamily: 'var(--fm)',
            fontSize: 9,
            color: 'var(--t2)',
            opacity: 0.7,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap'
          }}
        >
          FEM-b9e4…733e
        </div>
      </div>
    </div>
  )
}
