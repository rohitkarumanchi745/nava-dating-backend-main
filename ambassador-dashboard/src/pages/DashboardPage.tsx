import { useEffect, useState } from 'react'
import { useAuthStore } from '../hooks/useAuthStore'
import { ambassadorApi, PerformanceStats, DailySignup } from '../services/api'
import StatCard from '../components/StatCard'
import SignupChart from '../components/SignupChart'

export default function DashboardPage() {
  const { ambassador } = useAuthStore()
  const [stats, setStats] = useState<PerformanceStats | null>(null)
  const [dailyData, setDailyData] = useState<DailySignup[]>([])
  const [isLoading, setIsLoading] = useState(true)

  useEffect(() => {
    loadData()
  }, [])

  const loadData = async () => {
    try {
      const [perfRes, dailyRes] = await Promise.all([
        ambassadorApi.getPerformance(),
        ambassadorApi.getDailySignups(),
      ])
      setStats(perfRes.data)
      setDailyData(dailyRes.data.days)
    } catch (error) {
      console.error('Failed to load dashboard data:', error)
    } finally {
      setIsLoading(false)
    }
  }

  if (isLoading) {
    return (
      <div className="animate-pulse space-y-6">
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
          {[...Array(4)].map((_, i) => (
            <div key={i} className="h-32 bg-gray-200 rounded-xl" />
          ))}
        </div>
        <div className="h-96 bg-gray-200 rounded-xl" />
      </div>
    )
  }

  return (
    <div className="space-y-6">
      {/* Welcome */}
      <div>
        <h1 className="text-2xl font-bold text-gray-900">Welcome back, {ambassador?.name}!</h1>
        <p className="text-gray-500 mt-1">Here's how your referrals are performing</p>
      </div>

      {/* Stats Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        <StatCard
          title="Today's Signups"
          value={stats?.today_signups || 0}
          icon="📅"
          color="blue"
        />
        <StatCard
          title="This Week"
          value={stats?.this_week_signups || 0}
          icon="📊"
          color="green"
        />
        <StatCard
          title="This Month (Verified)"
          value={stats?.this_month_verified || 0}
          subtitle={`Target: ${stats?.monthly_target || 0}`}
          icon="🎯"
          color="red"
        />
        <StatCard
          title="Total Earnings"
          value={stats?.total_this_month || '₹0'}
          subtitle={stats?.bonus_earned !== '₹0' ? `Bonus: ${stats?.bonus_earned}` : undefined}
          icon="💰"
          color="yellow"
        />
      </div>

      {/* Target Progress */}
      {stats && (
        <div className="card">
          <div className="flex items-center justify-between mb-4">
            <h2 className="text-lg font-semibold text-gray-900">Monthly Target Progress</h2>
            <span className="text-sm text-gray-500">
              {stats.this_month_verified} / {stats.monthly_target} verified signups
            </span>
          </div>
          <div className="relative">
            <div className="w-full bg-gray-200 rounded-full h-4">
              <div
                className={`h-4 rounded-full transition-all duration-500 ${
                  stats.target_progress_pct >= 100
                    ? 'bg-green-500'
                    : stats.target_progress_pct >= 75
                    ? 'bg-yellow-500'
                    : 'bg-nava-red'
                }`}
                style={{ width: `${Math.min(stats.target_progress_pct, 100)}%` }}
              />
            </div>
            <div className="flex justify-between mt-2 text-sm">
              <span className="text-gray-500">0%</span>
              <span className={`font-medium ${stats.target_progress_pct >= 100 ? 'text-green-600' : 'text-gray-900'}`}>
                {stats.target_progress_pct.toFixed(1)}%
              </span>
              <span className="text-gray-500">100%</span>
            </div>
          </div>
          {stats.signups_to_target > 0 && (
            <p className="text-sm text-gray-500 mt-3">
              {stats.signups_to_target} more verified signups needed to reach your target
            </p>
          )}
          {stats.target_progress_pct >= 100 && (
            <p className="text-sm text-green-600 mt-3 font-medium">
              🎉 Congratulations! You've exceeded your target! Keep going for bonus earnings!
            </p>
          )}
        </div>
      )}

      {/* Chart */}
      <div className="card">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold text-gray-900">Signups Over Time</h2>
          <select className="text-sm border border-gray-300 rounded-lg px-3 py-1">
            <option>Last 30 days</option>
            <option>Last 7 days</option>
            <option>This month</option>
          </select>
        </div>
        <SignupChart data={dailyData} type="daily" />
      </div>

      {/* Quick Stats */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div className="card">
          <h3 className="text-sm font-medium text-gray-500 mb-2">All-Time Signups</h3>
          <p className="text-3xl font-bold text-gray-900">{stats?.total_signups || 0}</p>
          <p className="text-sm text-gray-500 mt-1">{stats?.total_verified || 0} verified</p>
        </div>
        <div className="card">
          <h3 className="text-sm font-medium text-gray-500 mb-2">Conversion Rate</h3>
          <p className="text-3xl font-bold text-gray-900">
            {stats?.total_signups
              ? ((stats.total_verified / stats.total_signups) * 100).toFixed(1)
              : 0}%
          </p>
          <p className="text-sm text-gray-500 mt-1">Verified / Total</p>
        </div>
        <div className="card">
          <h3 className="text-sm font-medium text-gray-500 mb-2">Total Earnings</h3>
          <p className="text-3xl font-bold text-gray-900">{stats?.total_all_time || '₹0'}</p>
          <p className="text-sm text-gray-500 mt-1">Since joining</p>
        </div>
      </div>
    </div>
  )
}
