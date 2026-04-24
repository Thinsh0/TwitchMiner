import { useQuery } from '@tanstack/react-query'
import { getBonusStatus } from '../lib/api'
import { Zap, Clock, Trophy, Loader2 } from 'lucide-react'

export default function BonusPage() {
    const { data: status, isLoading } = useQuery({
        queryKey: ['bonus'],
        queryFn: getBonusStatus,
        refetchInterval: 2000
    })

    if (isLoading) return <div className="flex justify-center p-10"><Loader2 className="animate-spin text-twitch" /></div>

    const daily = status?.daily_streak || {}
    const points = status?.points_farming || {}

    const farmingChannels = Object.entries(points.channels || {})

    return (
        <div className="space-y-6">
            <h1 className="text-2xl font-bold flex items-center gap-2">
                <Zap className="fill-twitch text-twitch" /> Farming & Bonuses
            </h1>

            {/* Daily Streak Card */}
            <div className="bg-dark-surface rounded-xl p-6 border border-white/5 relative overflow-hidden">
                <div className="flex justify-between items-start relative z-10">
                    <div>
                        <h2 className="text-lg font-bold mb-1">Daily Streak</h2>
                        <p className="text-gray-400 text-sm">Automatically watches channels for 6 minutes to keep streaks alive.</p>
                    </div>
                    <div className={`px-3 py-1 rounded-full text-xs font-bold ${daily.is_running ? "bg-green-500/20 text-green-400" : "bg-gray-700 text-gray-400"}`}>
                        {daily.is_running ? "RUNNING" : "IDLE"}
                    </div>
                </div>

                <div className="mt-6 flex items-center gap-4">
                    {daily.is_running ? (
                        <div className="flex items-center gap-3 bg-dark-bg p-3 rounded-lg border border-white/10 w-full animate-pulse">
                            <Clock className="w-5 h-5 text-twitch" />
                            <span className="font-mono">{daily.status}</span>
                        </div>
                    ) : (
                        <div className="text-gray-500 text-sm italic">
                            <Clock className="w-4 h-4 inline mr-2" />
                            Cycle completed or waiting for next run.
                        </div>
                    )}
                </div>
            </div>

            {/* Points Farming */}
            <div className="bg-dark-surface rounded-xl p-6 border border-white/5">
                <div className="flex justify-between items-center mb-6">
                    <h2 className="text-lg font-bold">Points Miner</h2>
                    <span className="text-xs text-gray-500">Auto-claims chests and points</span>
                </div>

                {farmingChannels.length === 0 ? (
                    <div className="text-center p-8 text-gray-500 bg-dark-bg rounded-lg border border-dashed border-white/10">
                        No active farming channels configured.
                    </div>
                ) : (
                    <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                        {farmingChannels.map(([channel, data]: [string, any]) => (
                            <div key={channel} className="bg-dark-surface2 p-4 rounded-lg flex items-center justify-between group hover:border-twitch/30 border border-transparent transition-all">
                                <div>
                                    <h3 className="font-bold text-lg">{channel}</h3>
                                    <p className="text-xs text-gray-400 mt-1">{data.status}</p>
                                </div>
                                <div className="text-right">
                                    <div className="flex items-center gap-1 justify-end text-twitch-light font-bold">
                                        <Trophy className="w-3 h-3" />
                                        {data.points}
                                    </div>
                                    <div className="text-[10px] text-gray-500 uppercase tracking-widest mt-1">Points</div>
                                </div>
                            </div>
                        ))}
                    </div>
                )}
            </div>
        </div>
    )
}
