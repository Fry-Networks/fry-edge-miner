import { useState, useEffect } from 'react'
import { BrowserRouter as Router, Routes, Route, NavLink } from 'react-router-dom'
import { getVersion } from '@tauri-apps/api/app'
import Dashboard from './pages/Dashboard'
import Integrations from './pages/Integrations'
import Rewards from './pages/Rewards'
import Settings from './pages/Settings'
import Updates from './pages/Updates'
import Migration from './pages/Migration'

const icons = {
  dashboard: (
    <svg className="w-4 h-4 shrink-0" viewBox="0 0 16 16" fill="currentColor">
      <rect x="1" y="1" width="6" height="6" rx="1" />
      <rect x="9" y="1" width="6" height="6" rx="1" />
      <rect x="1" y="9" width="6" height="6" rx="1" />
      <rect x="9" y="9" width="6" height="6" rx="1" />
    </svg>
  ),
  integrations: (
    <svg className="w-4 h-4 shrink-0" viewBox="0 0 16 16" fill="currentColor">
      <rect x="1" y="2" width="14" height="2" rx="1" />
      <rect x="1" y="7" width="14" height="2" rx="1" />
      <rect x="1" y="12" width="14" height="2" rx="1" />
    </svg>
  ),
  rewards: (
    <svg className="w-4 h-4 shrink-0" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="8" cy="8" r="6" />
      <path d="M6 8h4M8 6v4" />
    </svg>
  ),
  updates: (
    <svg className="w-4 h-4 shrink-0" viewBox="0 0 16 16" fill="currentColor">
      <path d="M7 13V5.4L4.7 7.7 3.3 6.3 8 1.6l4.7 4.7-1.4 1.4L9 5.4V13z" />
    </svg>
  ),
  settings: (
    <svg className="w-4 h-4 shrink-0" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5">
      <circle cx="8" cy="8" r="2.5" />
      <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.3 3.3l1.4 1.4M11.3 11.3l1.4 1.4M3.3 12.7l1.4-1.4M11.3 4.7l1.4-1.4" />
    </svg>
  ),
}

const navItems = [
  { to: '/', label: 'Dashboard', icon: icons.dashboard, end: true },
  { to: '/integrations', label: 'Integrations', icon: icons.integrations },
  { to: '/rewards', label: 'Rewards', icon: icons.rewards },
  { to: '/updates', label: 'Updates', icon: icons.updates },
  { to: '/settings', label: 'Settings', icon: icons.settings },
]

export default function App() {
  const [version, setVersion] = useState<string>('\u2014')
  useEffect(() => {
    getVersion().then((v) => setVersion(`v${v}`)).catch(() => setVersion('v0.2.2'))
  }, [])

  return (
    <Router>
      <div className="flex h-screen bg-fry-bg text-fry-text">
        {/* Sidebar */}
        <aside className="relative w-64 bg-fry-surface border-r border-fry-border flex flex-col">
          {/* Brand */}
          <div className="px-5 py-5 border-b border-fry-border-subtle">
            <p className="font-brand text-lg tracking-wide text-fry-text leading-none">
              Fry Edge Miner
            </p>
            <p className="text-[10px] text-fry-text-muted mt-1 tracking-widest uppercase">
              Edge Mining Client
            </p>
          </div>

          {/* Nav */}
          <nav className="px-3 space-y-0.5 mt-2 pb-14 overflow-y-auto flex-1">
            {navItems.map((item) => (
              <NavLink
                key={item.to}
                to={item.to}
                end={item.end}
                className={({ isActive }) =>
                  `flex items-center gap-3 px-4 py-2.5 rounded-lg text-sm transition-colors ${
                    isActive
                      ? 'bg-fry-surface-hover text-fry-red font-medium'
                      : 'text-fry-text-muted hover:bg-fry-surface-hover hover:text-fry-text'
                  }`
                }
              >
                {item.icon}
                {item.label}
              </NavLink>
            ))}
          </nav>

          {/* Version footer */}
          <div className="absolute bottom-0 left-0 right-0 px-4 py-3 border-t border-fry-border-subtle bg-fry-surface">
            <p className="text-[10px] text-fry-text-muted">{version}</p>
          </div>
        </aside>

        {/* Main Content */}
        <main className="flex-1 overflow-y-auto">
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/integrations" element={<Integrations />} />
            <Route path="/rewards" element={<Rewards />} />
            <Route path="/updates" element={<Updates />} />
            <Route path="/settings" element={<Settings />} />
            <Route path="/migration" element={<Migration />} />
          </Routes>
        </main>
      </div>
    </Router>
  )
}
