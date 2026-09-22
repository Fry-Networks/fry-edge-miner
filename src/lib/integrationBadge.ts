import type { HealthStatus, LifecycleState } from './types'
import { awaitsUserSetup, upstreamUnreachable } from './types'

export interface IntegrationBadgeInput {
  enabled: boolean
  healthy: boolean
  health: HealthStatus
  lifecycle: LifecycleState
  version: string | null
  unavailable_reason?: string | null
  dockerBlocked?: boolean
}

export type IntegrationBadgeKind =
  | 'installing'
  | 'unavailable'
  | 'notInstalled'
  | 'disabled'
  | 'running'
  | 'starting'
  | 'setupRequired'
  | 'upstreamUnreachable'
  | 'unhealthy'

export interface IntegrationBadge {
  kind: IntegrationBadgeKind
  label: string
  dot: 'run' | 'err' | 'stopped' | 'info'
  tag: 'run' | 'err' | 'info' | 'warn' | 'def'
}

/**
 * B14 D2: single source of truth for how an integration's enabled/health/
 * lifecycle state maps to a status badge. Extracted from IntCard.tsx's
 * original ladder so the Integrations card and the Dashboard tile can never
 * disagree.
 *
 * B14 D1 is folded in here: the `enabled && healthy` arm sits above the bare
 * `!inst` arm (a stale/null version probe on an otherwise-healthy card must
 * not shadow "Running"), and the `warn` tag no longer overrides a running
 * badge. The docker-blocked arm stays above the !enabled/!inst arms (a
 * disabled, uninstalled Docker integration must read "Unavailable", never
 * "Not installed" — see B19) and Starting stays above Setup required.
 */
export function integrationBadge(i: IntegrationBadgeInput): IntegrationBadge {
  const inst = i.version !== null
  const unavailable = !!i.unavailable_reason
  const dockerBlocked = !!i.dockerBlocked

  let kind: IntegrationBadgeKind
  let label: string
  let dot: IntegrationBadge['dot']

  if (i.lifecycle === 'Installing') {
    kind = 'installing'
    label = 'Installing'
    dot = 'info'
  } else if (unavailable) {
    kind = 'unavailable'
    label = 'Unavailable'
    dot = 'stopped'
  } else if (!inst && dockerBlocked) {
    kind = 'unavailable'
    label = 'Unavailable'
    dot = 'stopped'
  } else if (i.enabled && i.healthy) {
    kind = 'running'
    label = 'Running'
    dot = 'run'
  } else if (!inst) {
    kind = 'notInstalled'
    label = 'Not installed'
    dot = 'stopped'
  } else if (!i.enabled) {
    kind = 'disabled'
    label = 'Disabled'
    dot = 'stopped'
  } else if (i.health === 'Stopped' || i.health === 'Starting' || i.health === 'Unknown') {
    // Enabled but not running yet — the backend health loop auto-restarts;
    // don't scare the user with a red badge for a transient state.
    kind = 'starting'
    label = 'Starting'
    dot = 'info'
  } else if (awaitsUserSetup(i.health)) {
    // Not a failure — the partner is waiting on a setup step only the user
    // can complete (Storj node token + identity). Amber, and say what it is.
    kind = 'setupRequired'
    label = 'Setup required'
    dot = 'info'
  } else if (upstreamUnreachable(i.health)) {
    // B16 D4: an upstream network condition (e.g. the Titan scheduler is
    // unreachable), not a device fault — restarting the partner process
    // accomplishes nothing. Amber, not red.
    kind = 'upstreamUnreachable'
    label = 'Upstream unreachable'
    dot = 'info'
  } else {
    kind = 'unhealthy'
    label = 'Unhealthy'
    dot = 'err'
  }

  const tag: IntegrationBadge['tag'] =
    kind === 'installing'
      ? 'info'
      : !inst && dot !== 'run'
        ? 'warn'
        : dot === 'run'
          ? 'run'
          : dot === 'err'
            ? 'err'
            : dot === 'info'
              ? 'info'
              : 'def'

  return { kind, label, dot, tag }
}
