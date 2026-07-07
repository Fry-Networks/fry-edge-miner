import { useState } from 'react'
import { ChevronDown, ChevronUp } from 'lucide-react'
import Sidebar, { type NavPage } from './components/Sidebar'
import TopBar from './components/TopBar'
import Dashboard from './pages/Dashboard'
import Integrations from './pages/Integrations'
import Rewards from './pages/Rewards'
import SettingsPage from './pages/SettingsPage'
import Updates from './pages/Updates'
import Wizard from './wizard/Wizard'
import { useIntegrations } from './hooks/useIntegrations'
import { useDevice } from './hooks/useDevice'

// Truncated error banner with expandable details — raw multi-line backend
// output (e.g. Docker logs) must never flood the layout.
function ErrorBanner({ error }: { error: string }) {
  const [expanded, setExpanded] = useState(false)
  const firstLine = error.split('\n')[0]
  const summary = firstLine.length > 200 ? `${firstLine.slice(0, 200)}…` : firstLine
  const hasMore = error.length > summary.length

  return (
    <div
      style={{
        padding: '8px 16px',
        background: 'var(--red)18',
        borderBottom: '1px solid var(--red)40',
        fontFamily: 'var(--fb)',
        fontSize: 12,
        color: 'var(--red)'
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <span style={{ fontWeight: 600, flexShrink: 0 }}>Error:</span>
        <span style={{ minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{summary}</span>
        {hasMore && (
          <button
            onClick={() => setExpanded((v) => !v)}
            style={{
              flexShrink: 0,
              marginLeft: 'auto',
              display: 'inline-flex',
              alignItems: 'center',
              gap: 3,
              background: 'none',
              border: 'none',
              color: 'var(--red)',
              fontFamily: 'var(--fb)',
              fontSize: 11,
              cursor: 'pointer',
              padding: 0
            }}
          >
            {expanded ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
            {expanded ? 'Hide details' : 'Details'}
          </button>
        )}
      </div>
      {expanded && (
        <pre
          style={{
            margin: '8px 0 0',
            padding: '8px 10px',
            maxHeight: 160,
            overflow: 'auto',
            background: 'var(--s0)',
            border: '1px solid var(--red)30',
            borderRadius: 'var(--radsm)',
            fontFamily: 'var(--fm)',
            fontSize: 11,
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            color: 'var(--t1)'
          }}
        >
          {error}
        </pre>
      )}
    </div>
  )
}

function AppShell({ deviceName, deregister }: { deviceName: string; deregister: () => Promise<void> }) {
  const [page, setPage] = useState<NavPage>('dashboard')
  const { integrations, toggle, error, system, dockerProgress } = useIntegrations()
  const activeCount = integrations.filter((i) => i.enabled).length
  const hasUnhealthy = integrations.some((i) => i.enabled && !i.healthy)

  return (
    <div
      style={{
        display: 'flex',
        height: '100%',
        overflow: 'hidden',
        background: 'var(--bg)',
        fontFamily: 'var(--fb)'
      }}
    >
      <Sidebar page={page} onNav={setPage} activeCount={activeCount} hasUnhealthy={hasUnhealthy} deviceName={deviceName} />
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
        <TopBar page={page} />
        {error && <ErrorBanner error={error} />}
        <div style={{ flex: 1, overflow: 'hidden' }}>
          {page === 'dashboard' && <Dashboard intgs={integrations} />}
          {page === 'integrations' && (
            <Integrations intgs={integrations} onToggle={toggle} system={system} dockerProgress={dockerProgress} />
          )}
          {page === 'rewards' && <Rewards />}
          {page === 'settings' && <SettingsPage deviceName={deviceName} deregister={deregister} />}
          {page === 'updates' && <Updates />}
        </div>
      </div>
    </div>
  )
}

export default function FEMApp() {
  const [phase, setPhase] = useState<'wizard' | 'app'>('wizard')
  const [deviceName, setDeviceName] = useState('nimble-swift-wolf')
  const { device, loading, deregister } = useDevice()

  if (loading) return null // wait for get_device_info before deciding Wizard vs AppShell

  // If already registered, skip wizard
  const initialRegistered = device?.registered ?? false
  const effectivePhase = initialRegistered && phase === 'wizard' ? 'app' : phase

  if (effectivePhase === 'wizard') {
    return <Wizard onDone={(name) => { setDeviceName(name); setPhase('app') }} />
  }

  return <AppShell deviceName={deviceName} deregister={deregister} />
}
