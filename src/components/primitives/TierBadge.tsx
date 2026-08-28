import { OFFICIAL_PARTNER_BOOST, COMMUNITY_SDK_BOOST } from '../../lib/rewardModel'

type BadgeKind =
  | 'official'
  | 'sdk'
  | 'experimental'
  | 'required'
  | 'optionalPartner'
  | 'optionalCommunity'

interface TierBadgeProps {
  kind: BadgeKind
}

// The two optional labels quote different boosts because they earn different
// boosts: an official partner is worth OFFICIAL_PARTNER_BOOST, a community/SDK
// build COMMUNITY_SDK_BOOST (see lib/rewardModel.ts).
const LABEL: Record<BadgeKind, string> = {
  official: 'OFFICIAL PARTNER',
  sdk: 'SDK',
  experimental: 'EXPERIMENTAL',
  required: 'REQUIRED',
  optionalPartner: `OPTIONAL — BOOST +${Math.round(OFFICIAL_PARTNER_BOOST * 100)}%`,
  optionalCommunity: `OPTIONAL — BOOST +${Math.round(COMMUNITY_SDK_BOOST * 100)}%`
}

// Provenance badges carry a border; Tag (Running/Unhealthy/…) never does.
// That one detail is what separates a permanent fact about the partner from
// the integration's current state, without spending another accent colour.
// The dashed edge on EXPERIMENTAL reads "unfinished" where a solid amber
// would read "warning" — the SDK builds are supported, just not guaranteed.
// REQUIRED and OPTIONAL are the quietest of the five — they answer "must I run
// this?", which the section a card sits in no longer says now that the page
// groups by category. Both are borderless so they read as a caption on the
// tier badge beside them rather than competing with it.
const STYLE: Record<BadgeKind, { bg: string; co: string; bd: string }> = {
  official: { bg: 'var(--tealg)', co: 'var(--teal)', bd: '1px solid rgba(0,196,154,.28)' },
  sdk: { bg: 'transparent', co: 'var(--amb)', bd: '1px solid rgba(240,165,0,.30)' },
  experimental: { bg: 'rgba(240,165,0,.10)', co: 'var(--amb)', bd: '1px dashed rgba(240,165,0,.40)' },
  required: { bg: 'transparent', co: 'var(--teal)', bd: '1px solid transparent' },
  optionalPartner: { bg: 'transparent', co: 'var(--t1)', bd: '1px solid transparent' },
  optionalCommunity: { bg: 'transparent', co: 'var(--t1)', bd: '1px solid transparent' }
}

export default function TierBadge({ kind }: TierBadgeProps) {
  const s = STYLE[kind]
  return (
    <span
      data-testid={`tier-${kind}`}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        fontFamily: 'var(--fh)',
        fontWeight: 700,
        fontSize: 9,
        letterSpacing: '.09em',
        textTransform: 'uppercase',
        padding: '1px 6px',
        borderRadius: 'var(--radsm)',
        background: s.bg,
        color: s.co,
        border: s.bd,
        flexShrink: 0,
        whiteSpace: 'nowrap'
      }}
    >
      {LABEL[kind]}
    </span>
  )
}
