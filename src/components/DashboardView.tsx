import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Progress } from '@/components/ui/progress'
import { Play, Square, Activity, Zap, TrendingUp, Clock } from 'lucide-react'

interface DashboardViewProps {
  status: {
    status: string;
    is_running: boolean;
    current_streamer: string | null;
    current_drop: string | null;
    drop_image_url: string | null;
    streamer_image_url: string | null;
    progress: number;
    current_points: string;
  };
  streakStatus: {
    status: string;
    current_channels: string[];
    total_pings_sent: number;
    successful_pings: number;
    failed_pings: number;
    cycle_count: number;
    channels_info: Array<{
      channel_name: string;
      points: number;
      has_claim_available: boolean;
      status: string;
    }>;
  };
  streakRunning: boolean;
  onToggleDropMiner: () => void;
  onToggleStreakMiner: () => void;
}

export function DashboardView({ status, streakStatus, streakRunning, onToggleDropMiner, onToggleStreakMiner }: DashboardViewProps) {
  const [configuredChannels, setConfiguredChannels] = useState<string[]>([])
  const [priorityGames, setPriorityGames] = useState<string[]>([])

  useEffect(() => {
    invoke<any>('get_config').then(cfg => {
      const streakChs: string[] = cfg?.bonus?.daily_streaks?.channels ?? []
      const pointsChs: string[] = cfg?.bonus?.points_farming?.channels ?? []
      setConfiguredChannels([...new Set([...streakChs, ...pointsChs])])
      setPriorityGames(cfg?.priorities?.games ?? [])
    }).catch(console.error)
  }, [])

  const totalPoints = streakStatus.channels_info?.reduce((sum, ch) => sum + ch.points, 0) || 0;
  const activeChannels = streakStatus.channels_info?.filter(ch => ch.status === 'Watching' || ch.status === 'Ready').length || 0;
  const claimsAvailable = streakStatus.channels_info?.filter(ch => ch.has_claim_available).length || 0;

  return (
    <div className="space-y-6">
      {/* Welcome / Stats */}
      <div className="grid grid-cols-4 gap-4">
        <StatCard
          icon={<Activity className="w-5 h-5 text-purple-400" />}
          label="Drop Miner"
          value={status.is_running ? 'Active' : 'Idle'}
          color={status.is_running ? 'text-green-400' : 'text-zinc-400'}
        />
        <StatCard
          icon={<Zap className="w-5 h-5 text-blue-400" />}
          label="Points Miner"
          value={streakRunning ? 'Active' : 'Idle'}
          color={streakRunning ? 'text-blue-400' : 'text-zinc-400'}
        />
        <StatCard
          icon={<TrendingUp className="w-5 h-5 text-yellow-400" />}
          label="Total Points"
          value={totalPoints.toLocaleString()}
          color="text-yellow-400"
        />
        <StatCard
          icon={<Clock className="w-5 h-5 text-green-400" />}
          label="Claims Available"
          value={claimsAvailable.toString()}
          color={claimsAvailable > 0 ? 'text-green-400' : 'text-zinc-400'}
        />
      </div>

      {/* Quick Actions */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Drop Mining Quick Card */}
        <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
          <CardHeader>
            <CardTitle className="text-lg text-white flex items-center gap-2">
              <Activity className="w-5 h-5 text-purple-400" />
              Drop Mining
            </CardTitle>
            <CardDescription className="text-zinc-400">{status.status}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex items-center space-x-4 p-4 rounded-xl bg-white/5 border border-white/5">
              {status.streamer_image_url ? (
                <img src={status.streamer_image_url} alt="Streamer" className="w-16 h-16 rounded-full border-2 border-purple-500/50" />
              ) : (
                <div className="w-16 h-16 rounded-full bg-zinc-800 border-2 border-zinc-700 animate-pulse" />
              )}
              <div className="flex-1">
                <h3 className="font-medium text-lg text-white">{status.current_streamer || 'No Streamer'}</h3>
                <p className="text-sm text-zinc-400">{status.current_drop || 'No active drop'}</p>
              </div>
            </div>

            <div className="space-y-2">
              <div className="flex justify-between text-sm">
                <span className="text-zinc-400">Progress</span>
                <span className="text-purple-400">{status.progress.toFixed(1)}%</span>
              </div>
              <Progress value={status.progress} className="h-2 bg-zinc-800" />
            </div>

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
