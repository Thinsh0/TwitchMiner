import { useState, useEffect } from 'react'
import { api } from '@/api'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Progress } from '@/components/ui/progress'
import { open } from '@tauri-apps/plugin-shell'
import { useAppStore } from '@/store'
import type { ActiveDropInfo, MinerStatus, StreakMinerStatus } from '@/types'
import { Play, Square, Activity, Zap, TrendingUp, Clock, ImageOff, Gamepad2 } from 'lucide-react'

interface DashboardViewProps {
  status: MinerStatus;
  streakStatus: StreakMinerStatus;
  streakRunning: boolean;
  onToggleDropMiner: () => void;
  onToggleStreakMiner: () => void;
}

export function DashboardView({ status, streakStatus, streakRunning, onToggleDropMiner, onToggleStreakMiner }: DashboardViewProps) {
  const [configuredChannels, setConfiguredChannels] = useState<string[]>([])
  const [priorityGames, setPriorityGames] = useState<string[]>([])
  const dropCampaigns = useAppStore(state => state.dropCampaigns)
  const activities = useAppStore(state => state.activities)

  useEffect(() => {
    api.getConfig().then(cfg => {
      const streakChs: string[] = cfg?.bonus?.daily_streaks?.channels ?? []
      const pointsChs: string[] = cfg?.bonus?.points_farming?.channels ?? []
      setConfiguredChannels([...new Set([...streakChs, ...pointsChs])])
      setPriorityGames(cfg?.priorities?.games ?? [])
    }).catch(console.error)
  }, [])

  const totalPoints = streakStatus.channels_info?.reduce((sum, ch) => sum + ch.points, 0) || 0;
  const activeChannels = streakStatus.channels_info?.filter(ch => ch.status === 'Watching' || ch.status === 'Ready').length || 0;
  const claimsAvailable = streakStatus.channels_info?.filter(ch => ch.has_claim_available).length || 0;
  const fallbackDrop = dropCampaigns
    .flatMap(campaign => campaign.drops)
    .find(drop => !drop.is_claimed && drop.current_minutes < drop.required_minutes);
  const displayDrop = status.current_drop || fallbackDrop?.name || null;
  const streamerLogin = status.current_streamer_login || status.current_streamer?.toLowerCase().replace(/\s+/g, '')
  const streamerUrl = streamerLogin
    ? `https://www.twitch.tv/${encodeURIComponent(streamerLogin)}`
    : null;
  const openStreamer = async () => {
    if (!streamerUrl) return
    try {
      await open(streamerUrl)
    } catch (err) {
      console.error('Failed to open streamer URL:', err)
    }
  }

  const [uptime, setUptime] = useState<string>('--:--:--')
  const claimedDrops = useAppStore(state => state.claimedDrops)

  useEffect(() => {
    let interval: NodeJS.Timeout
    const startedAt = (status.is_running && status.started_at) || (streakRunning && streakStatus.started_at)
    
    if (startedAt) {
      interval = setInterval(() => {
        const start = new Date(startedAt).getTime()
        const now = Date.now()
        const diff = Math.max(0, Math.floor((now - start) / 1000))
        const h = Math.floor(diff / 3600)
        const m = Math.floor((diff % 3600) / 60)
        const s = diff % 60
        setUptime(`${h.toString().padStart(2, '0')}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`)
      }, 1000)
    } else {
      setUptime('--:--:--')
    }
    return () => clearInterval(interval)
  }, [status.is_running, status.started_at, streakRunning, streakStatus.started_at])

  return (
    <div className="space-y-6">
      {/* Welcome / Stats */}
      <div className="grid grid-cols-4 gap-4">
        <StatCard
          icon={<Clock className="w-5 h-5 text-purple-400" />}
          label="Miner Uptime"
          value={uptime}
          color={status.is_running ? 'text-green-400' : 'text-zinc-400'}
        />
        <StatCard
          icon={<Gamepad2 className="w-5 h-5 text-purple-400" />}
          label="Drops Claimed"
          value={claimedDrops.length.toString()}
          color="text-purple-400"
        />
        <StatCard
          icon={<TrendingUp className="w-5 h-5 text-yellow-400" />}
          label="Total Points"
          value={totalPoints.toLocaleString()}
          color="text-yellow-400"
        />
        <StatCard
          icon={<Zap className="w-5 h-5 text-blue-400" />}
          label="Points Miner"
          value={streakRunning ? 'Active' : 'Idle'}
          color={streakRunning ? 'text-blue-400' : 'text-zinc-400'}
        />
      </div>

      {/* Quick Actions */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
          <CardHeader>
            <CardTitle className="text-lg text-white flex items-center gap-2">
              <Activity className="w-5 h-5 text-purple-400" />
              Drop Mining
            </CardTitle>
            <CardDescription className="text-zinc-400">{status.status}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {/* Streamer row */}
            <div className="flex items-center space-x-3 p-3 rounded-xl bg-white/5 border border-white/5">
              {status.streamer_image_url ? (
                <img src={status.streamer_image_url} alt="Streamer" className="w-10 h-10 rounded-full border-2 border-purple-500/50 flex-shrink-0" />
              ) : (
                <div className="w-10 h-10 rounded-full bg-zinc-800 border-2 border-zinc-700 flex-shrink-0" />
              )}
              {status.current_streamer && streamerUrl ? (
                <button
                  type="button"
                  onClick={openStreamer}
                  className="font-medium text-sm text-purple-300 underline underline-offset-4 hover:text-purple-200 transition-colors truncate"
                >
                  {status.current_streamer}
                </button>
              ) : (
                <span className="text-sm text-zinc-400">No streamer</span>
              )}
            </div>

            {/* Active drops list */}
            <ActiveDropList drops={status.active_drops} fallbackDrop={displayDrop} fallbackImage={status.drop_image_url} fallbackProgress={status.progress} />

            <Button
              className="w-full"
              variant={status.is_running ? "destructive" : "default"}
              onClick={onToggleDropMiner}
            >
              {status.is_running ? <><Square className="mr-2 h-4 w-4" /> Stop Drop Mining</> : <><Play className="mr-2 h-4 w-4" /> Start Drop Mining</>}
            </Button>
          </CardContent>
        </Card>

        {/* Channel Points Quick Card */}
        <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
          <CardHeader>
            <CardTitle className="text-lg text-white flex items-center gap-2">
              <Zap className="w-5 h-5 text-blue-400" />
              Channel Points
            </CardTitle>
            <CardDescription className="text-zinc-400">
              {activeChannels} channels active | {totalPoints.toLocaleString()} points total
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="space-y-2">
              {(streakStatus.channels_info?.length > 0 ? streakStatus.channels_info.slice(0, 3) : configuredChannels.slice(0, 3).map(name => ({ channel_name: name, points: 0, has_claim_available: false, status: 'Idle' }))).map((channel, i) => (
                <div key={i} className="flex items-center justify-between p-3 rounded-lg bg-white/5 border border-white/5">
                  <div className="flex items-center gap-2">
                    <div className="w-8 h-8 rounded-full bg-gradient-to-br from-blue-500 to-purple-500 flex items-center justify-center text-white text-xs font-bold">
                      {channel.channel_name.charAt(0).toUpperCase()}
                    </div>
                    <span className="text-white text-sm">{channel.channel_name}</span>
                  </div>
                  <div className="flex items-center gap-2">
                    {channel.has_claim_available && (
                      <Badge className="bg-yellow-500/20 text-yellow-400 text-xs">Claim!</Badge>
                    )}
                    <span className="text-blue-400 font-medium">{channel.points > 0 ? channel.points.toLocaleString() : '—'}</span>
                  </div>
                </div>
              ))}
              {configuredChannels.length === 0 && streakStatus.channels_info?.length === 0 && (
                <div className="text-center py-4 text-zinc-500 text-sm">
                  No channels configured. Go to Settings to add channels.
                </div>
              )}
              {priorityGames.length > 0 && (
                <div className="flex flex-wrap gap-1 pt-1">
                  {priorityGames.slice(0, 3).map((g, i) => (
                    <Badge key={i} variant="secondary" className="bg-purple-500/20 text-purple-400 text-xs">{g}</Badge>
                  ))}
                  {priorityGames.length > 3 && (
                    <Badge variant="secondary" className="bg-zinc-800 text-zinc-400 text-xs">+{priorityGames.length - 3}</Badge>
                  )}
                </div>
              )}
            </div>

            <Button
              className="w-full"
              variant={streakRunning ? "destructive" : "default"}
              onClick={onToggleStreakMiner}
            >
              {streakRunning ? <><Square className="mr-2 h-4 w-4" /> Stop Points Mining</> : <><Play className="mr-2 h-4 w-4" /> Start Points Mining</>}
            </Button>
          </CardContent>
        </Card>
      </div>

      <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
        <CardHeader>
          <CardTitle className="text-lg text-white flex items-center gap-2">
            <Clock className="w-5 h-5 text-green-400" />
            Recent Activity
          </CardTitle>
          <CardDescription className="text-zinc-400">
            Latest miner, authentication, and system events
          </CardDescription>
        </CardHeader>
        <CardContent>
          {activities.length > 0 ? (
            <div className="space-y-2">
              {activities.slice(0, 6).map((event) => (
                <div key={event.id} className="flex items-start justify-between gap-3 rounded-lg border border-white/5 bg-white/5 p-3">
                  <div>
                    <div className="flex items-center gap-2">
                      <Badge variant="secondary" className="bg-zinc-800 text-zinc-300 text-xs">
                        {event.type}
                      </Badge>
                      <span className="text-sm font-medium text-white">{event.title}</span>
                    </div>
                    <p className="mt-1 text-sm text-zinc-400">{event.description}</p>
                  </div>
                  <span className="shrink-0 text-xs text-zinc-500">
                    {new Date(event.timestamp).toLocaleTimeString()}
                  </span>
                </div>
              ))}
            </div>
          ) : (
            <div className="rounded-lg border border-white/5 bg-white/5 p-4 text-sm text-zinc-500">
              Activity will appear here when miners start, stop, or report status changes.
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

function StatCard({ icon, label, value, color }: { icon: React.ReactNode; label: string; value: string; color: string }) {
  return (
    <Card className="bg-black/40 border-white/10">
      <CardContent className="p-4">
        <div className="flex items-center gap-2 mb-2">
          {icon}
          <span className="text-xs text-zinc-400">{label}</span>
        </div>
        <div className={`text-xl font-bold ${color}`}>{value}</div>
      </CardContent>
    </Card>
  );
}

function ActiveDropList({
  drops,
  fallbackDrop,
  fallbackImage,
  fallbackProgress,
}: {
  drops: ActiveDropInfo[];
  fallbackDrop: string | null;
  fallbackImage: string | null;
  fallbackProgress: number;
}) {
  // If we have real active_drops from backend, show them individually
  if (drops && drops.length > 0) {
    return (
      <div className="space-y-3 max-h-52 overflow-y-auto pr-0.5">
        {drops.map((drop) => (
          <div
            key={drop.id}
            className="flex items-center gap-3 p-3 rounded-xl bg-white/5 border border-white/5 hover:bg-white/10 transition-colors"
          >
            {/* Drop image */}
            <div className="w-12 h-12 rounded-lg overflow-hidden flex-shrink-0 bg-zinc-800 border border-white/10 flex items-center justify-center">
              {drop.image_url ? (
                <img
                  src={drop.image_url}
                  alt={drop.name}
                  className="w-full h-full object-cover"
                  onError={(e) => { (e.currentTarget as HTMLImageElement).style.display = 'none' }}
                />
              ) : (
                <ImageOff className="w-5 h-5 text-zinc-600" />
              )}
            </div>
            {/* Name + progress */}
            <div className="flex-1 min-w-0">
              <p className="text-sm font-medium text-white truncate" title={drop.name}>{drop.name}</p>
              <div className="flex items-center gap-2 mt-1">
                <Progress value={drop.progress} className="h-1.5 bg-zinc-700 flex-1" />
                <span className="text-xs text-purple-400 font-mono tabular-nums flex-shrink-0">
                  {drop.progress.toFixed(1)}%
                </span>
              </div>
              <p className="text-xs text-zinc-500 mt-0.5">
                {drop.current_minutes}m / {drop.required_minutes}m
              </p>
            </div>
          </div>
        ))}
      </div>
    );
  }

  // Fallback: backend hasn't sent active_drops yet (old status or idle)
  return (
    <div className="flex items-center gap-3 p-3 rounded-xl bg-white/5 border border-white/5">
      <div className="w-12 h-12 rounded-lg overflow-hidden flex-shrink-0 bg-zinc-800 border border-white/10 flex items-center justify-center">
        {fallbackImage ? (
          <img src={fallbackImage} alt="drop" className="w-full h-full object-cover" />
        ) : (
          <ImageOff className="w-5 h-5 text-zinc-600" />
        )}
      </div>
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium text-white truncate">{fallbackDrop || 'No active drop'}</p>
        {fallbackDrop && (
          <>
            <div className="flex items-center gap-2 mt-1">
              <Progress value={fallbackProgress} className="h-1.5 bg-zinc-700 flex-1" />
              <span className="text-xs text-purple-400 font-mono tabular-nums flex-shrink-0">
                {fallbackProgress.toFixed(1)}%
              </span>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
