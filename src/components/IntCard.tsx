import { useEffect, useState, type ReactNode } from 'react'
import { AlertTriangle, Download, Loader2, MessageCircle, RefreshCw } from 'lucide-react'
import type { FrontendIntegration } from '../hooks/useIntegrations'
import { consentBadge } from '../lib/consentDialog'
import { DISABLE_CONFIRM_MS, shouldConfirmDisable } from '../lib/disableConfirm'
import { condenseError, extractErrorMessage } from '../lib/error'
import { integrationBadge } from '../lib/integrationBadge'
import { isRequiredIntegration, SETUP_GUIDANCE } from '../lib/integrationMeta'
import { OFFICIAL_DISABLED_WARNING, REQUIRED_DISABLED_WARNING, SDK_REPORT_LINE } from '../lib/support'
import { safeInvoke } from '../lib/tauri'
import { awaitsUserSetup, unhealthyReason, sentinelFundingAddress } from '../lib/types'
import CopyField from './primitives/CopyField'
import Tag from './primitives/Tag'
import TierBadge from './primitives/TierBadge'
import Tog from './primitives/Tog'

interface IntCardProps {
  intg: FrontendIntegration
  onToggle: (id: string) => void
  // Docker prerequisite message when this card needs Docker and it isn't ready.
  dockerNote?: string | null
  // F2: clean-slate reinstall (Olostep only — backend rejects other ids).
  onForceReinstall?: (id: string) => void
  // Recorded bandwidth-sharing consent, for the integrations that need one.
  // null when this integration tracks no consent, or it isn't known yet.
  consentActive?: boolean | null
}

export default function IntCard({ intg, onToggle, dockerNote, onForceReinstall, consentActive = null }: IntCardProps) {
  const {
    id,
    name,
    tag,
    desc,
    Icon,
    col,
    tier,
    enabled,
    healthy,
    health,
    lifecycle,
    version,
    unavailable_reason: unavailableReason,
    error: lastError
  } = intg
  const inst = version !== null
  const reason = unhealthyReason(health)
  // Sentinel dVPN nodes must be funded manually (auto-funding is blocked — no
  // treasury balance and no mainnet DVPN faucet). When the node reports its
  // account isn't funded, surface its own sent1 address prominently + copyable
  // instead of burying it in a truncated one-liner.
  const fundingAddr = id === 'sentinel' ? sentinelFundingAddress(health) : null
  const isSdk = tier === 'sdk'
  // F17: reward role (required vs boost) is independent of partner provenance.
  const isRequired = isRequiredIntegration(id)
  // B18 D3: pass the card's own health reason so a stale consentActive flag
  // can never contradict what the health line already says.
  const consent = consentBadge(consentActive, reason)

  // F4: disabling an official partner costs reward proportion, so the first
  // click arms a caution row and the second commits. The row disarms itself so
  // a card left alone cannot silently keep a live confirmation.
  const [confirmingDisable, setConfirmingDisable] = useState(false)
  useEffect(() => {
    if (!confirmingDisable) return
    const timer = setTimeout(() => setConfirmingDisable(false), DISABLE_CONFIRM_MS)
    return () => clearTimeout(timer)
  }, [confirmingDisable])

  const handleToggle = () => {
    if (shouldConfirmDisable({ tier, enabled, confirming: confirmingDisable })) {
      setConfirmingDisable(true)
      return
    }
    setConfirmingDisable(false)
    onToggle(id)
  }

  // B17 D6: in-app way to supply the Iagon node_token the Setup required
  // guidance above asks for. Backend allow-list (partner_secret.rs,
  // T2/T3-owned) is Iagon-only today; mirror that here rather than showing a
  // dead input on partners the command will reject.
  const [secretValue, setSecretValue] = useState('')
  const [secretSaving, setSecretSaving] = useState(false)
  const [secretError, setSecretError] = useState<string | null>(null)
  const [secretSaved, setSecretSaved] = useState(false)
  const canSetSecret = id === 'iagon'
  const handleSaveSecret = async () => {
    if (!secretValue.trim()) return
    setSecretSaving(true)
    setSecretError(null)
    setSecretSaved(false)
    try {
      await safeInvoke('set_partner_secret', { id, value: secretValue })
      setSecretValue('')
      setSecretSaved(true)
      // G4 review finding 9: deliberately no call back into the enable/
      // disable switch here. This input only renders while stLbl ===
      // 'Setup required', and integrationBadge's setupRequired arm is only
      // reachable past the !enabled arm — so enabled is always true on the
      // one path that reaches this line, and flipping that switch would
      // DISABLE the integration instead of restarting it. iagon.rs's
      // node_token() is read fresh on every call (its own doc comment: "so
      // a user can paste their key ... without restarting the whole app"),
      // so the existing 30s poll's next health_check() picks up the saved
      // token on its own — no toggle or restart needed.
    } catch (e) {
      setSecretError(extractErrorMessage(e))
    } finally {
      setSecretSaving(false)
    }
  }

  const dockerBlocked = !!dockerNote
  // This machine cannot meet the partner's published minimums. Distinct from
  // "unhealthy": nothing is wrong, it simply can never run here — so the card
  // is inert rather than alarming.
  const unavailable = !!unavailableReason
  // Why the last enable attempt failed. Hardware-unavailable is the more
  // fundamental condition, so it wins; otherwise this is the most actionable
  // thing we can tell the user, and without it the toggle just springs back
  // to off with no explanation at all.
  // Condensed for the one-line card slot; the full text stays in the tooltip.
  // Cross-team fix (T3, via lead — B7 D4/B8 D3-D4): a STALE error from an
  // earlier failed toggle attempt must not outrank a LIVE awaitsUserSetup
  // reason — otherwise the funding/setup guidance (and its body text below)
  // never renders once any past attempt failed, even after health moves on.
  const startError = !unavailable && !awaitsUserSetup(health) && lastError ? condenseError(lastError) : null

  // B14 D2: single shared source for the status ladder — see
  // ../lib/integrationBadge.ts. The Dashboard tile consumes the same
  // function so the two pages can never disagree.
  const badge = integrationBadge({ ...intg, dockerBlocked })
  const st = badge.dot
  const stLbl = badge.label
  const stNode: ReactNode =
    badge.kind === 'installing' ? (
      <span style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}>
        <Loader2 size={11} style={{ animation: 'spin 1s linear infinite' }} />
        {stLbl}
      </span>
    ) : (
      stLbl
    )
  const tv = badge.tag
  const pct = enabled && healthy ? Math.round(intg.poc_contribution * 100) : 0

  return (
    <div
      className="ic"
      style={{
        background: 'var(--s1)',
        border: '1px solid var(--b0)',
        borderRadius: 'var(--rad)',
        borderLeft: `3px solid ${inst && enabled ? col : 'var(--b1)'}`,
        overflow: 'hidden',
        // Cards sit in a flex column; without this they shrink under pressure
        // from sibling cards and clip their own description text.
        flexShrink: 0
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
          <Icon size={20} color={unavailable ? `${col}40` : inst && enabled ? col : `${col}60`} />
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 7, marginBottom: 3, flexWrap: 'wrap' }}>
            <span
              style={{
                fontFamily: 'var(--fh)',
                fontWeight: 700,
                fontSize: 14,
                color: unavailable ? 'var(--t1)' : 'var(--txt)'
              }}
            >
              {name}
            </span>
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
            <TierBadge kind={tier} />
            {isSdk && <TierBadge kind="experimental" />}
            <TierBadge
              kind={
                isRequired ? 'required' : intg.tier === 'official' ? 'optionalPartner' : 'optionalCommunity'
              }
            />
            <span data-testid={`status-${id}`} title={reason ?? undefined} style={reason ? { cursor: 'help' } : undefined}>
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
            {consent && (
              <span data-testid={`consent-${id}`} title="Bandwidth sharing needs your explicit consent">
                <Tag v={consent.variant}>{consent.label}</Tag>
              </span>
            )}
            {startError && (
              <span
                data-testid={`starterror-${id}`}
                role="alert"
                style={{
                  fontFamily: 'var(--fb)',
                  fontSize: 11,
                  color: 'var(--amb)',
                  display: 'flex',
                  alignItems: 'flex-start',
                  gap: 4,
                  minWidth: 0,
                  maxWidth: 420,
                  // Warnings wrap instead of clipping: the full text is the
                  // instruction ("this device has 185 GB available…"), and a
                  // title tooltip is unreachable on touch and for screen
                  // readers on a role="alert" span. Matches the Sentinel
                  // funding block, the one warning that always read in full.
                  whiteSpace: 'normal',
                  overflowWrap: 'anywhere',
                  lineHeight: 1.4
                }}
                title={lastError ?? undefined}
              >
                <AlertTriangle size={11} style={{ flexShrink: 0, marginTop: 2 }} /> {startError}
              </span>
            )}
            {/* B17 D5 + B16 D4: also open for Setup required and Upstream
                unreachable (not just st === 'err') — once a state moves a
                partner out of 'err' (Sentinel/Iagon/Storj waiting on the
                user, or Titan's scheduler being unreachable), this gate must
                keep the explanation (and Sentinel's funding block) mounted.
                The trailing `|| awaitsUserSetup(health)` (cross-team fix for
                B7 D4/B8 D3-D4) additionally closes a ladder-precedence edge
                case: stLbl only reads 'Setup required' when no earlier arm
                in integrationBadge.ts's ladder fired first, but a live
                awaiting-user-setup reason should still explain itself even
                then. */}
            {reason && (st === 'err' || stLbl === 'Setup required' || stLbl === 'Upstream unreachable' || awaitsUserSetup(health)) && !startError && (
              fundingAddr ? (
                <div
                  data-testid={`sentinel-fund-${id}`}
                  role="alert"
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 6,
                    minWidth: 0,
                    fontFamily: 'var(--fb)',
                    fontSize: 11,
                    color: 'var(--amb)'
                  }}
                >
                  <span style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                    <AlertTriangle size={11} style={{ flexShrink: 0 }} /> Send 10 DVPN to this node address to activate it
                  </span>
                  <CopyField val={fundingAddr} />
                  <span style={{ color: 'var(--t2)' }}>
                    Fund manually — acquire DVPN on an exchange (e.g. Osmosis, KuCoin). No faucet exists.
                  </span>
                </div>
              ) : (
                <span
                  data-testid={`error-${id}`}
                  role="alert"
                  style={{
                    fontFamily: 'var(--fb)',
                    fontSize: 11,
                    color: 'var(--amb)',
                    display: 'flex',
                    alignItems: 'flex-start',
                    gap: 4,
                    minWidth: 0,
                    maxWidth: 420,
                    whiteSpace: 'normal',
                    overflowWrap: 'anywhere',
                    lineHeight: 1.4
                  }}
                  title={reason}
                >
                  <AlertTriangle size={11} style={{ flexShrink: 0, marginTop: 2 }} /> {reason}
                </span>
              )
            )}
            {stLbl === 'Setup required' && SETUP_GUIDANCE[id] && (
              <span
                role="note"
                style={{
                  fontFamily: 'var(--fb)',
                  fontSize: 11,
                  color: 'var(--amb)',
                  display: 'flex',
                  alignItems: 'flex-start',
                  gap: 4,
                  minWidth: 0,
                  maxWidth: 420,
                  whiteSpace: 'normal',
                  overflowWrap: 'anywhere',
                  lineHeight: 1.4
                }}
              >
                {SETUP_GUIDANCE[id].what}{' '}
                <a href={SETUP_GUIDANCE[id].url} target="_blank" rel="noreferrer">
                  {SETUP_GUIDANCE[id].urlLabel}
                </a>
              </span>
            )}
            {stLbl === 'Setup required' && canSetSecret && (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 4, minWidth: 0 }}>
                <div style={{ display: 'flex', gap: 6, minWidth: 0 }}>
                  <input
                    type="password"
                    data-testid={`secret-input-${id}`}
                    placeholder="Node auth token"
                    value={secretValue}
                    onChange={(e) => setSecretValue(e.target.value)}
                    disabled={secretSaving}
                    style={{
                      flex: 1,
                      minWidth: 0,
                      fontFamily: 'var(--fm)',
                      fontSize: 11,
                      padding: '4px 6px',
                      background: 'var(--s0)',
                      border: '1px solid var(--b0)',
                      borderRadius: 'var(--radsm)',
                      color: 'var(--txt)'
                    }}
                  />
                  <button
                    type="button"
                    data-testid={`secret-save-${id}`}
                    onClick={handleSaveSecret}
                    disabled={secretSaving || !secretValue.trim()}
                    style={{
                      fontFamily: 'var(--fh)',
                      fontSize: 11,
                      fontWeight: 700,
                      padding: '4px 10px',
                      borderRadius: 'var(--radsm)',
                      border: '1px solid var(--b0)',
                      background: 'var(--s1)',
                      color: 'var(--txt)',
                      cursor: secretSaving || !secretValue.trim() ? 'default' : 'pointer'
                    }}
                  >
                    {secretSaving ? 'Saving…' : 'Save'}
                  </button>
                </div>
                {secretError && (
                  <span
                    role="alert"
                    data-testid={`secret-error-${id}`}
                    style={{
                      fontFamily: 'var(--fb)',
                      fontSize: 11,
                      color: 'var(--red)',
                      whiteSpace: 'normal',
                      overflowWrap: 'anywhere'
                    }}
                  >
                    {condenseError(secretError)}
                  </span>
                )}
                {secretSaved && (
                  <span
                    data-testid={`secret-saved-${id}`}
                    style={{
                      fontFamily: 'var(--fb)',
                      fontSize: 11,
                      color: 'var(--teal)'
                    }}
                  >
                    Saved — should take effect on the next check.
                  </span>
                )}
              </div>
            )}
            {dockerNote && !startError && (
              <span
                style={{
                  fontFamily: 'var(--fb)',
                  fontSize: 11,
                  color: 'var(--amb)',
                  display: 'flex',
                  alignItems: 'flex-start',
                  gap: 4,
                  minWidth: 0,
                  maxWidth: 420,
                  // Warnings wrap instead of clipping: the full text is the
                  // instruction ("this device has 185 GB available…"), and a
                  // title tooltip is unreachable on touch and for screen
                  // readers on a role="alert" span. Matches the Sentinel
                  // funding block, the one warning that always read in full.
                  whiteSpace: 'normal',
                  overflowWrap: 'anywhere',
                  lineHeight: 1.4
                }}
                title={dockerNote}
              >
                <AlertTriangle size={11} style={{ flexShrink: 0, marginTop: 2 }} /> {dockerNote}
              </span>
            )}
            {unavailable && (
              <span
                data-testid={`unavailable-${id}`}
                role="note"
                style={{
                  fontFamily: 'var(--fb)',
                  fontSize: 11,
                  color: 'var(--amb)',
                  display: 'flex',
                  alignItems: 'flex-start',
                  gap: 4,
                  minWidth: 0,
                  maxWidth: 420,
                  // Warnings wrap instead of clipping: the full text is the
                  // instruction ("this device has 185 GB available…"), and a
                  // title tooltip is unreachable on touch and for screen
                  // readers on a role="alert" span. Matches the Sentinel
                  // funding block, the one warning that always read in full.
                  whiteSpace: 'normal',
                  overflowWrap: 'anywhere',
                  lineHeight: 1.4
                }}
                title={unavailableReason ?? undefined}
              >
                <AlertTriangle size={11} style={{ flexShrink: 0, marginTop: 2 }} /> {unavailableReason}
              </span>
            )}
            {!unavailable && !startError && !inst && !dockerNote && lifecycle !== 'Installing' && !(enabled && healthy) && (
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
            {confirmingDisable && (
              <span
                data-testid={`confirm-disable-${id}`}
                role="alert"
                style={{
                  fontFamily: 'var(--fb)',
                  fontSize: 11,
                  color: 'var(--amb)',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 4
                }}
              >
                <AlertTriangle size={11} style={{ flexShrink: 0 }} />{' '}
                {isRequired ? REQUIRED_DISABLED_WARNING : OFFICIAL_DISABLED_WARNING} Click the switch
                again to turn it off.
              </span>
            )}
            {isSdk && (
              <span
                data-testid={`sdk-report-${id}`}
                style={{
                  fontFamily: 'var(--fb)',
                  fontSize: 11,
                  color: 'var(--t1)',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 4
                }}
              >
                <MessageCircle size={11} style={{ flexShrink: 0 }} /> {SDK_REPORT_LINE}
              </span>
            )}
            {onForceReinstall && enabled && !healthy && lifecycle !== 'Installing' && (
              <button
                type="button"
                onClick={() => onForceReinstall(id)}
                data-testid={`reinstall-${id}`}
                aria-label={`Reinstall ${name}`}
                title="Remove all installed files and reinstall from scratch"
                style={{
                  fontFamily: 'var(--fb)',
                  fontSize: 11,
                  color: 'var(--teal)',
                  background: 'var(--tealg)',
                  border: '1px solid var(--b1)',
                  borderRadius: 'var(--radsm)',
                  padding: '3px 9px',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 4,
                  cursor: 'pointer'
                }}
              >
                <RefreshCw size={11} /> Reinstall
              </button>
            )}
          </div>
        </div>
        <Tog
          checked={enabled}
          onChange={handleToggle}
          // Unavailable blocks turning an integration ON, never turning one
          // OFF: an integration that became unavailable while enabled (e.g.
          // Diiisco once its device wallet stopped resolving) must stay
          // switchable off. The backend disable path has no requirements gate.
          disabled={unavailable && !enabled}
          label={`Toggle ${name} integration`}
          data-testid={`toggle-${id}`}
          aria-label={`Toggle ${name} integration`}
        />
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
