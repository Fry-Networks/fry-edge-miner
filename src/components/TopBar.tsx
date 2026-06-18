import Dot from './primitives/Dot'
import type { NavPage } from './Sidebar'

interface TopBarProps {
  page: NavPage
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

export default function TopBar({ page }: TopBarProps) {
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
        <Dot status="run" size={5} />
        <span style={{ fontFamily: 'var(--fm)', fontSize: 10, color: 'var(--teal)' }}>Connected</span>
        <span style={{ fontFamily: 'var(--fm)', fontSize: 10, color: 'var(--t2)', marginLeft: 3 }}>v0.2.3</span>
      </div>
    </div>
  )
}
