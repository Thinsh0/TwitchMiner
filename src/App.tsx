import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useAppStore } from './store'
import type { MinerStatus } from './store'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Progress } from '@/components/ui/progress'
import { Badge } from '@/components/ui/badge'
import { open } from '@tauri-apps/plugin-shell'
import { Play, Square, Loader2 } from 'lucide-react'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogFooter } from '@/components/ui/dialog'

function App() {
  const { status, setStatus, isAuthenticated, setAuthenticated } = useAppStore()
  const [authDialog, setAuthDialog] = useState(false)
  const [userCode, setUserCode] = useState('')
  const [authUri, setAuthUri] = useState('')
  const [authLoading, setAuthLoading] = useState(false)
  
  // Streak Miner state
  const [streakStatus, setStreakStatus] = useState({
    status: "Idle",
    current_channels: [],
    total_pings_sent: 0,
    successful_pings: 0,
    failed_pings: 0,
    cycle_count: 0,
  })
  const [streakRunning, setStreakRunning] = useState(false)

  // Real-time Events for Miner Status
  useEffect(() => {
    let unlisten: any;
    let streakUnlisten: any;
    
    if (isAuthenticated) {
      // Listen for real-time updates from backend
      listen<MinerStatus>('miner-status-update', (event) => {
        setStatus(event.payload);
      }).then((unlistenFn) => {
        unlisten = unlistenFn;
      }).catch(console.error);

      // Listen for streak miner updates
      listen('streak-miner-status-update', (event) => {
        const payload = event.payload as typeof streakStatus;
        setStreakStatus(payload);
        setStreakRunning(payload.status !== "Idle" && payload.status !== "Stopped");
      }).then((unlistenFn) => {
        streakUnlisten = unlistenFn;
      }).catch(console.error);

      // Get initial status
      invoke<MinerStatus>('get_status')
        .then(setStatus)
        .catch(console.error);
      
      invoke('get_streak_miner_status')
        .then((status: any) => {
          setStreakStatus(status);
          setStreakRunning(status.status !== "Idle" && status.status !== "Stopped");
        })
        .catch(console.error);
    }
    
    return () => {
      if (unlisten) {
        unlisten();
      }
      if (streakUnlisten) {
        streakUnlisten();
      }
    };
  }, [isAuthenticated, setStatus])

  const startAuth = async () => {
    try {
      setAuthLoading(true)
      const res: any = await invoke('start_auth')
      setUserCode(res.user_code)
      setAuthUri(res.verification_uri)
      setAuthDialog(true)

      // Start polling
      try {
        await invoke('poll_for_token', { deviceCode: res.device_code, interval: res.interval })
        setAuthenticated(true)
        setAuthDialog(false)
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
      await invoke('stop_miner')
    } else {
      await invoke('start_miner')
    }
  }

  const toggleStreakMiner = async () => {
    if (streakRunning) {
      await invoke('stop_streak_miner')
    } else {
      await invoke('start_streak_miner')
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
                Go to <a href="#" onClick={() => open(authUri)} className="text-purple-400 hover:underline">{authUri}</a> and enter the code below:
              </DialogDescription>
            </DialogHeader>
            <div className="flex items-center justify-center space-x-2 py-6">
              <span className="text-4xl font-mono tracking-[0.3em] font-bold text-purple-400">{userCode}</span>
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

  return (
    <div className="min-h-screen bg-black/95 text-white p-6 relative overflow-hidden font-sans">
      <div className="absolute top-0 left-0 w-full h-64 bg-gradient-to-b from-purple-900/20 to-transparent pointer-events-none" />
      
      <div className="max-w-5xl mx-auto space-y-6 relative z-10">
        
        {/* Header */}
        <div className="flex justify-between items-center bg-black/40 border border-white/10 backdrop-blur-xl p-4 rounded-2xl shadow-xl">
          <div>
            <h1 className="text-2xl font-bold bg-clip-text text-transparent bg-gradient-to-r from-purple-400 to-blue-400">Twitch Miner</h1>
            <p className="text-sm text-zinc-400">Running efficiently in Rust 🦀</p>
          </div>
          <div className="flex items-center space-x-4">
            <Badge variant={status.is_running ? "default" : "secondary"} className={status.is_running ? "bg-green-500/20 text-green-400 border-green-500/50" : "bg-zinc-800 text-zinc-400"}>
              {status.is_running ? 'Mining Active' : 'Idle'}
            </Badge>
            <Button 
              size="lg" 
              variant={status.is_running ? "destructive" : "default"}
              onClick={toggleMiner}
              className={!status.is_running ? "bg-purple-600 hover:bg-purple-700 text-white shadow-[0_0_15px_rgba(147,51,234,0.5)]" : ""}
            >
              {status.is_running ? <><Square className="mr-2 h-4 w-4 fill-current" /> Stop</> : <><Play className="mr-2 h-4 w-4 fill-current" /> Start Mining</>}
            </Button>
          </div>
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
          {/* Drop Mining Status Card */}
          <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
            <CardHeader>
              <CardTitle className="text-lg text-white">Drop Mining</CardTitle>
              <CardDescription className="text-zinc-400">{status.status}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
              
              <div className="flex items-center space-x-4 p-4 rounded-xl bg-white/5 border border-white/5">
                {status.streamer_image_url ? (
                  <img src={status.streamer_image_url} alt="Streamer" className="w-16 h-16 rounded-full border-2 border-purple-500/50" />
                ) : (
                  <div className="w-16 h-16 rounded-full bg-zinc-800 border-2 border-zinc-700 animate-pulse" />
                )}
                <div className="flex-1">
                  <h3 className="font-medium text-lg text-white">{status.current_streamer || 'No Streamer'}</h3>
                  <p className="text-sm text-zinc-400">Watching for drops...</p>
                </div>
              </div>

              <div className="space-y-2">
                <div className="flex justify-between text-sm">
                  <span className="font-medium text-white">{status.current_drop || 'No active drop'}</span>
                  <span className="text-purple-400">{status.progress.toFixed(1)}%</span>
                </div>
                <Progress value={status.progress} className="h-2 bg-zinc-800" />
              </div>

              <Button 
                className="w-full"
                variant={status.is_running ? "destructive" : "default"}
                onClick={toggleMiner}
              >
                {status.is_running ? <><Square className="mr-2 h-4 w-4" /> Stop Drop Mining</> : <><Play className="mr-2 h-4 w-4" /> Start Drop Mining</>}
              </Button>
            </CardContent>
          </Card>

          {/* Streak Mining Status Card */}
          <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
            <CardHeader>
              <CardTitle className="text-lg text-white">Channel Points Mining</CardTitle>
              <CardDescription className="text-zinc-400">{streakStatus.status}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
              
              <div className="grid grid-cols-2 gap-4">
                <div className="p-3 rounded-xl bg-white/5 border border-white/5 text-center">
                  <div className="text-2xl font-bold text-green-400">{streakStatus.successful_pings}</div>
                  <div className="text-xs text-zinc-400">Successful</div>
                </div>
                <div className="p-3 rounded-xl bg-white/5 border border-white/5 text-center">
                  <div className="text-2xl font-bold text-red-400">{streakStatus.failed_pings}</div>
                  <div className="text-xs text-zinc-400">Failed</div>
                </div>
              </div>

              <div className="space-y-2">
                <div className="flex justify-between text-sm">
                  <span className="text-zinc-400">Total Pings</span>
                  <span className="text-white font-medium">{streakStatus.total_pings_sent}</span>
                </div>
                <div className="flex justify-between text-sm">
                  <span className="text-zinc-400">Cycles</span>
                  <span className="text-white font-medium">{streakStatus.cycle_count}</span>
                </div>
                <div className="flex justify-between text-sm">
                  <span className="text-zinc-400">Success Rate</span>
                  <span className="text-white font-medium">
                    {streakStatus.total_pings_sent > 0 
                      ? `${((streakStatus.successful_pings / streakStatus.total_pings_sent) * 100).toFixed(1)}%`
                      : "0%"
                    }
                  </span>
                </div>
              </div>

              {streakStatus.current_channels.length > 0 && (
                <div className="space-y-2">
                  <div className="text-sm text-zinc-400">Active Channels:</div>
                  <div className="flex flex-wrap gap-1">
                    {streakStatus.current_channels.slice(0, 3).map((channel, i) => (
                      <Badge key={i} variant="secondary" className="text-xs bg-blue-500/20 text-blue-400 border-blue-500/50">
                        {channel}
                      </Badge>
                    ))}
                    {streakStatus.current_channels.length > 3 && (
                      <Badge variant="secondary" className="text-xs bg-zinc-800 text-zinc-400">
                        +{streakStatus.current_channels.length - 3}
                      </Badge>
                    )}
                  </div>
                </div>
              )}

              <Button 
                className="w-full"
                variant={streakRunning ? "destructive" : "default"}
                onClick={toggleStreakMiner}
              >
                {streakRunning ? <><Square className="mr-2 h-4 w-4" /> Stop Streak Mining</> : <><Play className="mr-2 h-4 w-4" /> Start Streak Mining</>}
              </Button>
            </CardContent>
          </Card>
        </div>

        {/* Session Info */}
        <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
          <CardHeader>
            <CardTitle className="text-lg text-white">Session Info</CardTitle>
          </CardHeader>
          <CardContent className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
            <div className="text-center">
              <div className="text-zinc-400">Backend</div>
              <div className="text-white font-medium">Tauri V2 + Rust</div>
            </div>
            <div className="text-center">
              <div className="text-zinc-400">Drop Status</div>
              <div className="text-white font-medium">{status.is_running ? 'Active' : 'Idle'}</div>
            </div>
            <div className="text-center">
              <div className="text-zinc-400">Streak Status</div>
              <div className="text-white font-medium">{streakRunning ? 'Active' : 'Idle'}</div>
            </div>
            <div className="text-center">
              <div className="text-zinc-400">Channel Points</div>
              <div className="text-white font-medium">{status.current_points}</div>
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}

export default App