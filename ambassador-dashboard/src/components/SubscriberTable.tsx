import { Subscriber } from '../services/api'

interface SubscriberTableProps {
  subscribers: Subscriber[]
  isLoading?: boolean
}

const statusColors = {
  pending: 'bg-yellow-100 text-yellow-800',
  verified: 'bg-green-100 text-green-800',
  churned: 'bg-red-100 text-red-800',
}

export default function SubscriberTable({ subscribers, isLoading }: SubscriberTableProps) {
  if (isLoading) {
    return (
      <div className="animate-pulse">
        {[...Array(5)].map((_, i) => (
          <div key={i} className="h-16 bg-gray-100 mb-2 rounded" />
        ))}
      </div>
    )
  }

  if (subscribers.length === 0) {
    return (
      <div className="text-center py-12 text-gray-500">
        <p className="text-lg">No subscribers found</p>
        <p className="text-sm mt-1">Share your referral code to get started!</p>
      </div>
    )
  }

  return (
    <div className="overflow-x-auto">
      <table className="w-full">
        <thead>
          <tr className="border-b border-gray-200">
            <th className="text-left py-3 px-4 text-sm font-medium text-gray-500">Name</th>
            <th className="text-left py-3 px-4 text-sm font-medium text-gray-500">Signed Up</th>
            <th className="text-left py-3 px-4 text-sm font-medium text-gray-500">Status</th>
            <th className="text-left py-3 px-4 text-sm font-medium text-gray-500">Subscription</th>
            <th className="text-left py-3 px-4 text-sm font-medium text-gray-500">Active</th>
          </tr>
        </thead>
        <tbody>
          {subscribers.map((subscriber) => (
            <tr key={subscriber.id} className="border-b border-gray-100 hover:bg-gray-50">
              <td className="py-3 px-4">
                <div>
                  <p className="font-medium text-gray-900">{subscriber.name}</p>
                  <p className="text-sm text-gray-500">{subscriber.email}</p>
                </div>
              </td>
              <td className="py-3 px-4 text-sm text-gray-600">
                {new Date(subscriber.signed_up_at).toLocaleDateString()}
                <br />
                <span className="text-xs text-gray-400">
                  {new Date(subscriber.signed_up_at).toLocaleTimeString()}
                </span>
              </td>
              <td className="py-3 px-4">
                <span className={`px-2 py-1 rounded-full text-xs font-medium ${statusColors[subscriber.status]}`}>
                  {subscriber.status}
                </span>
              </td>
              <td className="py-3 px-4 text-sm text-gray-600">
                {subscriber.subscription_tier || 'Free'}
              </td>
              <td className="py-3 px-4">
                {subscriber.is_active ? (
                  <span className="flex items-center gap-1 text-green-600">
                    <span className="w-2 h-2 bg-green-500 rounded-full animate-pulse" />
                    Active
                  </span>
                ) : (
                  <span className="text-gray-400">Inactive</span>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
