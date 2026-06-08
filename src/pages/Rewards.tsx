import { useEffect, useState } from "react"
import { invoke } from "@tauri-apps/api/core"

export default function Rewards() {
  const [rewards, setRewards] = useState({
    daily: 0,
    weekly: 0,
    total: 0,
  })

  useEffect(() => {
    invoke("get_rewards").then((result: any) => {
      setRewards(result)
    })
  }, [])

  const integrationData = [
    { name: "Mysterium", earnings: 0.0 },
    { name: "Presearch", earnings: 0.0 },
    { name: "Diiisco", earnings: 0.0 },
    { name: "Space Acres", earnings: 0.0 },
    { name: "AEM", earnings: 0.0 },
  ]

  return (
    <div className="p-8">
      <h1 className="text-4xl font-bold text-white mb-2">
        Proportional Rewards
      </h1>
      <p className="text-gray-400 mb-8">
        Earnings distribution across active integrations
      </p>

      {/* Summary Cards */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-6 mb-8">
        <div className="bg-gray-900 border border-gray-800 rounded-lg p-6">
          <p className="text-sm text-gray-500 mb-2">Daily Earnings</p>
          <p className="text-3xl font-bold text-emerald-400">
            ${rewards.daily.toFixed(2)}
          </p>
        </div>
        <div className="bg-gray-900 border border-gray-800 rounded-lg p-6">
          <p className="text-sm text-gray-500 mb-2">Weekly Earnings</p>
          <p className="text-3xl font-bold text-emerald-400">
            ${rewards.weekly.toFixed(2)}
          </p>
        </div>
        <div className="bg-gray-900 border border-gray-800 rounded-lg p-6">
          <p className="text-sm text-gray-500 mb-2">Total Earnings</p>
          <p className="text-3xl font-bold text-emerald-400">
            ${rewards.total.toFixed(2)}
          </p>
        </div>
      </div>

      {/* Integration Breakdown */}
      <div className="bg-gray-900 border border-gray-800 rounded-lg p-6">
        <h2 className="text-xl font-semibold text-white mb-6">
          Integration Breakdown
        </h2>
        <div className="space-y-4">
          {integrationData.map((integration, idx) => (
            <div
              key={idx}
              className="flex items-center justify-between pb-4 border-b border-gray-800 last:border-b-0"
            >
              <span className="text-gray-300">{integration.name}</span>
              <div className="text-right">
                <p className="text-lg font-semibold text-emerald-400">
                  ${integration.earnings.toFixed(2)}
                </p>
                <p className="text-xs text-gray-600">0.0%</p>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
