import { useEffect, useState } from 'react'
import { ambassadorApi, LeaderboardEntry } from '../services/api'
import { useAuthStore } from '../hooks/useAuthStore'

export default function LeaderboardPage() {
  const { ambassador } = useAuthStore()
  const [entries, setEntries] = useState<LeaderboardEntry[]>([])
  const [myRank, setMyRank] = useState<number | null>(null)
  const [period, setPeriod] = useState('month')
  const [isLoading, setIsLoading] = useState(true)

  useEffect(() => {
    loadLeaderboard()
  }, [period])

  const loadLeaderboard = async () => {
    setIsLoading(true)
    try {
      const res = await ambassadorApi.getLeaderboard(period)
      setEntries(res.data.entries)
      setMyRank(res.data.my_rank ?? null)
    } catch (error) {
      console.error('Failed to load leaderboard:', error)
    } finally {
      setIsLoading(false)
    }
  }

  const getRankBadge = (rank: number) => {
    if (rank === 1) return '🥇'
    if (rank === 2) return '🥈'
    if (rank === 3) return '🥉'
    return `#${rank}`
  }

  const getRankColor = (rank: number) => {
    if (rank === 1) return 'bg-yellow-100 text-yellow-800 border-yellow-300'
    if (rank === 2) return 'bg-gray-100 text-gray-800 border-gray-300'
    if (rank === 3) return 'bg-orange-100 text-orange-800 border-orange-300'
    return 'bg-white text-gray-800 border-gray-200'
  }

  const periodLabels: Record<string, string> = {
    today: 'Today',
    week: 'This Week',
    month: 'This Month',
    all_time: 'All Time',
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-gray-900">Leaderboard</h1>
          <p className="text-gray-500 mt-1">See how you rank against other ambassadors</p>
        </div>
        {myRank && (
          <div className="bg-nava-red text-white px-6 py-3 rounded-xl">
            <p className="text-sm opacity-80">Your Rank</p>
            <p className="text-2xl font-bold">#{myRank}</p>
          </div>
        )}
      </div>

      {/* Period Filter */}
      <div className="flex gap-2">
        {['today', 'week', 'month', 'all_time'].map((p) => (
          <button
            key={p}
            onClick={() => setPeriod(p)}
            className={`px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
              period === p
                ? 'bg-nava-red text-white'
                : 'bg-white text-gray-700 border border-gray-200 hover:bg-gray-50'
            }`}
          >
            {periodLabels[p]}
          </button>
        ))}
      </div>

      {/* Leaderboard */}
      <div className="card">
        {isLoading ? (
          <div className="animate-pulse space-y-4">
            {[...Array(10)].map((_, i) => (
              <div key={i} className="h-16 bg-gray-100 rounded-lg" />
            ))}
          </div>
        ) : entries.length === 0 ? (
          <div className="text-center py-12 text-gray-500">
            <p className="text-lg">No data for this period</p>
            <p className="text-sm mt-1">Start getting referrals to appear on the leaderboard!</p>
          </div>
        ) : (
          <div className="space-y-3">
            {entries.map((entry, index) => {
              const isMe = entry.ambassador_name === ambassador?.name
              return (
                <div
                  key={entry.rank}
                  className={`flex items-center gap-4 p-4 rounded-xl border transition-all ${
                    isMe
                      ? 'bg-red-50 border-nava-red'
                      : getRankColor(entry.rank)
                  }`}
                >
                  {/* Rank */}
                  <div className="w-12 text-center">
                    <span className={`text-2xl ${entry.rank <= 3 ? '' : 'text-gray-500 text-lg font-medium'}`}>
                      {getRankBadge(entry.rank)}
                    </span>
                  </div>

                  {/* Info */}
                  <div className="flex-1">
                    <div className="flex items-center gap-2">
                      <p className={`font-medium ${isMe ? 'text-nava-red' : 'text-gray-900'}`}>
                        {entry.ambassador_name}
                        {isMe && <span className="ml-2 text-xs bg-nava-red text-white px-2 py-0.5 rounded">You</span>}
                      </p>
                    </div>
                    <div className="flex items-center gap-2 mt-1">
                      <span className="text-xs bg-gray-100 text-gray-600 px-2 py-0.5 rounded capitalize">
                        {entry.tier}
                      </span>
                      {entry.university && (
                        <span className="text-xs text-gray-500">{entry.university}</span>
                      )}
                      {entry.city && (
                        <span className="text-xs text-gray-500">• {entry.city}</span>
                      )}
                    </div>
                  </div>

                  {/* Stats */}
                  <div className="text-right">
                    <p className="text-lg font-bold text-gray-900">{entry.verified}</p>
                    <p className="text-xs text-gray-500">verified signups</p>
                  </div>
                  <div className="text-right w-20">
                    <p className="text-sm text-gray-600">{entry.signups}</p>
                    <p className="text-xs text-gray-500">total</p>
                  </div>
                </div>
              )
            })}
          </div>
        )}
      </div>

      {/* Tips */}
      <div className="card bg-gradient-to-r from-nava-red/5 to-pink-50">
        <h3 className="font-semibold text-gray-900 mb-2">💡 Tips to climb the leaderboard</h3>
        <ul className="text-sm text-gray-600 space-y-1">
          <li>• Share your referral code on campus WhatsApp groups</li>
          <li>• Post about NAVA on Instagram stories with your code</li>
          <li>• Help new users complete their profiles to get verified</li>
          <li>• Organize small meet-ups or events to spread the word</li>
        </ul>
      </div>
    </div>
  )
}
