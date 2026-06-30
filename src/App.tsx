import { useState } from 'react'
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

function AppShell({ deviceName, deregister }: { deviceName: string; deregister: () => Promise<void> }) {
  const [page, setPage] = useState<NavPage>('dashboard')
  const { integrations, toggle } = useIntegrations()
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
        <div style={{ flex: 1, overflow: 'hidden' }}>
          {page === 'dashboard' && <Dashboard intgs={integrations} />}
          {page === 'integrations' && <Integrations intgs={integrations} onToggle={toggle} />}
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
