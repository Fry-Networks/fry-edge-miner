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

  // Step 0: Checking
  if (step === 0) {
    return (
      <div className="p-6 lg:p-8">
        <div className="flex items-center justify-center py-16 gap-3">
          <div className="h-4 w-4 rounded-full border-2 border-fry-border border-t-fry-neon animate-spin" />
          <p className="text-sm text-fry-text-muted">Checking for existing FryHub installation...</p>
        </div>
      </div>
    )
  }

  // Step -1: No migration needed
  if (step === -1) {
    return (
      <div className="p-6 lg:p-8">
        <div className="flex flex-col items-center justify-center py-24 text-center gap-4">
          <div className="h-12 w-12 rounded-full bg-fry-surface-2 flex items-center justify-center">
            <svg className="w-6 h-6 text-fry-neon" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M3.5 8.5l3 3 6-7" />
            </svg>
          </div>
          <div>
            <p className="text-sm font-medium text-fry-text">No migration required</p>
            <p className="text-xs text-fry-text-muted mt-1">No FryHub installation was detected on this machine.</p>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="p-6 lg:p-8 space-y-6">
      <h1 className="text-2xl font-medium tracking-tight text-fry-text">FryHub Migration</h1>

      {/* Step Progress Indicator */}
      {step >= 1 && (
        <div className="flex items-center gap-2 text-xs text-fry-text-muted">
          {[1, 2, 3, 4].map((n) => (
            <div key={n} className="contents">
              <span
                className={`h-6 w-6 rounded-full flex items-center justify-center text-xs font-medium ${
                  step > n
                    ? 'bg-fry-red/80 text-white'
                    : step === n
                      ? 'bg-fry-red text-white'
                      : 'bg-fry-surface-2 text-fry-text-muted'
                }`}
              >
                {step > n ? '\u2713' : n}
              </span>
              {n < 4 && (
                <span className={`flex-1 h-px ${step > n ? 'bg-fry-red/50' : 'bg-fry-border-subtle'}`} />
              )}
            </div>
          ))}
        </div>
      )}

      {/* Step 1: Detected */}
      {step >= 1 && migrationInfo && (
        <div className="bg-fry-surface border border-fry-border rounded-xl p-6 space-y-4">
          <div className="flex items-center gap-3">
            <span className="flex h-8 w-8 items-center justify-center rounded-full bg-fry-red/20 text-fry-red text-sm font-bold">1</span>
            <h2 className="text-sm font-medium text-fry-text">Existing Installation Detected</h2>
          </div>
          <p className="text-xs text-fry-text-muted ml-11">
            Found {migrationInfo.found_keys.length} active miner{migrationInfo.found_keys.length > 1 ? 's' : ''} in FryHub:
          </p>
          <div className="ml-11 space-y-2">
            {migrationInfo.found_keys.map((mk) => (
              <div key={mk.key} className="flex items-center gap-3 bg-fry-surface-2 border border-fry-border-subtle rounded-lg px-3 py-2">
                <span className="text-xs font-mono bg-fry-border px-2 py-0.5 rounded text-fry-text-muted">{mk.miner_type}</span>
                <span className="text-sm text-fry-text">{mk.display_name}</span>
                <span className="text-xs text-fry-text-muted font-mono ml-auto">{mk.key.slice(0, 20)}...</span>
              </div>
            ))}
          </div>
          {step === 1 && (
            <div className="ml-11">
              <button
                onClick={() => setStep(2)}
                className="px-5 py-2 bg-fry-surface-hover border border-fry-border hover:bg-fry-border text-fry-text rounded-lg text-sm font-medium transition-colors"
              >
                Continue
              </button>
            </div>
          )}
        </div>
      )}

      {/* Step 2: Confirm */}
      {step >= 2 && migrationInfo && (
        <div className="bg-fry-surface border border-fry-border rounded-xl p-6 space-y-4">
          <div className="flex items-center gap-3">
            <span className="flex h-8 w-8 items-center justify-center rounded-full bg-fry-red/20 text-fry-red text-sm font-bold">2</span>
            <h2 className="text-sm font-medium text-fry-text">Migration Plan</h2>
          </div>
          <p className="text-xs text-fry-text-muted ml-11">
            FEM will consolidate your miners into a single device with these integrations enabled:
          </p>
          <div className="ml-11 flex flex-wrap gap-2">
            {migrationInfo.suggested_integrations.map((id) => (
              <span key={id} className="px-2 py-0.5 bg-fry-surface-2 text-fry-text border border-fry-border rounded-full text-xs">
                {INTEGRATION_LABELS[id] || id}
              </span>
            ))}
          </div>
          <div className="ml-11 space-y-3">
            <label className="block">
              <span className="text-xs text-fry-text-muted">Wallet Address</span>
              <input
                type="text"
                value={wallet}
                onChange={(e) => setWallet(e.target.value)}
                placeholder="Enter your Algorand wallet address (58 chars)"
                className="mt-1 block w-full bg-fry-surface-2 border border-fry-border rounded-lg px-4 py-2 text-fry-text text-sm placeholder-fry-text-muted/50 focus-visible:ring-2 focus-visible:ring-fry-neon focus-visible:outline-none"
              />
            </label>
          </div>
          {error && (
            <p className="ml-11 text-fry-error text-xs">{error}</p>
          )}
          {step === 2 && (
            <div className="ml-11 flex gap-3">
              <button
                onClick={executeMigration}
                className="px-5 py-2 bg-fry-red text-white hover:bg-fry-red-dim rounded-lg text-sm font-medium transition-colors"
              >
                Migrate to FEM
              </button>
              <button
                onClick={() => setStep(1)}
                className="px-5 py-2 bg-fry-surface-hover border border-fry-border hover:bg-fry-border text-fry-text rounded-lg text-sm font-medium transition-colors"
              >
                Back
              </button>
            </div>
          )}
        </div>
      )}

      {/* Step 3: Migrating */}
      {step === 3 && (
        <div className="bg-fry-surface border border-fry-border rounded-xl p-6">
          <div className="flex items-center gap-3">
            <span className="flex h-8 w-8 items-center justify-center rounded-full bg-fry-warning/20 text-fry-warning text-sm font-bold">3</span>
            <h2 className="text-sm font-medium text-fry-text">Migrating...</h2>
          </div>
          <div className="ml-11 mt-3 flex items-center gap-3">
            <div className="h-4 w-4 rounded-full border-2 border-fry-border border-t-fry-neon animate-spin" />
            <p className="text-xs text-fry-text-muted">Setting up your Fry Edge Miner device...</p>
          </div>
        </div>
      )}

      {/* Step 4: Done */}
      {step === 4 && result && (
        <div className="bg-fry-surface border-l-4 border-l-fry-neon border border-fry-border rounded-xl p-6 space-y-4">
          <div className="flex items-center gap-3">
            <span className="flex h-8 w-8 items-center justify-center rounded-full bg-fry-neon/20 text-fry-neon text-sm font-bold">{'\u2713'}</span>
            <h2 className="text-sm font-medium text-fry-neon">Migration Complete</h2>
          </div>
          <div className="ml-11 space-y-3">
            <div>
              <p className="text-xs text-fry-text-muted">New FEM Key</p>
              <p className="font-mono text-xs text-fry-text break-all mt-0.5">{result.fem_key}</p>
            </div>
            <div>
              <p className="text-xs text-fry-text-muted">Enabled Integrations</p>
              <div className="flex flex-wrap gap-2 mt-1">
                {result.enabled_integrations.map((id) => (
                  <span key={id} className="px-2 py-0.5 bg-fry-neon/10 text-fry-neon border border-fry-neon/30 rounded-md text-xs">
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
