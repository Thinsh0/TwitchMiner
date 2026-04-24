import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { getStatus, startMiner, stopMiner } from '../lib/api'
import { Play, Square, Loader2, Twitch } from 'lucide-react'
import clsx from 'clsx'

export default function Dashboard() {
    const queryClient = useQueryClient()

    // Poll status every 1s
    const { data: status, isLoading } = useQuery({
        queryKey: ['status'],
        queryFn: getStatus,
        refetchInterval: 1000,
    })

    const startMut = useMutation({
        mutationFn: startMiner,
        onSuccess: () => queryClient.invalidateQueries({ queryKey: ['status'] })
    })

    const stopMut = useMutation({
        mutationFn: stopMiner,
        onSuccess: () => queryClient.invalidateQueries({ queryKey: ['status'] })
    })

    if (isLoading && !status) return <div className="flex items-center justify-center h-screen"><Loader2 className="animate-spin text-twitch" /></div>

    const isRunning = status?.status !== "Idle" && status?.status !== "Stopped" && status?.status !== "Offline"

    return (
        <div className="max-w-4xl mx-auto space-y-6">
            {/* Header */}
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                    <div className="bg-twitch p-2 rounded-lg">
                        <Twitch className="text-white w-6 h-6" />
                    </div>
                    <h1 className="text-2xl font-bold">Twitch Miner <span className="text-xs font-normal text-gray-500 ml-2">v2.0 Tauri</span></h1>
                </div>
                <div className="flex items-center gap-2">
                    <div className={clsx("w-3 h-3 rounded-full", status?.authenticated ? "bg-green-500" : "bg-red-500")} />
                    <span className="text-sm font-medium">{status?.authenticated ? "Connected" : "Not Logged In"}</span>
                </div>
            </div>

            {/* Main Status Card */}
            <div className="bg-dark-surface rounded-xl p-6 shadow-xl border border-white/5 relative overflow-hidden group">
                {/* Glow Effect */}
                <div className="absolute top-0 right-0 w-64 h-64 bg-twitch/10 blur-[100px] rounded-full pointer-events-none group-hover:bg-twitch/20 transition-all duration-500" />

                <div className="flex gap-6 relative z-10">
                    {/* Image Box */}
                    <div className="w-32 h-32 bg-dark-surface2 rounded-lg flex items-center justify-center overflow-hidden shrink-0 border border-white/5">
                        {status?.image_url ? (
                            <img src={status.image_url} alt="Drop" className="w-full h-full object-cover" />
                        ) : (
                            <span className="text-4xl">🎁</span>
                        )}
                    </div>

                    {/* Info */}
                    <div className="flex-1 space-y-4">
                        <div>
                            <h2 className="text-gray-400 text-sm uppercase tracking-wider font-semibold">Current Status</h2>
                            <p className="text-xl font-bold truncate">{status?.status || "Connecting..."}</p>
                        </div>

                        <div className="grid grid-cols-2 gap-4">
                            <div>
                                <h3 className="text-gray-500 text-xs">Streamer</h3>
                                <div className="flex items-center gap-2 mt-1">
                                    {status?.streamer_image_url && <img src={status.streamer_image_url} className="w-6 h-6 rounded-full" />}
                                    <span className="font-medium text-lg">{status?.streamer || "--"}</span>
                                </div>
                            </div>
                            <div>
                                <h3 className="text-gray-500 text-xs">Drop</h3>
                                <p className="font-medium text-lg text-twitch-light truncate">{status?.drop || "--"}</p>
                            </div>
                        </div>

                        {/* Progress */}
                        <div className="space-y-1">
                            <div className="flex justify-between text-xs">
                                <span className="text-gray-400">Progress</span>
                                <span className="font-bold">{Math.round(status?.progress || 0)}%</span>
                            </div>
                            <div className="h-2 bg-dark-bg rounded-full overflow-hidden">
                                <div
                                    className="h-full bg-twitch transition-all duration-500 ease-out"
                                    style={{ width: `${Math.max(5, status?.progress || 0)}%` }}
                                />
                            </div>
                        </div>
                    </div>
                </div>
            </div>

            {/* Controls */}
            <div className="grid grid-cols-2 gap-4">
                <button
                    onClick={() => startMut.mutate()}
                    disabled={isRunning}
                    className="bg-twitch hover:bg-twitch-dark disabled:opacity-50 disabled:cursor-not-allowed text-white p-4 rounded-xl flex items-center justify-center gap-2 font-bold transition-transform active:scale-95"
                >
                    <Play className="fill-current" />
                    Start Mining
                </button>

                <button
                    onClick={() => stopMut.mutate()}
                    disabled={!isRunning}
                    className="bg-dark-surface2 hover:bg-red-500/20 hover:text-red-500 hover:border-red-500/50 border border-transparent disabled:opacity-50 disabled:cursor-not-allowed text-white p-4 rounded-xl flex items-center justify-center gap-2 font-bold transition-all active:scale-95"
                >
                    <Square className="fill-current" />
                    Stop Mining
                </button>
            </div>

            {/* Footer Info */}
            <div className="grid grid-cols-2 gap-4 text-sm text-gray-500">
                <div className="bg-dark-surface p-4 rounded-lg border border-white/5">
                    <p className="uppercase text-xs font-bold mb-1">Campaign Total</p>
                    <p className="text-2xl font-mono text-blue-400">{Math.round(status?.campaign_progress || 0)}%</p>
                </div>

                <div className="bg-dark-surface p-4 rounded-lg border border-white/5">
                    <p className="uppercase text-xs font-bold mb-1">Points Farmed</p>
                    <p className="text-2xl font-mono text-teal-400">{status?.current_points || "0"}</p>
                </div>
            </div>

        </div>
    )
}
