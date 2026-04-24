import { LayoutDashboard, Gift, Zap, Settings, Loader2, LogIn } from 'lucide-react'
import clsx from 'clsx'
import { useState } from 'react'
import { startAuth, pollAuth } from '../lib/api'
import { useQueryClient } from '@tanstack/react-query'
// @ts-ignore
// import { open } from '@tauri-apps/plugin-opener'

type View = 'dashboard' | 'drops' | 'bonus' | 'settings'

interface SidebarProps {
    currentView: View
    onChangeView: (view: View) => void
    connected: boolean
}

export default function Sidebar({ currentView, onChangeView, connected }: SidebarProps) {
    const queryClient = useQueryClient()

    // Login State
    const [isLoggingIn, setIsLoggingIn] = useState(false)
    const [authData, setAuthData] = useState<any>(null)
    const [authStatus, setAuthStatus] = useState<string>("")
    const [isPolling, setIsPolling] = useState(false)

    const navItems = [
        { id: 'dashboard', icon: LayoutDashboard, label: 'Dashboard' },
        { id: 'drops', icon: Gift, label: 'Drops & Priority' },
        { id: 'bonus', icon: Zap, label: 'Farming' },
        { id: 'settings', icon: Settings, label: 'Settings' },
    ] as const

    const handleLogin = async () => {
        setIsLoggingIn(true)
        setAuthStatus("Requesting Device Code...")
        try {
            const data = await startAuth()
            if (data && data.device_code) {
                setAuthData(data)
                setAuthStatus("Waiting for you...")

                // Auto open?
                try {
                    window.open(data.verification_uri, '_blank');
                } catch (e) { console.error("Failed to open browser", e) }

                // Start Polling
                setIsPolling(true)
                const res = await pollAuth(data.device_code, data.interval)
                setIsPolling(false)

                if (res.success) {
                    setAuthStatus("Success!")
                    queryClient.invalidateQueries({ queryKey: ['status'] })
                    setTimeout(() => {
                        setIsLoggingIn(false)
                        setAuthData(null)
                    }, 1000)
                } else {
                    setAuthStatus(res.message || "Failed")
                }
            } else {
                setAuthStatus("Failed to init auth.")
            }
        } catch (e) {
            setAuthStatus("Error connecting to backend.")
            // setIsLoggingIn(false) // Keep modal open to show error
        }
    }

    const cancelLogin = () => {
        setIsLoggingIn(false)
        setAuthData(null)
        setIsPolling(false)
    }

    return (
        <div className="w-64 bg-dark-surface border-r border-white/5 flex flex-col h-screen relative">
            {/* Login Modal Overlay */}
            {isLoggingIn && (
                <div className="absolute inset-0 z-50 bg-dark-surface/95 backdrop-blur-sm flex flex-col items-center justify-center p-6 text-center animate-in fade-in duration-200">
                    <h2 className="text-xl font-bold mb-4">Login to Twitch</h2>

                    {authData ? (
                        <div className="space-y-4 w-full">
                            <div className="bg-dark-bg p-4 rounded-lg border border-white/10">
                                <p className="text-xs text-gray-400 mb-1">Enter this code:</p>
                                <p className="text-2xl font-mono text-twitch font-bold tracking-wider select-all">{authData.user_code}</p>
                            </div>

                            <div className="text-sm text-gray-300">
                                at <a href={authData.verification_uri} target="_blank" className="text-twitch hover:underline">{authData.verification_uri}</a>
                            </div>

                            <div className="flex items-center justify-center gap-2 text-xs text-gray-500">
                                {isPolling && <Loader2 className="w-3 h-3 animate-spin" />}
                                {authStatus}
                            </div>
                        </div>
                    ) : (
                        <div className="flex items-center gap-2 text-twitch">
                            <Loader2 className="animate-spin" />
                            <span>{authStatus}</span>
                        </div>
                    )}

                    <button onClick={cancelLogin} className="mt-8 text-sm text-gray-500 hover:text-white">Cancel</button>
                </div>
            )}

            <div className="p-6">
                <h1 className="text-xl font-bold bg-gradient-to-r from-twitch to-twitch-light bg-clip-text text-transparent">
                    Twitch Miner
                </h1>
                <p className="text-xs text-gray-500 mt-1">v2.0 Beta</p>
            </div>

            <nav className="flex-1 px-4 space-y-2">
                {navItems.map((item) => (
                    <button
                        key={item.id}
                        onClick={() => onChangeView(item.id)}
                        className={clsx(
                            "w-full flex items-center gap-3 px-4 py-3 rounded-lg transition-all font-medium text-sm",
                            currentView === item.id
                                ? "bg-twitch text-white shadow-lg shadow-twitch/20"
                                : "text-gray-400 hover:bg-white/5 hover:text-white"
                        )}
                    >
                        <item.icon className="w-5 h-5" />
                        {item.label}
                    </button>
                ))}
            </nav>

            <div className="p-4 border-t border-white/5">
                {connected ? (
                    <div className="bg-dark-surface2 p-3 rounded-lg flex items-center gap-3">
                        <div className="w-2 h-2 rounded-full bg-green-500 animate-pulse" />
                        <div>
                            <p className="text-xs font-bold text-white uppercase">Connected</p>
                            <p className="text-[10px] text-gray-500">Ready to mine</p>
                        </div>
                    </div>
                ) : (
                    <button
                        onClick={handleLogin}
                        className="w-full bg-twitch/10 hover:bg-twitch/20 border border-twitch/50 text-twitch hover:text-white p-3 rounded-lg flex items-center justify-center gap-2 transition-all group"
                    >
                        <LogIn className="w-4 h-4 group-hover:scale-110 transition-transform" />
                        <span className="font-bold text-sm">Login</span>
                    </button>
                )}
            </div>
        </div>
    )
}
