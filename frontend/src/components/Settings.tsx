import { useState, useEffect } from 'react'
import { api } from '@/api'
import { activityNotifications } from '@/activity'
import type { AppConfig } from '@/types'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Switch } from '@/components/ui/switch'
import { Separator } from '@/components/ui/separator'
import { useAppStore } from '@/store'
import { useTranslation } from '@/lib/i18n'
import {
  X, Plus, Gamepad2, Tv, Settings2, Clock, AlertTriangle,
  Check, Target, Users, Bell, Zap, FilterX, ChevronRight,
  RotateCcw, Coins, Star
} from 'lucide-react'
import { enable, disable, isEnabled } from '@tauri-apps/plugin-autostart'
import { type as osType } from '@tauri-apps/plugin-os'

// ── Game exception: game name + optional campaign keyword ─────────────────
interface GameException {
  game: string
  keyword: string // empty = all campaigns for that game
}

// ── Extended schedule with game exceptions ────────────────────────────────
interface ExtendedSchedule {
  enabled: boolean
  monday: { enabled: boolean; start_time: string; end_time: string }
  tuesday: { enabled: boolean; start_time: string; end_time: string }
  wednesday: { enabled: boolean; start_time: string; end_time: string }
  thursday: { enabled: boolean; start_time: string; end_time: string }
  friday: { enabled: boolean; start_time: string; end_time: string }
  saturday: { enabled: boolean; start_time: string; end_time: string }
  sunday: { enabled: boolean; start_time: string; end_time: string }
  exceptions: { date: string; is_allowed: boolean }[]
  // New fields (stored under schedule key, backward compatible via optional)
  game_exceptions?: GameException[]
  skip_short_campaigns_days?: number
  skip_expired?: boolean
  skip_already_owned?: boolean
}

const DAYS = ['monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday', 'sunday'] as const
const DAY_LABELS: Record<string, string> = {
  monday: 'Lundi', tuesday: 'Mardi', wednesday: 'Mercredi',
  thursday: 'Jeudi', friday: 'Vendredi', saturday: 'Samedi', sunday: 'Dimanche',
}

const DEFAULT_SCHEDULE: ExtendedSchedule = {
  enabled: false,
  monday: { enabled: false, start_time: '08:00', end_time: '18:00' },
  tuesday: { enabled: false, start_time: '08:00', end_time: '18:00' },
  wednesday: { enabled: false, start_time: '08:00', end_time: '18:00' },
  thursday: { enabled: false, start_time: '08:00', end_time: '18:00' },
  friday: { enabled: false, start_time: '08:00', end_time: '18:00' },
  saturday: { enabled: false, start_time: '08:00', end_time: '18:00' },
  sunday: { enabled: false, start_time: '08:00', end_time: '18:00' },
  exceptions: [],
  game_exceptions: [],
  skip_short_campaigns_days: 3,
  skip_expired: true,
  skip_already_owned: true,
}

// ── Reusable row component ────────────────────────────────────────────────
function SettingRow({
  label, description, children,
}: { label: string; description?: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 py-3 border-b border-white/5 last:border-0">
      <div className="flex-1 min-w-0">
        <div className="text-sm font-semibold text-white">{label}</div>
        {description && <p className="text-xs text-zinc-500 mt-0.5 leading-relaxed">{description}</p>}
      </div>
      <div className="flex-shrink-0">{children}</div>
    </div>
  )
}

// ── Chip list ─────────────────────────────────────────────────────────────
function ChipList({
  items, colorClass = 'bg-purple-500/20 text-purple-400 border-purple-500/50',
  onRemove,
}: { items: string[]; colorClass?: string; onRemove: (i: number) => void }) {
  if (items.length === 0) return null
  return (
    <div className="flex flex-wrap gap-1.5 mt-2">
      {items.map((item, i) => (
        <Badge
          key={i}
          variant="secondary"
          className={`${colorClass} cursor-pointer hover:bg-red-500/20 hover:text-red-400 hover:border-red-500/50 transition-colors`}
          onClick={() => onRemove(i)}
        >
          {item} <X className="w-3 h-3 ml-1" />
        </Badge>
      ))}
    </div>
  )
}

// ── Auto-save indicator ───────────────────────────────────────────────────
function SavedIndicator({ visible }: { visible: boolean }) {
  const { t } = useTranslation()
  return (
    <span className={`flex items-center gap-1 text-xs text-green-400 transition-opacity duration-300 ${visible ? 'opacity-100' : 'opacity-0'}`}>
      <Check className="w-3 h-3" /> {t('common.saved')}
    </span>
  )
}

// ── Section header ────────────────────────────────────────────────────────
function SectionTitle({ icon: Icon, children }: { icon: React.ElementType; children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-2 text-sm font-bold text-white mb-1">
      <Icon className="w-3.5 h-3.5 text-purple-400" />
      {children}
    </div>
  )
}

// ─────────────────────────────────────────────────────────────────────────
export interface SettingsPanelProps {
  /** Called when the user clicks a shortcut to navigate to another page */
  onNavigate?: (view: 'dashboard' | 'drops' | 'points' | 'history' | 'settings', settingsTab?: string) => void
  /** Default tab to open when the panel mounts */
  defaultTab?: string
}

export function SettingsPanel({ onNavigate, defaultTab = 'general' }: SettingsPanelProps) {
  const { t, lang } = useTranslation()
  const isMobile = osType() === 'android' || osType() === 'ios'

  const [config, setConfig] = useState<AppConfig | null>(null)
  const [loading, setLoading] = useState(true)
  const [savedVisible, setSavedVisible] = useState(false)
  const [autostartEnabled, setAutostartEnabled] = useState(false)
  const [resetConfirmOpen, setResetConfirmOpen] = useState(false)

  // Field inputs
  const [newGame, setNewGame] = useState('')
  const [newStreamer, setNewStreamer] = useState('')
  const [newStreakChannel, setNewStreakChannel] = useState('')
  const [newPointsChannel, setNewPointsChannel] = useState('')
  const [newExcGame, setNewExcGame] = useState('')
  const [newExcKeyword, setNewExcKeyword] = useState('')

  // Game search autocomplete
  const [gameSearchResults, setGameSearchResults] = useState<[string, string][]>([])
  const [isSearchingGames, setIsSearchingGames] = useState(false)
  const [showGameDropdown, setShowGameDropdown] = useState(false)

  // ── Load ──────────────────────────────────────────────────────────────
  useEffect(() => { loadConfig() }, [])

  // ── Auto-save (debounced) ─────────────────────────────────────────────
  useEffect(() => {
    if (!config || loading) return
    const t = setTimeout(async () => {
      try {
        await api.saveConfig(config)
        activityNotifications.setConfig(config)
        setSavedVisible(true)
        setTimeout(() => setSavedVisible(false), 2000)
      } catch (err) {
        console.error('Failed to save config:', err)
      }
    }, 800)
    return () => clearTimeout(t)
  }, [JSON.stringify(config)])

  // ── Game search ───────────────────────────────────────────────────────
  useEffect(() => {
    if (!newGame.trim() || newGame.length < 2) {
      setGameSearchResults([])
      setShowGameDropdown(false)
      return
    }
    const t = setTimeout(async () => {
      setIsSearchingGames(true)
      try {
        const results = await api.searchGames(newGame)
        setGameSearchResults(results)
        setShowGameDropdown(results.length > 0)
      } catch {
        setGameSearchResults([])
      } finally {
        setIsSearchingGames(false)
      }
    }, 300)
    return () => clearTimeout(t)
  }, [newGame])

  // ── Helpers ───────────────────────────────────────────────────────────
  const loadConfig = async () => {
    try {
      const cfg = await api.getConfig()
      setConfig(cfg)
      try {
        setAutostartEnabled(await isEnabled())
      } catch { /* ignore on mobile */ }
    } catch (err) {
      console.error('Failed to load config:', err)
    } finally {
      setLoading(false)
    }
  }

  const resetConfig = async () => {
    try {
      const cfg = await api.resetConfig()
      activityNotifications.setConfig(cfg)
      setConfig(cfg)
    } catch (err) {
      console.error('Failed to reset config:', err)
    } finally {
      setResetConfirmOpen(false)
    }
  }

  const toggleAutostart = async (checked: boolean) => {
    setAutostartEnabled(checked)
    try { checked ? await enable() : await disable() } catch { /* noop in dev */ }
  }

  const sched = (): ExtendedSchedule => (config?.schedule as ExtendedSchedule) ?? DEFAULT_SCHEDULE

  const updateSched = (patch: Partial<ExtendedSchedule>) =>
    config && setConfig({ ...config, schedule: { ...sched(), ...patch } as any })

  const updateSchedDay = (day: string, field: 'enabled' | 'start_time' | 'end_time', value: any) => {
    const s = sched()
    updateSched({ [day]: { ...(s[day as keyof typeof s] as any), [field]: value } })
  }

  const updateSetting = (key: keyof AppConfig['settings'], value: any) => {
    if (config) {
      setConfig({ ...config, settings: { ...config.settings, [key]: value } })
      if (key === 'language') {
        useAppStore.getState().setLanguage(value)
      }
      if (key === 'theme') {
        useAppStore.getState().setTheme(value)
      }
    }
  }

  const updateBonus = (key: 'daily_streaks' | 'points_farming', field: string, value: any) =>
    config && setConfig({ ...config, bonus: { ...config.bonus, [key]: { ...config.bonus[key], [field]: value } } })

  // Game exceptions helpers
  const gameExceptions = (): GameException[] => sched().game_exceptions ?? []
  const addGameException = () => {
    if (!newExcGame.trim()) return
    const exc: GameException = { game: newExcGame.trim(), keyword: newExcKeyword.trim() }
    updateSched({ game_exceptions: [...gameExceptions(), exc] })
    setNewExcGame(''); setNewExcKeyword('')
  }
  const removeGameException = (i: number) =>
    updateSched({ game_exceptions: gameExceptions().filter((_, idx) => idx !== i) })

  // ── Chip helpers ──────────────────────────────────────────────────────
  const addGame = (name = newGame.trim()) => {
    if (!config || !name || config.priorities.games.includes(name)) return
    setConfig({ ...config, priorities: { ...config.priorities, games: [...config.priorities.games, name] } })
    setNewGame(''); setShowGameDropdown(false)
  }
  const removeGame = (i: number) =>
    config && setConfig({ ...config, priorities: { ...config.priorities, games: config.priorities.games.filter((_, idx) => idx !== i) } })

  const addStreamer = () => {
    if (!config || !newStreamer.trim()) return
    setConfig({ ...config, priorities: { ...config.priorities, streamers: [...config.priorities.streamers, newStreamer.trim()] } })
    setNewStreamer('')
  }
  const removeStreamer = (i: number) =>
    config && setConfig({ ...config, priorities: { ...config.priorities, streamers: config.priorities.streamers.filter((_, idx) => idx !== i) } })

  const addStreakChannel = () => {
    if (!config || !newStreakChannel.trim()) return
    setConfig({ ...config, bonus: { ...config.bonus, daily_streaks: { ...config.bonus.daily_streaks, channels: [...config.bonus.daily_streaks.channels, newStreakChannel.trim()] } } })
    setNewStreakChannel('')
  }
  const removeStreakChannel = (i: number) =>
    config && setConfig({ ...config, bonus: { ...config.bonus, daily_streaks: { ...config.bonus.daily_streaks, channels: config.bonus.daily_streaks.channels.filter((_, idx) => idx !== i) } } })

  const addPointsChannel = () => {
    if (!config || !newPointsChannel.trim()) return
    setConfig({ ...config, bonus: { ...config.bonus, points_farming: { ...config.bonus.points_farming, channels: [...config.bonus.points_farming.channels, newPointsChannel.trim()] } } })
    setNewPointsChannel('')
  }
  const removePointsChannel = (i: number) =>
    config && setConfig({ ...config, bonus: { ...config.bonus, points_farming: { ...config.bonus.points_farming, channels: config.bonus.points_farming.channels.filter((_, idx) => idx !== i) } } })

  // ── Shortcut button ───────────────────────────────────────────────────
  const Shortcut = ({ view, label }: { view: Parameters<NonNullable<typeof onNavigate>>[0]; label: string }) =>
    onNavigate ? (
      <button
        className="flex items-center gap-1 text-xs text-purple-400 hover:text-purple-300 transition-colors"
        onClick={() => onNavigate(view)}
      >
        {label} <ChevronRight className="w-3 h-3" />
      </button>
    ) : null

  // ── Loading / Error states ────────────────────────────────────────────
  if (loading) {
    return (
      <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
        <CardContent className="p-6 text-center text-zinc-400">{t('common.loading')}</CardContent>
      </Card>
    )
  }
  if (!config) {
    return (
      <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
        <CardContent className="p-6 text-center text-red-400">
          {lang === 'FR' ? 'Impossible de charger la configuration.' : 'Failed to load configuration.'}
        </CardContent>
      </Card>
    )
  }

  // ── Render ────────────────────────────────────────────────────────────
  return (
    <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-base text-white flex items-center gap-2">
            <Settings2 className="w-4 h-4 text-purple-400" />
            {t('settings.title')}
          </CardTitle>
          <SavedIndicator visible={savedVisible} />
        </div>
      </CardHeader>

      <CardContent>
        <Tabs defaultValue={defaultTab} className="w-full">
          <TabsList className="grid w-full grid-cols-4 bg-white/5 mb-4">
            <TabsTrigger value="general" className="text-xs gap-1">
              <Settings2 className="w-3 h-3" /> {t('settings.tabs.general')}
            </TabsTrigger>
            <TabsTrigger value="drops" className="text-xs gap-1">
              <Gamepad2 className="w-3 h-3" /> {t('settings.tabs.drops')}
            </TabsTrigger>
            <TabsTrigger value="points" className="text-xs gap-1">
              <Tv className="w-3 h-3" /> {t('settings.tabs.points')}
            </TabsTrigger>
            <TabsTrigger value="schedule" className="text-xs gap-1">
              <Clock className="w-3 h-3" /> {t('settings.tabs.schedule')}
            </TabsTrigger>
          </TabsList>

          {/* ── TAB 1: GÉNÉRAL ─────────────────────────────────────────── */}
          <TabsContent value="general" className="space-y-4 mt-0">

            {/* Notifications */}
            <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
              <SectionTitle icon={Bell}>{t('settings.general.notifications')}</SectionTitle>
              <p className="text-xs text-zinc-500 mb-3">{t('settings.general.notificationsDesc')}</p>
              <SettingRow label={t('settings.general.enableNotifications')} description={t('settings.general.enableNotificationsDesc')}>
                <Switch checked={config.settings.notifications_enabled} onCheckedChange={v => updateSetting('notifications_enabled', v)} />
              </SettingRow>
              <SettingRow label={t('settings.general.newActiveDrop')} description={t('settings.general.newActiveDropDesc')}>
                <Switch checked={config.settings.notify_active_drop} onCheckedChange={v => updateSetting('notify_active_drop', v)} disabled={!config.settings.notifications_enabled} />
              </SettingRow>
              <SettingRow label={t('settings.general.dropClaimed')} description={t('settings.general.dropClaimedDesc')}>
                <Switch checked={config.settings.notify_claim} onCheckedChange={v => updateSetting('notify_claim', v)} disabled={!config.settings.notifications_enabled} />
              </SettingRow>
              <SettingRow label={t('settings.general.campaignFinish')} description={t('settings.general.campaignFinishDesc')}>
                <Switch checked={config.settings.notify_campaign_finish} onCheckedChange={v => updateSetting('notify_campaign_finish', v)} disabled={!config.settings.notifications_enabled} />
              </SettingRow>
            </div>

            {/* Miner */}
            <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
              <SectionTitle icon={Settings2}>{t('settings.general.miner')}</SectionTitle>
              <SettingRow label={t('settings.general.checkInterval')} description={t('settings.general.checkIntervalDesc')}>
                <div className="flex items-center gap-2">
                  <Input type="number" value={config.settings.check_interval} onChange={e => updateSetting('check_interval', parseInt(e.target.value) || 60)}
                    className="w-16 bg-white/5 border-white/10 text-white text-center" min={10} max={300} />
                  <span className="text-xs text-zinc-500">s</span>
                </div>
              </SettingRow>
              <SettingRow label={t('settings.general.language')} description={t('settings.general.languageDesc')}>
                <select value={config.settings.language} onChange={e => updateSetting('language', e.target.value)}
                  className="bg-zinc-900 border border-white/10 text-white rounded px-2 py-1 text-sm">
                  <option value="FR">🇫🇷 Français</option>
                  <option value="EN">🇬🇧 English</option>
                  <option value="ES">🇪🇸 Español</option>
                  <option value="DE">🇩🇪 Deutsch</option>
                </select>
              </SettingRow>
              <SettingRow label={lang === 'FR' ? 'Thème' : 'Theme'} description={lang === 'FR' ? "Thème de l'interface graphique." : 'UI color theme.'}>
                <select value={config.settings.theme || 'dark'} onChange={e => updateSetting('theme', e.target.value)}
                  className="bg-zinc-900 border border-white/10 text-white rounded px-2 py-1 text-sm">
                  <option value="light">☀️ {lang === 'FR' ? 'Clair' : 'Light'}</option>
                  <option value="dark">🌙 {lang === 'FR' ? 'Sombre' : 'Dark'}</option>
                  <option value="oled">🌑 OLED</option>
                </select>
              </SettingRow>
            </div>

            {/* Desktop-only: System & Tray */}
            {!isMobile && (
              <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
                <SectionTitle icon={Settings2}>{t('settings.general.system')}</SectionTitle>
                <SettingRow label={t('settings.general.autostart')} description={t('settings.general.autostartDesc')}>
                  <Switch checked={autostartEnabled} onCheckedChange={toggleAutostart} />
                </SettingRow>
                <SettingRow label={t('settings.general.minToTray')} description={t('settings.general.minToTrayDesc')}>
                  <Switch checked={config.settings.minimize_to_tray ?? true} onCheckedChange={v => updateSetting('minimize_to_tray', v)} />
                </SettingRow>
                <SettingRow label={t('settings.general.trayIcon')} description={t('settings.general.trayIconDesc')}>
                  <Switch checked={config.settings.show_tray_icon ?? true} onCheckedChange={v => updateSetting('show_tray_icon', v)} />
                </SettingRow>
                <SettingRow label={t('settings.general.logsFolder')} description="">
                  <Button variant="outline" size="sm" className="border-white/10 text-zinc-400 hover:text-white hover:bg-white/5 text-xs"
                    onClick={async () => {
                      const { appLogDir } = await import('@tauri-apps/api/path')
                      const { open } = await import('@tauri-apps/plugin-shell')
                      await open(await appLogDir())
                    }}>
                    {t('common.openLogs')}
                  </Button>
                </SettingRow>
              </div>
            )}

            {/* Danger zone */}
            <div className="bg-red-500/5 border border-red-500/20 rounded-xl p-4">
              <SectionTitle icon={AlertTriangle}>{t('settings.general.dangerZone')}</SectionTitle>
              <p className="text-xs text-red-400/70 mb-3">
                {t('settings.general.dangerZoneDesc')}
              </p>
              {resetConfirmOpen ? (
                <div className="flex gap-2">
                  <Button size="sm" variant="outline" className="border-white/10 text-zinc-400 flex-1" onClick={() => setResetConfirmOpen(false)}>
                    {t('common.cancel')}
                  </Button>
                  <Button size="sm" className="bg-red-600 hover:bg-red-700 text-white flex-1" onClick={resetConfig}>
                    <RotateCcw className="w-3 h-3 mr-1" /> {t('common.confirm')}
                  </Button>
                </div>
              ) : (
                <Button size="sm" className="bg-red-500/20 hover:bg-red-500/30 text-red-400 border border-red-500/30 w-full"
                  onClick={() => setResetConfirmOpen(true)}>
                  <RotateCcw className="w-3 h-3 mr-1" /> {t('settings.general.resetButton')}
                </Button>
              )}
            </div>
          </TabsContent>

          {/* ── TAB 2: DROPS MINING ────────────────────────────────────── */}
          <TabsContent value="drops" className="space-y-4 mt-0">

            {/* Jeux ciblés */}
            <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
              <SectionTitle icon={Target}>{t('settings.drops.games')}</SectionTitle>
              <p className="text-xs text-zinc-500 mb-3">
                {t('settings.drops.gamesDesc')}
              </p>
              <div className="flex gap-2 relative">
                <div className="relative flex-1">
                  <Input value={newGame} onChange={e => setNewGame(e.target.value)} placeholder={t('settings.drops.gamesPlaceholder')}
                    className="bg-white/5 border-white/10 text-white placeholder:text-zinc-500"
                    onKeyDown={e => e.key === 'Enter' && addGame()}
                    onFocus={() => newGame.length >= 2 && setShowGameDropdown(gameSearchResults.length > 0)} />
                  {isSearchingGames && (
                    <div className="absolute right-3 top-1/2 -translate-y-1/2">
                      <div className="w-3.5 h-3.5 border-2 border-purple-500 border-t-transparent rounded-full animate-spin" />
                    </div>
                  )}
                  {showGameDropdown && gameSearchResults.length > 0 && (
                    <div className="absolute top-full left-0 right-0 mt-1 bg-zinc-900 border border-white/10 rounded-lg shadow-xl z-50 max-h-48 overflow-y-auto">
                      {gameSearchResults.map(([id, name]) => (
                        <button key={id} className="w-full text-left px-3 py-2 text-sm text-white hover:bg-white/10 transition-colors"
                          onClick={() => addGame(name)}>{name}</button>
                      ))}
                    </div>
                  )}
                </div>
                <Button onClick={() => addGame()} size="sm" className="bg-purple-600 hover:bg-purple-700 text-white">
                  <Plus className="w-4 h-4" />
                </Button>
              </div>
              <ChipList items={config.priorities.games} onRemove={removeGame} />
            </div>

            {/* Streamers ciblés */}
            <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
              <SectionTitle icon={Users}>{t('settings.drops.streamers')}</SectionTitle>
              <p className="text-xs text-zinc-500 mb-3">
                {t('settings.drops.streamersDesc')}
              </p>
              <div className="flex gap-2">
                <Input value={newStreamer} onChange={e => setNewStreamer(e.target.value)} placeholder={t('settings.drops.streamersPlaceholder')}
                  className="bg-white/5 border-white/10 text-white placeholder:text-zinc-500"
                  onKeyDown={e => e.key === 'Enter' && addStreamer()} />
                <Button onClick={addStreamer} size="sm" className="bg-purple-600 hover:bg-purple-700 text-white">
                  <Plus className="w-4 h-4" />
                </Button>
              </div>
              <ChipList items={config.priorities.streamers} colorClass="bg-blue-500/20 text-blue-400 border-blue-500/50" onRemove={removeStreamer} />
            </div>

            {/* Comportement */}
            <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
              <SectionTitle icon={Settings2}>{t('settings.drops.behavior')}</SectionTitle>
              <SettingRow label={t('settings.drops.mineAll')} description={t('settings.drops.mineAllDesc')}>
                <Switch checked={config.settings.mine_all_inventory ?? false} onCheckedChange={v => updateSetting('mine_all_inventory' as any, v)} />
              </SettingRow>
              <SettingRow label={t('settings.drops.autoStartDrops')} description={t('settings.drops.autoStartDropsDesc')}>
                <Switch checked={config.settings.auto_start_drops} onCheckedChange={v => updateSetting('auto_start_drops', v)} />
              </SettingRow>
            </div>
          </TabsContent>

          {/* ── TAB 3: CHANNEL POINTS ──────────────────────────────────── */}
          <TabsContent value="points" className="space-y-4 mt-0">

            {/* Watch Bonus */}
            <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
              <div className="flex items-center justify-between mb-1">
                <SectionTitle icon={Star}>{t('settings.points.dailyBonus')}</SectionTitle>
                <Shortcut view="points" label={lang === 'FR' ? 'Voir les chaînes' : 'View channels'} />
              </div>
              <p className="text-xs text-zinc-500 mb-3">
                {t('settings.points.dailyBonusDesc')}
              </p>
              <div className="flex gap-2">
                <Input value={newStreakChannel} onChange={e => setNewStreakChannel(e.target.value)} placeholder={t('settings.points.channelPlaceholder')}
                  className="bg-white/5 border-white/10 text-white placeholder:text-zinc-500"
                  onKeyDown={e => e.key === 'Enter' && addStreakChannel()} />
                <Button onClick={addStreakChannel} size="sm" className="bg-purple-600 hover:bg-purple-700 text-white">
                  <Plus className="w-4 h-4" />
                </Button>
              </div>
              <ChipList items={config.bonus.daily_streaks.channels} colorClass="bg-green-500/20 text-green-400 border-green-500/50" onRemove={removeStreakChannel} />
              <Separator className="bg-white/10 my-3" />
              <SettingRow label={t('settings.points.autoStartPoints')} description={t('settings.points.autoStartPointsDesc')}>
                <Switch checked={config.bonus.daily_streaks.auto_start} onCheckedChange={v => updateBonus('daily_streaks', 'auto_start', v)} />
              </SettingRow>
              <SettingRow label={t('settings.points.monitoredMode')} description={t('settings.points.monitoredModeDesc')}>
                <select value={config.bonus.daily_streaks.mode} onChange={e => updateBonus('daily_streaks', 'mode', e.target.value)}
                  className="bg-zinc-900 border border-white/10 text-white rounded px-2 py-1 text-sm">
                  <option value="sequential">{t('settings.points.sequential')}</option>
                  <option value="parallel">{t('settings.points.parallel')}</option>
                </select>
              </SettingRow>
            </div>

            {/* Points Farming */}
            <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
              <div className="flex items-center justify-between mb-1">
                <SectionTitle icon={Coins}>{t('settings.points.pointsFarming')}</SectionTitle>
                <Shortcut view="points" label={lang === 'FR' ? 'Voir les points' : 'View points'} />
              </div>
              <p className="text-xs text-zinc-500 mb-3">
                {t('settings.points.pointsFarmingDesc')}
              </p>
              <div className="flex gap-2">
                <Input value={newPointsChannel} onChange={e => setNewPointsChannel(e.target.value)} placeholder={t('settings.points.channelPlaceholder')}
                  className="bg-white/5 border-white/10 text-white placeholder:text-zinc-500"
                  onKeyDown={e => e.key === 'Enter' && addPointsChannel()} />
                <Button onClick={addPointsChannel} size="sm" className="bg-purple-600 hover:bg-purple-700 text-white">
                  <Plus className="w-4 h-4" />
                </Button>
              </div>
              <ChipList items={config.bonus.points_farming.channels} colorClass="bg-yellow-500/20 text-yellow-400 border-yellow-500/50" onRemove={removePointsChannel} />
              <Separator className="bg-white/10 my-3" />
              <SettingRow label={t('settings.points.autoStartFarming')} description={t('settings.points.pointsFarmingDesc')}>
                <Switch checked={config.bonus.points_farming.auto_start} onCheckedChange={v => updateBonus('points_farming', 'auto_start', v)} />
              </SettingRow>
              <SettingRow label={t('settings.points.maxConcurrent')} description={t('settings.points.maxConcurrentDesc')}>
                <Input type="number" value={config.bonus.points_farming.max_concurrent}
                  onChange={e => updateBonus('points_farming', 'max_concurrent', parseInt(e.target.value) || 1)}
                  className="w-16 bg-white/5 border-white/10 text-white text-center" min={1} max={10} />
              </SettingRow>
            </div>
          </TabsContent>

          {/* ── TAB 4: PLANIFICATION ───────────────────────────────────── */}
          <TabsContent value="schedule" className="space-y-4 mt-0">

            {/* Master toggle */}
            <div className="flex items-center justify-between p-4 bg-white/[0.03] border border-white/8 rounded-xl">
              <div>
                <div className="text-sm font-semibold text-white">{t('settings.schedule.enableSchedule')}</div>
                <p className="text-xs text-zinc-500 mt-0.5">{t('settings.schedule.enableScheduleDesc')}</p>
              </div>
              <Switch checked={sched().enabled} onCheckedChange={v => updateSched({ enabled: v })} />
            </div>

            {sched().enabled && (
              <div className="space-y-4 animate-in fade-in slide-in-from-top-1 duration-200">

                {/* Weekly schedule */}
                <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
                  <SectionTitle icon={Clock}>{t('settings.schedule.scheduleTimes')}</SectionTitle>
                  <div className="space-y-1 mt-2">
                    {DAYS.map(day => {
                      const d = (sched()[day] as any) ?? { enabled: false, start_time: '08:00', end_time: '18:00' }
                      const dayLabel = lang === 'FR' ? DAY_LABELS[day] : day.charAt(0).toUpperCase() + day.slice(1)
                      return (
                        <div key={day} className="flex flex-wrap items-center gap-3 py-2 border-b border-white/5 last:border-0">
                          <div className="flex items-center gap-2 w-28">
                            <Switch checked={d.enabled} onCheckedChange={v => updateSchedDay(day, 'enabled', v)} className="data-[state=checked]:bg-purple-500" />
                            <span className="text-xs font-medium text-zinc-300 capitalize">{dayLabel}</span>
                          </div>
                          {d.enabled ? (
                            <div className="flex items-center gap-2">
                              <Input type="time" value={d.start_time} onChange={e => updateSchedDay(day, 'start_time', e.target.value)}
                                className="w-24 bg-zinc-900 border-white/10 text-white text-xs" />
                              <span className="text-zinc-500 text-xs">→</span>
                              <Input type="time" value={d.end_time} onChange={e => updateSchedDay(day, 'end_time', e.target.value)}
                                className="w-24 bg-zinc-900 border-white/10 text-white text-xs" />
                            </div>
                          ) : (
                            <span className="text-xs text-zinc-600 italic">{t('settings.schedule.allDay')}</span>
                          )}
                        </div>
                      )
                    })}
                  </div>
                </div>

                {/* Game exceptions */}
                <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
                  <SectionTitle icon={FilterX}>{t('settings.schedule.gameExceptions')}</SectionTitle>
                  <p className="text-xs text-zinc-500 mb-3">
                    {t('settings.schedule.gameExceptionsDesc')}
                  </p>
                  <div className="flex gap-2 mb-2 flex-wrap">
                    <Input value={newExcGame} onChange={e => setNewExcGame(e.target.value)} placeholder={t('settings.schedule.gameNamePlaceholder')}
                      className="bg-white/5 border-white/10 text-white placeholder:text-zinc-500 flex-1 min-w-28" />
                    <Input value={newExcKeyword} onChange={e => setNewExcKeyword(e.target.value)} placeholder={t('settings.schedule.campaignKeywordPlaceholder')}
                      className="bg-white/5 border-white/10 text-white placeholder:text-zinc-500 flex-1 min-w-32" />
                    <Button onClick={addGameException} size="sm" className="bg-red-600/80 hover:bg-red-600 text-white">
                      <Plus className="w-4 h-4" />
                    </Button>
                  </div>
                  <div className="space-y-1.5 mt-2">
                    {gameExceptions().length === 0 ? (
                      <p className="text-xs text-zinc-600 italic text-center py-3">{t('settings.schedule.noExceptions')}</p>
                    ) : gameExceptions().map((exc, i) => (
                      <div key={i} className="flex items-center justify-between gap-2 px-3 py-2 bg-red-500/5 border border-red-500/20 rounded-lg">
                        <div className="flex items-center gap-2 min-w-0">
                          <Badge className="bg-red-500/20 text-red-400 border border-red-500/30 text-xs flex-shrink-0">{lang === 'FR' ? 'Exclu' : 'Excluded'}</Badge>
                          <div className="min-w-0">
                            <div className="text-xs font-semibold text-white truncate">{exc.game}</div>
                            {exc.keyword && <div className="text-xs text-zinc-500">{lang === 'FR' ? 'si contient' : 'if contains'} "{exc.keyword}"</div>}
                            {!exc.keyword && <div className="text-xs text-zinc-500">{lang === 'FR' ? 'Toutes les campagnes' : 'All campaigns'}</div>}
                          </div>
                        </div>
                        <Button size="icon" variant="ghost" className="h-7 w-7 text-zinc-500 hover:text-red-400 hover:bg-red-500/10 flex-shrink-0"
                          onClick={() => removeGameException(i)}>
                          <X className="w-3.5 h-3.5" />
                        </Button>
                      </div>
                    ))}
                  </div>
                </div>

                {/* Auto exceptions */}
                <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
                  <SectionTitle icon={Zap}>{t('settings.schedule.autoExceptions')}</SectionTitle>
                  <p className="text-xs text-zinc-500 mb-3">{t('settings.schedule.autoExceptionsDesc')}</p>
                  <SettingRow
                    label={t('settings.schedule.skipShort')}
                    description={t('settings.schedule.skipShortDesc')}
                  >
                    <div className="flex items-center gap-1.5">
                      <Input
                        type="number"
                        value={sched().skip_short_campaigns_days ?? 3}
                        onChange={e => updateSched({ skip_short_campaigns_days: parseInt(e.target.value) || 0 })}
                        className="w-14 bg-white/5 border-white/10 text-white text-center text-xs"
                        min={0} max={30}
                      />
                      <span className="text-xs text-zinc-500">{t('common.days').charAt(0)}</span>
                    </div>
                  </SettingRow>
                  <SettingRow label={t('settings.schedule.skipExpired')} description={t('settings.schedule.skipExpiredDesc')}>
                    <Switch checked={sched().skip_expired ?? true} onCheckedChange={v => updateSched({ skip_expired: v })} />
                  </SettingRow>
                  <SettingRow label={t('settings.schedule.skipOwned')} description={t('settings.schedule.skipOwnedDesc')}>
                    <Switch checked={sched().skip_already_owned ?? true} onCheckedChange={v => updateSched({ skip_already_owned: v })} />
                  </SettingRow>
                </div>
              </div>
            )}
          </TabsContent>
        </Tabs>
      </CardContent>
    </Card>
  )
}
