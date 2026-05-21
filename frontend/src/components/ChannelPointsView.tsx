import { useState, useEffect } from 'react'
import { api } from '@/api'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Tv, Play, Square, Gift, AlertCircle, CheckCircle2, Clock, Eye } from 'lucide-react'
import type { ChannelPointsInfo, StreakMinerStatus } from '@/types'

interface ChannelPointsViewProps {
  streakStatus: StreakMinerStatus;
  streakRunning: boolean;
  onToggle: () => void;
}

export function ChannelPointsView({ streakStatus, streakRunning, onToggle }: ChannelPointsViewProps) {
  const [configuredChannels, setConfiguredChannels] = useState<string[]>([])

  useEffect(() => {
    api.getConfig().then(cfg => {
      const streakChs: string[] = cfg?.bonus?.daily_streaks?.channels ?? []
      const pointsChs: string[] = cfg?.bonus?.points_farming?.channels ?? []
      // Merge both lists (unique)
      const all = [...new Set([...streakChs, ...pointsChs])]
      setConfiguredChannels(all)
    }).catch(console.error)
  }, [])

  // Merge live channels_info with configured channels (live data takes priority)
  const mergedChannels: ChannelPointsInfo[] = configuredChannels.map(name => {
    const live = streakStatus.channels_info.find(
      ch => ch.channel_name.toLowerCase() === name.toLowerCase()
    )
    return live ?? { channel_name: name, points: 0, has_claim_available: false, status: 'Idle' }
  })

  const totalPoints = mergedChannels.reduce((sum, ch) => sum + ch.points, 0);

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex justify-between items-center">
        <div>
          <h2 className="text-xl font-bold text-white flex items-center gap-2">
            <Tv className="w-5 h-5 text-blue-400" />
            Channel Points Mining
          </h2>
          <p className="text-sm text-zinc-400 mt-1">
            Watch channels to earn points and claim rewards
          </p>
        </div>
        <Button
          size="lg"
          variant={streakRunning ? "destructive" : "default"}
          onClick={onToggle}
          className={!streakRunning ? "bg-blue-600 hover:bg-blue-700 text-white" : ""}
        >
          {streakRunning ? <><Square className="mr-2 h-4 w-4 fill-current" /> Stop Mining</> : <><Play className="mr-2 h-4 w-4 fill-current" /> Start Mining</>}
        </Button>
      </div>

      {/* Stats Overview */}
      <div className="grid grid-cols-4 gap-4">
        <Card className="bg-black/40 border-white/10">
          <CardContent className="p-4 text-center">
            <div className="text-2xl font-bold text-blue-400">{totalPoints.toLocaleString()}</div>
            <div className="text-xs text-zinc-400 mt-1">Total Points</div>
          </CardContent>
        </Card>
        <Card className="bg-black/40 border-white/10">
          <CardContent className="p-4 text-center">
            <div className="text-2xl font-bold text-green-400">{streakStatus.successful_pings}</div>
            <div className="text-xs text-zinc-400 mt-1">Successful Pings</div>
          </CardContent>
        </Card>
        <Card className="bg-black/40 border-white/10">
          <CardContent className="p-4 text-center">
            <div className="text-2xl font-bold text-purple-400">{streakStatus.cycle_count}</div>
            <div className="text-xs text-zinc-400 mt-1">Cycles</div>
          </CardContent>
        </Card>
        <Card className="bg-black/40 border-white/10">
          <CardContent className="p-4 text-center">
            <div className="text-2xl font-bold text-yellow-400">
              {streakStatus.channels_info.filter(ch => ch.has_claim_available).length}
            </div>
            <div className="text-xs text-zinc-400 mt-1">Claims Available</div>
          </CardContent>
        </Card>
      </div>

      {/* Channels List */}
      <Card className="bg-black/40 border-white/10">
        <CardHeader>
          <CardTitle className="text-lg text-white flex items-center gap-2">
            <Eye className="w-5 h-5 text-blue-400" />
            Your Channels
            <Badge variant="secondary" className="bg-blue-500/20 text-blue-400">
              {mergedChannels.length} channels
            </Badge>
          </CardTitle>
        </CardHeader>
        <CardContent>
          {mergedChannels.length === 0 ? (
            <div className="text-center py-8 text-zinc-500">
              No channels configured. Go to Settings → Streaks or Points to add channels.
            </div>
          ) : (
            <ScrollArea className="h-[400px]">
              <div className="space-y-2">
                {mergedChannels.map((channel, i) => (
                  <div
                    key={i}
                    className="flex items-center justify-between p-3 rounded-lg bg-white/5 border border-white/5 hover:bg-white/10 transition-colors"
                  >
                    <div className="flex items-center gap-3">
                      <div className="w-10 h-10 rounded-full bg-gradient-to-br from-blue-500 to-purple-500 flex items-center justify-center text-white font-bold text-sm">
                        {channel.channel_name.charAt(0).toUpperCase()}
                      </div>
                      <div>
                        <div className="text-white font-medium">{channel.channel_name}</div>
                        <div className="flex items-center gap-2 text-xs">
                          <StatusBadge status={channel.status} />
                        </div>
                      </div>
                    </div>
                    <div className="flex items-center gap-4">
                      <div className="text-right">
                        <div className="text-lg font-bold text-blue-400">{channel.points.toLocaleString()}</div>
                        <div className="text-xs text-zinc-400">points</div>
                      </div>
                      {channel.has_claim_available && (
                        <Badge className="bg-yellow-500/20 text-yellow-400 border-yellow-500/50 animate-pulse">
                          <Gift className="w-3 h-3 mr-1" />
                          Claim!
                        </Badge>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            </ScrollArea>
          )}
        </CardContent>
      </Card>

      {/* Status */}
      {streakRunning && (
        <Card className="bg-black/40 border-white/10">
          <CardContent className="p-4">
            <div className="flex items-center gap-2 text-sm text-zinc-400">
              <Clock className="w-4 h-4 animate-spin" />
              {streakStatus.status}
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

function StatusBadge({ status }: { status: string }) {
  if (status === 'Watching' || status === 'Ready') {
    return (
      <Badge variant="secondary" className="bg-green-500/20 text-green-400 text-xs">
        <CheckCircle2 className="w-3 h-3 mr-1" />
        {status}
      </Badge>
    );
  }
  if (status === 'Offline') {
    return (
      <Badge variant="secondary" className="bg-zinc-500/20 text-zinc-400 text-xs">
        <Clock className="w-3 h-3 mr-1" />
        Offline
      </Badge>
    );
  }
  if (status === 'Error' || status === 'Ping failed') {
    return (
      <Badge variant="secondary" className="bg-red-500/20 text-red-400 text-xs">
        <AlertCircle className="w-3 h-3 mr-1" />
        {status}
      </Badge>
    );
  }
  return (
    <Badge variant="secondary" className="bg-blue-500/20 text-blue-400 text-xs">
      <Clock className="w-3 h-3 mr-1" />
      {status}
    </Badge>
  );
}
