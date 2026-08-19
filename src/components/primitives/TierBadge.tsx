type BadgeKind = 'official' | 'sdk' | 'experimental'

interface TierBadgeProps {
  kind: BadgeKind
}

const LABEL: Record<BadgeKind, string> = {
  official: 'OFFICIAL PARTNER',
  sdk: 'SDK',
  experimental: 'EXPERIMENTAL'
}

// Provenance badges carry a border; Tag (Running/Unhealthy/…) never does.
// That one detail is what separates a permanent fact about the partner from
// the integration's current state, without spending another accent colour.
// The dashed edge on EXPERIMENTAL reads "unfinished" where a solid amber
// would read "warning" — the SDK builds are supported, just not guaranteed.
const STYLE: Record<BadgeKind, { bg: string; co: string; bd: string }> = {
  official: { bg: 'var(--tealg)', co: 'var(--teal)', bd: '1px solid rgba(0,196,154,.28)' },
  sdk: { bg: 'transparent', co: 'var(--amb)', bd: '1px solid rgba(240,165,0,.30)' },
  experimental: { bg: 'rgba(240,165,0,.10)', co: 'var(--amb)', bd: '1px dashed rgba(240,165,0,.40)' }
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
