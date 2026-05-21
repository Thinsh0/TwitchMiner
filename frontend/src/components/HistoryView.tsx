import { useAppStore } from '@/store'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { CheckCircle2, History } from 'lucide-react'
import { ScrollArea } from '@/components/ui/scroll-area'

export function HistoryView() {
  const { claimedDrops } = useAppStore()

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold tracking-tight text-white flex items-center gap-2">
          <History className="w-6 h-6 text-purple-400" />
          Claim History
        </h2>
        <p className="text-zinc-400">
          A history of drops you have successfully claimed.
        </p>
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
              <div className="space-y-4">
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
                          {drop.required_minutes} minutes required
                        </p>
                      </div>
                    </div>
                    <div className="flex items-center gap-2 text-green-400 bg-green-400/10 px-3 py-1.5 rounded-full text-sm font-medium">
                      <CheckCircle2 className="w-4 h-4" />
                      Claimed
                    </div>
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
