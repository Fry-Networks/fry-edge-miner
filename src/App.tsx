import { BrowserRouter as Router, Routes, Route, Link } from "react-router-dom"
import Dashboard from "./pages/Dashboard"
import Integrations from "./pages/Integrations"
import Rewards from "./pages/Rewards"
import Settings from "./pages/Settings"
import Updates from "./pages/Updates"
import Migration from "./pages/Migration"

export default function App() {
  return (
    <Router>
      <div className="flex h-screen bg-fry-bg text-fry-text">
        {/* Sidebar */}
        <aside className="w-64 bg-fry-surface border-r border-fry-border overflow-y-auto">
          <div className="p-6">
            <h1 className="text-2xl font-bold text-fry-red">FEM</h1>
            <p className="text-xs text-fry-text-muted mt-1">Fry Edge Miner</p>
          </div>

          <nav className="space-y-2 px-4">
            <Link
              to="/"
              className="block px-4 py-3 rounded-lg hover:bg-fry-surface-hover transition text-fry-text hover:text-fry-text"
            >
              Dashboard
            </Link>
            <Link
              to="/integrations"
              className="block px-4 py-3 rounded-lg hover:bg-fry-surface-hover transition text-fry-text hover:text-fry-text"
            >
              Integrations
            </Link>
            <Link
              to="/rewards"
              className="block px-4 py-3 rounded-lg hover:bg-fry-surface-hover transition text-fry-text hover:text-fry-text"
            >
              Rewards
            </Link>
            <Link
              to="/updates"
              className="block px-4 py-3 rounded-lg hover:bg-fry-surface-hover transition text-fry-text hover:text-fry-text"
            >
              Updates
            </Link>
            <Link
              to="/settings"
              className="block px-4 py-3 rounded-lg hover:bg-fry-surface-hover transition text-fry-text hover:text-fry-text"
            >
              Settings
            </Link>
          </nav>
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
