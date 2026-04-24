import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { getConfig, updateSetting } from '../lib/api'
import { Settings } from 'lucide-react'
import clsx from 'clsx'

export default function SettingsPage() {
    const queryClient = useQueryClient()
    const { data: config } = useQuery({
        queryKey: ['config'],
        queryFn: getConfig
    })

    const settings = config?.settings || {}

    const updateMut = useMutation({
        mutationFn: (vars: { key: string, value: any }) => updateSetting("settings", vars.key, vars.value),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: ['config'] })
    })

    const Toggle = ({ label, desc, configKey }: { label: string, desc: string, configKey: string }) => {
        const enabled = settings[configKey] === true
        return (
            <div className="flex items-center justify-between p-4 bg-dark-bg rounded-lg border border-white/5">
                <div>
                    <h3 className="font-bold">{label}</h3>
                    <p className="text-xs text-gray-500 mt-1">{desc}</p>
                </div>
                <button
                    onClick={() => updateMut.mutate({ key: configKey, value: !enabled })}
                    className={clsx(
                        "w-12 h-6 rounded-full relative transition-colors",
                        enabled ? "bg-twitch" : "bg-gray-700"
                    )}
                >
                    <div className={clsx(
                        "absolute top-1 w-4 h-4 rounded-full bg-white transition-all",
                        enabled ? "left-7" : "left-1"
                    )} />
                </button>
            </div>
        )
    }

    return (
        <div className="space-y-6">
            <h1 className="text-2xl font-bold flex items-center gap-2">
                <Settings className="text-gray-400" /> Settings
            </h1>

            <div className="bg-dark-surface rounded-xl p-6 border border-white/5 space-y-4">
                <h2 className="text-lg font-bold mb-4">General Configuration</h2>

                <Toggle
                    label="Mine Unplanned Campaigns"
                    desc="If no priority drops are found, watch random active campaigns."
                    configKey="mine_unplanned_campaigns"
                />

                <Toggle
                    label="Start Minimized"
                    desc="Hide the application window on startup."
                    configKey="start_minimized"
                />

                <Toggle
                    label="Close to Tray"
                    desc="Minimize to system tray instead of closing."
                    configKey="close_to_tray"
                />
            </div>

            <div className="bg-dark-surface rounded-xl p-6 border border-white/5">
                <h2 className="text-lg font-bold mb-4 text-red-400">Advanced</h2>
                <p className="text-sm text-gray-500">Edit `config.json` manually for advanced options like Discord Webhooks or Proxy settings.</p>
            </div>
        </div>
    )
}
