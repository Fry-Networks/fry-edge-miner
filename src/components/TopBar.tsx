import Dot from './primitives/Dot'
import type { NavPage } from './Sidebar'
import { APP_VERSION } from '../lib/version'

interface TopBarProps {
  page: NavPage
  connectivity: 'connected' | 'degraded' | 'disconnected'
}

const CONNECTIVITY_STYLE = {
  connected:    { dot: 'run' as const,     label: 'Connected',    col: 'var(--teal)' },
  degraded:     { dot: 'warn' as const,    label: 'Degraded',     col: 'var(--amb)'  },
  disconnected: { dot: 'stopped' as const, label: 'Disconnected', col: 'var(--red)'  },
}

const L: Record<NavPage, string> = {
  dashboard: 'Dashboard',
  integrations: 'Integrations',
  rewards: 'Rewards',
  settings: 'Settings',
  updates: 'Updates'
}

const S: Record<NavPage, string> = {
  dashboard: 'System overview and live reward status',
  integrations: 'Manage partner integration processes',
  rewards: 'Earnings history and proof-of-contribution',
  settings: 'Device configuration and preferences',
  updates: 'Software version management'
}

export default function TopBar({ page, connectivity }: TopBarProps) {
  const cs = CONNECTIVITY_STYLE[connectivity]
  return (
    <div
      style={{
        height: 56,
        borderBottom: '1px solid var(--b0)',
        display: 'flex',
        alignItems: 'center',
        padding: '0 24px',
        background: 'var(--s0)',
        flexShrink: 0
      }}
    >
      <div style={{ flex: 1 }}>
        <div
          style={{
            fontFamily: 'var(--fh)',
            fontWeight: 700,
            fontSize: 16,
            color: 'var(--txt)',
            letterSpacing: '.02em'
          }}
        >
          {L[page]}
        </div>
        <div style={{ fontFamily: 'var(--fb)', fontSize: 11, color: 'var(--t2)', marginTop: 1 }}>{S[page]}</div>
      </div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 7 }}>
        <Dot status={cs.dot} size={5} />
        <span style={{ fontFamily: 'var(--fm)', fontSize: 10, color: cs.col }}>{cs.label}</span>
        <span style={{ fontFamily: 'var(--fm)', fontSize: 10, color: 'var(--t2)', marginLeft: 3 }}>v{APP_VERSION}</span>
      </div>
    </div>
  )
}
