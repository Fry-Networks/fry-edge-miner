import { useIntegrations } from '../hooks/useIntegrations'
import { PageHeader } from '../components/PageHeader'

export default function Updates() {
  const { integrations, loading } = useIntegrations()

  const handleCheckUpdates = () => {
    console.log('Checking for updates...')
  }

  return (
    <div className="p-6 lg:p-8 space-y-6">
      <PageHeader title="Updates" subtitle="Monitor FEM and integration versions" />

      {/* Status Card */}
      <div className="bg-fry-surface border-l-4 border-l-fry-neon border border-fry-border rounded-lg p-5">
        <div className="flex items-center gap-3">
          <span className="h-2 w-2 rounded-full bg-fry-neon animate-pulse" />
          <div>
            <p className="text-sm font-medium text-fry-text">Up to date</p>
            <p className="text-xs text-fry-text-muted">FEM is running the latest version</p>
          </div>
        </div>
      </div>

      {/* Check Updates Button */}
      <button
        onClick={handleCheckUpdates}
        className="inline-flex items-center gap-2 px-5 py-2 bg-fry-surface border border-fry-border hover:bg-fry-surface-hover text-fry-text text-sm font-medium rounded-lg transition-colors"
      >
        <svg className="w-4 h-4" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <path d="M14 8A6 6 0 114.8 3.8" />
          <path d="M14 2v4h-4" />
        </svg>
        Check for Updates
      </button>

      {/* Partner Versions */}
      <div className="space-y-3">
        <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">Partner Versions</p>

        {loading ? (
          <div className="space-y-1">
            {Array.from({ length: 5 }).map((_, idx) => (
              <div
                key={idx}
                className="rounded-lg p-3 animate-pulse"
              >
                <div className="h-4 bg-fry-border-subtle rounded w-32" />
              </div>
            ))}
          </div>
        ) : (
          <div className="space-y-1">
            {integrations.map((integration) => (
              <div
                key={integration.id}
                className="flex items-center justify-between px-4 py-3 rounded-lg hover:bg-fry-surface-hover transition-colors"
              >
                <span className="text-sm font-medium text-fry-text">
                  {integration.display_name}
                </span>
                {integration.version ? (
                  <span className="font-mono text-xs bg-fry-surface-2 text-fry-text-muted px-2 py-0.5 rounded">
                    v{integration.version}
                  </span>
                ) : (
                  <span className="text-xs text-fry-text-muted italic">Not installed</span>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
