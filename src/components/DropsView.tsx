import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Progress } from '@/components/ui/progress'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Gamepad2, Play, Square, Clock, Package, CheckCircle2, Gift } from 'lucide-react'

interface DropCampaign {
  id: string;
  name: string;
  game_name: string;
  image_url?: string;
  status: string;
  start_date: string;
  end_date: string;
  drops: DropItem[];
}

interface DropItem {
  id: string;
  name: string;
  required_minutes: number;
  current_minutes: number;
  is_claimed: boolean;
  image_url?: string;
}

interface DropsViewProps {
  dropStatus: {
    status: string;
    is_running: boolean;
    current_streamer: string | null;
    current_drop: string | null;
    drop_image_url: string | null;
    streamer_image_url: string | null;
    progress: number;
    current_points: string;
  };
  onToggleMiner: () => void;
}

export function DropsView({ dropStatus, onToggleMiner }: DropsViewProps) {
  const [campaigns, setCampaigns] = useState<DropCampaign[]>([]);
  const [priorityGames, setPriorityGames] = useState<string[]>([]);

  useEffect(() => {
    invoke<any>('get_config').then(cfg => {
      const games: string[] = cfg?.priorities?.games ?? []
      setPriorityGames(games)
    }).catch(console.error)
  }, [])

  const fetchCampaigns = async () => {
    try {
      const result = await invoke<DropCampaign[]>('get_drop_campaigns').catch(() => []);
      setCampaigns(result);
    } catch (err) {
      console.error('Failed to fetch campaigns:', err);
    }
  };

  useEffect(() => {
    fetchCampaigns();
  }, []);

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex justify-between items-center">
        <div>
          <h2 className="text-xl font-bold text-white flex items-center gap-2">
            <Gamepad2 className="w-5 h-5 text-purple-400" />
            Drop Campaigns
          </h2>
          <p className="text-sm text-zinc-400 mt-1">
            Available drops for your selected games
          </p>
        </div>
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={fetchCampaigns}
            className="border-white/10 text-zinc-400"
          >
            Refresh
          </Button>
          <Button
            size="lg"
            variant={dropStatus.is_running ? "destructive" : "default"}
            onClick={onToggleMiner}
            className={!dropStatus.is_running ? "bg-purple-600 hover:bg-purple-700 text-white" : ""}
          >
            {dropStatus.is_running ? <><Square className="mr-2 h-4 w-4 fill-current" /> Stop Mining</> : <><Play className="mr-2 h-4 w-4 fill-current" /> Start Mining</>}
          </Button>
        </div>
      </div>

      {/* Active Campaigns */}
      <ScrollArea className="h-[600px]">
        <div className="space-y-4">
          {campaigns.length > 0 ? (
            campaigns.map((campaign) => (
              <CampaignCard key={campaign.id} campaign={campaign} />
            ))
          ) : priorityGames.length > 0 ? (
            <>
              {/* Show configured games waiting for drop campaigns */}
              {priorityGames.map((game, i) => (
                <Card key={i} className="bg-black/40 border-white/10">
                  <CardContent className="p-4 flex items-center justify-between">
                    <div className="flex items-center gap-3">
                      <div className="w-12 h-12 rounded-lg bg-gradient-to-br from-purple-500/40 to-blue-500/40 flex items-center justify-center text-white font-bold">
                        {game.charAt(0)}
                      </div>
                      <div>
                        <div className="text-white font-medium">{game}</div>
                        <div className="text-xs text-zinc-500">Checking for active campaigns...</div>
                      </div>
                    </div>
                    <Badge variant="secondary" className="bg-zinc-800 text-zinc-400">
                      No active drops
                    </Badge>
                  </CardContent>
                </Card>
              ))}
            </>
          ) : (
            <Card className="bg-black/40 border-white/10">
              <CardContent className="p-8 text-center">
                <Package className="w-12 h-12 text-zinc-600 mx-auto mb-4" />
                <p className="text-zinc-400">No games configured.</p>
                <p className="text-sm text-zinc-500 mt-2">
                  Go to Settings → Priorities to add games.
                </p>
              </CardContent>
            </Card>
          )}
        </div>
      </ScrollArea>
    </div>
  );
}

function CampaignCard({ campaign }: { campaign: DropCampaign }) {
  const totalDrops = campaign.drops.length;
  const claimedDrops = campaign.drops.filter(d => d.is_claimed).length;

  return (
    <Card className="bg-black/40 border-white/10 overflow-hidden">
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            {campaign.image_url ? (
              <img src={campaign.image_url} alt={campaign.game_name} className="w-12 h-12 rounded-lg object-cover" />
            ) : (
              <div className="w-12 h-12 rounded-lg bg-gradient-to-br from-purple-500 to-blue-500 flex items-center justify-center text-white font-bold">
                {campaign.game_name.charAt(0)}
              </div>
            )}
            <div>
              <CardTitle className="text-lg text-white">{campaign.game_name}</CardTitle>
              <p className="text-sm text-zinc-400">{campaign.name}</p>
            </div>
          </div>
          <Badge className="bg-green-500/20 text-green-400">
            <Clock className="w-3 h-3 mr-1" />
            Active
          </Badge>
        </div>
      </CardHeader>
      <CardContent>
        <div className="text-sm text-zinc-400 mb-3">
          {claimedDrops}/{totalDrops} drops claimed
        </div>
        <div className="space-y-3">
          {campaign.drops.map((drop) => (
            <DropItemCard key={drop.id} drop={drop} />
          ))}
        </div>
      </CardContent>
    </Card>
  );
}

function DropItemCard({ drop }: { drop: DropItem }) {
  const progress = drop.required_minutes > 0
    ? Math.min(100, (drop.current_minutes / drop.required_minutes) * 100)
    : 0;

  return (
    <div className="flex items-center gap-3 p-3 rounded-lg bg-white/5 border border-white/5">
      {drop.image_url ? (
        <img src={drop.image_url} alt={drop.name} className="w-10 h-10 rounded object-cover" />
      ) : (
        <div className="w-10 h-10 rounded bg-zinc-800 flex items-center justify-center">
          <Gift className="w-5 h-5 text-zinc-600" />
        </div>
      )}
      <div className="flex-1">
        <div className="text-white text-sm font-medium">{drop.name}</div>
        <div className="flex items-center gap-2 mt-1">
          <Progress value={progress} className="h-2 flex-1" />
          <span className="text-xs text-zinc-400 w-20 text-right">
            {drop.current_minutes}/{drop.required_minutes} min
          </span>
        </div>
      </div>
      {drop.is_claimed ? (
        <Badge className="bg-green-500/20 text-green-400">
          <CheckCircle2 className="w-3 h-3 mr-1" />
          Claimed
        </Badge>
      ) : progress >= 100 ? (
        <Button size="sm" className="bg-yellow-600 hover:bg-yellow-700 text-white">
          <Gift className="w-3 h-3 mr-1" />
          Claim
        </Button>
      ) : (
        <Badge variant="secondary" className="bg-zinc-800 text-zinc-400">
          {Math.round(progress)}%
        </Badge>
      )}
    </div>
  );
}

