import { AlertTriangle, Cpu, HardDrive, Loader2, Radio, type LucideIcon } from 'lucide-react'
import CategorySection from '../components/CategorySection'
import IntCard from '../components/IntCard'
import EmptyState from '../components/primitives/EmptyState'
import type { FrontendIntegration } from '../hooks/useIntegrations'
import { CATEGORIES, type IntegrationCategory } from '../lib/integrationMeta'
import type { DockerProgress, SystemStatus } from '../lib/types'

const CATEGORY_ICONS: Record<IntegrationCategory, LucideIcon> = {
  'VPN & Bandwidth': Radio,
  'Storage & Farming': HardDrive,
  'AI & Data': Cpu
}

interface IntegrationsProps {
  intgs: FrontendIntegration[]
  onToggle: (id: string) => void
  system?: SystemStatus | null
  dockerProgress?: DockerProgress | null
  onForceReinstall?: (id: string) => void
}

export default function Integrations({ intgs, onToggle, system, dockerProgress, onForceReinstall }: IntegrationsProps) {
  const active = intgs.filter((i) => i.enabled).length
  const dockerNotReady = !!system && system.docker !== 'ready'
  const anyNeedsDocker = intgs.some((i) => i.requires_docker)

  return (
    <div
      className="sc"
      style={{
        padding: '20px 24px',
        display: 'flex',
        flexDirection: 'column',
        gap: 12,
        overflowY: 'auto',
        height: '100%'
      }}
    >
      <div
        style={{
          background: 'var(--s2)',
          border: '1px solid var(--b0)',
          borderRadius: 'var(--rad)',
          padding: '11px 16px',
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center'
        }}
      >
        <span style={{ fontFamily: 'var(--fb)', fontSize: 13, color: 'var(--t1)' }}>
          Each <span style={{ color: 'var(--teal)', fontFamily: 'var(--fm)' }}>running</span> integration contributes {intgs.length > 0 ? Math.round(100 / intgs.length) : 0}% to your daily
          reward.
        </span>
        <div
          style={{
            fontFamily: 'var(--fm)',
            fontSize: 12,
            padding: '5px 11px',
            borderRadius: 'var(--radsm)',
            background: 'var(--tealg)',
            color: 'var(--teal)',
            flexShrink: 0,
            marginLeft: 12
          }}
        >
          {active}/{intgs.length} · {intgs.length > 0 ? Math.round((active / intgs.length) * 100) : 0}%
        </div>
      </div>
      {dockerProgress && (
        <div
          style={{
            background: 'rgba(74,158,255,.08)',
            border: '1px solid rgba(74,158,255,.25)',
            borderRadius: 'var(--rad)',
            padding: '10px 16px',
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            fontFamily: 'var(--fb)',
            fontSize: 12,
            color: 'var(--blu)'
          }}
        >
          <Loader2 size={13} style={{ animation: 'spin 1s linear infinite', flexShrink: 0 }} />
          {dockerProgress.detail}
        </div>
      )}
      {dockerNotReady && anyNeedsDocker && !dockerProgress && (
        <div
          style={{
            background: 'rgba(240,165,0,.08)',
            border: '1px solid rgba(240,165,0,.25)',
            borderRadius: 'var(--rad)',
            padding: '10px 16px',
            display: 'flex',
            alignItems: 'flex-start',
            gap: 8,
            fontFamily: 'var(--fb)',
            fontSize: 12,
            color: 'var(--amb)',
            lineHeight: 1.5
          }}
        >
          <AlertTriangle size={13} style={{ flexShrink: 0, marginTop: 2 }} />
          <span>{system?.docker_message}</span>
        </div>
      )}
      {intgs.length === 0 ? (
        <EmptyState message="No integrations available" sub="Connect to the backend to manage partner integrations" />
      ) : (
        CATEGORIES.map((cat) => {
          const members = intgs.filter((i) => i.category === cat)
          if (members.length === 0) return null
          const activeCount = members.filter((i) => i.enabled).length
          return (
            <CategorySection
              key={cat}
              title={cat}
              Icon={CATEGORY_ICONS[cat]}
              activeCount={activeCount}
              totalCount={members.length}
              onToggleAll={(next) => {
                // Only flip the ones not already in the target state, and stagger
                // them: firing every toggle in one tick raced the backend and
                // silently dropped all but the first.
                members
                  .filter((i) => i.enabled !== next)
                  .forEach((i, idx) => setTimeout(() => onToggle(i.id), idx * 450))
              }}
            >
              {members.map((intg) => (
                <IntCard
                  key={intg.id}
                  intg={intg}
                  onToggle={onToggle}
                  dockerNote={intg.requires_docker && dockerNotReady ? system?.docker_message : null}
                  onForceReinstall={intg.id === 'aem' ? onForceReinstall : undefined}
                />
              ))}
            </CategorySection>
          )
        })
      )}
    </div>
  )
}
