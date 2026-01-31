import { useEffect, useState } from 'react'
import { ambassadorApi, Subscriber } from '../services/api'
import SubscriberTable from '../components/SubscriberTable'
import StatCard from '../components/StatCard'

export default function SubscribersPage() {
  const [subscribers, setSubscribers] = useState<Subscriber[]>([])
  const [activeSubscribers, setActiveSubscribers] = useState<Subscriber[]>([])
  const [isLoading, setIsLoading] = useState(true)
  const [statusFilter, setStatusFilter] = useState<string>('all')
  const [page, setPage] = useState(1)
  const [total, setTotal] = useState(0)
  const [showActiveOnly, setShowActiveOnly] = useState(false)

  useEffect(() => {
    loadSubscribers()
    loadActiveSubscribers()
  }, [])

  useEffect(() => {
    loadSubscribers()
  }, [statusFilter, page])

  const loadSubscribers = async () => {
    setIsLoading(true)
    try {
      const status = statusFilter === 'all' ? undefined : statusFilter
      const res = await ambassadorApi.getSubscribers(page, 20, status)
      setSubscribers(res.data.subscribers)
      setTotal(res.data.total)
    } catch (error) {
      console.error('Failed to load subscribers:', error)
    } finally {
      setIsLoading(false)
    }
  }

  const loadActiveSubscribers = async () => {
    try {
      const res = await ambassadorApi.getActiveSubscribers()
      setActiveSubscribers(res.data.subscribers)
    } catch (error) {
      console.error('Failed to load active subscribers:', error)
    }
  }

  const stats = {
    total: total,
    active: activeSubscribers.length,
    verified: subscribers.filter((s) => s.status === 'verified').length,
    pending: subscribers.filter((s) => s.status === 'pending').length,
  }

  const displayedSubscribers = showActiveOnly ? activeSubscribers : subscribers

  return (
    <div className="space-y-6">
      {/* Header */}
      <div>
        <h1 className="text-2xl font-bold text-gray-900">Subscribers</h1>
        <p className="text-gray-500 mt-1">Users who signed up using your referral code</p>
      </div>

      {/* Stats */}
      <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
        <StatCard
          title="Total Subscribers"
          value={stats.total}
          icon="👥"
          color="blue"
        />
        <StatCard
          title="Active Now"
          value={stats.active}
          subtitle="Currently using the app"
          icon="🟢"
          color="green"
        />
        <StatCard
          title="Verified"
          value={stats.verified}
          icon="✅"
          color="green"
        />
        <StatCard
          title="Pending"
          value={stats.pending}
          icon="⏳"
          color="yellow"
        />
      </div>

      {/* Active Users Banner */}
      {stats.active > 0 && (
        <div className="bg-green-50 border border-green-200 rounded-xl p-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="w-3 h-3 bg-green-500 rounded-full animate-pulse" />
              <div>
                <p className="font-medium text-green-800">
                  {stats.active} user{stats.active > 1 ? 's' : ''} active right now
                </p>
                <p className="text-sm text-green-600">
                  Your referrals are using the app!
                </p>
              </div>
            </div>
            <button
              onClick={() => setShowActiveOnly(!showActiveOnly)}
              className={`px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
                showActiveOnly
                  ? 'bg-green-600 text-white'
                  : 'bg-white text-green-700 border border-green-300 hover:bg-green-50'
              }`}
            >
              {showActiveOnly ? 'Show All' : 'Show Active Only'}
            </button>
          </div>
        </div>
      )}

      {/* Filters */}
      <div className="card">
        <div className="flex flex-wrap items-center justify-between gap-4 mb-6">
          <div className="flex gap-2">
            {['all', 'verified', 'pending', 'churned'].map((status) => (
              <button
                key={status}
                onClick={() => {
                  setStatusFilter(status)
                  setPage(1)
                  setShowActiveOnly(false)
                }}
                className={`px-4 py-2 rounded-lg text-sm font-medium capitalize transition-colors ${
                  statusFilter === status && !showActiveOnly
                    ? 'bg-nava-red text-white'
                    : 'bg-gray-100 text-gray-700 hover:bg-gray-200'
                }`}
              >
                {status}
              </button>
            ))}
          </div>

          <div className="flex items-center gap-2">
            <input
              type="text"
              placeholder="Search by name..."
              className="input w-64"
            />
          </div>
        </div>

        {/* Table */}
        <SubscriberTable subscribers={displayedSubscribers} isLoading={isLoading} />

        {/* Pagination */}
        {!showActiveOnly && total > 20 && (
          <div className="flex items-center justify-between mt-6 pt-4 border-t border-gray-200">
            <p className="text-sm text-gray-500">
              Showing {(page - 1) * 20 + 1} - {Math.min(page * 20, total)} of {total}
            </p>
            <div className="flex gap-2">
              <button
                onClick={() => setPage(page - 1)}
                disabled={page === 1}
                className="btn-secondary disabled:opacity-50"
              >
                Previous
              </button>
              <button
                onClick={() => setPage(page + 1)}
                disabled={page * 20 >= total}
                className="btn-secondary disabled:opacity-50"
              >
                Next
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
