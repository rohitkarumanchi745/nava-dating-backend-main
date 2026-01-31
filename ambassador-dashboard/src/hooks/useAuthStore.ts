import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import { api } from '../services/api'

interface Ambassador {
  id: number
  user_id: number
  name: string
  email: string
  tier: string
  university?: string
  city?: string
  region?: string
  referral_code: string
  status: string
}

interface AuthState {
  token: string | null
  ambassador: Ambassador | null
  isAuthenticated: boolean
  isLoading: boolean
  error: string | null
  login: (email: string, password: string) => Promise<void>
  logout: () => void
  clearError: () => void
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set, get) => ({
      token: null,
      ambassador: null,
      isAuthenticated: false,
      isLoading: false,
      error: null,

      login: async (email: string, password: string) => {
        set({ isLoading: true, error: null })

        try {
          const response = await api.post('/api/ambassador/login', {
            email,
            password,
          })

          const { access_token, ambassador } = response.data

          // Store token in localStorage for the API interceptor
          localStorage.setItem('ambassador_token', access_token)

          set({
            token: access_token,
            ambassador,
            isAuthenticated: true,
            isLoading: false,
          })

          // Set token for future requests
          api.defaults.headers.common['Authorization'] = `Bearer ${access_token}`
        } catch (error: any) {
          set({
            isLoading: false,
            error: error.response?.data?.error || 'Login failed. Please check your credentials.',
          })
          throw error
        }
      },

      logout: () => {
        localStorage.removeItem('ambassador_token')
        set({
          token: null,
          ambassador: null,
          isAuthenticated: false,
        })
        delete api.defaults.headers.common['Authorization']
      },

      clearError: () => set({ error: null }),
    }),
    {
      name: 'ambassador-auth',
      partialize: (state) => ({
        token: state.token,
        ambassador: state.ambassador,
        isAuthenticated: state.isAuthenticated,
      }),
    }
  )
)

// Initialize token on app load
const { token } = useAuthStore.getState()
if (token) {
  api.defaults.headers.common['Authorization'] = `Bearer ${token}`
}
