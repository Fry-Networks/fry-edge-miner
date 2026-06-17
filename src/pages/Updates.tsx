import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { RefreshCw, Download, Loader2 } from 'lucide-react'
import { useIntegrations } from '../hooks/useIntegrations'
import { PageHeader } from '../components/PageHeader'

export default function Updates() {
  const { integrations, loading, refetch } = useIntegrations()
  const [checking, setChecking] = useState(false)
  const [installingId, setInstallingId] = useState<string | null>(null)
  const [installError, setInstallError] = useState<string | null>(null)

  const handleCheckUpdates = () => {
    setChecking(true)
    setTimeout(() => setChecking(false), 1500)
  }

  return (
    <div className="p-6 lg:p-8 space-y-6">
      <PageHeader
        title="Updates"
        subtitle="Monitor FEM and integration versions"
        action={
          <button
            onClick={handleCheckUpdates}
            disabled={checking}
            className="inline-flex items-center gap-2 px-4 py-2 bg-fry-surface border border-fry-border hover:bg-fry-surface-hover text-fry-text text-sm font-medium rounded-lg transition-colors disabled:opacity-60"
          >
            <RefreshCw className={`w-4 h-4 ${checking ? 'animate-spin' : ''}`} />
            Check for Updates
          </button>
        }
      />

      {/* FEM status card */}
      <div className="bg-fry-surface border-l-4 border-l-fry-neon border border-fry-border rounded-xl p-5">
        <div className="flex items-center gap-3">
          <span
            className="h-2.5 w-2.5 rounded-full bg-fry-neon shrink-0"
            style={{ boxShadow: '0 0 6px #00B69B', animation: 'status-pulse 2s ease-in-out infinite' }}
          />
          <div>
            <p className="text-sm font-medium text-fry-text">Fry Edge Miner — Up to date</p>
            <p className="text-xs text-fry-text-muted mt-0.5">Running the latest release</p>
          </div>
        </div>
      </div>

      {/* Partner list */}
      <div className="space-y-2">
        <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">Partner Software</p>

        {installError && (
          <div className="bg-fry-surface-2 border border-fry-warning text-fry-warning rounded-lg px-4 py-2 text-xs">
            {installError}
          </div>
        )}

        {loading ? (
          <div className="space-y-2">
            {Array.from({ length: 5 }).map((_, idx) => (
              <div key={idx} className="bg-fry-surface border border-fry-border rounded-xl p-4 animate-pulse h-14" />
            ))}
          </div>
        ) : (
          <div className="space-y-2">
            {integrations.map((integration) => {
              const notInstalled = integration.version === null
              const running = !notInstalled && integration.lifecycle === 'Running'

              return (
                <div
                  key={integration.id}
                  className={`bg-fry-surface border border-fry-border rounded-xl px-4 py-3 flex items-center justify-between gap-4 ${
                    notInstalled
                      ? 'border-l-2 border-l-fry-border opacity-70'
                      : running
                        ? 'border-l-2 border-l-fry-neon'
                        : 'border-l-2 border-l-fry-warning'
                  }`}
                >
                  <div className="min-w-0">
                    <span className="text-sm font-medium text-fry-text block truncate">
                      {integration.display_name}
                    </span>
                    {!notInstalled && !running && (
                      <span className="text-xs text-fry-warning">Needs restart</span>
                    )}
                    {notInstalled && (
                      <span className="text-xs text-fry-text-muted">Not installed</span>
                    )}
                  </div>

                  <div className="flex items-center gap-3 shrink-0">
                    {!notInstalled && integration.version && (
                      <span className="font-mono text-xs bg-fry-surface-2 text-fry-text-muted px-2 py-0.5 rounded">
                        {/^\d/.test(integration.version ?? '') ? `v${integration.version}` : integration.version}
                      </span>
                    )}
                    {notInstalled && (
                      <button
                        disabled={installingId === integration.id}
                        onClick={async () => {
                          setInstallingId(integration.id)
                          setInstallError(null)
                          try {
                            await invoke('install_integration', { id: integration.id })
                            await refetch()
                          } catch (e) {
                            const msg = e instanceof Error ? e.message : String(e)
                            setInstallError(`${integration.display_name} install failed: ${msg}`)
                            console.error('Install failed:', e)
                          } finally {
                            setInstallingId(null)
                          }
                        }}
                        className="inline-flex items-center gap-1.5 px-4 py-1.5 bg-fry-neon text-white rounded-lg text-xs font-semibold hover:bg-fry-neon-dim transition-colors disabled:opacity-60"
                      >
                        {installingId === integration.id ? (
                          <><Loader2 className="w-3 h-3 animate-spin" />Installing…</>
                        ) : (
                          <><Download className="w-3 h-3" />Install</>
                        )}
                      </button>
                    )}
                  </div>
                </div>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )
}
