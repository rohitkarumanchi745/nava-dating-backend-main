import { useEffect, useState } from 'react'
import { format, subDays } from 'date-fns'
import { ambassadorApi, DailySignup, HourlySignup } from '../services/api'
import SignupChart from '../components/SignupChart'
import StatCard from '../components/StatCard'

export default function SignupsPage() {
  const [dailyData, setDailyData] = useState<DailySignup[]>([])
  const [hourlyData, setHourlyData] = useState<HourlySignup[]>([])
  const [selectedDate, setSelectedDate] = useState(format(new Date(), 'yyyy-MM-dd'))
  const [dateRange, setDateRange] = useState('30')
  const [viewMode, setViewMode] = useState<'daily' | 'hourly'>('daily')
  const [isLoading, setIsLoading] = useState(true)
  const [totals, setTotals] = useState({ signups: 0, verified: 0 })

  useEffect(() => {
    loadDailyData()
  }, [dateRange])

  useEffect(() => {
    if (viewMode === 'hourly') {
      loadHourlyData()
    }
  }, [selectedDate, viewMode])

  const loadDailyData = async () => {
    setIsLoading(true)
    try {
      const endDate = format(new Date(), 'yyyy-MM-dd')
      const startDate = format(subDays(new Date(), parseInt(dateRange)), 'yyyy-MM-dd')
      const res = await ambassadorApi.getDailySignups(startDate, endDate)
      setDailyData(res.data.days)
      setTotals({
        signups: res.data.total_signups,
        verified: res.data.total_verified,
      })
    } catch (error) {
      console.error('Failed to load daily data:', error)
    } finally {
      setIsLoading(false)
    }
  }

  const loadHourlyData = async () => {
    setIsLoading(true)
    try {
      const res = await ambassadorApi.getHourlySignups(selectedDate)
      setHourlyData(res.data.hours)
    } catch (error) {
      console.error('Failed to load hourly data:', error)
    } finally {
      setIsLoading(false)
    }
  }

  const conversionRate = totals.signups > 0
    ? ((totals.verified / totals.signups) * 100).toFixed(1)
    : '0'

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-gray-900">Signup Analytics</h1>
          <p className="text-gray-500 mt-1">Track your referral signups by day and hour</p>
        </div>
        <div className="flex gap-2">
          <button
            onClick={() => setViewMode('daily')}
            className={`px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
              viewMode === 'daily'
                ? 'bg-nava-red text-white'
                : 'bg-gray-100 text-gray-700 hover:bg-gray-200'
            }`}
          >
            Daily View
          </button>
          <button
            onClick={() => setViewMode('hourly')}
            className={`px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
              viewMode === 'hourly'
                ? 'bg-nava-red text-white'
                : 'bg-gray-100 text-gray-700 hover:bg-gray-200'
            }`}
          >
            Hourly View
          </button>
        </div>
      </div>

      {/* Stats */}
      <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
        <StatCard
          title="Total Signups"
          value={totals.signups}
          subtitle={`Last ${dateRange} days`}
          icon="👥"
          color="blue"
        />
        <StatCard
          title="Verified Signups"
          value={totals.verified}
          icon="✅"
          color="green"
        />
        <StatCard
          title="Conversion Rate"
          value={`${conversionRate}%`}
          icon="📊"
          color="yellow"
        />
        <StatCard
          title="Avg Daily"
          value={(totals.signups / parseInt(dateRange)).toFixed(1)}
          icon="📈"
          color="red"
        />
      </div>

      {/* Filters */}
      <div className="card">
        <div className="flex flex-wrap items-center gap-4 mb-6">
          {viewMode === 'daily' ? (
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">
                Date Range
              </label>
              <select
                value={dateRange}
                onChange={(e) => setDateRange(e.target.value)}
                className="input w-40"
              >
                <option value="7">Last 7 days</option>
                <option value="14">Last 14 days</option>
                <option value="30">Last 30 days</option>
                <option value="60">Last 60 days</option>
                <option value="90">Last 90 days</option>
              </select>
            </div>
          ) : (
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">
                Select Date
              </label>
              <input
                type="date"
                value={selectedDate}
                onChange={(e) => setSelectedDate(e.target.value)}
                max={format(new Date(), 'yyyy-MM-dd')}
                className="input w-48"
              />
            </div>
          )}
        </div>

        {/* Chart */}
        {isLoading ? (
          <div className="h-80 bg-gray-100 rounded-lg animate-pulse" />
        ) : (
          <SignupChart
            data={viewMode === 'daily' ? dailyData : hourlyData}
            type={viewMode}
          />
        )}
      </div>

      {/* Data Table */}
      <div className="card">
        <h2 className="text-lg font-semibold text-gray-900 mb-4">
          {viewMode === 'daily' ? 'Daily Breakdown' : 'Hourly Breakdown'}
        </h2>
        <div className="overflow-x-auto">
          <table className="w-full">
            <thead>
              <tr className="border-b border-gray-200">
                <th className="text-left py-3 px-4 text-sm font-medium text-gray-500">
                  {viewMode === 'daily' ? 'Date' : 'Hour'}
                </th>
                <th className="text-right py-3 px-4 text-sm font-medium text-gray-500">Total Signups</th>
                <th className="text-right py-3 px-4 text-sm font-medium text-gray-500">Verified</th>
                <th className="text-right py-3 px-4 text-sm font-medium text-gray-500">Conversion</th>
              </tr>
            </thead>
            <tbody>
              {viewMode === 'daily'
                ? dailyData.map((row) => (
                    <tr key={row.date} className="border-b border-gray-100 hover:bg-gray-50">
                      <td className="py-3 px-4 text-sm text-gray-900">
                        {new Date(row.date).toLocaleDateString('en-IN', {
                          weekday: 'short',
                          month: 'short',
                          day: 'numeric',
                        })}
                      </td>
                      <td className="py-3 px-4 text-sm text-right text-gray-900 font-medium">
                        {row.signups}
                      </td>
                      <td className="py-3 px-4 text-sm text-right text-green-600 font-medium">
                        {row.verified}
                      </td>
                      <td className="py-3 px-4 text-sm text-right text-gray-600">
                        {row.conversion_rate.toFixed(1)}%
                      </td>
                    </tr>
                  ))
                : hourlyData.map((row) => (
                    <tr key={row.hour} className="border-b border-gray-100 hover:bg-gray-50">
                      <td className="py-3 px-4 text-sm text-gray-900">
                        {row.hour}:00 - {row.hour + 1}:00
                      </td>
                      <td className="py-3 px-4 text-sm text-right text-gray-900 font-medium">
                        {row.signups}
                      </td>
                      <td className="py-3 px-4 text-sm text-right text-green-600 font-medium">
                        {row.verified}
                      </td>
                      <td className="py-3 px-4 text-sm text-right text-gray-600">
                        {row.signups > 0
                          ? ((row.verified / row.signups) * 100).toFixed(1)
                          : 0}%
                      </td>
                    </tr>
                  ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  )
}
