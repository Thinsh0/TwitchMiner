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
  return (
    <span className={`flex items-center gap-1 text-xs text-green-400 transition-opacity duration-300 ${visible ? 'opacity-100' : 'opacity-0'}`}>
      <Check className="w-3 h-3" /> Sauvegardé
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

  const updateSetting = (key: keyof AppConfig['settings'], value: any) =>
    config && setConfig({ ...config, settings: { ...config.settings, [key]: value } })

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
        <CardContent className="p-6 text-center text-zinc-400">Chargement de la configuration...</CardContent>
      </Card>
    )
  }
  if (!config) {
    return (
      <Card className="bg-black/40 border-white/10 backdrop-blur-xl shadow-xl">
        <CardContent className="p-6 text-center text-red-400">Impossible de charger la configuration.</CardContent>
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
            Configuration
          </CardTitle>
          <SavedIndicator visible={savedVisible} />
        </div>
      </CardHeader>

      <CardContent>
        <Tabs defaultValue={defaultTab} className="w-full">
          <TabsList className="grid w-full grid-cols-4 bg-white/5 mb-4">
            <TabsTrigger value="general" className="text-xs gap-1">
              <Settings2 className="w-3 h-3" /> Général
            </TabsTrigger>
            <TabsTrigger value="drops" className="text-xs gap-1">
              <Gamepad2 className="w-3 h-3" /> Drops
            </TabsTrigger>
            <TabsTrigger value="points" className="text-xs gap-1">
              <Tv className="w-3 h-3" /> Channel Points
            </TabsTrigger>
            <TabsTrigger value="schedule" className="text-xs gap-1">
              <Clock className="w-3 h-3" /> Planification
            </TabsTrigger>
          </TabsList>

          {/* ── TAB 1: GÉNÉRAL ─────────────────────────────────────────── */}
          <TabsContent value="general" className="space-y-4 mt-0">

            {/* Notifications */}
            <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
              <SectionTitle icon={Bell}>Notifications</SectionTitle>
              <p className="text-xs text-zinc-500 mb-3">Événements qui déclenchent une notification système.</p>
              <SettingRow label="Activer les notifications" description="Active/désactive toutes les notifications de l'app.">
                <Switch checked={config.settings.notifications_enabled} onCheckedChange={v => updateSetting('notifications_enabled', v)} />
              </SettingRow>
              <SettingRow label="Nouveau drop actif" description="Quand le mineur commence à accumuler un nouveau drop.">
                <Switch checked={config.settings.notify_active_drop} onCheckedChange={v => updateSetting('notify_active_drop', v)} disabled={!config.settings.notifications_enabled} />
              </SettingRow>
              <SettingRow label="Drop réclamé" description="À chaque drop obtenu avec succès.">
                <Switch checked={config.settings.notify_claim} onCheckedChange={v => updateSetting('notify_claim', v)} disabled={!config.settings.notifications_enabled} />
              </SettingRow>
              <SettingRow label="Campagne terminée" description="Quand tous les drops d'une campagne sont complétés.">
                <Switch checked={config.settings.notify_campaign_finish} onCheckedChange={v => updateSetting('notify_campaign_finish', v)} disabled={!config.settings.notifications_enabled} />
              </SettingRow>
            </div>

            {/* Miner */}
            <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
              <SectionTitle icon={Settings2}>Mineur</SectionTitle>
              <SettingRow label="Intervalle de vérification" description="Fréquence à laquelle le mineur interroge Twitch (10–300 s).">
                <div className="flex items-center gap-2">
                  <Input type="number" value={config.settings.check_interval} onChange={e => updateSetting('check_interval', parseInt(e.target.value) || 60)}
                    className="w-16 bg-white/5 border-white/10 text-white text-center" min={10} max={300} />
                  <span className="text-xs text-zinc-500">s</span>
                </div>
              </SettingRow>
              <SettingRow label="Langue" description="Langue de l'interface.">
                <select value={config.settings.language} onChange={e => updateSetting('language', e.target.value)}
                  className="bg-zinc-900 border border-white/10 text-white rounded px-2 py-1 text-sm">
                  <option value="FR">🇫🇷 Français</option>
                  <option value="EN">🇬🇧 English</option>
                  <option value="ES">🇪🇸 Español</option>
                  <option value="DE">🇩🇪 Deutsch</option>
                </select>
              </SettingRow>
            </div>

            {/* Desktop-only: System & Tray */}
            {!isMobile && (
              <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
                <SectionTitle icon={Settings2}>Système &amp; Tray</SectionTitle>
                <SettingRow label="Lancement au démarrage" description="Démarre l'app automatiquement au démarrage de Windows.">
                  <Switch checked={autostartEnabled} onCheckedChange={toggleAutostart} />
                </SettingRow>
                <SettingRow label="Minimiser dans la zone de notification" description="Reste actif en arrière-plan à la fermeture.">
                  <Switch checked={config.settings.minimize_to_tray ?? true} onCheckedChange={v => updateSetting('minimize_to_tray', v)} />
                </SettingRow>
                <SettingRow label="Icône dans le tray" description="Affiche l'icône dans la barre des tâches système.">
                  <Switch checked={config.settings.show_tray_icon ?? true} onCheckedChange={v => updateSetting('show_tray_icon', v)} />
                </SettingRow>
                <SettingRow label="Lancer les dossiers de logs" description="">
                  <Button variant="outline" size="sm" className="border-white/10 text-zinc-400 hover:text-white hover:bg-white/5 text-xs"
                    onClick={async () => {
                      const { appLogDir } = await import('@tauri-apps/api/path')
                      const { open } = await import('@tauri-apps/plugin-shell')
                      await open(await appLogDir())
                    }}>
                    Ouvrir les logs
                  </Button>
                </SettingRow>
              </div>
            )}

            {/* Danger zone */}
            <div className="bg-red-500/5 border border-red-500/20 rounded-xl p-4">
              <SectionTitle icon={AlertTriangle}>Zone de danger</SectionTitle>
              <p className="text-xs text-red-400/70 mb-3">
                Efface toutes vos configurations et remet les valeurs par défaut. Irréversible.
              </p>
              {resetConfirmOpen ? (
                <div className="flex gap-2">
                  <Button size="sm" variant="outline" className="border-white/10 text-zinc-400 flex-1" onClick={() => setResetConfirmOpen(false)}>
                    Annuler
                  </Button>
                  <Button size="sm" className="bg-red-600 hover:bg-red-700 text-white flex-1" onClick={resetConfig}>
                    <RotateCcw className="w-3 h-3 mr-1" /> Confirmer
                  </Button>
                </div>
              ) : (
                <Button size="sm" className="bg-red-500/20 hover:bg-red-500/30 text-red-400 border border-red-500/30 w-full"
                  onClick={() => setResetConfirmOpen(true)}>
                  <RotateCcw className="w-3 h-3 mr-1" /> Réinitialiser la configuration
                </Button>
              )}
            </div>
          </TabsContent>

          {/* ── TAB 2: DROPS MINING ────────────────────────────────────── */}
          <TabsContent value="drops" className="space-y-4 mt-0">

            {/* Jeux ciblés */}
            <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
              <SectionTitle icon={Target}>Jeux ciblés</SectionTitle>
              <p className="text-xs text-zinc-500 mb-3">
                Le mineur cherche des drops <span className="text-white font-semibold">uniquement</span> pour ces jeux.
                Sans jeu configuré, aucun drop n'est miné.
              </p>
              <div className="flex gap-2 relative">
                <div className="relative flex-1">
                  <Input value={newGame} onChange={e => setNewGame(e.target.value)} placeholder="Rechercher un jeu..."
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
              <SectionTitle icon={Users}>Streamers ciblés</SectionTitle>
              <p className="text-xs text-zinc-500 mb-3">
                Surveille ces chaînes en priorité lorsqu'elles diffusent un jeu ciblé.
              </p>
              <div className="flex gap-2">
                <Input value={newStreamer} onChange={e => setNewStreamer(e.target.value)} placeholder="Nom du streamer..."
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
              <SectionTitle icon={Settings2}>Comportement</SectionTitle>
              <SettingRow label="Autoclaim automatique" description="Réclame les drops terminés sans action manuelle dès qu'ils sont disponibles.">
                <Switch checked={config.settings.mine_all_inventory ?? false} onCheckedChange={v => updateSetting('mine_all_inventory' as any, v)} />
              </SettingRow>
              <SettingRow label="Lancement automatique" description="Démarre le mineur de drops dès l'ouverture de l'app.">
                <Switch checked={config.settings.auto_start_drops} onCheckedChange={v => updateSetting('auto_start_drops', v)} />
              </SettingRow>
            </div>
          </TabsContent>

          {/* ── TAB 3: CHANNEL POINTS ──────────────────────────────────── */}
          <TabsContent value="points" className="space-y-4 mt-0">

            {/* Watch Bonus */}
            <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
              <div className="flex items-center justify-between mb-1">
                <SectionTitle icon={Star}>Watch Bonus (claim quotidien)</SectionTitle>
                <Shortcut view="points" label="Voir les chaînes" />
              </div>
              <p className="text-xs text-zinc-500 mb-3">
                Regarde ces chaînes pour déclencher le bonus de présence quotidien.
              </p>
              <div className="flex gap-2">
                <Input value={newStreakChannel} onChange={e => setNewStreakChannel(e.target.value)} placeholder="Nom de la chaîne..."
                  className="bg-white/5 border-white/10 text-white placeholder:text-zinc-500"
                  onKeyDown={e => e.key === 'Enter' && addStreakChannel()} />
                <Button onClick={addStreakChannel} size="sm" className="bg-purple-600 hover:bg-purple-700 text-white">
                  <Plus className="w-4 h-4" />
                </Button>
              </div>
              <ChipList items={config.bonus.daily_streaks.channels} colorClass="bg-green-500/20 text-green-400 border-green-500/50" onRemove={removeStreakChannel} />
              <Separator className="bg-white/10 my-3" />
              <SettingRow label="Démarrage automatique" description="Lance le watch bonus au démarrage de l'app.">
                <Switch checked={config.bonus.daily_streaks.auto_start} onCheckedChange={v => updateBonus('daily_streaks', 'auto_start', v)} />
              </SettingRow>
              <SettingRow label="Mode de surveillance" description="Séquentiel : une chaîne à la fois. Parallèle : toutes simultanément.">
                <select value={config.bonus.daily_streaks.mode} onChange={e => updateBonus('daily_streaks', 'mode', e.target.value)}
                  className="bg-zinc-900 border border-white/10 text-white rounded px-2 py-1 text-sm">
                  <option value="sequential">Séquentiel</option>
                  <option value="parallel">Parallèle</option>
                </select>
              </SettingRow>
            </div>

            {/* Points Farming */}
            <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
              <div className="flex items-center justify-between mb-1">
                <SectionTitle icon={Coins}>Farming de channel points</SectionTitle>
                <Shortcut view="points" label="Voir les points" />
              </div>
              <p className="text-xs text-zinc-500 mb-3">
                Réclame automatiquement les channel points toutes les heures.
              </p>
              <div className="flex gap-2">
                <Input value={newPointsChannel} onChange={e => setNewPointsChannel(e.target.value)} placeholder="Nom de la chaîne..."
                  className="bg-white/5 border-white/10 text-white placeholder:text-zinc-500"
                  onKeyDown={e => e.key === 'Enter' && addPointsChannel()} />
                <Button onClick={addPointsChannel} size="sm" className="bg-purple-600 hover:bg-purple-700 text-white">
                  <Plus className="w-4 h-4" />
                </Button>
              </div>
              <ChipList items={config.bonus.points_farming.channels} colorClass="bg-yellow-500/20 text-yellow-400 border-yellow-500/50" onRemove={removePointsChannel} />
              <Separator className="bg-white/10 my-3" />
              <SettingRow label="Démarrage automatique" description="Lance le farming au démarrage de l'app.">
                <Switch checked={config.bonus.points_farming.auto_start} onCheckedChange={v => updateBonus('points_farming', 'auto_start', v)} />
              </SettingRow>
              <SettingRow label="Chaînes simultanées max" description="Nombre de chaînes surveillées en même temps (1–10).">
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
                <div className="text-sm font-semibold text-white">Activer la planification</div>
                <p className="text-xs text-zinc-500 mt-0.5">Suspend le minage en dehors des plages horaires définies.</p>
              </div>
              <Switch checked={sched().enabled} onCheckedChange={v => updateSched({ enabled: v })} />
            </div>

            {sched().enabled && (
              <div className="space-y-4 animate-in fade-in slide-in-from-top-1 duration-200">

                {/* Weekly schedule */}
                <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
                  <SectionTitle icon={Clock}>Plages horaires</SectionTitle>
                  <div className="space-y-1 mt-2">
                    {DAYS.map(day => {
                      const d = (sched()[day] as any) ?? { enabled: false, start_time: '08:00', end_time: '18:00' }
                      return (
                        <div key={day} className="flex flex-wrap items-center gap-3 py-2 border-b border-white/5 last:border-0">
                          <div className="flex items-center gap-2 w-28">
                            <Switch checked={d.enabled} onCheckedChange={v => updateSchedDay(day, 'enabled', v)} className="data-[state=checked]:bg-purple-500" />
                            <span className="text-xs font-medium text-zinc-300 capitalize">{DAY_LABELS[day]}</span>
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
                            <span className="text-xs text-zinc-600 italic">Toute la journée</span>
                          )}
                        </div>
                      )
                    })}
                  </div>
                </div>

                {/* Game exceptions */}
                <div className="bg-white/[0.03] border border-white/8 rounded-xl p-4">
                  <SectionTitle icon={FilterX}>Exceptions par jeu</SectionTitle>
                  <p className="text-xs text-zinc-500 mb-3">
                    Exclure définitivement un jeu du minage. Filtrage optionnel par mot-clé dans le nom de la campagne.
                  </p>
                  <div className="flex gap-2 mb-2 flex-wrap">
                    <Input value={newExcGame} onChange={e => setNewExcGame(e.target.value)} placeholder="Nom du jeu..."
                      className="bg-white/5 border-white/10 text-white placeholder:text-zinc-500 flex-1 min-w-28" />
                    <Input value={newExcKeyword} onChange={e => setNewExcKeyword(e.target.value)} placeholder="Mot-clé campagne (optionnel)"
                      className="bg-white/5 border-white/10 text-white placeholder:text-zinc-500 flex-1 min-w-32" />
                    <Button onClick={addGameException} size="sm" className="bg-red-600/80 hover:bg-red-600 text-white">
                      <Plus className="w-4 h-4" />
                    </Button>
                  </div>
                  <div className="space-y-1.5 mt-2">
                    {gameExceptions().length === 0 ? (
                      <p className="text-xs text-zinc-600 italic text-center py-3">Aucune exception configurée.</p>
                    ) : gameExceptions().map((exc, i) => (
                      <div key={i} className="flex items-center justify-between gap-2 px-3 py-2 bg-red-500/5 border border-red-500/20 rounded-lg">
                        <div className="flex items-center gap-2 min-w-0">
                          <Badge className="bg-red-500/20 text-red-400 border border-red-500/30 text-xs flex-shrink-0">Exclu</Badge>
                          <div className="min-w-0">
                            <div className="text-xs font-semibold text-white truncate">{exc.game}</div>
                            {exc.keyword && <div className="text-xs text-zinc-500">si contient "{exc.keyword}"</div>}
                            {!exc.keyword && <div className="text-xs text-zinc-500">Toutes les campagnes</div>}
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
                  <SectionTitle icon={Zap}>Exceptions automatiques</SectionTitle>
                  <p className="text-xs text-zinc-500 mb-3">Skip automatiquement les campagnes qui ne valent pas le temps investi.</p>
                  <SettingRow
                    label="Ignorer les drops courts"
                    description="Skip si la campagne dure moins de N jours (bêtas, tests limités…)."
                  >
                    <div className="flex items-center gap-1.5">
                      <Input
                        type="number"
                        value={sched().skip_short_campaigns_days ?? 3}
                        onChange={e => updateSched({ skip_short_campaigns_days: parseInt(e.target.value) || 0 })}
                        className="w-14 bg-white/5 border-white/10 text-white text-center text-xs"
                        min={0} max={30}
                      />
                      <span className="text-xs text-zinc-500">j</span>
                    </div>
                  </SettingRow>
                  <SettingRow label="Ignorer les drops expirés" description="Skip les campagnes terminées depuis plus de 24h.">
                    <Switch checked={sched().skip_expired ?? true} onCheckedChange={v => updateSched({ skip_expired: v })} />
                  </SettingRow>
                  <SettingRow label="Ignorer si déjà possédé" description="Ne mine pas un drop déjà obtenu (sauf si «Miner tout l'inventaire» est actif).">
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
