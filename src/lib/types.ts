// These match the Rust types from src-tauri/src/integrations/mod.rs and commands/

export interface IntegrationStatus {
  id: string
  display_name: string
  enabled: boolean
  health: HealthStatus
  lifecycle: LifecycleState
  version: string | null
  poc_contribution: number
  requires_docker?: boolean
}

export type DockerStatusKind = 'ready' | 'daemon_stopped' | 'not_installed' | 'virtualization_disabled'

export interface SystemStatus {
  docker: DockerStatusKind
  docker_message: string
  virtualization_supported: boolean
}

export interface DockerProgress {
  stage: string
  detail: string
  attempt: number
  total: number
}

// Rust enums with PascalCase rename serialize like this:
export type HealthStatus =
  | 'Healthy'
  | { Unhealthy: string }
  | 'Stopped'
  | 'Installing'
  | 'Starting'
  | 'Unknown'

export type LifecycleState =
  | 'Disabled'
  | 'Installing'
  | 'Starting'
  | 'Running'
  | 'Unhealthy'
  | 'Restarting'
  | 'Failed'
  | 'Stopping'
  | 'Updating'

export interface RewardSummary {
  active_count: number
  total_count: number
  proportion: number
  estimated_daily: number
  base_reward: number
  reward_amount: number
  reward_token_asa_id: string
  reward_token_name: string
  stake_token_asa_id: string
  stake_token_name: string
  stake_multiplier: number
  stake_label: string
  stake_tiers?: Record<string, { multiplier: number; label: string }>
}

export interface PocSlot {
  slot_index: number
  data: boolean
  online: boolean
  mac_match: boolean
  pol: boolean
  poi: boolean
  poa: boolean
  tools_active: string[]
  tools_count: number
  multiplier: number
}

export interface DeviceInfo {
  miner_key: string | null
  wallet_address: string | null
  registered: boolean
}

export interface FemConfig {
  miner_key: string | null
  wallet_address: string | null
  integrations_enabled: Record<string, boolean>
  api_base_url: string
  start_on_boot?: boolean
  minimize_to_tray?: boolean
  auto_update?: boolean
  notifications?: boolean
}

export interface UpdateInfo {
  id: string
  name: string
  current_version: string | null
  latest_version: string | null
  available: boolean
  error?: string
  kind: 'app' | 'integration'
  download_url: string | null
  body: string | null
}

export interface RewardRow {
  date: string
  reward: number
  slots: number
  factor: number
  status: 'paid' | 'none'
}

export interface PocSlotUi {
  done: boolean
  pass: boolean | null
}

// Helper to extract health status display string
export function getHealthLabel(health: HealthStatus): string {
  if (typeof health === 'string') return health
  if ('Unhealthy' in health) return `Unhealthy: ${health.Unhealthy}`
  return 'Unknown'
}

export function isHealthy(health: HealthStatus): boolean {
  return health === 'Healthy'
}

// The reason string carried by an Unhealthy status, if any.
export function unhealthyReason(health: HealthStatus): string | null {
  if (typeof health !== 'string' && 'Unhealthy' in health) return health.Unhealthy
  return null
}
