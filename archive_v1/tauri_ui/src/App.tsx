import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import Dashboard from './components/Dashboard'
import Sidebar from './components/Sidebar'
import DropsPage from './components/DropsPage'
import BonusPage from './components/BonusPage'
import SettingsPage from './components/SettingsPage'
import { getStatus } from './lib/api'
import './index.css'

const queryClient = new QueryClient()

import { useEffect } from 'react'
import { startSidecar } from './lib/api'

// ...

function MainLayout() {
  const [view, setView] = useState<'dashboard' | 'drops' | 'bonus' | 'settings'>('dashboard')

  useEffect(() => {
    startSidecar();
  }, [])


  // Global Status for Sidebar
  const { data: status } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: 2000
  })

  return (
    <div className="flex h-screen bg-dark-bg text-dark-text overflow-hidden font-sans">
      <Sidebar currentView={view} onChangeView={setView} connected={!!status?.authenticated} />

      <main className="flex-1 overflow-y-auto p-8 relative">
        {/* Background Decor */}
        <div className="absolute top-0 left-0 w-full h-96 bg-gradient-to-b from-twitch/5 to-transparent pointer-events-none" />

        <div className="relative z-10 max-w-6xl mx-auto h-full">
          {view === 'dashboard' && <Dashboard />}
          {view === 'drops' && <DropsPage />}
          {view === 'bonus' && <BonusPage />}
          {view === 'settings' && <SettingsPage />}
        </div>
      </main>
    </div>
  )
}

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <MainLayout />
    </QueryClientProvider>
  )
}

export default App
