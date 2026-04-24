import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
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

  // Polling Miner Status
  useEffect(() => {
    let interval: number | undefined;
    
    if (isAuthenticated) {
      // Poll backend every 2s
      interval = window.setInterval(async () => {
        try {
          const newStatus = await invoke<MinerStatus>('get_status');
          setStatus(newStatus);
        } catch (e) {
          console.error(e);
        }
      }, 2000);
    }
    
    return () => clearInterval(interval);
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

        <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
          {/* Main Status Card */}
          <Card className="md:col-span-2 bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
            <CardHeader>
              <CardTitle className="text-lg text-white">Current Operation</CardTitle>
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
            </CardContent>
          </Card>

          {/* Side Panel */}
          <div className="space-y-6">
            <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
              <CardHeader>
                <CardTitle className="text-lg text-white">Channel Points</CardTitle>
              </CardHeader>
              <CardContent>
                <div className="text-3xl font-bold text-green-400 drop-shadow-[0_0_10px_rgba(74,222,128,0.3)]">
                  {status.current_points}
                </div>
                <p className="text-sm text-zinc-400 mt-1">Farmable Points</p>
              </CardContent>
            </Card>

            <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
              <CardHeader>
                <CardTitle className="text-lg text-white">Session Info</CardTitle>
              </CardHeader>
              <CardContent className="space-y-2 text-sm text-zinc-400">
                <div className="flex justify-between">
                  <span>Backend</span>
                  <span className="text-white">Tauri V2 + Rust</span>
                </div>
                <div className="flex justify-between">
                  <span>Status</span>
                  <span className="text-white">{status.is_running ? 'Connected' : 'Disconnected'}</span>
                </div>
              </CardContent>
            </Card>
          </div>
        </div>

      </div>
    </div>
  )
}

export default App