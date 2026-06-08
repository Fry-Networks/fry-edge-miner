import { useState, useEffect } from "react"
import { invoke } from "@tauri-apps/api/core"

export default function Settings() {
  const [settings, setSettings] = useState({
    minerKey: "",
    walletAddress: "",
    apiBaseUrl: "https://hardwareapi.frynetworks.com",
  })

  useEffect(() => {
    invoke("get_settings").then((result: any) => {
      setSettings(result || settings)
    })
  }, [])

  const handleChange = (field: string, value: string) => {
    setSettings((prev) => ({ ...prev, [field]: value }))
  }

  const handleSave = async () => {
    try {
      await invoke("save_settings", { settings })
      alert("Settings saved successfully")
    } catch (error) {
      console.error("Error saving settings:", error)
      alert("Failed to save settings")
    }
  }

  return (
    <div className="p-8">
      <h1 className="text-4xl font-bold text-white mb-2">Settings</h1>
      <p className="text-gray-400 mb-8">Configure your Fry Edge Miner</p>

      <div className="max-w-2xl">
        <div className="bg-gray-900 border border-gray-800 rounded-lg p-8 space-y-6">
          {/* Miner Key */}
          <div>
            <label className="block text-sm font-medium text-gray-300 mb-2">
              Miner Key
            </label>
            <input
              type="text"
              value={settings.minerKey}
              disabled
              className="w-full px-4 py-2 bg-gray-800 border border-gray-700 rounded-lg text-gray-500 cursor-not-allowed"
              placeholder="Miner key (read-only)"
            />
            <p className="text-xs text-gray-600 mt-1">
              Your unique miner identifier
            </p>
          </div>

          {/* Wallet Address */}
          <div>
            <label className="block text-sm font-medium text-gray-300 mb-2">
              Wallet Address
            </label>
            <input
              type="text"
              value={settings.walletAddress}
              onChange={(e) =>
                handleChange("walletAddress", e.target.value)
              }
              className="w-full px-4 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white focus:border-emerald-500 focus:outline-none transition"
              placeholder="Your Algorand wallet address"
            />
            <p className="text-xs text-gray-600 mt-1">
              Rewards will be sent to this address
            </p>
          </div>

          {/* API Base URL */}
          <div>
            <label className="block text-sm font-medium text-gray-300 mb-2">
              API Base URL
            </label>
            <input
              type="text"
              value={settings.apiBaseUrl}
              onChange={(e) =>
                handleChange("apiBaseUrl", e.target.value)
              }
              className="w-full px-4 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white focus:border-emerald-500 focus:outline-none transition"
              placeholder="https://hardwareapi.frynetworks.com"
            />
            <p className="text-xs text-gray-600 mt-1">
              Hardware API endpoint
            </p>
          </div>

          {/* Save Button */}
          <button
            onClick={handleSave}
            className="w-full px-6 py-3 bg-emerald-600 hover:bg-emerald-700 text-white font-medium rounded-lg transition"
          >
            Save Settings
          </button>
        </div>
      </div>
    </div>
  )
}
