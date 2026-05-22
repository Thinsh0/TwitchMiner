import { useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { useAppStore } from './store'
import type { MinerStatus, StreakMinerStatus } from './types'
import { api } from './api'
import { activityNotifications } from './activity'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { openUrl } from '@tauri-apps/plugin-opener'
import { 
  Loader2, Settings, LayoutDashboard, 
  Gamepad2, Tv, History, Copy, Check
} from 'lucide-react'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogFooter } from '@/components/ui/dialog'
import { SettingsPanel } from '@/components/Settings'
import { ChannelPointsView } from '@/components/ChannelPointsView'
import { DropsView } from '@/components/DropsView'
import { DashboardView } from '@/components/DashboardView'
import { HistoryView } from '@/components/HistoryView'
import { OnboardingTutorial } from '@/components/OnboardingTutorial'
import { motion, AnimatePresence } from 'framer-motion'

function App() {
  const { status, setStatus, isAuthenticated, setAuthenticated, setDropCampaigns, addActivity, loadActivities } = useAppStore()
  const [authDialog, setAuthDialog] = useState(false)
  const [userCode, setUserCode] = useState('')
  const [authUri, setAuthUri] = useState('')
  const [authLoading, setAuthLoading] = useState(false)
  const [userInfo, setUserInfo] = useState<{ displayName: string, login: string, profileImageURL: string } | null>(null)
  
  // Streak Miner state
  const [streakStatus, setStreakStatus] = useState<StreakMinerStatus>({
    status: "Idle",
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
  const [codeCopied, setCodeCopied] = useState(false)
  const lastMinerStatus = useRef(status.status)
  const lastStreakStatus = useRef(streakStatus.status)
  
  const statusRef = useRef<MinerStatus>(status)
  const streakStatusRef = useRef<StreakMinerStatus>(streakStatus)
  const prevActiveDropIds = useRef<string[]>([])
  const hasInitializedDrops = useRef(false)
  const justStartedMiner = useRef(true)


  useEffect(() => {
    statusRef.current = status
  }, [status])

  useEffect(() => {
    streakStatusRef.current = streakStatus
  }, [streakStatus])

  const updateForegroundServiceLabel = (drops: MinerStatus, streak: StreakMinerStatus) => {
    let dropsText = "Drops: "
    if (!drops.is_running) {
      dropsText += "Idle"
    } else {
      if (drops.current_streamer) {
        dropsText += `Mining ${drops.current_streamer}`
        if (drops.current_drop) {
          dropsText += ` (${drops.current_drop})`
        }
      } else {
        dropsText += drops.status || "Running"
      }
    }

    let streakText = "Points: "
    const isStreakRunning = streak.status !== "Idle" && streak.status !== "Stopped"
    if (!isStreakRunning) {
      streakText += "Idle"
    } else {
      if (streak.current_channels && streak.current_channels.length > 0) {
        streakText += `Watching ${streak.current_channels.join(', ')}`
      } else {
        streakText += streak.status || "Running"
      }
    }

    const label = `${dropsText} | ${streakText}`
    api.updateForegroundLabel(label).catch(err => {
      console.error('Failed to update foreground label:', err)
    })
  }

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

  // Check authentication status on mount
  useEffect(() => {
    const checkAuth = async () => {
      try {
        const isAuth = await api.checkAuthStatus();
        console.log('Auth status check:', isAuth);
        setAuthenticated(isAuth);
        if (isAuth) {
          api.getUserInfo().then((info: any) => {
            if (info?.data?.currentUser) {
              setUserInfo(info.data.currentUser)
            }
          }).catch(console.error)
        }
      } catch (err) {
        console.error('Auth check failed:', err);
        setAuthenticated(false);
      }
    };
    
    checkAuth();
  }, [setAuthenticated]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let streakUnlisten: (() => void) | undefined;
    let claimUnlisten: (() => void) | undefined;
    
    if (isAuthenticated) {
      // Listen for real-time updates from backend
      listen<MinerStatus>('miner-status-update', (event) => {
        setStatus(event.payload);

        // Detect new drops/campaigns
        const currentActiveDropIds = event.payload.active_drops?.map(d => d.id) || []
        
        if (!event.payload.is_running) {
          justStartedMiner.current = true
          prevActiveDropIds.current = []
        } else {
          if (justStartedMiner.current) {
            prevActiveDropIds.current = currentActiveDropIds
            if (event.payload.status !== "Starting..." && event.payload.status !== "Checking Configuration...") {
              justStartedMiner.current = false
            }
          } else {
            const wasRunning = statusRef.current?.is_running ?? false
            const newDrops = currentActiveDropIds.filter(id => !prevActiveDropIds.current.includes(id))
            
            if (newDrops.length > 0) {
              newDrops.forEach(id => {
                const dropDetail = event.payload.active_drops.find(d => d.id === id)
                if (dropDetail) {
                  const isFinished = dropDetail.progress >= 100 || dropDetail.current_minutes >= dropDetail.required_minutes
                  const shouldAlert = wasRunning && !isFinished
                  addActivity({
                    type: 'drop',
                    title: 'New Drop Campaign Active',
                    description: `Started mining drop: ${dropDetail.name}`,
                    silent: !shouldAlert,
                  })
                }
              })
            }
            prevActiveDropIds.current = currentActiveDropIds
          }
        }

        if (event.payload.status !== lastMinerStatus.current) {
          lastMinerStatus.current = event.payload.status
          addActivity({
            type: 'drop',
            title: 'Drop miner',
            description: event.payload.status,
            silent: true,
          })
        }

        updateForegroundServiceLabel(event.payload, streakStatusRef.current)
      }).then((unlistenFn) => {
        unlisten = unlistenFn;
      }).catch(console.error);

      // Listen for streak miner updates
      listen<StreakMinerStatus>('streak-miner-status-update', (event) => {
        setStreakStatus(event.payload);
        setStreakRunning(event.payload.status !== "Idle" && event.payload.status !== "Stopped");
        if (event.payload.status !== lastStreakStatus.current) {
          lastStreakStatus.current = event.payload.status
          addActivity({
            type: 'points',
            title: 'Points miner',
            description: event.payload.status,
            silent: true,
          })
        }

        updateForegroundServiceLabel(statusRef.current, event.payload)
      }).then((unlistenFn) => {
        streakUnlisten = unlistenFn;
      }).catch(console.error);

      listen<any>('drop-claimed', (event) => {
        useAppStore.getState().addClaimedDrop(event.payload);
        addActivity({
          type: 'drop',
          title: 'Drop Claimed!',
          description: `Successfully claimed drop: ${event.payload.name || 'Unknown Drop'}`,
          silent: false,
        })
      }).then((unlistenFn) => {
        claimUnlisten = unlistenFn;
      }).catch(console.error);

      // Get initial status
      api.getStatus()
        .then((s) => {
          setStatus(s);
          if (s.active_drops) {
            prevActiveDropIds.current = s.active_drops.map(d => d.id);
          }
          hasInitializedDrops.current = true;
          updateForegroundServiceLabel(s, streakStatusRef.current);
        })
        .catch(console.error);
      
      api.getStreakMinerStatus()
        .then((status) => {
          setStreakStatus(status);
          setStreakRunning(status.status !== "Idle" && status.status !== "Stopped");
          updateForegroundServiceLabel(statusRef.current, status);
        })
        .catch(console.error);

      // Fetch drop campaigns once on mount and store globally
      api.getDropCampaigns()
        .then((campaigns) => {
          setDropCampaigns(campaigns ?? [])
        })
        .catch(console.error);

      useAppStore.getState().loadClaimedDrops();

      // Handle Auto-Start based on configuration
      api.getConfig().then(async (config) => {
        if (!config) return;
        
        // Auto-start Drops Miner
        if (config.settings?.auto_start_drops) {
          const currentStatus = await api.getStatus();
          if (!currentStatus.is_running) {
            console.log('Auto-starting Drops Miner');
            await api.startMiner();
          }
        }

        // Auto-start Streak/Points Miner
        if (config.bonus?.daily_streaks?.auto_start || config.bonus?.points_farming?.auto_start) {
          const streakStatus = await api.getStreakMinerStatus();
          if (streakStatus.status === "Idle" || streakStatus.status === "Stopped") {
            console.log('Auto-starting Streak/Points Miner');
            await api.startStreakMiner();
          }
        }
      }).catch(console.error);
    }
    
    return () => {
      if (unlisten) {
        unlisten();
      }
      if (streakUnlisten) {
        streakUnlisten();
      }
      if (claimUnlisten) {
        claimUnlisten();
      }
    };
  }, [addActivity, isAuthenticated, setDropCampaigns, setStatus])

  const startAuth = async () => {
    try {
      setAuthLoading(true)
      const res = await api.startAuth()
      setUserCode(res.user_code)
      setAuthUri(res.verification_uri)
      setAuthDialog(true)

      // Start polling
      try {
        await api.pollForToken(res.device_code, res.interval)
        setAuthenticated(true)
        setAuthDialog(false)
        addActivity({
          type: 'auth',
          title: 'Authentication',
          description: 'Twitch account connected successfully.',
        })
      } catch (err) {
        alert(err)
      }
    } catch (err) {
      alert(err)
    } finally {
      setAuthLoading(false)
    }
  }

  const toggleMiner = async () => {
    if (status.is_running) {
      await api.stopMiner()
    } else {
      await api.startMiner()
    }
  }

  const toggleStreakMiner = async () => {
    if (streakRunning) {
      await api.stopStreakMiner()
    } else {
      await api.startStreakMiner()
    }
  }

  const handleCopyCode = async () => {
    try {
      const { writeText } = await import('@tauri-apps/plugin-clipboard-manager')
      await writeText(userCode)
    } catch (e) {
      await navigator.clipboard.writeText(userCode)
    }
    setCodeCopied(true)
    setTimeout(() => setCodeCopied(false), 2000)
  }

  const handleLogout = async () => {
    try {
      if (status.is_running) {
        await api.stopMiner()
      }
      if (streakRunning) {
        await api.stopStreakMiner()
      }
      await api.logout()
      setAuthenticated(false)
      setAuthDialog(false)
      addActivity({
        type: 'auth',
        title: 'Authentication',
        description: 'Logged out and stopped active miners.',
      })
    } catch (err) {
      console.error('Logout failed:', err)
      alert(err)
    }
  }

  if (!isAuthenticated) {
    return (
      <div className="flex h-screen items-center justify-center bg-black/95 relative overflow-hidden">
        <div className="absolute top-[-20%] left-[-10%] w-[500px] h-[500px] bg-purple-600/30 blur-[120px] rounded-full" />
        <div className="absolute bottom-[-20%] right-[-10%] w-[400px] h-[400px] bg-blue-600/20 blur-[100px] rounded-full" />
        
        <Card className="w-[400px] bg-black/40 border-white/10 backdrop-blur-xl z-10 shadow-2xl">
          <CardHeader>
            <CardTitle className="text-2xl font-bold text-white">Twitch Miner V2</CardTitle>
            <CardDescription className="text-zinc-400">Please authenticate to start mining.</CardDescription>
          </CardHeader>
          <CardContent>
            <Button className="w-full bg-purple-600 hover:bg-purple-700 text-white" onClick={startAuth} disabled={authLoading}>
              {authLoading ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
              Login with Twitch
            </Button>
          </CardContent>
        </Card>

        <Dialog open={authDialog} onOpenChange={setAuthDialog}>
          <DialogContent className="bg-zinc-950 border-white/10 text-white sm:max-w-md">
            <DialogHeader>
              <DialogTitle>Link your Account</DialogTitle>
              <DialogDescription className="text-zinc-400">
                Go to <a href="#" onClick={(e) => { e.preventDefault(); openUrl(authUri); }} className="text-purple-400 hover:underline">{authUri}</a> and enter the code below:
              </DialogDescription>
            </DialogHeader>
            <div 
              className="flex flex-col items-center justify-center space-y-2 py-6 cursor-pointer group"
              onClick={handleCopyCode}
            >
              <span className="text-4xl font-mono tracking-[0.3em] font-bold text-purple-400 group-hover:text-purple-300 transition-colors">
                {userCode}
              </span>
              <span className={`text-sm transition-opacity ${codeCopied ? 'text-green-400 opacity-100' : 'text-zinc-500 opacity-0 group-hover:opacity-100'}`}>
                {codeCopied ? <><Check className="w-4 h-4 inline mr-1" /> Copied!</> : <><Copy className="w-4 h-4 inline mr-1" /> Click to copy</>}
              </span>
            </div>
            <DialogFooter className="sm:justify-start">
              <p className="text-sm text-zinc-500 flex items-center">
                <Loader2 className="mr-2 h-4 w-4 animate-spin" /> Waiting for confirmation...
              </p>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </div>
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
              onClick={handleLogout}
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
          <h1 className="text-xl font-bold bg-clip-text text-transparent bg-gradient-to-r from-purple-400 to-blue-400">
            Twitch Miner
          </h1>
          <p className="text-xs text-zinc-500 mt-1">V2 - Rust Backend</p>
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
                className={`w-full flex items-center gap-3 px-4 py-3 rounded-xl text-sm font-medium transition-all ${
                  isActive
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
              <div className="flex flex-col">
                <span className="text-sm font-semibold text-white">{userInfo.displayName}</span>
                <span className="text-xs text-zinc-400">@{userInfo.login}</span>
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
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={handleLogout}
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
              className={`flex flex-col items-center justify-center flex-1 h-full py-1 text-[10px] font-medium transition-colors relative ${
                isActive ? 'text-purple-400' : 'text-zinc-500 hover:text-zinc-300'
              }`}
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

      {/* Auth Dialog (kept for compatibility) */}
      <Dialog open={authDialog} onOpenChange={setAuthDialog}>
        <DialogContent className="bg-zinc-950 border-white/10 text-white sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Link your Account</DialogTitle>
            <DialogDescription className="text-zinc-400">
              Go to <a href="#" onClick={(e) => { e.preventDefault(); openUrl(authUri); }} className="text-purple-400 hover:underline">{authUri}</a> and enter the code below:
            </DialogDescription>
          </DialogHeader>
          <div 
            className="flex flex-col items-center justify-center space-y-2 py-6 cursor-pointer group"
            onClick={handleCopyCode}
          >
            <span className="text-4xl font-mono tracking-[0.3em] font-bold text-purple-400 group-hover:text-purple-300 transition-colors">
              {userCode}
            </span>
            <span className={`text-sm transition-opacity ${codeCopied ? 'text-green-400 opacity-100' : 'text-zinc-500 opacity-0 group-hover:opacity-100'}`}>
              {codeCopied ? <><Check className="w-4 h-4 inline mr-1" /> Copied!</> : <><Copy className="w-4 h-4 inline mr-1" /> Click to copy</>}
            </span>
          </div>
          <DialogFooter className="sm:justify-start">
            <p className="text-sm text-zinc-500 flex items-center">
              <Loader2 className="mr-2 h-4 w-4 animate-spin" /> Waiting for confirmation...
            </p>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      {showTutorial && <OnboardingTutorial onComplete={() => setShowTutorial(false)} />}
    </div>
  )
}

export default App