import { useState, useEffect } from 'react'
import { BrowserRouter as Router, Routes, Route, NavLink } from 'react-router-dom'
import { getVersion } from '@tauri-apps/api/app'
import { LayoutDashboard, Plug, Coins, ArrowUpCircle, Settings as SettingsIcon } from 'lucide-react'
import { useDevice } from './hooks/useDevice'
import fryLogoWhite from './assets/fry-logo-white.svg'
import Dashboard from './pages/Dashboard'
import Integrations from './pages/Integrations'
import Rewards from './pages/Rewards'
import SettingsPage from './pages/Settings'
import Updates from './pages/Updates'
import Migration from './pages/Migration'

const navItems = [
  { to: '/', label: 'Dashboard', Icon: LayoutDashboard, end: true },
  { to: '/integrations', label: 'Integrations', Icon: Plug },
  { to: '/rewards', label: 'Rewards', Icon: Coins },
  { to: '/updates', label: 'Updates', Icon: ArrowUpCircle },
  { to: '/settings', label: 'Settings', Icon: SettingsIcon },
]

function BrandLogo() {
  return (
    <img
      src={fryLogoWhite}
      alt=""
      className="w-20 h-20 absolute -right-4 -top-4 pointer-events-none opacity-15"
      aria-hidden="true"
    />
  )
}

export default function App() {
  const [version, setVersion] = useState<string>('\u2014')
  const { device } = useDevice()

  useEffect(() => {
    getVersion().then((v) => setVersion(`v${v}`)).catch(() => setVersion('v0.2.7'))
  }, [])

  const keyPreview = device?.miner_key
    ? device.miner_key.slice(0, 12) + '\u2026'
    : '\u2014'

  return (
    <Router>
      <div className="flex h-screen bg-fry-bg text-fry-text">
        {/* Sidebar */}
        <aside className="relative w-60 bg-fry-surface border-r border-fry-border flex flex-col shrink-0">
          {/* Brand header */}
          <div className="relative overflow-hidden px-5 py-5 border-b border-fry-border-subtle">
            <BrandLogo />
            <p className="font-brand text-base tracking-wide text-fry-text leading-none relative z-10">
              Fry Edge Miner
            </p>
            <p className="text-[10px] text-fry-text-muted mt-1 tracking-widest uppercase relative z-10">
              Edge Mining Client
            </p>
          </div>

          {/* Nav */}
          <nav className="px-2 space-y-0.5 mt-2 pb-20 overflow-y-auto flex-1">
            {navItems.map(({ to, label, Icon, end }) => (
              <NavLink
                key={to}
                to={to}
                end={end}
                className={({ isActive }) =>
                  `flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm transition-colors ${
                    isActive
                      ? 'bg-fry-surface-2 border-l-2 border-fry-red text-fry-red font-medium'
                      : 'text-fry-text-muted hover:bg-fry-surface-2 hover:text-fry-text border-l-2 border-transparent'
                  }`
                }
              >
                <Icon className="w-4 h-4 shrink-0" />
                {label}
              </NavLink>
            ))}
          </nav>

          {/* Device + version footer */}
          <div className="absolute bottom-0 left-0 right-0 px-4 py-3 border-t border-fry-border-subtle bg-fry-surface space-y-1">
            <p className="text-[10px] text-fry-text-muted font-mono truncate">{keyPreview}</p>
            <p className="text-[10px] text-fry-text-muted/60">{version}</p>
          </div>
        </aside>

        {/* Main content */}
        <main className="flex-1 overflow-y-auto">
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/integrations" element={<Integrations />} />
            <Route path="/rewards" element={<Rewards />} />
            <Route path="/updates" element={<Updates />} />
            <Route path="/settings" element={<SettingsPage />} />
            <Route path="/migration" element={<Migration />} />
          </Routes>
        </main>
      </div>
    </Router>
  )
}
