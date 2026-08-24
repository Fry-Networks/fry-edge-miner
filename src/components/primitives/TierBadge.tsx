type BadgeKind = 'official' | 'sdk' | 'experimental' | 'required' | 'optional'

interface TierBadgeProps {
  kind: BadgeKind
}

const LABEL: Record<BadgeKind, string> = {
  official: 'OFFICIAL PARTNER',
  sdk: 'SDK',
  experimental: 'EXPERIMENTAL',
  required: 'REQUIRED',
  optional: 'OPTIONAL — BOOST +5%'
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
  optional: { bg: 'transparent', co: 'var(--t1)', bd: '1px solid transparent' }
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
