import { useEffect, useRef, useState, useCallback } from 'react'
import { listen } from '@tauri-apps/api/event'
import { useAppStore } from './store'
import type { MinerStatus, StreakMinerStatus, DropItem } from './types'
import { api } from './api'
import type { UserInfo } from './api'
import { activityNotifications } from './activity'
import { Button } from '@/components/ui/button'
import {
  Settings, LayoutDashboard,
  Gamepad2, Tv, History, Wifi, WifiOff
} from 'lucide-react'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { SettingsPanel } from '@/components/Settings'
import { ChannelPointsView } from '@/components/ChannelPointsView'
import { DropsView } from '@/components/DropsView'
import { DashboardView } from '@/components/DashboardView'
import { HistoryView } from '@/components/HistoryView'
import { OnboardingTutorial } from '@/components/OnboardingTutorial'
import { AuthScreen } from './components/Auth'
import { motion, AnimatePresence } from 'framer-motion'
import { Toaster, toast } from 'sonner'

function App() {
  const { status, setStatus, isAuthenticated, setAuthenticated, setDropCampaigns, addActivity, loadActivities } = useAppStore()
  const [authDialog, setAuthDialog] = useState(false)
  const [userCode, setUserCode] = useState('')
  const [authUri, setAuthUri] = useState('')
  const [authLoading, setAuthLoading] = useState(false)
  const [userInfo, setUserInfo] = useState<UserInfo | null>(null)
  const [logoutConfirmOpen, setLogoutConfirmOpen] = useState(false)

  // Streak Miner state
  const [streakStatus, setStreakStatus] = useState<StreakMinerStatus>({
    status: 'Idle',
    current_channels: [],
    total_pings_sent: 0,
    successful_pings: 0,
    failed_pings: 0,
    cycle_count: 0,
    channels_info: [],
  })
  const [streakRunning, setStreakRunning] = useState(false)
  const [activeView, setActiveView] = useState<'dashboard' | 'drops' | 'points' | 'history' | 'settings'>('dashboard')
  const [showTutorial, setShowTutorial] = useState(false)
  const [backendStatus, setBackendStatus] = useState<'unknown' | 'connected' | 'disconnected'>('unknown')
  const lastMinerStatus = useRef(status.status)
  const lastStreakStatus = useRef(streakStatus.status)
  const backendFailCount = useRef(0)

  const statusRef = useRef<MinerStatus>(status)
  const streakStatusRef = useRef<StreakMinerStatus>(streakStatus)
  const prevActiveDropIds = useRef<string[]>([])
  const minerStartupPhase = useRef(true)

  useEffect(() => { statusRef.current = status }, [status])
  useEffect(() => { streakStatusRef.current = streakStatus }, [streakStatus])

  const updateForegroundServiceLabel = useCallback((drops: MinerStatus, streak: StreakMinerStatus) => {
    let dropsText = 'Drops: '
    if (!drops.is_running) {
      dropsText += 'Idle'
    } else {
      if (drops.current_streamer) {
        dropsText += `Mining ${drops.current_streamer}`
        if (drops.current_drop) dropsText += ` (${drops.current_drop})`
      } else {
        dropsText += drops.status || 'Running'
      }
    }

    let streakText = 'Points: '
    const isStreakRunning = streak.status !== 'Idle' && streak.status !== 'Stopped'
    if (!isStreakRunning) {
      streakText += 'Idle'
    } else {
      if (streak.current_channels && streak.current_channels.length > 0) {
        streakText += `Watching ${streak.current_channels.join(', ')}`
      } else {
        streakText += streak.status || 'Running'
      }
    }

    api.updateForegroundLabel(`${dropsText} | ${streakText}`).catch(() => {/* non-fatal */})
  }, [])

  useEffect(() => {
    if (!localStorage.getItem('has_seen_tutorial')) {
      setShowTutorial(true)
    }
  }, [])

  useEffect(() => {
    loadActivities().catch(console.error)
    api.getConfig()
      .then(activityNotifications.setConfig)
      .catch(console.error)
  }, [loadActivities])

  // Health check: ping every 30s when authenticated
  useEffect(() => {
    if (!isAuthenticated) return
    const check = async () => {
      try {
        await api.ping()
        backendFailCount.current = 0
        setBackendStatus('connected')
      } catch {
        backendFailCount.current += 1
        if (backendFailCount.current >= 3) {
          setBackendStatus('disconnected')
        }
      }
    }
    check()
    const interval = setInterval(check, 30_000)
    return () => clearInterval(interval)
  }, [isAuthenticated, setBackendStatus])

  // Check authentication status on mount
  useEffect(() => {
    const checkAuth = async () => {
      try {
        const isAuth = await api.checkAuthStatus()
        setAuthenticated(isAuth)
        if (isAuth) {
          api.getUserInfo().then((res) => {
            if (res?.data?.currentUser) {
              setUserInfo(res.data.currentUser)
            }
          }).catch(console.error)
        }
      } catch {
        setAuthenticated(false)
      }
    }
    checkAuth()
  }, [setAuthenticated])

  useEffect(() => {
    let unlisten: (() => void) | undefined
    let streakUnlisten: (() => void) | undefined
    let claimUnlisten: (() => void) | undefined

    if (isAuthenticated) {
      listen<MinerStatus>('miner-status-update', (event) => {
        setStatus(event.payload)

        const currentActiveDropIds = event.payload.active_drops?.map(d => d.id) || []
        const isStartupStatus = ['Starting...', 'Checking Configuration...', 'Idle', 'Stopped'].includes(event.payload.status)

        if (!event.payload.is_running) {
          minerStartupPhase.current = true
          prevActiveDropIds.current = []
        } else if (minerStartupPhase.current) {
          prevActiveDropIds.current = currentActiveDropIds
          if (!isStartupStatus && currentActiveDropIds.length > 0) {
            minerStartupPhase.current = false
          }
        } else {
          const newDrops = currentActiveDropIds.filter(id => !prevActiveDropIds.current.includes(id))
          if (newDrops.length > 0) {
            newDrops.forEach(id => {
              const dropDetail = event.payload.active_drops.find(d => d.id === id)
              if (dropDetail) {
                const isFinished = dropDetail.progress >= 100 || dropDetail.current_minutes >= dropDetail.required_minutes
                addActivity({
                  type: 'drop',
                  title: 'New Drop Campaign Active',
                  description: `Started mining drop: ${dropDetail.name}`,
                  silent: isFinished,
                  image_url: dropDetail.image_url,
                })
              }
            })
          }
          prevActiveDropIds.current = currentActiveDropIds
        }

        if (event.payload.status !== lastMinerStatus.current) {
          lastMinerStatus.current = event.payload.status
          addActivity({ type: 'drop', title: 'Drop miner', description: event.payload.status, silent: true })
        }

        updateForegroundServiceLabel(event.payload, streakStatusRef.current)
      }).then(fn => { unlisten = fn }).catch(console.error)

      listen<StreakMinerStatus>('streak-miner-status-update', (event) => {
        setStreakStatus(event.payload)
        setStreakRunning(event.payload.status !== 'Idle' && event.payload.status !== 'Stopped')
        if (event.payload.status !== lastStreakStatus.current) {
          lastStreakStatus.current = event.payload.status
          addActivity({ type: 'points', title: 'Points miner', description: event.payload.status, silent: true })
        }
        updateForegroundServiceLabel(statusRef.current, event.payload)
      }).then(fn => { streakUnlisten = fn }).catch(console.error)

      listen<DropItem>('drop-claimed', (event) => {
        useAppStore.getState().addClaimedDrop(event.payload)
        addActivity({
          type: 'drop',
          title: 'Drop Claimed!',
          description: `Successfully claimed drop: ${event.payload.name || 'Unknown Drop'}`,
          silent: false,
          image_url: event.payload.image_url,
        })
      }).then(fn => { claimUnlisten = fn }).catch(console.error)

      api.getStatus()
        .then(s => {
          setStatus(s)
          if (s.active_drops) prevActiveDropIds.current = s.active_drops.map(d => d.id)
          if (s.is_running && s.active_drops && s.active_drops.length > 0) minerStartupPhase.current = false
          updateForegroundServiceLabel(s, streakStatusRef.current)
        })
        .catch(console.error)

      api.getStreakMinerStatus()
        .then(s => {
          setStreakStatus(s)
          setStreakRunning(s.status !== 'Idle' && s.status !== 'Stopped')
          updateForegroundServiceLabel(statusRef.current, s)
        })
        .catch(console.error)

      api.getDropCampaigns()
        .then(campaigns => setDropCampaigns(campaigns ?? []))
        .catch(console.error)

      useAppStore.getState().loadClaimedDrops()

      // Auto-start
      api.getConfig().then(async config => {
        if (!config) return
        if (config.settings?.auto_start_drops) {
          const current = await api.getStatus()
          if (!current.is_running) await api.startMiner()
        }
        if (config.bonus?.daily_streaks?.auto_start || config.bonus?.points_farming?.auto_start) {
          const s = await api.getStreakMinerStatus()
          if (s.status === 'Idle' || s.status === 'Stopped') await api.startStreakMiner()
        }
      }).catch(console.error)
    } else {
      // Reset streak state on logout
      setStreakRunning(false)
    }

    return () => {
      unlisten?.()
      streakUnlisten?.()
      claimUnlisten?.()
    }
  }, [addActivity, isAuthenticated, setDropCampaigns, setStatus, updateForegroundServiceLabel])

  const startAuth = async () => {
    try {
      setAuthLoading(true)
      const res = await api.startAuth()
      setUserCode(res.user_code)
      setAuthUri(res.verification_uri)
      setAuthDialog(true)
      try {
        await api.pollForToken(res.device_code, res.interval)
        setAuthenticated(true)
        setAuthDialog(false)
        addActivity({ type: 'auth', title: 'Authentication', description: 'Twitch account connected successfully.' })
        toast.success('Twitch account connected!')
      } catch (err) {
        toast.error(String(err))
      }
    } catch (err) {
      toast.error(String(err))
    } finally {
      setAuthLoading(false)
    }
  }

  const toggleMiner = async () => {
    try {
      if (status.is_running) {
        await api.stopMiner()
      } else {
        await api.startMiner()
      }
    } catch (err) {
      toast.error(`Miner error: ${String(err)}`)
    }
  }

  const toggleStreakMiner = async () => {
    try {
      if (streakRunning) {
        await api.stopStreakMiner()
      } else {
        await api.startStreakMiner()
      }
    } catch (err) {
      toast.error(`Points miner error: ${String(err)}`)
    }
  }

  const handleLogout = async () => {
    try {
      if (status.is_running) await api.stopMiner()
      if (streakRunning) await api.stopStreakMiner()
      await api.logout()
      setAuthenticated(false)
      setAuthDialog(false)
      setStreakRunning(false)
      setUserInfo(null)
      setBackendStatus('unknown')
      addActivity({ type: 'auth', title: 'Authentication', description: 'Logged out and stopped active miners.' })
      toast.success('Logged out successfully.')
    } catch (err) {
      toast.error(`Logout failed: ${String(err)}`)
    }
  }

  if (!isAuthenticated) {
    return (
      <AuthScreen
        authLoading={authLoading}
        authDialog={authDialog}
        authUri={authUri}
        userCode={userCode}
        startAuth={startAuth}
        setAuthDialog={setAuthDialog}
      />
    )
  }

  const navItems = [
    { id: 'dashboard' as const, label: 'Dashboard', icon: LayoutDashboard },
    { id: 'drops' as const, label: 'Drops', icon: Gamepad2 },
    { id: 'points' as const, label: 'Channel Points', icon: Tv },
    { id: 'history' as const, label: 'History', icon: History },
    { id: 'settings' as const, label: 'Settings', icon: Settings },
  ]

  const renderView = () => {
    switch (activeView) {
      case 'dashboard':
        return <DashboardView status={status} streakStatus={streakStatus} streakRunning={streakRunning} onToggleDropMiner={toggleMiner} onToggleStreakMiner={toggleStreakMiner} />
      case 'drops':
        return <DropsView dropStatus={status} onToggleMiner={toggleMiner} />
      case 'points':
        return <ChannelPointsView streakStatus={streakStatus} streakRunning={streakRunning} onToggle={toggleStreakMiner} />
      case 'history':
        return <HistoryView />
      case 'settings':
        return <SettingsPanel />
      default:
        return null
    }
  }

  return (
    <div className="h-screen bg-black/95 text-white flex flex-col md:flex-row relative overflow-hidden font-sans">
      <Toaster richColors position="top-right" />
      <div className="absolute top-0 left-0 w-full h-64 bg-gradient-to-b from-purple-900/20 to-transparent pointer-events-none" />

      {/* Top Header for Mobile */}
      <header className="md:hidden flex h-16 items-center justify-between px-6 bg-black/40 border-b border-white/10 backdrop-blur-xl shrink-0 relative z-10" style={{ paddingTop: 'env(safe-area-inset-top)' }}>
        <h1 className="text-lg font-bold bg-clip-text text-transparent bg-gradient-to-r from-purple-400 to-blue-400">
          Twitch Miner
        </h1>
        {userInfo && (
          <div className="flex items-center gap-3">
            <img src={userInfo.profileImageURL} alt={userInfo.displayName} className="w-8 h-8 rounded-full border border-purple-500/50" />
            <button
              onClick={() => setLogoutConfirmOpen(true)}
              className="text-xs text-zinc-400 hover:text-white px-2 py-1 bg-white/5 border border-white/10 rounded-md transition-colors"
            >
              Logout
            </button>
          </div>
        )}
      </header>

      {/* Sidebar Navigation (Desktop) */}
      <aside className="hidden md:flex w-64 bg-black/40 border-r border-white/10 backdrop-blur-xl flex-col relative z-10">
        {/* Logo */}
        <div className="p-6 border-b border-white/10">
          <div className="flex items-center justify-between">
            <div>
              <h1 className="text-xl font-bold bg-clip-text text-transparent bg-gradient-to-r from-purple-400 to-blue-400">
                Twitch Miner
              </h1>
              <p className="text-xs text-zinc-500 mt-1">V2 - Rust Backend</p>
            </div>
            {/* Backend status indicator */}
            <div className="flex items-center gap-1">
              {backendStatus === 'connected' && <Wifi className="w-3 h-3 text-green-400" />}
              {backendStatus === 'disconnected' && <WifiOff className="w-3 h-3 text-red-400 animate-pulse" />}
            </div>
          </div>
        </div>

        {/* Nav Items */}
        <nav className="flex-1 p-4 space-y-1">
          {navItems.map((item) => {
            const Icon = item.icon
            const isActive = activeView === item.id
            return (
              <button
                key={item.id}
                onClick={() => setActiveView(item.id)}
                className={`w-full flex items-center gap-3 px-4 py-3 rounded-xl text-sm font-medium transition-all ${isActive
                  ? 'bg-purple-600/20 text-purple-400 border border-purple-500/30'
                  : 'text-zinc-400 hover:text-white hover:bg-white/5'
                }`}
              >
                <Icon className="w-5 h-5" />
                {item.label}
                {item.id === 'drops' && status.is_running && (
                  <span className="ml-auto w-2 h-2 rounded-full bg-green-500 animate-pulse" />
                )}
                {item.id === 'points' && streakRunning && (
                  <span className="ml-auto w-2 h-2 rounded-full bg-blue-500 animate-pulse" />
                )}
              </button>
            )
          })}
        </nav>

        {/* Status Footer */}
        <div className="p-4 border-t border-white/10">
          {userInfo && (
            <div className="flex items-center gap-3 mb-4 p-2 bg-white/5 rounded-lg border border-white/5">
              <img src={userInfo.profileImageURL} alt={userInfo.displayName} className="w-8 h-8 rounded-full" />
              <div className="flex flex-col min-w-0">
                <span className="text-sm font-semibold text-white truncate">{userInfo.displayName}</span>
                <span className="text-xs text-zinc-400 truncate">@{userInfo.login}</span>
              </div>
            </div>
          )}
          <div className="space-y-2 text-xs">
            <div className="flex justify-between text-zinc-500">
              <span>Drop Miner</span>
              <span className={status.is_running ? 'text-green-400' : 'text-zinc-600'}>
                {status.is_running ? 'Active' : 'Idle'}
              </span>
            </div>
            <div className="flex justify-between text-zinc-500">
              <span>Points Miner</span>
              <span className={streakRunning ? 'text-blue-400' : 'text-zinc-600'}>
                {streakRunning ? 'Active' : 'Idle'}
              </span>
            </div>
            {backendStatus === 'disconnected' && (
              <div className="flex items-center gap-1 text-red-400 text-xs pt-1">
                <WifiOff className="w-3 h-3" />
                Backend unreachable
              </div>
            )}
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setLogoutConfirmOpen(true)}
            className="mt-4 w-full border-white/10 text-zinc-400 hover:text-white hover:bg-white/5"
          >
            Logout
          </Button>
        </div>
      </aside>

      {/* Main Content */}
      <main className="flex-1 p-4 md:p-6 overflow-y-auto relative z-10 pb-24 md:pb-6">
        <div className="max-w-6xl mx-auto">
          <AnimatePresence mode="wait">
            <motion.div
              key={activeView}
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -10 }}
              transition={{ duration: 0.2 }}
            >
              {renderView()}
            </motion.div>
          </AnimatePresence>
        </div>
      </main>

      {/* Bottom Navigation (Mobile) */}
      <nav className="md:hidden fixed bottom-0 left-0 right-0 h-16 bg-zinc-950/80 border-t border-white/10 backdrop-blur-2xl flex justify-around items-center z-20 pb-safe" style={{ paddingBottom: 'env(safe-area-inset-bottom)' }}>
        {navItems.map((item) => {
          const Icon = item.icon
          const isActive = activeView === item.id
          return (
            <button
              key={item.id}
              onClick={() => setActiveView(item.id)}
              className={`flex flex-col items-center justify-center flex-1 h-full py-1 text-[10px] font-medium transition-colors relative ${isActive ? 'text-purple-400' : 'text-zinc-500 hover:text-zinc-300'}`}
            >
              <Icon className="w-5 h-5 mb-0.5" />
              <span>{item.label}</span>
              {item.id === 'drops' && status.is_running && (
                <span className="absolute top-2 right-[32%] w-2 h-2 rounded-full bg-green-500 animate-pulse" />
              )}
              {item.id === 'points' && streakRunning && (
                <span className="absolute top-2 right-[32%] w-2 h-2 rounded-full bg-blue-500 animate-pulse" />
              )}
            </button>
          )
        })}
      </nav>

      {/* Logout Confirmation Dialog */}
      <AlertDialog open={logoutConfirmOpen} onOpenChange={setLogoutConfirmOpen}>
        <AlertDialogContent className="bg-zinc-950 border-white/10 text-white">
          <AlertDialogHeader>
            <AlertDialogTitle>Log out?</AlertDialogTitle>
            <AlertDialogDescription className="text-zinc-400">
              This will stop all active miners and disconnect your Twitch account.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel className="border-white/10 bg-transparent text-zinc-400 hover:text-white hover:bg-white/5">Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleLogout}
              className="bg-red-600 hover:bg-red-700 text-white"
            >
              Log out
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {showTutorial && <OnboardingTutorial onComplete={() => setShowTutorial(false)} />}
    </div>
  )
}

export default App