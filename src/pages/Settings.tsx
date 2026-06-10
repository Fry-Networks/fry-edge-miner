import { useState } from 'react'
import { useDevice } from '../hooks/useDevice'
import { useSettings } from '../hooks/useSettings'

export default function Settings() {
  const { device, setWallet, loading: deviceLoading } = useDevice()
  const { config, save, loading: configLoading } = useSettings()
  const [walletInput, setWalletInput] = useState('')
  const [apiUrlInput, setApiUrlInput] = useState('')
  const [savedMessage, setSavedMessage] = useState('')

  const handleSaveWallet = async () => {
    if (!walletInput.trim()) return
    try {
      await setWallet(walletInput)
      setSavedMessage('Wallet updated successfully')
      setTimeout(() => setSavedMessage(''), 3000)
    } catch (error) {
      setSavedMessage('Failed to save wallet')
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

  return (
    <div className="p-8 space-y-8">
      {/* Page Header */}
      <div>
        <h1 className="text-4xl font-bold text-fry-text mb-2">Settings</h1>
        <p className="text-fry-text-muted">Configure your Fry Edge Miner</p>
      </div>

      {/* Success Message */}
      {savedMessage && (
        <div className="bg-fry-neon/20 border border-fry-neon/50 rounded-xl p-4">
          <p className="text-sm text-fry-neon">{savedMessage}</p>
        </div>
      )}

      <div className="max-w-2xl space-y-6">
        {/* Device Section */}
        <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-6 space-y-6">
          <h2 className="text-lg font-semibold text-fry-text">Device</h2>

          {deviceLoading ? (
            <div className="space-y-4">
              <div className="h-12 bg-fry-border rounded animate-pulse" />
              <div className="h-12 bg-fry-border rounded animate-pulse" />
            </div>
          ) : (
            <>
              {/* Miner Key (Read-only) */}
              <div>
                <label className="block text-sm font-medium text-fry-text mb-2">
                  Miner Key
                </label>
                <input
                  type="text"
                  value={device?.miner_key || ''}
                  disabled
                  className="w-full px-4 py-2 bg-fry-surface-hover/60 border border-fry-border rounded-lg text-fry-text-muted cursor-not-allowed font-mono text-sm"
                  placeholder="Miner key (read-only)"
                />
                <p className="text-xs text-fry-text-muted/60 mt-1">
                  Your unique miner identifier (set at registration)
                </p>
              </div>

              {/* Wallet Address */}
              <div>
                <label className="block text-sm font-medium text-fry-text mb-2">
                  Wallet Address
                </label>
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={walletInput || device?.wallet_address || ''}
                    onChange={(e) => setWalletInput(e.target.value)}
                    className="flex-1 px-4 py-2 bg-fry-surface-hover/60 border border-fry-border rounded-lg text-fry-text focus:border-fry-red focus:ring-1 focus:ring-fry-red/50 transition font-mono text-sm"
                    placeholder="Your Algorand wallet address"
                  />
                  <button
                    onClick={handleSaveWallet}
                    className="px-6 py-2 bg-fry-red/20 text-fry-red hover:bg-fry-red/30 border border-fry-red/50 rounded-lg font-medium transition"
                  >
                    Save
                  </button>
                </div>
                <p className="text-xs text-fry-text-muted/60 mt-1">
                  Rewards will be sent to this address
                </p>
              </div>
            </>
          )}
        </div>

        {/* API Section */}
        <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-6 space-y-6">
          <h2 className="text-lg font-semibold text-fry-text">API</h2>

          {configLoading ? (
            <div className="h-12 bg-fry-border rounded animate-pulse" />
          ) : (
            <div>
              <label className="block text-sm font-medium text-fry-text mb-2">
                Base URL
              </label>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={
                    apiUrlInput || config?.api_base_url || 'https://hardwareapi.frynetworks.com'
                  }
                  onChange={(e) => setApiUrlInput(e.target.value)}
                  className="flex-1 px-4 py-2 bg-fry-surface-hover/60 border border-fry-border rounded-lg text-fry-text focus:border-fry-red focus:ring-1 focus:ring-fry-red/50 transition font-mono text-sm"
                  placeholder="https://hardwareapi.frynetworks.com"
                />
                <button
                  onClick={handleSaveApiUrl}
                  className="px-6 py-2 bg-fry-red/20 text-fry-red hover:bg-fry-red/30 border border-fry-red/50 rounded-lg font-medium transition"
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
        <div className="bg-fry-surface/80 border border-fry-border/60 rounded-xl p-6 space-y-4">
          <h2 className="text-lg font-semibold text-fry-text">About</h2>
          <div className="space-y-3 text-sm">
            <div className="flex justify-between">
              <span className="text-fry-text-muted">App Version</span>
              <span className="text-fry-text font-mono">0.2.0</span>
            </div>
            <div className="flex justify-between">
              <span className="text-fry-text-muted">Build Type</span>
              <span className="text-fry-text font-mono">Development</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
