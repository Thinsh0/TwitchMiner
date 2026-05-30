import { useState, useEffect } from 'react'
import { api } from '@/api'
import { useAppStore } from '@/store'
import { useTranslation } from '@/lib/i18n'
import type { DropCampaign, DropItem, MinerStatus } from '@/types'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Progress } from '@/components/ui/progress'
import { Input } from '@/components/ui/input'
import { Gamepad2, Play, Square, Clock, Package, CheckCircle2, Gift, ChevronDown, ChevronRight, Loader2, Search, Zap } from 'lucide-react'

type DropSortKey = 'name' | 'date' | 'duration';
type SortDirection = 'asc' | 'desc';

interface DropsViewProps {
  dropStatus: MinerStatus;
  onToggleMiner: () => void;
}

export function DropsView({ dropStatus, onToggleMiner }: DropsViewProps) {
  const { t, lang } = useTranslation()
  const { dropCampaigns, setDropCampaigns } = useAppStore()
  const [priorityGames, setPriorityGames] = useState<string[]>([]);
  const [collapsedGames, setCollapsedGames] = useState<Record<string, boolean>>({});
  const [globalSortKey, setGlobalSortKey] = useState<DropSortKey>('date');
  const [globalSortDirection, setGlobalSortDirection] = useState<SortDirection>('asc');
  const [groupSorts, setGroupSorts] = useState<Record<string, { key: DropSortKey; direction: SortDirection }>>({});
  const [loading, setLoading] = useState(false);
  const [search, setSearch] = useState('');

  const fetchCampaigns = async () => {
    setLoading(true);
    try {
      const result = await api.getDropCampaigns().catch(() => []);
      setDropCampaigns(result);
    } catch (err) {
      console.error('Failed to fetch campaigns:', err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    api.getConfig().then(cfg => {
      const games: string[] = cfg?.priorities?.games ?? []
      setPriorityGames(games)
    }).catch(console.error)
    // Fetch campaigns on mount
    fetchCampaigns()
  }, [])

  // Auto-refresh campaigns every 2 minutes while miner is running
  useEffect(() => {
    if (!dropStatus.is_running) return
    const interval = setInterval(() => {
      fetchCampaigns()
    }, 120_000)
    return () => clearInterval(interval)
  }, [dropStatus.is_running])

  const sortedCampaigns = sortCampaigns(dropCampaigns, globalSortKey, globalSortDirection);
  const filteredSorted = search.trim()
    ? sortedCampaigns.filter(c =>
        c.game_name.toLowerCase().includes(search.toLowerCase()) ||
        c.name.toLowerCase().includes(search.toLowerCase())
      )
    : sortedCampaigns;
  const campaignsByGame = filteredSorted.reduce<Record<string, DropCampaign[]>>((acc, campaign) => {
    const gameName = campaign.game_name || 'Unknown Game';
    if (!acc[gameName]) acc[gameName] = [];
    acc[gameName].push(campaign);
    return acc;
  }, {});

  const emptyGames = priorityGames.filter(game =>
    !Object.keys(campaignsByGame).some(k => k.toLowerCase() === game.toLowerCase()) &&
    game.toLowerCase().includes(search.toLowerCase())
  );

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
      <div className="flex flex-col sm:flex-row gap-2 justify-between items-start sm:items-center">
        <div>
          <h2 className="text-xl font-bold text-white flex items-center gap-2">
            <Gamepad2 className="w-5 h-5 text-purple-400" />
            {t('drops.title')}
          </h2>
          <p className="text-sm text-zinc-400 mt-1">
            {t('drops.subtitle')}
          </p>
        </div>
        <div className="flex gap-2 flex-wrap">
          <Button
            variant="outline"
            size="sm"
            onClick={fetchCampaigns}
            disabled={loading}
            className="border-white/10 text-zinc-400"
          >
            {loading ? <Loader2 className="w-4 h-4 mr-1 animate-spin" /> : null}
            {t('common.refresh')}
          </Button>
          <Button
            size="lg"
            variant={dropStatus.is_running ? "destructive" : "default"}
            onClick={onToggleMiner}
            className={!dropStatus.is_running ? "bg-purple-600 hover:bg-purple-700 text-white" : ""}
          >
            {dropStatus.is_running ? <><Square className="mr-2 h-4 w-4 fill-current" />{t('dashboard.stopMiner')}</> : <><Play className="mr-2 h-4 w-4 fill-current" />{t('dashboard.startMiner')}</>}
          </Button>
        </div>
      </div>

      {/* Search */}
      <div className="relative">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-zinc-500" />
        <Input
          value={search}
          onChange={e => setSearch(e.target.value)}
          placeholder={t('drops.searchPlaceholder')}
          className="pl-9 bg-white/5 border-white/10 text-white placeholder:text-zinc-500"
        />
      </div>

      {/* Current mining highlight */}
      {dropStatus.is_running && dropStatus.current_drop && (
        <Card className="bg-purple-900/30 border-purple-500/40">
          <CardContent className="p-3 flex items-center gap-3">
            {dropStatus.drop_image_url && <img src={dropStatus.drop_image_url} alt="" className="w-10 h-10 rounded" />}
            <div className="flex-1 min-w-0">
              <p className="text-xs text-purple-300 font-semibold uppercase tracking-wide flex items-center gap-1">
                <Zap className="w-3 h-3" /> {t('drops.currentlyMining')}
              </p>
              <p className="text-white font-medium truncate">{dropStatus.current_drop}</p>
              {dropStatus.current_streamer && (
                <p className="text-xs text-zinc-400">{lang === 'FR' ? 'sur' : 'on'} {dropStatus.current_streamer}</p>
              )}
            </div>
            <div className="text-right shrink-0">
              <span className="text-purple-400 font-bold">{Math.round(dropStatus.progress)}%</span>
            </div>
          </CardContent>
        </Card>
      )}

      <Card className="bg-black/40 border-white/10">
        <CardContent className="p-3 flex flex-wrap items-center gap-3">
          <span className="text-sm text-zinc-400">{t('drops.globalSort')}</span>
          <select
            value={globalSortKey}
            onChange={(event) => setGlobalSortKey(event.target.value as DropSortKey)}
            className="bg-zinc-900 border border-white/10 rounded-md px-3 py-2 text-sm text-white"
          >
            <option value="name">{t('drops.sortAlpha')}</option>
            <option value="date">{t('drops.sortDate')}</option>
            <option value="duration">{t('drops.sortDuration')}</option>
          </select>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => setGlobalSortDirection(prev => prev === 'asc' ? 'desc' : 'asc')}
            className="border-white/10 text-zinc-300"
          >
            {globalSortDirection === 'asc' ? t('drops.asc') : t('drops.desc')}
          </Button>
        </CardContent>
      </Card>

      {/* Loading state */}
      {loading ? (
        <Card className="bg-black/40 border-white/10">
          <CardContent className="p-10 flex items-center justify-center gap-3 text-zinc-400">
            <Loader2 className="w-6 h-6 animate-spin" />
            {t('drops.loadingCampaigns')}
          </CardContent>
        </Card>
      ) : Object.keys(campaignsByGame).length > 0 || emptyGames.length > 0 ? (
        <>
          {Object.entries(campaignsByGame).map(([gameName, gameCampaigns]) => (
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
              onClaimSuccess={fetchCampaigns}
              currentDropName={dropStatus.current_drop}
            />
          ))}
          {emptyGames.map((game, i) => (
            <Card key={`empty-${i}`} className="bg-black/40 border-white/10">
              <CardContent className="p-4 flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <div className="w-12 h-12 rounded-lg bg-gradient-to-br from-purple-500/40 to-blue-500/40 flex items-center justify-center text-white font-bold">
                    {game.charAt(0)}
                  </div>
                  <div>
                    <div className="text-white font-medium">{game}</div>
                    <div className="text-xs text-zinc-500">{lang === 'FR' ? 'Recherche de campagnes actives...' : 'Checking for active campaigns...'}</div>
                  </div>
                </div>
                <Badge variant="secondary" className="bg-zinc-800 text-zinc-400">
                  {lang === 'FR' ? 'Aucun drop actif' : 'No active drops'}
                </Badge>
              </CardContent>
            </Card>
          ))}
        </>
      ) : priorityGames.length === 0 && search.trim() === '' ? (
        <Card className="bg-black/40 border-white/10">
          <CardContent className="p-8 text-center">
            <Package className="w-12 h-12 text-zinc-600 mx-auto mb-4" />
            <p className="text-zinc-400">{t('drops.noGames')}</p>
            <p className="text-sm text-zinc-500 mt-2">
              {t('drops.noGamesHint')}
            </p>
          </CardContent>
        </Card>
      ) : (
        <Card className="bg-black/40 border-white/10">
          <CardContent className="p-8 text-center text-zinc-500">
            <Search className="w-10 h-10 mx-auto mb-3 opacity-20" />
            {t('drops.noMatch')}
          </CardContent>
        </Card>
      )}
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
  onClaimSuccess,
  currentDropName,
}: {
  gameName: string;
  campaigns: DropCampaign[];
  collapsed: boolean;
  onToggle: () => void;
  sortKey: DropSortKey;
  sortDirection: SortDirection;
  onSortKeyChange: (key: DropSortKey) => void;
  onSortDirectionToggle: () => void;
  onClaimSuccess?: () => void;
  currentDropName?: string | null;
}) {
  const { t, lang } = useTranslation();
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
              {campaigns.length} {lang === 'FR' ? `campagne${campaigns.length > 1 ? 's' : ''}` : `campaign${campaigns.length > 1 ? 's' : ''}`} • {claimedDrops}/{totalDrops} {lang === 'FR' ? 'drops obtenus' : 'drops claimed'}
            </p>
          </div>
        </div>
        {groupStatus === 'COMPLETED' ? (
          <Badge className="bg-blue-500/20 text-blue-400">
            <CheckCircle2 className="w-3 h-3 mr-1" />
            {lang === 'FR' ? 'Terminé' : 'Completed'}
          </Badge>
        ) : groupStatus === 'READY' ? (
          <Badge className="bg-yellow-500/20 text-yellow-400">
            <Gift className="w-3 h-3 mr-1" />
            {lang === 'FR' ? 'Prêt' : 'Ready'}
          </Badge>
        ) : (
          <Badge className="bg-green-500/20 text-green-400">
            <Clock className="w-3 h-3 mr-1" />
            {lang === 'FR' ? 'Actif' : 'Active'}
          </Badge>
        )}
      </div>

      {!collapsed && (
        <CardContent className="space-y-3 pt-0">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-xs text-zinc-500">{lang === 'FR' ? 'Trier ce jeu' : 'Sort this game'}</span>
            <select
              value={sortKey}
              onChange={(event) => onSortKeyChange(event.target.value as DropSortKey)}
              className="bg-zinc-900 border border-white/10 rounded-md px-2 py-1 text-xs text-white"
            >
              <option value="name">{t('drops.sortAlpha')}</option>
              <option value="date">{t('drops.sortDate')}</option>
              <option value="duration">{t('drops.sortDuration')}</option>
            </select>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={onSortDirectionToggle}
              className="h-7 border-white/10 text-xs text-zinc-300"
            >
              {sortDirection === 'asc' ? t('drops.asc') : t('drops.desc')}
            </Button>
          </div>
          <div className="max-h-[400px] overflow-y-auto pr-2 space-y-3">
            {sortedCampaigns.map((campaign) => (
              <CampaignCard key={campaign.id} campaign={campaign} compact onClaimSuccess={onClaimSuccess} currentDropName={currentDropName} />
            ))}
          </div>
        </CardContent>
      )}
    </Card>
  );
}

function CampaignCard({ campaign, compact = false, onClaimSuccess, currentDropName }: { campaign: DropCampaign; compact?: boolean; onClaimSuccess?: () => void; currentDropName?: string | null }) {
  const { lang } = useTranslation();
  const totalDrops = campaign.drops.length;
  const claimedDrops = campaign.drops.filter(d => d.is_claimed).length;
  const endDate = formatDropDate(campaign.end_date);
  const isCurrentlyMining = currentDropName
    ? campaign.drops.some(d => d.name === currentDropName)
    : false;

  return (
    <Card className={`${isCurrentlyMining ? 'bg-purple-950/30 border-purple-500/60' : compact ? 'bg-white/[0.03]' : 'bg-black/40'} border-white/10 overflow-hidden transition-colors`}>
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
                {compact ? `${claimedDrops}/{totalDrops} ${lang === 'FR' ? 'drops obtenus' : 'drops claimed'}` : campaign.name}
              </p>
              {endDate && (
                <p className="text-xs text-orange-300 mt-1">
                  {lang === 'FR' ? `Se termine le ${endDate}` : `Ends ${endDate}`}
                </p>
              )}
            </div>
          </div>
          {campaign.status === 'COMPLETED' ? (
            <Badge className="bg-blue-500/20 text-blue-400">
              <CheckCircle2 className="w-3 h-3 mr-1" />
              {lang === 'FR' ? 'Terminé' : 'Completed'}
            </Badge>
          ) : campaign.status === 'READY' ? (
            <Badge className="bg-yellow-500/20 text-yellow-400">
              <Gift className="w-3 h-3 mr-1" />
              {lang === 'FR' ? 'Prêt' : 'Ready'}
            </Badge>
          ) : campaign.status === 'AVAILABLE' ? (
            <Badge className="bg-zinc-500/20 text-zinc-400">
              <Clock className="w-3 h-3 mr-1" />
              {lang === 'FR' ? 'Disponible' : 'Available'}
            </Badge>
          ) : (
            <Badge className="bg-green-500/20 text-green-400">
              <Clock className="w-3 h-3 mr-1" />
              {lang === 'FR' ? 'Actif' : 'Active'}
            </Badge>
          )}
        </div>
      </CardHeader>
      <CardContent>
        {!compact && (
          <div className="text-sm text-zinc-400 mb-3">
            {claimedDrops}/{totalDrops} {lang === 'FR' ? 'drops obtenus' : 'drops claimed'}
          </div>
        )}
        <div className="space-y-3">
          {campaign.drops.map((drop) => (
            <DropItemCard key={drop.id} drop={drop} onClaimSuccess={onClaimSuccess} />
          ))}
        </div>
      </CardContent>
    </Card>
  );
}

function DropItemCard({ drop, onClaimSuccess }: { drop: DropItem; onClaimSuccess?: () => void }) {
  const { t, lang } = useTranslation();
  const [isClaiming, setIsClaiming] = useState(false);
  const progress = drop.required_minutes > 0
    ? Math.min(100, (drop.current_minutes / drop.required_minutes) * 100)
    : 0;

  const handleClaim = async () => {
    if (isClaiming) return;
    setIsClaiming(true);
    try {
      await api.claimDrop(drop.id);
      if (onClaimSuccess) {
        onClaimSuccess();
      }
    } catch (err) {
      console.error("Failed to claim drop:", err);
    } finally {
      setIsClaiming(false);
    }
  };

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
        {progress >= 100 || drop.is_claimed ? (
          <div className="text-xs text-zinc-400 mt-1">
            {t('drops.durationReq')} : {drop.required_minutes} min
          </div>
        ) : (
          <div className="flex items-center gap-2 mt-1">
            <Progress value={progress} className="h-2 flex-1" />
            <span className="text-xs text-zinc-400 w-20 text-right">
              {drop.current_minutes}/{drop.required_minutes} min
            </span>
          </div>
        )}
      </div>
      {drop.is_claimed ? (
        <Badge className="bg-green-500/20 text-green-400">
          <CheckCircle2 className="w-3 h-3 mr-1" />
          {t('common.claimed')}
        </Badge>
      ) : progress >= 100 ? (
        <Button 
          size="sm" 
          className="bg-yellow-600 hover:bg-yellow-700 text-white"
          onClick={handleClaim}
          disabled={isClaiming}
        >
          {isClaiming ? (
            <Loader2 className="w-3 h-3 mr-1 animate-spin" />
          ) : (
            <Gift className="w-3 h-3 mr-1" />
          )}
          {isClaiming ? (lang === 'FR' ? 'Réclamation...' : 'Claiming...') : t('common.claim')}
        </Button>
      ) : (
        <Badge variant="secondary" className="bg-zinc-800 text-zinc-400">
          {Math.round(progress)}%
        </Badge>
      )}
    </div>
  );
}

