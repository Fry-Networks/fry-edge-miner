import type { ReactNode } from 'react'
import { AlertTriangle, Download, Loader2 } from 'lucide-react'
import type { FrontendIntegration } from '../hooks/useIntegrations'
import { unhealthyReason } from '../lib/types'
import Tag from './primitives/Tag'
import Tog from './primitives/Tog'

interface IntCardProps {
  intg: FrontendIntegration
  onToggle: (id: string) => void
  // Docker prerequisite message when this card needs Docker and it isn't ready.
  dockerNote?: string | null
}

export default function IntCard({ intg, onToggle, dockerNote }: IntCardProps) {
  const {
    id,
    name,
    tag,
    desc,
    Icon,
    col,
    enabled,
    healthy,
    health,
    lifecycle,
    version
  } = intg
  const inst = version !== null
  const reason = unhealthyReason(health)
  const dockerBlocked = !!dockerNote

  let st: 'run' | 'err' | 'stopped' | 'info' = 'stopped'
  let stLbl = 'Not installed'
  let stNode: ReactNode = stLbl

  if (lifecycle === 'Installing') {
    st = 'info'
    stLbl = 'Installing'
    stNode = (
      <span style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}>
        <Loader2 size={11} style={{ animation: 'spin 1s linear infinite' }} />
        {stLbl}
      </span>
    )
  } else if (!inst && dockerBlocked) {
    st = 'stopped'
    stLbl = 'Unavailable'
  } else if (!inst) {
    st = 'stopped'
    stLbl = 'Not installed'
  } else if (!enabled) {
    st = 'stopped'
    stLbl = 'Disabled'
  } else if (healthy) {
    st = 'run'
    stLbl = 'Running'
  } else if (health === 'Stopped' || health === 'Starting' || health === 'Unknown') {
    // Enabled but not running yet — the backend health loop auto-restarts;
    // don't scare the user with a red badge for a transient state.
    st = 'info'
    stLbl = 'Starting'
  } else {
    st = 'err'
    stLbl = 'Unhealthy'
  }
  if (lifecycle !== 'Installing') stNode = stLbl

  const tv =
    lifecycle === 'Installing'
      ? 'info'
      : !inst
        ? 'warn'
        : st === 'run'
          ? 'run'
          : st === 'err'
            ? 'err'
            : st === 'info'
              ? 'info'
              : 'def'
  const pct = enabled && healthy ? Math.round(intg.poc_contribution * 100) : 0

  return (
    <div
      className="ic"
      style={{
        background: 'var(--s1)',
        border: '1px solid var(--b0)',
        borderRadius: 'var(--rad)',
        borderLeft: `3px solid ${inst && enabled ? col : 'var(--b1)'}`,
        overflow: 'hidden'
      }}
    >
      <div style={{ display: 'flex', alignItems: 'flex-start', gap: 12, padding: '16px 16px 12px' }}>
        <div
          style={{
            width: 46,
            height: 46,
            borderRadius: 'var(--rad)',
            flexShrink: 0,
            background: `${col}12`,
            border: `1px solid ${col}24`,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center'
          }}
        >
          <Icon size={20} color={inst && enabled ? col : `${col}60`} />
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 7, marginBottom: 3, flexWrap: 'wrap' }}>
            <span style={{ fontFamily: 'var(--fh)', fontWeight: 700, fontSize: 14, color: 'var(--txt)' }}>{name}</span>
            <span
              style={{
                fontFamily: 'var(--fh)',
                fontWeight: 700,
                fontSize: 9,
                letterSpacing: '.07em',
                color: col,
                opacity: 0.85
              }}
            >
              {tag}
            </span>
            <span title={reason ?? undefined} style={reason ? { cursor: 'help' } : undefined}>
              <Tag v={tv}>{stNode}</Tag>
            </span>
          </div>
          <div
            style={{
              fontFamily: 'var(--fb)',
              fontSize: 12,
              color: 'var(--t2)',
              marginBottom: 7,
              lineHeight: 1.5
            }}
          >
            {desc}
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
            {version && (
              <span
                style={{
                  fontFamily: 'var(--fm)',
                  fontSize: 11,
                  color: 'var(--t2)',
                  background: 'var(--s3)',
                  padding: '2px 7px',
                  borderRadius: 'var(--radsm)'
                }}
              >
                {/^\d/.test(version) ? `v${version}` : 'Installed'}
              </span>
            )}
            {reason && st === 'err' && (
              <span
                style={{
                  fontFamily: 'var(--fb)',
                  fontSize: 11,
                  color: 'var(--amb)',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 4,
                  minWidth: 0,
                  maxWidth: 420,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap'
                }}
                title={reason}
              >
                <AlertTriangle size={11} style={{ flexShrink: 0 }} /> {reason}
              </span>
            )}
            {dockerNote && (
              <span
                style={{
                  fontFamily: 'var(--fb)',
                  fontSize: 11,
                  color: 'var(--amb)',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 4,
                  minWidth: 0,
                  maxWidth: 420,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap'
                }}
                title={dockerNote}
              >
                <AlertTriangle size={11} style={{ flexShrink: 0 }} /> {dockerNote}
              </span>
            )}
            {!inst && !dockerNote && lifecycle !== 'Installing' && (
              <span
                style={{
                  fontFamily: 'var(--fb)',
                  fontSize: 11,
                  color: 'var(--amb)',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 4
                }}
              >
                <Download size={11} /> Auto-installs on enable
              </span>
            )}
          </div>
        </div>
        <Tog checked={enabled} onChange={() => onToggle(id)} label={`Toggle ${name}`} />
      </div>
      <div
        style={{
          padding: '9px 16px',
          borderTop: '1px solid var(--b0)',
          background: 'var(--s0)',
          display: 'flex',
          alignItems: 'center',
          gap: 10
        }}
      >
        <span style={{ fontFamily: 'var(--fb)', fontSize: 11, color: 'var(--t2)', flexShrink: 0 }}>Reward contribution</span>
        <div style={{ flex: 1, height: 4, background: 'var(--b1)', borderRadius: 2, overflow: 'hidden' }}>
          <div
            style={{
              height: '100%',
              width: `${pct}%`,
              background: `${col}bb`,
              borderRadius: 2,
              transition: 'width .4s ease'
            }}
          />
        </div>
        <span
          style={{
            fontFamily: 'var(--fm)',
            fontSize: 11,
            color: pct > 0 ? 'var(--teal)' : 'var(--t2)',
            flexShrink: 0
          }}
        >
          {pct}%
        </span>
      </div>
    </div>
  )
}
