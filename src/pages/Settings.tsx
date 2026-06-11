import { useState, useEffect } from 'react'
import { getVersion } from '@tauri-apps/api/app'
import { useDevice } from '../hooks/useDevice'
import { useSettings } from '../hooks/useSettings'
import { PageHeader } from '../components/PageHeader'

export default function Settings() {
  const { device, register, deregister, loading: deviceLoading } = useDevice()
  const { config, save, loading: configLoading } = useSettings()
  const [walletInput, setWalletInput] = useState('')
  const [apiUrlInput, setApiUrlInput] = useState('')
  const [savedMessage, setSavedMessage] = useState('')
  const [version, setVersion] = useState<string>('\u2014')

  useEffect(() => {
    getVersion().then((v) => setVersion(`v${v}`)).catch(() => setVersion('v0.2.2'))
  }, [])

  const handleRegister = async () => {
    if (!walletInput.trim()) return
    try {
      await register(walletInput)
      setSavedMessage('Device registered successfully')
      setTimeout(() => setSavedMessage(''), 3000)
    } catch (error) {
      setSavedMessage(`Registration failed: ${error}`)
    }
  }

  const handleSaveApiUrl = async () => {
    if (!apiUrlInput.trim()) return
    try {
      await save({ api_base_url: apiUrlInput })
      setSavedMessage('API URL updated successfully')
      setTimeout(() => setSavedMessage(''), 3000)
    } catch (error) {
      setSavedMessage('Failed to save API URL')
    }
  }

  const handleDeregister = async () => {
    if (!window.confirm('Are you sure you want to deregister this device? This will remove your miner key and stop all integrations.')) return
    try {
      await deregister()
      setWalletInput('')
      setSavedMessage('Device deregistered successfully')
      setTimeout(() => setSavedMessage(''), 3000)
    } catch (error) {
      setSavedMessage(`Deregistration failed: ${error}`)
    }
  }

  const isError = savedMessage.toLowerCase().includes('failed')

  return (
    <div className="p-6 lg:p-8 space-y-6">
      <PageHeader title="Settings" subtitle="Configure your Fry Edge Miner" />

      {/* Toast */}
      {savedMessage && (
        <div className={`flex items-center gap-3 bg-fry-surface border-l-4 ${isError ? 'border-l-fry-error' : 'border-l-fry-neon'} border border-fry-border px-4 py-3 rounded-lg`}>
          <span className={`text-sm ${isError ? 'text-fry-error' : 'text-fry-neon'}`}>{savedMessage}</span>
        </div>
      )}

      <div className="max-w-2xl space-y-6">
        {/* Device Section */}
        <div className="bg-fry-surface border border-fry-border rounded-xl p-6 space-y-6">
          <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">Device</p>

          {deviceLoading ? (
            <div className="space-y-4">
              <div className="h-12 bg-fry-border-subtle rounded animate-pulse" />
              <div className="h-12 bg-fry-border-subtle rounded animate-pulse" />
            </div>
          ) : (
            <>
              {/* Miner Key (Read-only) */}
              <div>
                <label className="block text-xs font-medium text-fry-text-muted mb-2">
                  Miner Key
                </label>
                <input
                  type="text"
                  value={device?.miner_key || ''}
                  disabled
                  className="w-full px-4 py-2 bg-fry-surface-2 border border-fry-border rounded-lg text-fry-text-muted cursor-not-allowed font-mono text-xs select-all"
                  placeholder="Miner key (read-only)"
                />
                <p className="text-xs text-fry-text-muted/60 mt-1">
                  Your unique miner identifier (set at registration)
                </p>
              </div>

              {/* Wallet Address */}
              <div>
                <label className="block text-xs font-medium text-fry-text-muted mb-2">
                  Wallet Address
                </label>
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={walletInput || device?.wallet_address || ''}
                    onChange={(e) => setWalletInput(e.target.value)}
                    className="flex-1 px-4 py-2 bg-fry-surface-2 border border-fry-border rounded-lg text-fry-text focus-visible:ring-2 focus-visible:ring-fry-neon focus-visible:outline-none transition font-mono text-xs"
                    placeholder="Your Algorand wallet address"
                  />
                  <button
                    onClick={handleRegister}
                    className="px-5 py-2 bg-fry-neon/15 text-fry-neon hover:bg-fry-neon/25 border border-fry-neon/40 rounded-lg text-sm font-medium transition-colors"
                  >
                    Save
                  </button>
                </div>
                <p className="text-xs text-fry-text-muted/60 mt-1">
                  Rewards will be sent to this address
                </p>
              </div>

              {/* Deregister */}
              {device?.miner_key && (
                <div className="pt-4 border-t border-fry-border-subtle">
                  <button
                    onClick={handleDeregister}
                    className="px-5 py-2 bg-fry-red/10 text-fry-red hover:bg-fry-red/20 border border-fry-red/30 rounded-lg text-sm font-medium transition-colors"
                  >
                    Deregister Device
                  </button>
                  <p className="text-xs text-fry-text-muted/60 mt-1">
                    Remove this device from the network and clear your miner key
                  </p>
                </div>
              )}
            </>
          )}
        </div>

        {/* API Section */}
        <div className="bg-fry-surface border border-fry-border rounded-xl p-6 space-y-6">
          <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">API</p>

          {configLoading ? (
            <div className="h-12 bg-fry-border-subtle rounded animate-pulse" />
          ) : (
            <div>
              <label className="block text-xs font-medium text-fry-text-muted mb-2">
                Base URL
              </label>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={
                    apiUrlInput || config?.api_base_url || 'https://hardwareapi.frynetworks.com'
                  }
                  onChange={(e) => setApiUrlInput(e.target.value)}
                  className="flex-1 px-4 py-2 bg-fry-surface-2 border border-fry-border rounded-lg text-fry-text focus-visible:ring-2 focus-visible:ring-fry-neon focus-visible:outline-none transition font-mono text-xs"
                  placeholder="https://hardwareapi.frynetworks.com"
                />
                <button
                  onClick={handleSaveApiUrl}
                  className="px-5 py-2 bg-fry-neon/15 text-fry-neon hover:bg-fry-neon/25 border border-fry-neon/40 rounded-lg text-sm font-medium transition-colors"
                >
                  Save
                </button>
              </div>
              <p className="text-xs text-fry-text-muted/60 mt-1">
                Hardware API endpoint for reward distribution
              </p>
            </div>
          )}
        </div>

        {/* About Section */}
        <div className="bg-fry-surface border border-fry-border rounded-xl p-6 space-y-4">
          <p className="text-xs font-medium uppercase tracking-widest text-fry-text-muted">About</p>
          <div className="text-sm">
            <div className="flex justify-between py-2 border-b border-fry-border-subtle">
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
