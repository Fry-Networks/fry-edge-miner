import { useState } from "react"
import { invoke } from "@tauri-apps/api/core"

interface Integration {
  id: string
  name: string
  description: string
  enabled: boolean
}

export default function Integrations() {
  const [integrations, setIntegrations] = useState<Integration[]>([
    {
      id: "mysterium",
      name: "Mysterium",
      description: "VPN & Bandwidth Sharing",
      enabled: false,
    },
    {
      id: "presearch",
      name: "Presearch",
      description: "Decentralized Search Node",
      enabled: false,
    },
    {
      id: "diiisco",
      name: "Diiisco",
      description: "Data Sharing Network",
      enabled: false,
    },
    {
      id: "space_acres",
      name: "Space Acres",
      description: "Storage Provider",
      enabled: false,
    },
    {
      id: "aem",
      name: "AEM",
      description: "AI Edge Mining",
      enabled: false,
    },
  ])

  const handleToggle = async (id: string, enabled: boolean) => {
    try {
      await invoke("toggle_integration", { id, enabled })
      setIntegrations(
        integrations.map((i) =>
          i.id === id ? { ...i, enabled } : i
        )
      )
    } catch (error) {
      console.error("Error toggling integration:", error)
    }
  }

  return (
    <div className="p-8">
      <h1 className="text-4xl font-bold text-white mb-2">Integrations</h1>
      <p className="text-gray-400 mb-8">
        Enable or disable earning integrations
      </p>

      <div className="grid grid-cols-1 gap-4">
        {integrations.map((integration) => (
          <div
            key={integration.id}
            className="bg-gray-900 border border-gray-800 rounded-lg p-6 flex items-center justify-between"
          >
            <div>
              <h3 className="text-lg font-semibold text-white mb-1">
                {integration.name}
              </h3>
              <p className="text-sm text-gray-500">
                {integration.description}
              </p>
            </div>
            <div className="flex items-center gap-4">
              <div
                className={`w-3 h-3 rounded-full ${
                  integration.enabled ? "bg-emerald-400" : "bg-gray-700"
                }`}
              />
              <button
                onClick={() =>
                  handleToggle(integration.id, !integration.enabled)
                }
                className={`px-6 py-2 rounded-lg font-medium transition ${
                  integration.enabled
                    ? "bg-emerald-600 hover:bg-emerald-700 text-white"
                    : "bg-gray-800 hover:bg-gray-700 text-gray-300"
                }`}
              >
                {integration.enabled ? "Disable" : "Enable"}
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
