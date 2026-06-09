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
        <h1 className="text-4xl font-bold text-white mb-2">Updates</h1>
        <p className="text-gray-400">Monitor FEM and partner versions</p>
      </div>

      {/* Status Card */}
      <div className="bg-emerald-500/20 border border-emerald-500/50 rounded-xl p-6">
        <p className="text-emerald-400 font-medium mb-1">Up to date</p>
        <p className="text-sm text-emerald-300">
          FEM is running the latest version
        </p>
      </div>

      {/* Check Updates Button */}
      <button
        onClick={handleCheckUpdates}
        className="px-6 py-3 bg-emerald-500/20 text-emerald-400 hover:bg-emerald-500/30 border border-emerald-500/50 rounded-lg font-medium transition"
      >
        Check for Updates
      </button>

      {/* Partner Versions */}
      <div className="space-y-4">
        <h2 className="text-xl font-semibold text-white">Partner Versions</h2>

        {loading ? (
          <div className="space-y-2">
            {Array.from({ length: 5 }).map((_, idx) => (
              <div
                key={idx}
                className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-4 animate-pulse"
              >
                <div className="h-6 bg-gray-800 rounded" />
              </div>
            ))}
          </div>
        ) : (
          <div className="space-y-2">
            {integrations.map((integration) => (
              <div
                key={integration.id}
                className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-4 flex items-center justify-between"
              >
                <div>
                  <p className="text-white font-medium">
                    {integration.display_name}
                  </p>
                </div>
                <div>
                  {integration.version ? (
                    <p className="text-gray-400 font-mono text-sm">
                      v{integration.version}
                    </p>
                  ) : (
                    <p className="text-gray-600 text-sm">No version</p>
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
