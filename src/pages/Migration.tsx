import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'

interface DetectedMinerKey {
  key: string
  miner_type: string
  display_name: string
}

interface MigrationInfo {
  found_keys: DetectedMinerKey[]
  wallet: string | null
  suggested_integrations: string[]
}

interface MigrationResult {
  fem_key: string
  enabled_integrations: string[]
  migrated_keys: string[]
}

const INTEGRATION_LABELS: Record<string, string> = {
  mysterium: 'MystNodes',
  presearch: 'Presearch',
  diiisco: 'Diiisco',
  space_acres: 'SpaceAcres',
  aem: 'AEM',
}

export default function Migration() {
  const [step, setStep] = useState(0) // 0=checking, 1=detected, 2=confirm, 3=migrating, 4=done
  const [migrationInfo, setMigrationInfo] = useState<MigrationInfo | null>(null)
  const [wallet, setWallet] = useState('')
  const [result, setResult] = useState<MigrationResult | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    checkMigration()
  }, [])

  async function checkMigration() {
    try {
      const info = await invoke<MigrationInfo | null>('check_migration')
      if (info && info.found_keys.length > 0) {
        setMigrationInfo(info)
        setWallet(info.wallet || '')
        setStep(1)
      } else {
        setStep(-1) // no migration needed
      }
    } catch (e) {
      setError(String(e))
      setStep(-1)
    }
  }

  async function executeMigration() {
    if (!wallet) {
      setError('Please enter your Algorand wallet address')
      return
    }
    setStep(3)
    setError(null)
    try {
      const res = await invoke<MigrationResult>('run_migration', { wallet })
      setResult(res)
      setStep(4)
    } catch (e) {
      setError(String(e))
      setStep(2)
    }
  }

  if (step === 0) {
    return (
      <div className="p-8">
        <div className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-8 text-center">
          <div className="animate-pulse text-gray-400">Checking for existing FryHub installation...</div>
        </div>
      </div>
    )
  }

  if (step === -1) {
    return (
      <div className="p-8">
        <div className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-8 text-center">
          <h2 className="text-xl font-semibold text-white mb-2">No Migration Needed</h2>
          <p className="text-gray-400">No existing FryHub installation was detected on this machine.</p>
        </div>
      </div>
    )
  }

  return (
    <div className="p-8 space-y-6">
      <h1 className="text-3xl font-bold text-white">FryHub → FEM Migration</h1>

      {/* Step 1: Detected */}
      {step >= 1 && migrationInfo && (
        <div className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-6 space-y-4">
          <div className="flex items-center gap-3">
            <span className="flex h-8 w-8 items-center justify-center rounded-full bg-emerald-500/20 text-emerald-400 text-sm font-bold">1</span>
            <h2 className="text-lg font-semibold text-white">Existing Installation Detected</h2>
          </div>
          <p className="text-gray-400 ml-11">
            Found {migrationInfo.found_keys.length} active miner{migrationInfo.found_keys.length > 1 ? 's' : ''} in FryHub:
          </p>
          <div className="ml-11 space-y-2">
            {migrationInfo.found_keys.map((mk) => (
              <div key={mk.key} className="flex items-center gap-3 bg-gray-800/50 rounded-lg px-4 py-3">
                <span className="text-xs font-mono bg-gray-700 px-2 py-1 rounded text-gray-300">{mk.miner_type}</span>
                <span className="text-gray-300">{mk.display_name}</span>
                <span className="text-xs text-gray-500 font-mono ml-auto">{mk.key.slice(0, 20)}...</span>
              </div>
            ))}
          </div>
          {step === 1 && (
            <div className="ml-11">
              <button
                onClick={() => setStep(2)}
                className="px-6 py-2 bg-emerald-600 hover:bg-emerald-500 rounded-lg font-medium transition-colors"
              >
                Continue
              </button>
            </div>
          )}
        </div>
      )}

      {/* Step 2: Confirm */}
      {step >= 2 && migrationInfo && (
        <div className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-6 space-y-4">
          <div className="flex items-center gap-3">
            <span className="flex h-8 w-8 items-center justify-center rounded-full bg-emerald-500/20 text-emerald-400 text-sm font-bold">2</span>
            <h2 className="text-lg font-semibold text-white">Migration Plan</h2>
          </div>
          <p className="text-gray-400 ml-11">
            FEM will consolidate your miners into a single device with these integrations enabled:
          </p>
          <div className="ml-11 flex flex-wrap gap-2">
            {migrationInfo.suggested_integrations.map((id) => (
              <span key={id} className="px-3 py-1 bg-emerald-500/20 text-emerald-400 rounded-full text-sm">
                {INTEGRATION_LABELS[id] || id}
              </span>
            ))}
          </div>
          <div className="ml-11 space-y-3">
            <label className="block">
              <span className="text-sm text-gray-400">Wallet Address</span>
              <input
                type="text"
                value={wallet}
                onChange={(e) => setWallet(e.target.value)}
                placeholder="Enter your Algorand wallet address (58 chars)"
                className="mt-1 block w-full bg-gray-800 border border-gray-700 rounded-lg px-4 py-2 text-white placeholder-gray-600 focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500/50"
              />
            </label>
          </div>
          {error && (
            <p className="ml-11 text-red-400 text-sm">{error}</p>
          )}
          {step === 2 && (
            <div className="ml-11 flex gap-3">
              <button
                onClick={executeMigration}
                className="px-6 py-2 bg-emerald-600 hover:bg-emerald-500 rounded-lg font-medium transition-colors"
              >
                Migrate to FEM
              </button>
              <button
                onClick={() => setStep(1)}
                className="px-6 py-2 bg-gray-700 hover:bg-gray-600 rounded-lg font-medium transition-colors"
              >
                Back
              </button>
            </div>
          )}
        </div>
      )}

      {/* Step 3: Migrating */}
      {step === 3 && (
        <div className="bg-gray-900/80 border border-gray-800/60 rounded-xl p-6">
          <div className="flex items-center gap-3">
            <span className="flex h-8 w-8 items-center justify-center rounded-full bg-amber-500/20 text-amber-400 text-sm font-bold">3</span>
            <h2 className="text-lg font-semibold text-white">Migrating...</h2>
          </div>
          <div className="ml-11 mt-3 animate-pulse text-gray-400">
            Setting up your Fry Edge Miner device...
          </div>
        </div>
      )}

      {/* Step 4: Done */}
      {step === 4 && result && (
        <div className="bg-emerald-500/10 border border-emerald-500/30 rounded-xl p-6 space-y-4">
          <div className="flex items-center gap-3">
            <span className="flex h-8 w-8 items-center justify-center rounded-full bg-emerald-500/20 text-emerald-400 text-sm font-bold">✓</span>
            <h2 className="text-lg font-semibold text-emerald-400">Migration Complete</h2>
          </div>
          <div className="ml-11 space-y-3">
            <div>
              <p className="text-sm text-gray-500">New FEM Key</p>
              <p className="font-mono text-sm text-white break-all">{result.fem_key}</p>
            </div>
            <div>
              <p className="text-sm text-gray-500">Enabled Integrations</p>
              <div className="flex flex-wrap gap-2 mt-1">
                {result.enabled_integrations.map((id) => (
                  <span key={id} className="px-3 py-1 bg-emerald-500/20 text-emerald-400 rounded-full text-sm">
                    {INTEGRATION_LABELS[id] || id}
                  </span>
                ))}
              </div>
            </div>
            <p className="text-xs text-gray-500 mt-4">
              Your old miner keys will be deactivated after the next reward cycle.
            </p>
          </div>
        </div>
      )}
    </div>
  )
}
