import axios from 'axios'

const API_BASE_URL = import.meta.env.VITE_API_URL || ''

export const api = axios.create({
  baseURL: API_BASE_URL,
  headers: {
    'Content-Type': 'application/json',
  },
})

// Add auth token interceptor
api.interceptors.request.use((config) => {
  const token = localStorage.getItem('ambassador_token')
  if (token) {
    config.headers.Authorization = `Bearer ${token}`
  }
  return config
})

// Handle 401 errors (token expired)
api.interceptors.response.use(
  (response) => response,
  (error) => {
    if (error.response?.status === 401) {
      localStorage.removeItem('ambassador_token')
      localStorage.removeItem('ambassador_data')
      window.location.href = '/login'
    }
    return Promise.reject(error)
  }
)

// Types
export interface DailySignup {
  date: string
  signups: number
  verified: number
  conversion_rate: number
}

export interface HourlySignup {
  hour: number
  signups: number
  verified: number
}

export interface Subscriber {
  id: number
  name: string
  email: string
  phone?: string
  signed_up_at: string
  verified_at?: string
  status: 'pending' | 'verified' | 'churned'
  subscription_tier?: string
  is_active: boolean
}

export interface PerformanceStats {
  today_signups: number
  this_week_signups: number
  this_month_signups: number
  this_month_verified: number
  monthly_target: number
  target_progress_pct: number
  signups_to_target: number
  base_stipend: string
  bonus_earned: string
  total_this_month: string
  total_all_time: string
  total_signups: number
  total_verified: number
}

export interface LeaderboardEntry {
  rank: number
  ambassador_name: string
  university?: string
  city?: string
  tier: string
  signups: number
  verified: number
}

// API functions
export const ambassadorApi = {
  // Auth
  login: (email: string, password: string) =>
    api.post('/api/ambassador/login', { email, password }),

  // Profile
  getProfile: () => api.get('/api/ambassador/me'),

  // Performance
  getPerformance: () => api.get<PerformanceStats>('/api/ambassador/performance'),

  // Signups
  getDailySignups: (startDate?: string, endDate?: string) =>
    api.get<{ days: DailySignup[]; total_signups: number; total_verified: number }>('/api/ambassador/daily', {
      params: { start_date: startDate, end_date: endDate },
    }),

  getHourlySignups: (date: string) =>
    api.get<{ hours: HourlySignup[] }>('/api/ambassador/hourly', {
      params: { date },
    }),

  // Subscribers
  getSubscribers: (page: number = 1, limit: number = 20, status?: string) =>
    api.get<{ subscribers: Subscriber[]; total: number; page: number }>('/api/ambassador/subscribers', {
      params: { page, limit, status },
    }),

  getActiveSubscribers: () =>
    api.get<{ subscribers: Subscriber[]; total: number }>('/api/ambassador/subscribers/active'),

  // Leaderboard
  getLeaderboard: (period: string = 'month') =>
    api.get<{ entries: LeaderboardEntry[]; my_rank?: number }>('/api/ambassador/leaderboard', {
      params: { period },
    }),
}
