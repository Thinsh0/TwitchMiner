import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore, type DropCampaign, type DropItem } from '@/store'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Progress } from '@/components/ui/progress'
import { Gamepad2, Play, Square, Clock, Package, CheckCircle2, Gift, ChevronDown, ChevronRight } from 'lucide-react'

type DropSortKey = 'name' | 'date' | 'duration';
type SortDirection = 'asc' | 'desc';

interface DropsViewProps {
  dropStatus: {
    status: string;
    is_running: boolean;
    current_streamer: string | null;
    current_streamer_login: string | null;
    current_drop: string | null;
    drop_image_url: string | null;
    streamer_image_url: string | null;
    progress: number;
    current_points: string;
  };
  onToggleMiner: () => void;
}

export function DropsView({ dropStatus, onToggleMiner }: DropsViewProps) {
  const { dropCampaigns, setDropCampaigns } = useAppStore()
  const [priorityGames, setPriorityGames] = useState<string[]>([]);
  const [collapsedGames, setCollapsedGames] = useState<Record<string, boolean>>({});
  const [globalSortKey, setGlobalSortKey] = useState<DropSortKey>('date');
  const [globalSortDirection, setGlobalSortDirection] = useState<SortDirection>('asc');
  const [groupSorts, setGroupSorts] = useState<Record<string, { key: DropSortKey; direction: SortDirection }>>({});

  useEffect(() => {
    invoke<any>('get_config').then(cfg => {
      const games: string[] = cfg?.priorities?.games ?? []
      setPriorityGames(games)
    }).catch(console.error)
  }, [])

  const fetchCampaigns = async () => {
    try {
      const result = await invoke<any[]>('get_drop_campaigns').catch(() => []);
      setDropCampaigns(result);
    } catch (err) {
      console.error('Failed to fetch campaigns:', err);
    }
  };

  const sortedCampaigns = sortCampaigns(dropCampaigns, globalSortKey, globalSortDirection);
  const campaignsByGame = sortedCampaigns.reduce<Record<string, DropCampaign[]>>((acc, campaign) => {
    const gameName = campaign.game_name || 'Unknown Game';
    if (!acc[gameName]) acc[gameName] = [];
    acc[gameName].push(campaign);
    return acc;
  }, {});

  const toggleGame = (gameName: string) => {
    setCollapsedGames(prev => ({
      ...prev,
      [gameName]: !prev[gameName],
    }));
  };

  const updateGroupSort = (gameName: string, key: DropSortKey) => {
    setGroupSorts(prev => ({
      ...prev,
      [gameName]: {
        key,
        direction: prev[gameName]?.direction ?? globalSortDirection,
      },
    }));
  };

  const toggleGroupSortDirection = (gameName: string) => {
    setGroupSorts(prev => {
      const current = prev[gameName] ?? { key: globalSortKey, direction: globalSortDirection };
      return {
        ...prev,
        [gameName]: {
          ...current,
          direction: current.direction === 'asc' ? 'desc' : 'asc',
        },
      };
    });
  };

  return (
    <div className="space-y-4">
      {/* Header - Fixed */}
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

      <Card className="bg-black/40 border-white/10">
        <CardContent className="p-3 flex flex-wrap items-center gap-3">
          <span className="text-sm text-zinc-400">Global sort</span>
          <select
            value={globalSortKey}
            onChange={(event) => setGlobalSortKey(event.target.value as DropSortKey)}
            className="bg-zinc-900 border border-white/10 rounded-md px-3 py-2 text-sm text-white"
          >
            <option value="name">Alphabetical</option>
            <option value="date">Expiration date</option>
            <option value="duration">Total duration</option>
          </select>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => setGlobalSortDirection(prev => prev === 'asc' ? 'desc' : 'asc')}
            className="border-white/10 text-zinc-300"
          >
            {globalSortDirection === 'asc' ? 'Ascending' : 'Descending'}
          </Button>
        </CardContent>
      </Card>

      {/* Active Campaigns */}
      <div className="space-y-4">
        {dropCampaigns.length > 0 ? (
          Object.entries(campaignsByGame).map(([gameName, gameCampaigns]) => (
            <GameCampaignGroup
              key={gameName}
              gameName={gameName}
              campaigns={gameCampaigns}
              collapsed={collapsedGames[gameName] ?? false}
              onToggle={() => toggleGame(gameName)}
              sortKey={groupSorts[gameName]?.key ?? globalSortKey}
              sortDirection={groupSorts[gameName]?.direction ?? globalSortDirection}
              onSortKeyChange={(key) => updateGroupSort(gameName, key)}
              onSortDirectionToggle={() => toggleGroupSortDirection(gameName)}
            />
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
    </div>
  );
}

function formatDropDate(date: string) {
  if (!date) return null;
  const parsed = new Date(date);
  if (Number.isNaN(parsed.getTime())) return null;
  return new Intl.DateTimeFormat('fr-FR', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(parsed);
}

function campaignDuration(campaign: DropCampaign) {
  return campaign.drops.reduce((sum, drop) => sum + drop.required_minutes, 0);
}

function campaignEndTimestamp(campaign: DropCampaign) {
  if (!campaign.end_date) return Number.MAX_SAFE_INTEGER;
  const timestamp = new Date(campaign.end_date).getTime();
  return Number.isNaN(timestamp) ? Number.MAX_SAFE_INTEGER : timestamp;
}

function sortCampaigns(campaigns: DropCampaign[], key: DropSortKey, direction: SortDirection) {
  const multiplier = direction === 'asc' ? 1 : -1;
  return [...campaigns].sort((a, b) => {
    if (key === 'name') {
      return a.name.localeCompare(b.name) * multiplier;
    }
    if (key === 'duration') {
      return (campaignDuration(a) - campaignDuration(b)) * multiplier;
    }
    return (campaignEndTimestamp(a) - campaignEndTimestamp(b)) * multiplier;
  });
}

function GameCampaignGroup({
  gameName,
  campaigns,
  collapsed,
  onToggle,
  sortKey,
  sortDirection,
  onSortKeyChange,
  onSortDirectionToggle,
}: {
  gameName: string;
  campaigns: DropCampaign[];
  collapsed: boolean;
  onToggle: () => void;
  sortKey: DropSortKey;
  sortDirection: SortDirection;
  onSortKeyChange: (key: DropSortKey) => void;
  onSortDirectionToggle: () => void;
}) {
  const gameImage = campaigns.find(c => c.image_url)?.image_url;
  const totalDrops = campaigns.reduce((sum, campaign) => sum + campaign.drops.length, 0);
  const claimedDrops = campaigns.reduce(
    (sum, campaign) => sum + campaign.drops.filter(drop => drop.is_claimed).length,
    0
  );
  const allCompleted = campaigns.every(c => c.status === 'COMPLETED' || c.drops.every(d => d.is_claimed));
  const hasReady = campaigns.some(c => c.status === 'READY');
  const groupStatus = allCompleted ? 'COMPLETED' : hasReady ? 'READY' : 'ACTIVE';
  const sortedCampaigns = sortCampaigns(campaigns, sortKey, sortDirection);

  return (
    <Card className="bg-black/50 border-white/10 overflow-hidden">
      <div className="flex items-center justify-between p-4">
        <div className="flex items-center gap-3">
          {gameImage ? (
            <img src={gameImage} alt={gameName} className="w-12 h-12 rounded-lg object-cover" />
          ) : (
            <div className="w-12 h-12 rounded-lg bg-gradient-to-br from-purple-500 to-blue-500 flex items-center justify-center text-white font-bold">
              {gameName.charAt(0)}
            </div>
          )}
          <div>
            <button
              type="button"
              onClick={onToggle}
              className="flex items-center gap-2 rounded-md text-left hover:text-purple-300 transition-colors"
            >
            <CardTitle className="text-lg text-white">
              {gameName}
            </CardTitle>
            {collapsed ? (
              <ChevronRight className="w-4 h-4 text-zinc-500" />
            ) : (
              <ChevronDown className="w-4 h-4 text-zinc-500" />
            )}
            </button>
            <p className="text-sm text-zinc-400">
              {campaigns.length} campaign{campaigns.length > 1 ? 's' : ''} • {claimedDrops}/{totalDrops} drops claimed
            </p>
          </div>
        </div>
        {groupStatus === 'COMPLETED' ? (
          <Badge className="bg-blue-500/20 text-blue-400">
            <CheckCircle2 className="w-3 h-3 mr-1" />
            Completed
          </Badge>
        ) : groupStatus === 'READY' ? (
          <Badge className="bg-yellow-500/20 text-yellow-400">
            <Gift className="w-3 h-3 mr-1" />
            Ready
          </Badge>
        ) : (
          <Badge className="bg-green-500/20 text-green-400">
            <Clock className="w-3 h-3 mr-1" />
            Active
          </Badge>
        )}
      </div>

      {!collapsed && (
        <CardContent className="space-y-3 pt-0">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-xs text-zinc-500">Sort this game</span>
            <select
              value={sortKey}
              onChange={(event) => onSortKeyChange(event.target.value as DropSortKey)}
              className="bg-zinc-900 border border-white/10 rounded-md px-2 py-1 text-xs text-white"
            >
              <option value="name">Alphabetical</option>
              <option value="date">Expiration date</option>
              <option value="duration">Total duration</option>
            </select>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={onSortDirectionToggle}
              className="h-7 border-white/10 text-xs text-zinc-300"
            >
              {sortDirection === 'asc' ? 'Ascending' : 'Descending'}
            </Button>
          </div>
          <div className="max-h-[400px] overflow-y-auto pr-2 space-y-3">
            {sortedCampaigns.map((campaign) => (
              <CampaignCard key={campaign.id} campaign={campaign} compact />
            ))}
          </div>
        </CardContent>
      )}
    </Card>
  );
}

function CampaignCard({ campaign, compact = false }: { campaign: DropCampaign; compact?: boolean }) {
  const totalDrops = campaign.drops.length;
  const claimedDrops = campaign.drops.filter(d => d.is_claimed).length;
  const endDate = formatDropDate(campaign.end_date);

  return (
    <Card className={`${compact ? 'bg-white/[0.03]' : 'bg-black/40'} border-white/10 overflow-hidden`}>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            {!compact && campaign.image_url ? (
              <img src={campaign.image_url} alt={campaign.game_name} className="w-12 h-12 rounded-lg object-cover" />
            ) : !compact ? (
              <div className="w-12 h-12 rounded-lg bg-gradient-to-br from-purple-500 to-blue-500 flex items-center justify-center text-white font-bold">
                {campaign.game_name.charAt(0)}
              </div>
            ) : null}
            <div>
              <CardTitle className={compact ? "text-base text-white" : "text-lg text-white"}>
                {compact ? campaign.name : campaign.game_name}
              </CardTitle>
              <p className="text-sm text-zinc-400">
                {compact ? `${claimedDrops}/${totalDrops} drops claimed` : campaign.name}
              </p>
              {endDate && (
                <p className="text-xs text-orange-300 mt-1">
                  Ends {endDate}
                </p>
              )}
            </div>
          </div>
          {campaign.status === 'COMPLETED' ? (
            <Badge className="bg-blue-500/20 text-blue-400">
              <CheckCircle2 className="w-3 h-3 mr-1" />
              Completed
            </Badge>
          ) : campaign.status === 'READY' ? (
            <Badge className="bg-yellow-500/20 text-yellow-400">
              <Gift className="w-3 h-3 mr-1" />
              Ready
            </Badge>
          ) : campaign.status === 'AVAILABLE' ? (
            <Badge className="bg-zinc-500/20 text-zinc-400">
              <Clock className="w-3 h-3 mr-1" />
              Available
            </Badge>
          ) : (
            <Badge className="bg-green-500/20 text-green-400">
              <Clock className="w-3 h-3 mr-1" />
              Active
            </Badge>
          )}
        </div>
      </CardHeader>
      <CardContent>
        {!compact && (
          <div className="text-sm text-zinc-400 mb-3">
            {claimedDrops}/{totalDrops} drops claimed
          </div>
        )}
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

