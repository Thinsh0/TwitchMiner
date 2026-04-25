import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Switch } from '@/components/ui/switch'
import { Label } from '@/components/ui/label'
import { Separator } from '@/components/ui/separator'
import { X, Save, RotateCcw, Plus, Gamepad2, Users, Tv, Settings as SettingsIcon } from 'lucide-react'

interface AppConfig {
  client_id: string;
  priorities: {
    games: string[];
    streamers: string[];
  };
  bonus: {
    daily_streaks: {
      channels: string[];
      auto_start: boolean;
      mode: string;
    };
    points_farming: {
      channels: string[];
      auto_start: boolean;
      max_concurrent: number;
    };
  };
  settings: {
    language: string;
    check_interval: number;
    notifications_enabled: boolean;
    notify_active_drop: boolean;
    notify_claim: boolean;
    notify_campaign_finish: boolean;
  };
}

export function SettingsPanel() {
  const [config, setConfig] = useState<AppConfig | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [newGame, setNewGame] = useState('')
  const [newStreamer, setNewStreamer] = useState('')
  const [newStreakChannel, setNewStreakChannel] = useState('')
  const [newPointsChannel, setNewPointsChannel] = useState('')
  
  // Game search autocomplete
  const [gameSearchResults, setGameSearchResults] = useState<[string, string][]>([])
  const [isSearchingGames, setIsSearchingGames] = useState(false)
  const [showGameDropdown, setShowGameDropdown] = useState(false)

  useEffect(() => {
    loadConfig()
  }, [])

  // Auto-save config when it changes (debounced)
  useEffect(() => {
    if (!config || loading) return
    const timeout = setTimeout(() => {
      console.log('Auto-saving config...')
      saveConfig()
    }, 1000)
    return () => clearTimeout(timeout)
  }, [config?.priorities.games.length, config?.priorities.streamers.length, config?.bonus.daily_streaks.channels.length, config?.bonus.points_farming.channels.length])

  // Debounced game search
  useEffect(() => {
    if (!newGame.trim() || newGame.length < 2) {
      setGameSearchResults([])
      setShowGameDropdown(false)
      return
    }

    const timeout = setTimeout(async () => {
      setIsSearchingGames(true)
      try {
        const results = await invoke<[string, string][]>('search_games_cmd', { query: newGame })
        setGameSearchResults(results)
        setShowGameDropdown(results.length > 0)
      } catch (err) {
        console.error('Game search failed:', err)
        setGameSearchResults([])
      } finally {
        setIsSearchingGames(false)
      }
    }, 300)

    return () => clearTimeout(timeout)
  }, [newGame])

  const loadConfig = async () => {
    try {
      console.log('Loading config...')
      const cfg = await invoke<AppConfig>('get_config')
      console.log('Config loaded:', cfg)
      setConfig(cfg)
    } catch (err) {
      console.error('Failed to load config:', err)
    } finally {
      setLoading(false)
    }
  }

  const saveConfig = async () => {
    if (!config) return
    setSaving(true)
    try {
      console.log('Saving config:', config)
      await invoke('save_config', { config })
      console.log('Config saved successfully')
    } catch (err) {
      console.error('Failed to save config:', err)
    } finally {
      setSaving(false)
    }
  }

  const resetConfig = async () => {
    try {
      const cfg = await invoke<AppConfig>('reset_config')
      setConfig(cfg)
    } catch (err) {
      console.error('Failed to reset config:', err)
    }
  }

  const addGame = () => {
    if (!config || !newGame.trim()) return
    setConfig({
      ...config,
      priorities: {
        ...config.priorities,
        games: [...config.priorities.games, newGame.trim()]
      }
    })
    setNewGame('')
  }

  const removeGame = (index: number) => {
    if (!config) return
    setConfig({
      ...config,
      priorities: {
        ...config.priorities,
        games: config.priorities.games.filter((_, i) => i !== index)
      }
    })
  }

  const addStreamer = () => {
    if (!config || !newStreamer.trim()) return
    setConfig({
      ...config,
      priorities: {
        ...config.priorities,
        streamers: [...config.priorities.streamers, newStreamer.trim()]
      }
    })
    setNewStreamer('')
  }

  const removeStreamer = (index: number) => {
    if (!config) return
    setConfig({
      ...config,
      priorities: {
        ...config.priorities,
        streamers: config.priorities.streamers.filter((_, i) => i !== index)
      }
    })
  }

  const addStreakChannel = () => {
    if (!config || !newStreakChannel.trim()) return
    setConfig({
      ...config,
      bonus: {
        ...config.bonus,
        daily_streaks: {
          ...config.bonus.daily_streaks,
          channels: [...config.bonus.daily_streaks.channels, newStreakChannel.trim()]
        }
      }
    })
    setNewStreakChannel('')
  }

  const removeStreakChannel = (index: number) => {
    if (!config) return
    setConfig({
      ...config,
      bonus: {
        ...config.bonus,
        daily_streaks: {
          ...config.bonus.daily_streaks,
          channels: config.bonus.daily_streaks.channels.filter((_, i) => i !== index)
        }
      }
    })
  }

  const addPointsChannel = () => {
    if (!config || !newPointsChannel.trim()) return
    setConfig({
      ...config,
      bonus: {
        ...config.bonus,
        points_farming: {
          ...config.bonus.points_farming,
          channels: [...config.bonus.points_farming.channels, newPointsChannel.trim()]
        }
      }
    })
    setNewPointsChannel('')
  }

  const removePointsChannel = (index: number) => {
    if (!config) return
    setConfig({
      ...config,
      bonus: {
        ...config.bonus,
        points_farming: {
          ...config.bonus.points_farming,
          channels: config.bonus.points_farming.channels.filter((_, i) => i !== index)
        }
      }
    })
  }

  const updateSetting = (key: keyof AppConfig['settings'], value: any) => {
    if (!config) return
    setConfig({
      ...config,
      settings: {
        ...config.settings,
        [key]: value
      }
    })
  }

  const updateBonus = (key: 'daily_streaks' | 'points_farming', field: string, value: any) => {
    if (!config) return
    setConfig({
      ...config,
      bonus: {
        ...config.bonus,
        [key]: {
          ...config.bonus[key],
          [field]: value
        }
      }
    })
  }

  if (loading) {
    return (
      <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
        <CardContent className="p-6 text-center text-zinc-400">
          Loading configuration...
        </CardContent>
      </Card>
    )
  }

  if (!config) {
    return (
      <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
        <CardContent className="p-6 text-center text-red-400">
          Failed to load configuration
        </CardContent>
      </Card>
    )
  }

  return (
    <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="text-lg text-white flex items-center gap-2">
            <SettingsIcon className="w-5 h-5" />
            Configuration
          </CardTitle>
          <div className="flex gap-2">
            <Button variant="outline" size="sm" onClick={resetConfig} className="border-white/10 text-zinc-400 hover:text-white hover:bg-white/5">
              <RotateCcw className="w-4 h-4 mr-1" />
              Reset
            </Button>
            <Button size="sm" onClick={saveConfig} disabled={saving} className="bg-purple-600 hover:bg-purple-700 text-white">
              <Save className="w-4 h-4 mr-1" />
              {saving ? 'Saving...' : 'Save'}
            </Button>
          </div>
        </div>
      </CardHeader>
      <CardContent>
        <Tabs defaultValue="priorities" className="w-full">
          <TabsList className="grid w-full grid-cols-4 bg-white/5">
            <TabsTrigger value="priorities" className="text-xs">
              <Gamepad2 className="w-3 h-3 mr-1" />
              Priorities
            </TabsTrigger>
            <TabsTrigger value="streaks" className="text-xs">
              <Tv className="w-3 h-3 mr-1" />
              Streaks
            </TabsTrigger>
            <TabsTrigger value="points" className="text-xs">
              <Users className="w-3 h-3 mr-1" />
              Points
            </TabsTrigger>
            <TabsTrigger value="settings" className="text-xs">
              <SettingsIcon className="w-3 h-3 mr-1" />
              Settings
            </TabsTrigger>
          </TabsList>

          {/* Priorities Tab */}
          <TabsContent value="priorities" className="space-y-4 mt-4">
            {/* Games */}
            <div className="space-y-2 relative">
              <Label className="text-white">Priority Games</Label>
              <div className="flex gap-2 relative">
                <div className="relative flex-1">
                  <Input
                    value={newGame}
                    onChange={(e) => setNewGame(e.target.value)}
                    placeholder="Search and add games..."
                    className="bg-white/5 border-white/10 text-white placeholder:text-zinc-500"
                    onKeyDown={(e) => e.key === 'Enter' && addGame()}
                    onFocus={() => newGame.length >= 2 && setShowGameDropdown(gameSearchResults.length > 0)}
                  />
                  {isSearchingGames && (
                    <div className="absolute right-3 top-1/2 -translate-y-1/2">
                      <div className="w-4 h-4 border-2 border-purple-500 border-t-transparent rounded-full animate-spin" />
                    </div>
                  )}
                  {/* Dropdown */}
                  {showGameDropdown && gameSearchResults.length > 0 && (
                    <div className="absolute top-full left-0 right-0 mt-1 bg-zinc-900 border border-white/10 rounded-lg shadow-xl z-50 max-h-48 overflow-y-auto">
                      {gameSearchResults.map(([id, name]) => (
                        <button
                          key={id}
                          className="w-full text-left px-3 py-2 text-sm text-white hover:bg-white/10 transition-colors"
                          onClick={() => {
                            if (!config.priorities.games.includes(name)) {
                              setConfig({
                                ...config,
                                priorities: {
                                  ...config.priorities,
                                  games: [...config.priorities.games, name]
                                }
                              })
                            }
                            setNewGame('')
                            setShowGameDropdown(false)
                          }}
                        >
                          {name}
                        </button>
                      ))}
                    </div>
                  )}
                </div>
                <Button onClick={addGame} size="sm" className="bg-purple-600 hover:bg-purple-700 text-white">
                  <Plus className="w-4 h-4" />
                </Button>
              </div>
              <div className="flex flex-wrap gap-1">
                {config.priorities.games.map((game, i) => (
                  <Badge key={i} variant="secondary" className="bg-purple-500/20 text-purple-400 border-purple-500/50 cursor-pointer hover:bg-red-500/20 hover:text-red-400 hover:border-red-500/50 transition-colors"
                    onClick={() => removeGame(i)}>
                    {game} <X className="w-3 h-3 ml-1" />
                  </Badge>
                ))}
              </div>
            </div>

            <Separator className="bg-white/10" />

            {/* Streamers */}
            <div className="space-y-2">
              <Label className="text-white">Priority Streamers</Label>
              <div className="flex gap-2">
                <Input
                  value={newStreamer}
                  onChange={(e) => setNewStreamer(e.target.value)}
                  placeholder="Add streamer name..."
                  className="bg-white/5 border-white/10 text-white placeholder:text-zinc-500"
                  onKeyDown={(e) => e.key === 'Enter' && addStreamer()}
                />
                <Button onClick={addStreamer} size="sm" className="bg-purple-600 hover:bg-purple-700 text-white">
                  <Plus className="w-4 h-4" />
                </Button>
              </div>
              <div className="flex flex-wrap gap-1">
                {config.priorities.streamers.map((streamer, i) => (
                  <Badge key={i} variant="secondary" className="bg-blue-500/20 text-blue-400 border-blue-500/50 cursor-pointer hover:bg-red-500/20 hover:text-red-400 hover:border-red-500/50 transition-colors"
                    onClick={() => removeStreamer(i)}>
                    {streamer} <X className="w-3 h-3 ml-1" />
                  </Badge>
                ))}
              </div>
            </div>
          </TabsContent>

          {/* Streaks Tab */}
          <TabsContent value="streaks" className="space-y-4 mt-4">
            <div className="space-y-2">
              <Label className="text-white">Daily Streak Channels</Label>
              <div className="flex gap-2">
                <Input
                  value={newStreakChannel}
                  onChange={(e) => setNewStreakChannel(e.target.value)}
                  placeholder="Add channel name..."
                  className="bg-white/5 border-white/10 text-white placeholder:text-zinc-500"
                  onKeyDown={(e) => e.key === 'Enter' && addStreakChannel()}
                />
                <Button onClick={addStreakChannel} size="sm" className="bg-purple-600 hover:bg-purple-700 text-white">
                  <Plus className="w-4 h-4" />
                </Button>
              </div>
              <div className="flex flex-wrap gap-1">
                {config.bonus.daily_streaks.channels.map((channel, i) => (
                  <Badge key={i} variant="secondary" className="bg-green-500/20 text-green-400 border-green-500/50 cursor-pointer hover:bg-red-500/20 hover:text-red-400 hover:border-red-500/50 transition-colors"
                    onClick={() => removeStreakChannel(i)}>
                    {channel} <X className="w-3 h-3 ml-1" />
                  </Badge>
                ))}
              </div>
            </div>

            <div className="flex items-center gap-4">
              <div className="flex items-center space-x-2">
                <Switch
                  checked={config.bonus.daily_streaks.auto_start}
                  onCheckedChange={(checked) => updateBonus('daily_streaks', 'auto_start', checked)}
                  id="streak-auto"
                />
                <Label htmlFor="streak-auto" className="text-zinc-400">Auto Start</Label>
              </div>

              <div className="flex items-center space-x-2">
                <Label className="text-zinc-400">Mode:</Label>
                <select
                  value={config.bonus.daily_streaks.mode}
                  onChange={(e) => updateBonus('daily_streaks', 'mode', e.target.value)}
                  className="bg-white/5 border-white/10 text-white rounded px-2 py-1 text-sm"
                >
                  <option value="parallel">Parallel</option>
                  <option value="sequential">Sequential</option>
                </select>
              </div>
            </div>
          </TabsContent>

          {/* Points Tab */}
          <TabsContent value="points" className="space-y-4 mt-4">
            <div className="space-y-2">
              <Label className="text-white">Points Farming Channels</Label>
              <div className="flex gap-2">
                <Input
                  value={newPointsChannel}
                  onChange={(e) => setNewPointsChannel(e.target.value)}
                  placeholder="Add channel name..."
                  className="bg-white/5 border-white/10 text-white placeholder:text-zinc-500"
                  onKeyDown={(e) => e.key === 'Enter' && addPointsChannel()}
                />
                <Button onClick={addPointsChannel} size="sm" className="bg-purple-600 hover:bg-purple-700 text-white">
                  <Plus className="w-4 h-4" />
                </Button>
              </div>
              <div className="flex flex-wrap gap-1">
                {config.bonus.points_farming.channels.map((channel, i) => (
                  <Badge key={i} variant="secondary" className="bg-yellow-500/20 text-yellow-400 border-yellow-500/50 cursor-pointer hover:bg-red-500/20 hover:text-red-400 hover:border-red-500/50 transition-colors"
                    onClick={() => removePointsChannel(i)}>
                    {channel} <X className="w-3 h-3 ml-1" />
                  </Badge>
                ))}
              </div>
            </div>

            <div className="flex items-center gap-4">
              <div className="flex items-center space-x-2">
                <Switch
                  checked={config.bonus.points_farming.auto_start}
                  onCheckedChange={(checked) => updateBonus('points_farming', 'auto_start', checked)}
                  id="points-auto"
                />
                <Label htmlFor="points-auto" className="text-zinc-400">Auto Start</Label>
              </div>

              <div className="flex items-center space-x-2">
                <Label className="text-zinc-400">Max Concurrent:</Label>
                <Input
                  type="number"
                  value={config.bonus.points_farming.max_concurrent}
                  onChange={(e) => updateBonus('points_farming', 'max_concurrent', parseInt(e.target.value) || 1)}
                  className="w-20 bg-white/5 border-white/10 text-white"
                  min={1}
                  max={10}
                />
              </div>
            </div>
          </TabsContent>

          {/* Settings Tab */}
          <TabsContent value="settings" className="space-y-4 mt-4">
            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label className="text-zinc-400">Check Interval (seconds)</Label>
                <Input
                  type="number"
                  value={config.settings.check_interval}
                  onChange={(e) => updateSetting('check_interval', parseInt(e.target.value) || 60)}
                  className="bg-white/5 border-white/10 text-white"
                  min={10}
                  max={300}
                />
              </div>

              <div className="space-y-2">
                <Label className="text-zinc-400">Language</Label>
                <select
                  value={config.settings.language}
                  onChange={(e) => updateSetting('language', e.target.value)}
                  className="w-full bg-white/5 border-white/10 text-white rounded px-3 py-2"
                >
                  <option value="EN">English</option>
                  <option value="FR">French</option>
                  <option value="ES">Spanish</option>
                  <option value="DE">German</option>
                </select>
              </div>
            </div>

            <Separator className="bg-white/10" />

            <div className="space-y-3">
              <Label className="text-white">Notifications</Label>
              
              <div className="flex items-center space-x-2">
                <Switch
                  checked={config.settings.notifications_enabled}
                  onCheckedChange={(checked) => updateSetting('notifications_enabled', checked)}
                  id="notif-enabled"
                />
                <Label htmlFor="notif-enabled" className="text-zinc-400">Enable Notifications</Label>
              </div>

              <div className="flex items-center space-x-2">
                <Switch
                  checked={config.settings.notify_active_drop}
                  onCheckedChange={(checked) => updateSetting('notify_active_drop', checked)}
                  id="notif-drop"
                />
                <Label htmlFor="notif-drop" className="text-zinc-400">Notify Active Drop</Label>
              </div>

              <div className="flex items-center space-x-2">
                <Switch
                  checked={config.settings.notify_claim}
                  onCheckedChange={(checked) => updateSetting('notify_claim', checked)}
                  id="notif-claim"
                />
                <Label htmlFor="notif-claim" className="text-zinc-400">Notify Claim</Label>
              </div>

              <div className="flex items-center space-x-2">
                <Switch
                  checked={config.settings.notify_campaign_finish}
                  onCheckedChange={(checked) => updateSetting('notify_campaign_finish', checked)}
                  id="notif-finish"
                />
                <Label htmlFor="notif-finish" className="text-zinc-400">Notify Campaign Finish</Label>
              </div>
            </div>
          </TabsContent>
        </Tabs>
      </CardContent>
    </Card>
  )
}
