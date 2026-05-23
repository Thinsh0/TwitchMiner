import { useState } from 'react'
import { api } from '@/api'
import { useAppStore } from '@/store'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { CheckCircle2, History, RefreshCw, Loader2, Package, Trophy, Calendar } from 'lucide-react'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Badge } from '@/components/ui/badge'

function formatClaimDate(dateStr?: string | null): string {
  if (!dateStr) return '—'
  try {
    return new Intl.DateTimeFormat('fr-FR', {
      day: '2-digit', month: '2-digit', year: 'numeric',
      hour: '2-digit', minute: '2-digit',
    }).format(new Date(dateStr))
  } catch {
    return '—'
  }
}

export function HistoryView() {
  const { claimedDrops, setClaimedDrops } = useAppStore()
  const [refreshing, setRefreshing] = useState(false)

  const handleRefresh = async () => {
    setRefreshing(true)
    try {
      const drops = await api.getClaimedDrops()
      setClaimedDrops(drops.reverse())
    } catch (e) {
      console.error('Failed to refresh claimed drops', e)
    } finally {
      setRefreshing(false)
    }
  }

  // Compute stats
  const totalDrops = claimedDrops.length
  const dropsThisWeek = claimedDrops.filter(d => {
    if (!d.claimed_at) return false
    const claimDate = new Date(d.claimed_at)
    const weekAgo = new Date()
    weekAgo.setDate(weekAgo.getDate() - 7)
    return claimDate >= weekAgo
  }).length

  // Game count from name heuristics (group by name prefix — best-effort)
  const uniqueNames = new Set(claimedDrops.map(d => d.name))

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold tracking-tight text-white flex items-center gap-2">
            <History className="w-6 h-6 text-purple-400" />
            Claim History
          </h2>
          <p className="text-zinc-400">A history of drops you have successfully claimed.</p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={handleRefresh}
          disabled={refreshing}
          className="border-white/10 text-zinc-400 hover:text-white hover:bg-white/5"
        >
          {refreshing ? <Loader2 className="w-4 h-4 mr-2 animate-spin" /> : <RefreshCw className="w-4 h-4 mr-2" />}
          Refresh
        </Button>
      </div>

      {/* Stats row */}
      <div className="grid grid-cols-2 md:grid-cols-3 gap-4">
        <Card className="bg-black/40 border-white/10">
          <CardContent className="p-4 flex items-center gap-3">
            <Trophy className="w-8 h-8 text-yellow-400 shrink-0" />
            <div>
              <div className="text-2xl font-bold text-white">{totalDrops}</div>
              <div className="text-xs text-zinc-400">Total claimed</div>
            </div>
          </CardContent>
        </Card>
        <Card className="bg-black/40 border-white/10">
          <CardContent className="p-4 flex items-center gap-3">
            <Calendar className="w-8 h-8 text-purple-400 shrink-0" />
            <div>
              <div className="text-2xl font-bold text-white">{dropsThisWeek}</div>
              <div className="text-xs text-zinc-400">This week</div>
            </div>
          </CardContent>
        </Card>
        <Card className="bg-black/40 border-white/10 col-span-2 md:col-span-1">
          <CardContent className="p-4 flex items-center gap-3">
            <Package className="w-8 h-8 text-blue-400 shrink-0" />
            <div>
              <div className="text-2xl font-bold text-white">{uniqueNames.size}</div>
              <div className="text-xs text-zinc-400">Unique drops</div>
            </div>
          </CardContent>
        </Card>
      </div>

      <Card className="bg-black/40 border-white/10 backdrop-blur-md">
        <CardHeader>
          <CardTitle className="text-lg">Claimed Drops ({claimedDrops.length})</CardTitle>
          <CardDescription className="text-zinc-400">
            Recently claimed drops are shown first.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {claimedDrops.length === 0 ? (
            <div className="text-center py-10 text-zinc-500">
              <History className="w-12 h-12 mx-auto mb-3 opacity-20" />
              <p>No drops claimed yet.</p>
            </div>
          ) : (
            <ScrollArea className="h-[500px] pr-4">
              <div className="space-y-3">
                {claimedDrops.map((drop, idx) => (
                  <div
                    key={idx}
                    className="flex items-center justify-between p-4 rounded-xl bg-white/5 border border-white/5 hover:bg-white/10 transition-colors"
                  >
                    <div className="flex items-center gap-4">
                      {drop.image_url ? (
                        <img src={drop.image_url} alt={drop.name} className="w-12 h-12 rounded bg-black/50 object-cover" />
                      ) : (
                        <div className="w-12 h-12 rounded bg-white/10 flex items-center justify-center">
                          <CheckCircle2 className="w-6 h-6 text-zinc-500" />
                        </div>
                      )}
                      <div>
                        <h4 className="font-medium text-white">{drop.name}</h4>
                        <p className="text-sm text-zinc-400">
                          {drop.required_minutes} min required
                        </p>
                        {drop.claimed_at && (
                          <p className="text-xs text-zinc-500 mt-0.5">
                            Claimed {formatClaimDate(drop.claimed_at)}
                          </p>
                        )}
                      </div>
                    </div>
                    <Badge className="bg-green-500/20 text-green-400 border-green-500/30 shrink-0">
                      <CheckCircle2 className="w-3 h-3 mr-1" />
                      Claimed
                    </Badge>
                  </div>
                ))}
              </div>
            </ScrollArea>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
