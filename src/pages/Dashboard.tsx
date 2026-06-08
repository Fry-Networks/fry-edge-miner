import { useEffect, useState } from "react"
import { invoke } from "@tauri-apps/api/core"

export default function Dashboard() {
  const [integrations, setIntegrations] = useState<any[]>([])

  useEffect(() => {
    invoke("get_integrations").then((result: any) => {
      setIntegrations(result)
    })
  }, [])

  const integrationNames = [
    "Mysterium",
    "Presearch",
    "Diiisco",
    "Space Acres",
    "AEM",
  ]

  return (
    <div className="p-8">
      <h1 className="text-4xl font-bold text-white mb-2">
        Fry Edge Miner
      </h1>
      <p className="text-gray-400 mb-8">
        Multi-integration decentralized platform client
      </p>

      {/* Status Cards */}
      <div className="grid grid-cols-1 md:grid-cols-5 gap-4 mb-8">
        {integrationNames.map((name, idx) => (
          <div
            key={idx}
            className="bg-gray-900 border border-gray-800 rounded-lg p-6"
          >
            <h3 className="text-sm font-semibold text-gray-300 mb-3">
              {name}
            </h3>
            <div className="text-3xl font-bold text-gray-500 mb-2">-</div>
            <p className="text-xs text-gray-600">Status: Stopped</p>
          </div>
        ))}
      </div>

      {/* Summary Section */}
      <div className="bg-gray-900 border border-gray-800 rounded-lg p-6">
        <h2 className="text-xl font-semibold text-white mb-4">Summary</h2>
        <div className="grid grid-cols-3 gap-6">
          <div>
            <p className="text-sm text-gray-500 mb-1">Active Integrations</p>
            <p className="text-2xl font-bold text-emerald-400">0</p>
          </div>
          <div>
            <p className="text-sm text-gray-500 mb-1">Daily Earnings</p>
            <p className="text-2xl font-bold text-emerald-400">$0.00</p>
          </div>
          <div>
            <p className="text-sm text-gray-500 mb-1">Total Earnings</p>
            <p className="text-2xl font-bold text-emerald-400">$0.00</p>
          </div>
        </div>
      </div>
    </div>
  )
}
