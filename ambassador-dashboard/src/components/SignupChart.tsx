import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  BarChart,
  Bar,
  Legend,
} from 'recharts'

interface DailyData {
  date: string
  signups: number
  verified: number
}

interface HourlyData {
  hour: number
  signups: number
  verified: number
}

interface SignupChartProps {
  data: DailyData[] | HourlyData[]
  type: 'daily' | 'hourly'
}

export default function SignupChart({ data, type }: SignupChartProps) {
  if (type === 'hourly') {
    const hourlyData = data as HourlyData[]
    return (
      <div className="h-80">
        <ResponsiveContainer width="100%" height="100%">
          <BarChart data={hourlyData}>
            <CartesianGrid strokeDasharray="3 3" stroke="#f0f0f0" />
            <XAxis
              dataKey="hour"
              tickFormatter={(hour) => `${hour}:00`}
              tick={{ fontSize: 12 }}
            />
            <YAxis tick={{ fontSize: 12 }} />
            <Tooltip
              formatter={(value: number, name: string) => [
                value,
                name === 'signups' ? 'Total Signups' : 'Verified',
              ]}
              labelFormatter={(hour) => `${hour}:00 - ${hour + 1}:00`}
            />
            <Legend />
            <Bar dataKey="signups" name="Total Signups" fill="#E63946" radius={[4, 4, 0, 0]} />
            <Bar dataKey="verified" name="Verified" fill="#2DD4BF" radius={[4, 4, 0, 0]} />
          </BarChart>
        </ResponsiveContainer>
      </div>
    )
  }

  const dailyData = data as DailyData[]
  return (
    <div className="h-80">
      <ResponsiveContainer width="100%" height="100%">
        <AreaChart data={dailyData}>
          <defs>
            <linearGradient id="colorSignups" x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor="#E63946" stopOpacity={0.3} />
              <stop offset="95%" stopColor="#E63946" stopOpacity={0} />
            </linearGradient>
            <linearGradient id="colorVerified" x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor="#2DD4BF" stopOpacity={0.3} />
              <stop offset="95%" stopColor="#2DD4BF" stopOpacity={0} />
            </linearGradient>
          </defs>
          <CartesianGrid strokeDasharray="3 3" stroke="#f0f0f0" />
          <XAxis
            dataKey="date"
            tickFormatter={(date) => {
              const d = new Date(date)
              return `${d.getDate()}/${d.getMonth() + 1}`
            }}
            tick={{ fontSize: 12 }}
          />
          <YAxis tick={{ fontSize: 12 }} />
          <Tooltip
            formatter={(value: number, name: string) => [
              value,
              name === 'signups' ? 'Total Signups' : 'Verified',
            ]}
            labelFormatter={(date) => new Date(date).toLocaleDateString()}
          />
          <Legend />
          <Area
            type="monotone"
            dataKey="signups"
            name="Total Signups"
            stroke="#E63946"
            fill="url(#colorSignups)"
            strokeWidth={2}
          />
          <Area
            type="monotone"
            dataKey="verified"
            name="Verified"
            stroke="#2DD4BF"
            fill="url(#colorVerified)"
            strokeWidth={2}
          />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  )
}
