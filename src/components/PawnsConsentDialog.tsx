import { useEffect, useId, useRef, useState } from 'react'
import { Loader2 } from 'lucide-react'
import { canConfirm, type ConsentStatus } from '../lib/consentDialog'
import { INTEGRATION_META } from '../lib/integrationMeta'

interface PawnsConsentDialogProps {
  status: ConsentStatus
  /** Called with the checkbox state; the hook decides what that permits. */
  onConfirm: (checked: boolean) => void
  onCancel: () => void
  /** Consent is being recorded and the integration started. */
  busy?: boolean
}

const PAWNS_META = INTEGRATION_META.find((m) => m.id === 'pawns')
const ACCENT = PAWNS_META?.col ?? '#f5a623'

/**
 * Explicit consent before Pawns.app bandwidth sharing starts.
 *
 * The disclosure paragraph is whatever the backend sent — it is the audited
 * Addendum §5.4 (a)–(e) wording and is stored verbatim with the consent record,
 * so rendering a retyped copy here would mean showing one thing and recording
 * another.
 */
export default function PawnsConsentDialog({ status, onConfirm, onCancel, busy = false }: PawnsConsentDialogProps) {
  const [checked, setChecked] = useState(false)
  const titleId = useId()
  const bodyId = useId()
  const panelRef = useRef<HTMLDivElement>(null)
  const ready = canConfirm(checked) && !busy

  // Move focus into the dialog so the keyboard lands somewhere sensible and a
  // screen reader announces it on open.
  useEffect(() => {
    panelRef.current?.focus()
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onCancel()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [onCancel])

  const Icon = PAWNS_META?.Icon

  return (
    <div
      onClick={onCancel}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(0,0,0,.6)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 1000,
        padding: 24
      }}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={bodyId}
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
        style={{
          background: 'var(--s2)',
          border: '1px solid var(--b0)',
          borderLeft: `3px solid ${ACCENT}`,
          borderRadius: 'var(--rad)',
          maxWidth: 520,
          width: '100%',
          maxHeight: '88vh',
          overflowY: 'auto',
          padding: 22,
          display: 'flex',
          flexDirection: 'column',
          gap: 14,
          outline: 'none',
          animation: 'fadeUp .18s ease-out'
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 11 }}>
          <div
            style={{
              width: 38,
              height: 38,
              borderRadius: 'var(--rad)',
              flexShrink: 0,
              background: `${ACCENT}12`,
              border: `1px solid ${ACCENT}24`,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center'
            }}
          >
            {Icon && <Icon size={18} color={ACCENT} />}
          </div>
          <div style={{ minWidth: 0 }}>
            <div id={titleId} style={{ fontFamily: 'var(--fh)', fontWeight: 700, fontSize: 16, color: 'var(--txt)' }}>
              Pawns.app — Bandwidth Sharing Consent
            </div>
            <div
              style={{
                fontFamily: 'var(--fm)',
                fontSize: 10,
                letterSpacing: '.08em',
                textTransform: 'uppercase',
                color: 'var(--t2)',
                marginTop: 2
              }}
            >
              Notice version {status.wording_version}
            </div>
          </div>
        </div>

        <p
          id={bodyId}
          style={{
            fontFamily: 'var(--fb)',
            fontSize: 12.5,
            lineHeight: 1.65,
            color: 'var(--t1)',
            margin: 0,
            background: 'var(--s0)',
            border: '1px solid var(--b0)',
            borderRadius: 'var(--radsm)',
            padding: '12px 14px'
          }}
        >
          {status.disclosure}
        </p>

        <ul
          style={{
            listStyle: 'none',
            margin: 0,
            padding: 0,
            display: 'flex',
            flexDirection: 'column',
            gap: 7,
            fontFamily: 'var(--fb)',
            fontSize: 12,
            color: 'var(--t1)',
            lineHeight: 1.55
          }}
        >
          {[
            <>
              Pawns.app pays <strong style={{ color: ACCENT, fontFamily: 'var(--fm)' }}>$0.20/GB</strong> of bandwidth
              shared from this device.
            </>,
            <>You can turn this integration off at any time on the Integrations page — sharing stops immediately.</>,
            <>
              Sharing will not start yet: this device has no Pawns.app account configured (account verification is still
              pending), so agreeing now records your consent but does not begin earning.
            </>
          ].map((item, i) => (
            <li key={i} style={{ display: 'flex', gap: 9 }}>
              <span aria-hidden="true" style={{ color: ACCENT, fontFamily: 'var(--fm)', flexShrink: 0 }}>
                —
              </span>
              <span>{item}</span>
            </li>
          ))}
        </ul>

        <p style={{ fontFamily: 'var(--fb)', fontSize: 11.5, color: 'var(--t2)', margin: 0, lineHeight: 1.55 }}>
          Your decision is recorded with the exact wording above, along with the terms you accepted.
        </p>

        <label
          style={{
            display: 'flex',
            alignItems: 'flex-start',
            gap: 9,
            fontFamily: 'var(--fb)',
            fontSize: 12.5,
            color: 'var(--txt)',
            lineHeight: 1.5,
            cursor: 'pointer',
            background: 'var(--s1)',
            border: `1px solid ${checked ? `${ACCENT}55` : 'var(--b0)'}`,
            borderRadius: 'var(--radsm)',
            padding: '11px 13px'
          }}
        >
          <input
            type="checkbox"
            checked={checked}
            onChange={(e) => setChecked(e.target.checked)}
            data-testid="pawns-consent-checkbox"
            style={{ accentColor: ACCENT, width: 15, height: 15, marginTop: 1, flexShrink: 0, cursor: 'pointer' }}
          />
          <span>
            I understand and agree to share this device's bandwidth, and I acknowledge and accept the Pawns.app{' '}
            <a href={status.terms_url} target="_blank" rel="noreferrer" style={{ color: 'var(--teal)' }}>
              Terms
            </a>
            .
          </span>
        </label>

        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 10, marginTop: 2 }}>
          <button
            type="button"
            onClick={onCancel}
            data-testid="pawns-consent-cancel"
            style={{
              fontFamily: 'var(--fm)',
              fontSize: 12,
              padding: '8px 16px',
              borderRadius: 'var(--radsm)',
              background: 'transparent',
              border: '1px solid var(--b0)',
              color: 'var(--t2)',
              cursor: 'pointer'
            }}
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={() => onConfirm(checked)}
            disabled={!ready}
            data-testid="pawns-consent-confirm"
            style={{
              fontFamily: 'var(--fm)',
              fontSize: 12,
              padding: '8px 16px',
              borderRadius: 'var(--radsm)',
              background: ready ? `${ACCENT}1e` : 'transparent',
              border: `1px solid ${ready ? ACCENT : 'var(--b1)'}`,
              color: ready ? ACCENT : 'var(--t2)',
              cursor: ready ? 'pointer' : 'not-allowed',
              display: 'flex',
              alignItems: 'center',
              gap: 6
            }}
          >
            {busy && <Loader2 size={12} style={{ animation: 'spin 1s linear infinite' }} />}
            Enable Pawns
          </button>
        </div>
      </div>
    </div>
  )
}
