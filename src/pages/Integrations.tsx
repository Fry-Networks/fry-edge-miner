import { AlertTriangle, Loader2, ShieldCheck, Zap, type LucideIcon } from 'lucide-react'
import CategorySection from '../components/CategorySection'
import IntCard from '../components/IntCard'
import EmptyState from '../components/primitives/EmptyState'
import type { FrontendIntegration } from '../hooks/useIntegrations'
import { categoryCounts, toggleAllTargets } from '../lib/availability'
import { CATEGORIES } from '../lib/integrationMeta'
import type { IntegrationTier } from '../lib/integrationMeta'
import { splitByTier } from '../lib/tierSplit'
import type { DockerProgress, SystemStatus } from '../lib/types'

// F4: the page groups by tier, not by category — what a user needs to decide
// here is "must I run this?", and the category is already on every card as
// its tag. Members stay in category order inside a group so related
// integrations still sit together.
const TIER_GROUPS: { tier: IntegrationTier; title: string; Icon: LucideIcon }[] = [
  { tier: 'official', title: 'Required', Icon: ShieldCheck },
  { tier: 'sdk', title: 'Optional — Boost', Icon: Zap }
]

const byCategoryOrder = (a: FrontendIntegration, b: FrontendIntegration) =>
  CATEGORIES.indexOf(a.category) - CATEGORIES.indexOf(b.category)

interface IntegrationsProps {
  intgs: FrontendIntegration[]
  onToggle: (id: string) => void
  system?: SystemStatus | null
  dockerProgress?: DockerProgress | null
  onForceReinstall?: (id: string) => void
}

export default function Integrations({ intgs, onToggle, system, dockerProgress, onForceReinstall }: IntegrationsProps) {
  const active = intgs.filter((i) => i.enabled).length
  // Divide by what this machine can run, matching the denominator the PoC
  // reporter actually submits — otherwise the banner and each card's reward
  // contribution disagree.
  const available = categoryCounts(intgs).availableTotal
  const dockerNotReady = !!system && system.docker !== 'ready'
  const anyNeedsDocker = intgs.some((i) => i.requires_docker)
  const byTier = splitByTier(intgs)

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
          Each <span style={{ color: 'var(--teal)', fontFamily: 'var(--fm)' }}>running</span> integration contributes {available > 0 ? Math.round(100 / available) : 0}% to your daily
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
          {active}/{available} · {available > 0 ? Math.round((active / available) * 100) : 0}%
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
        TIER_GROUPS.map(({ tier, title, Icon }) => {
          const members = [...byTier[tier]].sort(byCategoryOrder)
          if (members.length === 0) return null
          const { activeCount, availableTotal, unavailableCount } = categoryCounts(members)
          return (
            <CategorySection
              key={title}
              title={title}
              Icon={Icon}
              activeCount={activeCount}
              totalCount={availableTotal}
              unavailableCount={unavailableCount}
              onToggleAll={(next) => {
                // Stagger the flips: firing every toggle in one tick raced the
                // backend and silently dropped all but the first.
                toggleAllTargets(members, next).forEach((i, idx) =>
                  setTimeout(() => onToggle(i.id), idx * 450)
                )
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
