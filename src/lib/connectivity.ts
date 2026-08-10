import type { DockerStatusKind } from './types'

export type Connectivity = 'connected' | 'degraded' | 'disconnected'

export interface ConnectivityInputs {
  /** Device/API-level failure from useDevice — backend unreachable. */
  deviceError: string | null
  /** get_integrations fetch failure from useIntegrations. */
  integrationsError: string | null
  /** system.docker, or null when system status is unknown (browser mode / not yet fetched). */
  dockerStatus: DockerStatusKind | null
}

/**
 * Derive the TopBar connectivity badge state.
 * - 'disconnected': the backend/device API is unreachable.
 * - 'degraded': the integrations IPC is failing.
 *
 * `dockerStatus` deliberately never affects the badge: Docker is a local
 * prerequisite for SOME integrations, not connectivity to Fry. It is shown
 * as a separate Docker chip in the TopBar and per-card errors instead —
 * conflating it with connectivity made users believe their miner was
 * offline whenever Docker Desktop was stopped.
 */
export function deriveConnectivity({ deviceError, integrationsError }: ConnectivityInputs): Connectivity {
  if (deviceError) return 'disconnected'
  if (integrationsError) return 'degraded'
  return 'connected'
}
