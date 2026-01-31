import { Link, useLocation } from 'react-router-dom'
import { useAuthStore } from '../hooks/useAuthStore'

interface LayoutProps {
  children: React.ReactNode
}

const navItems = [
  { path: '/', label: 'Dashboard', icon: '📊' },
  { path: '/signups', label: 'Signups', icon: '📈' },
  { path: '/subscribers', label: 'Subscribers', icon: '👥' },
  { path: '/leaderboard', label: 'Leaderboard', icon: '🏆' },
]

export default function Layout({ children }: LayoutProps) {
  const location = useLocation()
  const { ambassador, logout } = useAuthStore()

  return (
    <div className="min-h-screen bg-gray-50">
      {/* Header */}
      <header className="bg-white border-b border-gray-200">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex justify-between items-center h-16">
            {/* Logo */}
            <div className="flex items-center">
              <span className="text-2xl font-bold text-nava-red">NAVA</span>
              <span className="ml-2 text-sm text-gray-500">Ambassador Portal</span>
            </div>

            {/* User menu */}
            <div className="flex items-center gap-4">
              <div className="text-right">
                <p className="text-sm font-medium text-gray-900">{ambassador?.name}</p>
                <p className="text-xs text-gray-500 capitalize">{ambassador?.tier} Ambassador</p>
              </div>
              <button
                onClick={logout}
                className="text-sm text-gray-600 hover:text-gray-900"
              >
                Logout
              </button>
            </div>
          </div>
        </div>
      </header>

      {/* Navigation */}
      <nav className="bg-white border-b border-gray-200">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex space-x-8">
            {navItems.map((item) => {
              const isActive = location.pathname === item.path
              return (
                <Link
                  key={item.path}
                  to={item.path}
                  className={`flex items-center gap-2 py-4 px-1 border-b-2 text-sm font-medium transition-colors ${
                    isActive
                      ? 'border-nava-red text-nava-red'
                      : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300'
                  }`}
                >
                  <span>{item.icon}</span>
                  {item.label}
                </Link>
              )
            })}
          </div>
        </div>
      </nav>

      {/* Main content */}
      <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
        {children}
      </main>

      {/* Footer */}
      <footer className="bg-white border-t border-gray-200 mt-auto">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-4">
          <div className="flex justify-between items-center text-sm text-gray-500">
            <p>Referral Code: <span className="font-mono font-bold text-nava-red">{ambassador?.referral_code}</span></p>
            <p>© 2024 NAVA. All rights reserved.</p>
          </div>
        </div>
      </footer>
    </div>
  )
}
