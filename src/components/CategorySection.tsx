import { useState, type ReactNode } from 'react'
import { ChevronDown, type LucideIcon } from 'lucide-react'
import Tog from './primitives/Tog'

interface CategorySectionProps {
  title: string
  Icon: LucideIcon
  /** Integrations in this category that are currently enabled. */
  activeCount: number
  totalCount: number
  /** Enable/disable every integration in the category at once. */
  onToggleAll: (next: boolean) => void
  children: ReactNode
}

const storageKey = (title: string) => `fem.category.collapsed.${title}`

export default function CategorySection({
  title,
  Icon,
  activeCount,
  totalCount,
  onToggleAll,
  children
}: CategorySectionProps) {
  // Expanded by default; the user's choice per category survives restarts.
  const [collapsed, setCollapsed] = useState(() => {
    try {
      return localStorage.getItem(storageKey(title)) === '1'
    } catch {
      return false
    }
  })

  const toggleCollapsed = () => {
    setCollapsed((prev) => {
      const next = !prev
      try {
        localStorage.setItem(storageKey(title), next ? '1' : '0')
      } catch {
        /* private mode / quota — collapse still works for this session */
      }
      return next
    })
  }

  // The switch reflects "anything on", not "everything on": if a member fails
  // to start, an all-on-only rule would leave the switch stuck off and every
  // click would re-request enable instead of turning the category off.
  const anyOn = activeCount > 0

  return (
    <section style={{ display: 'flex', flexDirection: 'column', gap: 10, flexShrink: 0 }}>
      <header
        style={{
          background: 'var(--s2)',
          border: '1px solid var(--b0)',
          borderRadius: 'var(--rad)',
          padding: '10px 14px',
          display: 'flex',
          alignItems: 'center',
          gap: 12
        }}
      >
        <button
          type="button"
          onClick={toggleCollapsed}
          aria-expanded={!collapsed}
          aria-label={`${collapsed ? 'Expand' : 'Collapse'} ${title}`}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 10,
            flex: 1,
            minWidth: 0,
            background: 'none',
            border: 'none',
            padding: 0,
            cursor: 'pointer',
            textAlign: 'left',
            color: 'inherit'
          }}
        >
          <ChevronDown
            size={15}
            color="var(--t1)"
            style={{
              flexShrink: 0,
              transition: 'transform .22s cubic-bezier(.4,0,.2,1)',
              transform: collapsed ? 'rotate(-90deg)' : 'rotate(0deg)'
            }}
          />
          <Icon size={16} color="var(--teal)" style={{ flexShrink: 0 }} />
          <span
            style={{
              fontFamily: 'var(--fh)',
              fontWeight: 700,
              fontSize: 13,
              letterSpacing: '.06em',
              textTransform: 'uppercase',
              color: 'var(--txt)',
              whiteSpace: 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis'
            }}
          >
            {title}
          </span>
          <span
            style={{
              fontFamily: 'var(--fm)',
              fontSize: 11,
              padding: '2px 8px',
              borderRadius: 'var(--radsm)',
              background: activeCount > 0 ? 'var(--tealg)' : 'var(--s3)',
              color: activeCount > 0 ? 'var(--teal)' : 'var(--t1)',
              flexShrink: 0
            }}
          >
            {activeCount}/{totalCount} active
          </span>
        </button>
        <Tog
          checked={anyOn}
          onChange={() => onToggleAll(!anyOn)}
          aria-label={`Toggle all ${title} integrations`}
        />
      </header>
      <div
        style={{
          display: 'grid',
          gridTemplateRows: collapsed ? '0fr' : '1fr',
          transition: 'grid-template-rows .26s cubic-bezier(.4,0,.2,1), opacity .2s ease',
          opacity: collapsed ? 0 : 1
        }}
      >
        <div style={{ overflow: 'hidden', minHeight: 0 }}>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 10, paddingBottom: 2 }}>{children}</div>
        </div>
      </div>
    </section>
  )
}
