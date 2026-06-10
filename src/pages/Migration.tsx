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
        <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-8 text-center">
          <div className="animate-pulse text-fry-text-muted">Checking for existing FryHub installation...</div>
        </div>
      </div>
    )
  }

  if (step === -1) {
    return (
      <div className="p-8">
        <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-8 text-center">
          <h2 className="text-xl font-semibold text-fry-text mb-2">No Migration Needed</h2>
          <p className="text-fry-text-muted">No existing FryHub installation was detected on this machine.</p>
        </div>
      </div>
    )
  }

  return (
    <div className="p-8 space-y-6">
      <h1 className="text-3xl font-bold text-fry-text">FryHub → FEM Migration</h1>

      {/* Step 1: Detected */}
      {step >= 1 && migrationInfo && (
        <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-6 space-y-4">
          <div className="flex items-center gap-3">
            <span className="flex h-8 w-8 items-center justify-center rounded-full bg-fry-red/20 text-fry-red text-sm font-bold">1</span>
            <h2 className="text-lg font-semibold text-fry-text">Existing Installation Detected</h2>
          </div>
          <p className="text-fry-text-muted ml-11">
            Found {migrationInfo.found_keys.length} active miner{migrationInfo.found_keys.length > 1 ? 's' : ''} in FryHub:
          </p>
          <div className="ml-11 space-y-2">
            {migrationInfo.found_keys.map((mk) => (
              <div key={mk.key} className="flex items-center gap-3 bg-fry-border/50 rounded-lg px-4 py-3">
                <span className="text-xs font-mono bg-fry-border px-2 py-1 rounded text-fry-text">{mk.miner_type}</span>
                <span className="text-fry-text">{mk.display_name}</span>
                <span className="text-xs text-fry-text-muted font-mono ml-auto">{mk.key.slice(0, 20)}...</span>
              </div>
            ))}
          </div>
          {step === 1 && (
            <div className="ml-11">
              <button
                onClick={() => setStep(2)}
                className="px-6 py-2 bg-fry-red/60 hover:bg-fry-red/50 rounded-lg font-medium transition-colors"
              >
                Continue
              </button>
            </div>
          )}
        </div>
      )}

      {/* Step 2: Confirm */}
      {step >= 2 && migrationInfo && (
        <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-6 space-y-4">
          <div className="flex items-center gap-3">
            <span className="flex h-8 w-8 items-center justify-center rounded-full bg-fry-red/20 text-fry-red text-sm font-bold">2</span>
            <h2 className="text-lg font-semibold text-fry-text">Migration Plan</h2>
          </div>
          <p className="text-fry-text-muted ml-11">
            FEM will consolidate your miners into a single device with these integrations enabled:
          </p>
          <div className="ml-11 flex flex-wrap gap-2">
            {migrationInfo.suggested_integrations.map((id) => (
              <span key={id} className="px-3 py-1 bg-fry-red/20 text-fry-red rounded-full text-sm">
                {INTEGRATION_LABELS[id] || id}
              </span>
            ))}
          </div>
          <div className="ml-11 space-y-3">
            <label className="block">
              <span className="text-sm text-fry-text-muted">Wallet Address</span>
              <input
                type="text"
                value={wallet}
                onChange={(e) => setWallet(e.target.value)}
                placeholder="Enter your Algorand wallet address (58 chars)"
                className="mt-1 block w-full bg-fry-surface-hover border border-fry-border rounded-lg px-4 py-2 text-fry-text placeholder-fry-text-muted/50 focus:border-fry-red focus:ring-1 focus:ring-fry-red/50"
              />
            </label>
          </div>
          {error && (
            <p className="ml-11 text-fry-error text-sm">{error}</p>
          )}
          {step === 2 && (
            <div className="ml-11 flex gap-3">
              <button
                onClick={executeMigration}
                className="px-6 py-2 bg-fry-red/60 hover:bg-fry-red/50 rounded-lg font-medium transition-colors"
              >
                Migrate to FEM
              </button>
              <button
                onClick={() => setStep(1)}
                className="px-6 py-2 bg-fry-border hover:bg-fry-border/80 rounded-lg font-medium transition-colors text-fry-text"
              >
                Back
              </button>
            </div>
          )}
        </div>
      )}

      {/* Step 3: Migrating */}
      {step === 3 && (
        <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-6">
          <div className="flex items-center gap-3">
            <span className="flex h-8 w-8 items-center justify-center rounded-full bg-fry-warning/20 text-fry-warning text-sm font-bold">3</span>
            <h2 className="text-lg font-semibold text-fry-text">Migrating...</h2>
          </div>
          <div className="ml-11 mt-3 animate-pulse text-fry-text-muted">
            Setting up your Fry Edge Miner device...
          </div>
        </div>
      )}

      {/* Step 4: Done */}
      {step === 4 && result && (
        <div className="bg-fry-neon/10 border border-fry-neon/30 rounded-xl p-6 space-y-4">
          <div className="flex items-center gap-3">
            <span className="flex h-8 w-8 items-center justify-center rounded-full bg-fry-neon/20 text-fry-neon text-sm font-bold">✓</span>
            <h2 className="text-lg font-semibold text-fry-neon">Migration Complete</h2>
          </div>
          <div className="ml-11 space-y-3">
            <div>
              <p className="text-sm text-fry-text-muted">New FEM Key</p>
              <p className="font-mono text-sm text-fry-text break-all">{result.fem_key}</p>
            </div>
            <div>
              <p className="text-sm text-fry-text-muted">Enabled Integrations</p>
              <div className="flex flex-wrap gap-2 mt-1">
                {result.enabled_integrations.map((id) => (
                  <span key={id} className="px-3 py-1 bg-fry-neon/20 text-fry-neon rounded-full text-sm">
                    {INTEGRATION_LABELS[id] || id}
                  </span>
                ))}
              </div>
            </div>
            <p className="text-xs text-fry-text-muted/60 mt-4">
              Your old miner keys will be deactivated after the next reward cycle.
            </p>
          </div>
        </div>
      )}
    </div>
  )
}
