import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { getConfig, getInventory, searchGames, updatePriorities } from '../lib/api'
import { Plus, X, Search, Loader2 } from 'lucide-react'

export default function DropsPage() {
    const queryClient = useQueryClient()
    const [searchTerm, setSearchTerm] = useState('')
    const [searchResults, setSearchResults] = useState<any[]>([])
    const [isSearching, setIsSearching] = useState(false)

    // Fetch Config (Priorities)
    const { data: config } = useQuery({
        queryKey: ['config'],
        queryFn: getConfig
    })

    // Fetch Inventory
    const { data: inventory, isLoading: isInvLoading } = useQuery({
        queryKey: ['inventory'],
        queryFn: getInventory,
        refetchInterval: 30000
    })

    const priorityGames = config?.priorities?.games || []
    const priorityStreamers = config?.priorities?.streamers || []

    // Mutations
    const updateMut = useMutation({
        mutationFn: (vars: { games: string[], streamers: string[] }) => updatePriorities(vars.games, vars.streamers),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: ['config'] })
    })

    const handleSearch = async (e: React.FormEvent) => {
        e.preventDefault()
        if (searchTerm.length < 2) return
        setIsSearching(true)
        try {
            const res = await searchGames(searchTerm)
            const edges = res?.data?.searchFor?.games?.edges || []
            setSearchResults(edges.map((e: any) => e.item))
        } catch (err) {
            console.error(err)
        }
        setIsSearching(false)
    }

    const addGame = (name: string) => {
        if (!priorityGames.includes(name)) {
            updateMut.mutate({
                games: [...priorityGames, name],
                streamers: priorityStreamers
            })
            setSearchTerm('')
            setSearchResults([])
        }
    }

    const removeGame = (name: string) => {
        updateMut.mutate({
            games: priorityGames.filter((g: string) => g !== name),
            streamers: priorityStreamers
        })
    }

    // Helper to find drops
    const activeCampaigns = inventory?.data?.currentUser?.inventory?.dropCampaignsInProgress || []

    return (
        <div className="grid grid-cols-12 gap-6 h-full">
            {/* Left Col: Priority */}
            <div className="col-span-4 space-y-6">
                <div className="bg-dark-surface rounded-xl p-4 border border-white/5 h-full flex flex-col">
                    <h2 className="text-lg font-bold mb-4 flex items-center gap-2">
                        <span className="text-twitch">●</span> Priority Games
                    </h2>

                    {/* Search */}
                    <form onSubmit={handleSearch} className="mb-4 relative">
                        {isSearching ? (
                            <Loader2 className="animate-spin absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-twitch" />
                        ) : (
                            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-500" />
                        )}
                        <input
                            type="text"
                            placeholder="Search Twitch..."
                            className="w-full bg-dark-bg border border-white/10 rounded-lg pl-9 pr-4 py-2 text-sm focus:border-twitch outline-none transition-colors"
                            value={searchTerm}
                            onChange={e => setSearchTerm(e.target.value)}
                        />
                    </form>

                    {/* Results Overlay or List */}
                    {searchResults.length > 0 && (
                        <div className="bg-dark-surface2 rounded-lg p-2 mb-4 space-y-1 max-h-48 overflow-y-auto">
                            {searchResults.map(game => (
                                <button
                                    key={game.id}
                                    onClick={() => addGame(game.displayName)}
                                    className="w-full text-left p-2 rounded hover:bg-white/5 flex items-center justify-between group"
                                >
                                    <div className="flex items-center gap-2">
                                        <img src={game.boxArtURL?.replace('{width}x{height}', '52x72')} className="w-6 h-8 rounded" alt="" />
                                        <span className="text-sm font-medium">{game.displayName}</span>
                                    </div>
                                    <Plus className="w-4 h-4 text-twitch opacity-0 group-hover:opacity-100" />
                                </button>
                            ))}
                        </div>
                    )}

                    {/* Priority List */}
                    <div className="space-y-2 overflow-y-auto flex-1 pr-2 custom-scrollbar">
                        {priorityGames.map((game: string) => (
                            <div key={game} className="bg-dark-surface2 p-3 rounded-lg flex items-center justify-between group border border-transparent hover:border-white/10 transition-all">
                                <span className="font-medium">{game}</span>
                                <button onClick={() => removeGame(game)} className="text-gray-500 hover:text-red-500">
                                    <X className="w-4 h-4" />
                                </button>
                            </div>
                        ))}
                    </div>
                </div>
            </div>

            {/* Right Col: Inventory */}
            <div className="col-span-8">
                <div className="bg-dark-surface rounded-xl p-6 border border-white/5 min-h-full">
                    <div className="flex items-center justify-between mb-6">
                        <h2 className="text-xl font-bold">Active Campaigns</h2>
                        <button onClick={() => queryClient.invalidateQueries({ queryKey: ['inventory'] })} className="text-xs bg-twitch hover:bg-twitch-dark px-3 py-1 rounded text-white font-medium">
                            Refresh
                        </button>
                    </div>

                    {isInvLoading ? (
                        <div className="flex justify-center p-10"><Loader2 className="animate-spin" /></div>
                    ) : (
                        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                            {activeCampaigns.map((camp: any) => {
                                const gameName = camp.game?.displayName || "Unknown"
                                const drops = camp.timeBasedDrops || []
                                if (drops.length === 0) return null

                                return (
                                    <div key={camp.id} className="bg-dark-bg p-4 rounded-lg border border-white/5 hover:border-twitch/30 transition-colors">
                                        <h3 className="font-bold text-lg mb-3">{gameName}</h3>
                                        <div className="space-y-3">
                                            {drops.map((drop: any) => {
                                                const self = drop.self || {}
                                                const claimed = self.isClaimed
                                                const progress = Math.min(100, (self.currentMinutesWatched / drop.requiredMinutesWatched) * 100) || 0

                                                return (
                                                    <div key={drop.id} className="bg-dark-surface2 p-3 rounded flex gap-3">
                                                        <div className="w-12 h-12 bg-gray-800 rounded shrink-0 overflow-hidden relative">
                                                            <img src={drop.benefit?.imageAssetURL || drop.imageURL} className="w-full h-full object-cover" alt="" />
                                                            {claimed && <div className="absolute inset-0 bg-green-500/80 flex items-center justify-center text-[10px] font-bold">DONE</div>}
                                                        </div>
                                                        <div className="flex-1 min-w-0">
                                                            <p className="text-sm font-medium truncate">{drop.name}</p>
                                                            <div className="mt-2 w-full h-1.5 bg-gray-700 rounded-full overflow-hidden">
                                                                <div className="bg-twitch h-full" style={{ width: `${progress}%` }} />
                                                            </div>
                                                            <div className="flex justify-between text-[10px] text-gray-400 mt-1">
                                                                <span>{self.currentMinutesWatched || 0}/{drop.requiredMinutesWatched}m</span>
                                                                <span>{Math.round(progress)}%</span>
                                                            </div>
                                                        </div>
                                                    </div>
                                                )
                                            })}
                                        </div>
                                    </div>
                                )
                            })}
                        </div>
                    )}
                </div>
            </div>
        </div>
    )
}
