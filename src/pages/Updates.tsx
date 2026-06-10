import { useIntegrations } from '../hooks/useIntegrations'

export default function Updates() {
  const { integrations, loading } = useIntegrations()

  const handleCheckUpdates = () => {
    console.log('Checking for updates...')
  }

  return (
    <div className="p-8 space-y-8">
      {/* Page Header */}
      <div>
        <h1 className="text-4xl font-bold text-fry-text mb-2">Updates</h1>
        <p className="text-fry-text-muted">Monitor FEM and partner versions</p>
      </div>

      {/* Status Card */}
      <div className="bg-fry-neon/20 border border-fry-neon/50 rounded-xl p-6">
        <p className="text-fry-neon font-medium mb-1">Up to date</p>
        <p className="text-sm text-fry-neon/90">
          FEM is running the latest version
        </p>
      </div>

      {/* Check Updates Button */}
      <button
        onClick={handleCheckUpdates}
        className="px-6 py-3 bg-fry-red/20 text-fry-red hover:bg-fry-red/30 border border-fry-red/50 rounded-lg font-medium transition"
      >
        Check for Updates
      </button>

      {/* Partner Versions */}
      <div className="space-y-4">
        <h2 className="text-xl font-semibold text-fry-text">Partner Versions</h2>

        {loading ? (
          <div className="space-y-2">
            {Array.from({ length: 5 }).map((_, idx) => (
              <div
                key={idx}
                className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-4 animate-pulse"
              >
                <div className="h-6 bg-fry-border rounded" />
              </div>
            ))}
          </div>
        ) : (
          <div className="space-y-2">
            {integrations.map((integration) => (
              <div
                key={integration.id}
                className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-4 flex items-center justify-between"
              >
                <div>
                  <p className="text-fry-text font-medium">
                    {integration.display_name}
                  </p>
                </div>
                <div>
                  {integration.version ? (
                    <p className="text-fry-text-muted font-mono text-sm">
                      v{integration.version}
                    </p>
                  ) : (
                    <p className="text-fry-text-muted/60 text-sm">No version</p>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
