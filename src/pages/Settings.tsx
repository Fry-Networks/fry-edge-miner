import { useState, useEffect } from 'react'
import { getVersion } from '@tauri-apps/api/app'
import { useDevice } from '../hooks/useDevice'
import { useSettings } from '../hooks/useSettings'
import { Toggle } from '../components/Toggle'
import { CopyField } from '../components/CopyField'
import { PageHeader } from '../components/PageHeader'

const PREF_DEFS = [
  { key: 'start_on_boot', label: 'Start on boot' },
  { key: 'minimize_to_tray', label: 'Minimize to tray' },
  { key: 'auto_update', label: 'Auto-update' },
  { key: 'notifications', label: 'Notifications' },
] as const

type PrefKey = typeof PREF_DEFS[number]['key']

function loadPrefs(): Record<PrefKey, boolean> {
  return Object.fromEntries(
    PREF_DEFS.map(({ key }) => [key, localStorage.getItem(`pref_${key}`) !== 'false'])
  ) as Record<PrefKey, boolean>
}

export default function Settings() {
  const { device, register, deregister, loading: deviceLoading } = useDevice()
  const { config, save, loading: configLoading } = useSettings()
  const [walletInput, setWalletInput] = useState('')
  const [apiUrlInput, setApiUrlInput] = useState('')
  const [savedMessage, setSavedMessage] = useState('')
  const [version, setVersion] = useState<string>('\u2014')
  // TODO: wire to Tauri plugin-autostart/tray when backend commands added
  const [prefs, setPrefs] = useState<Record<PrefKey, boolean>>(loadPrefs)

  useEffect(() => {
    getVersion().then((v) => setVersion(`v${v}`)).catch(() => setVersion('v0.2.6'))
  }, [])

  const setPref = (key: PrefKey, value: boolean) => {
    localStorage.setItem(`pref_${key}`, String(value))
    setPrefs((p) => ({ ...p, [key]: value }))
  }

  const toast = (msg: string) => {
    setSavedMessage(msg)
    setTimeout(() => setSavedMessage(''), 3000)
  }

  const handleRegister = async () => {
    if (!walletInput.trim()) return
    try {
      await register(walletInput)
      toast('Device registered successfully')
    } catch (error) {
      toast(`Registration failed: ${error}`)
    }
  }

  const handleSaveApiUrl = async () => {
    if (!apiUrlInput.trim()) return
    try {
      await save({ api_base_url: apiUrlInput })
      toast('API URL updated successfully')
    } catch (error) {
      toast('Failed to save API URL')
    }
  }

  const handleDeregister = async () => {
    if (!window.confirm('Deregister this device? This removes your miner key and stops all integrations.')) return
    try {
      await deregister()
      setWalletInput('')
      toast('Device deregistered successfully')
    } catch (error) {
      toast(`Deregistration failed: ${error}`)
    }
  }

  const isError = savedMessage.toLowerCase().includes('failed')

  return (
    <div className="p-6 lg:p-8 space-y-6">
      <PageHeader title="Settings" subtitle="Configure your Fry Edge Miner" />

      {savedMessage && (
        <div className={`flex items-center gap-3 bg-fry-surface border-l-4 ${isError ? 'border-l-fry-error' : 'border-l-fry-neon'} border border-fry-border px-4 py-3 rounded-lg`}>
          <span className={`text-sm ${isError ? 'text-fry-error' : 'text-fry-neon'}`}>{savedMessage}</span>
        </div>
      )}

      <div className="max-w-2xl space-y-6">
        {/* Card 1: Device Info */}
        <div className="bg-fry-surface border border-fry-border rounded-xl p-6 space-y-4">
          <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">Device Info</p>
          {deviceLoading ? (
            <div className="space-y-3">
              <div className="h-10 bg-fry-border-subtle rounded animate-pulse" />
              <div className="h-10 bg-fry-border-subtle rounded animate-pulse" />
            </div>
          ) : (
            <>
              <CopyField value={device?.miner_key ?? null} label="Miner Key" truncate={24} />
              <CopyField value={device?.wallet_address ?? null} label="Wallet Address" truncate={24} />
              {device?.miner_key && (
                <div className="pt-2 border-t border-fry-border-subtle">
                  <button
                    onClick={handleDeregister}
                    className="px-4 py-2 bg-fry-red/10 text-fry-red hover:bg-fry-red/20 border border-fry-red/30 rounded-lg text-sm font-medium transition-colors"
                  >
                    Deregister Device
                  </button>
                  <p className="text-xs text-fry-text-muted/60 mt-1">
                    Removes miner key and stops all integrations
                  </p>
                </div>
              )}
            </>
          )}
        </div>

        {/* Card 2: Reward Wallet */}
        <div className="bg-fry-surface border border-fry-border rounded-xl p-6 space-y-4">
          <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">Reward Wallet</p>
          <div>
            <label className="block text-xs text-fry-text-muted mb-1">Algorand Wallet Address</label>
            <div className="flex gap-2">
              <input
                type="text"
                value={walletInput || device?.wallet_address || ''}
                onChange={(e) => setWalletInput(e.target.value)}
                placeholder="58-character Algorand address"
                className="flex-1 px-4 py-2 bg-fry-surface-2 border border-fry-border rounded-lg text-fry-text focus-visible:ring-2 focus-visible:ring-fry-neon focus-visible:outline-none transition font-mono text-xs"
              />
              <button
                onClick={handleRegister}
                className="px-5 py-2 bg-fry-neon/15 text-fry-neon hover:bg-fry-neon/25 border border-fry-neon/40 rounded-lg text-sm font-medium transition-colors shrink-0"
              >
                {device?.registered ? 'Update' : 'Register'}
              </button>
            </div>
            <p className="text-xs text-fry-text-muted/60 mt-1">Rewards will be sent to this address</p>
          </div>
        </div>

        {/* Card 3: API */}
        <div className="bg-fry-surface border border-fry-border rounded-xl p-6 space-y-4">
          <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">API</p>
          {configLoading ? (
            <div className="h-10 bg-fry-border-subtle rounded animate-pulse" />
          ) : (
            <div>
              <label className="block text-xs text-fry-text-muted mb-1">Base URL</label>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={apiUrlInput || config?.api_base_url || 'https://hardwareapi.frynetworks.com'}
                  onChange={(e) => setApiUrlInput(e.target.value)}
                  placeholder="https://hardwareapi.frynetworks.com"
                  className="flex-1 px-4 py-2 bg-fry-surface-2 border border-fry-border rounded-lg text-fry-text focus-visible:ring-2 focus-visible:ring-fry-neon focus-visible:outline-none transition font-mono text-xs"
                />
                <button
                  onClick={handleSaveApiUrl}
                  className="px-5 py-2 bg-fry-neon/15 text-fry-neon hover:bg-fry-neon/25 border border-fry-neon/40 rounded-lg text-sm font-medium transition-colors shrink-0"
                >
                  Save
                </button>
              </div>
              <p className="text-xs text-fry-text-muted/60 mt-1">Hardware API endpoint for reward distribution</p>
            </div>
          )}
        </div>

        {/* Card 4: Preferences */}
        <div className="bg-fry-surface border border-fry-border rounded-xl p-6 space-y-4">
          <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">Preferences</p>
          <div className="space-y-3">
            {PREF_DEFS.map(({ key, label }) => (
              <div key={key} className="flex items-center justify-between">
                <span className="text-sm text-fry-text">{label}</span>
                {/* TODO: wire to Tauri plugin-autostart / tray / notification API when backend commands added */}
                <Toggle checked={prefs[key]} onChange={() => setPref(key, !prefs[key])} />
              </div>
            ))}
          </div>
        </div>

        {/* Card 5: About */}
        <div className="bg-fry-surface border border-fry-border rounded-xl p-6 space-y-3">
          <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">About</p>
          <div className="text-sm divide-y divide-fry-border-subtle">
            <div className="flex justify-between py-2">
              <span className="text-fry-text-muted">App Version</span>
              <span className="font-mono text-xs text-fry-text">{version}</span>
            </div>
            <div className="flex justify-between py-2">
              <span className="text-fry-text-muted">Build Type</span>
              <span className="font-mono text-xs text-fry-text">Release</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
