import { useAppStore } from '../store'

export const translations = {
  FR: {
    common: {
      active: 'Actif',
      idle: 'Inactif',
      running: 'En cours',
      stopped: 'Arrêté',
      error: 'Erreur',
      loading: 'Chargement...',
      cancel: 'Annuler',
      confirm: 'Confirmer',
      logout: 'Déconnexion',
      save: 'Sauvegarder',
      saved: 'Sauvegardé',
      refresh: 'Actualiser',
      none: 'Aucun',
      minutes: 'minutes',
      days: 'jours',
      sec: 's',
      openLogs: 'Ouvrir les logs',
      reset: 'Réinitialiser la configuration',
      ready: 'Prêt',
      claimed: 'Réclamé',
      claim: 'Réclamer',
      watching: 'Surveillance'
    },
    nav: {
      dashboard: 'Tableau de bord',
      drops: 'Drops',
      points: 'Channel Points',
      history: 'Historique',
      settings: 'Configuration'
    },
    dashboard: {
      welcome: 'Bienvenue,',
      uptime: 'Temps d\'activité',
      claimed: 'Drops réclamés',
      points: 'Points cumulés',
      status: 'Statut du mineur',
      currentlyMining: 'Mining en cours',
      on: 'sur',
      stopMiner: 'Arrêter le mineur',
      startMiner: 'Démarrer le mineur',
      activeFarming: 'Farming actif',
      noActiveFarming: 'Farming inactif',
      autoclaimOn: 'Autoclaim ON',
      autoclaimOff: 'Autoclaim OFF'
    },
    drops: {
      title: 'Campagnes de drops',
      subtitle: 'Drops disponibles pour vos jeux ciblés',
      currentlyMining: 'Actuellement miné',
      globalSort: 'Tri global',
      sortAlpha: 'Alphabétique',
      sortDate: 'Date d\'expiration',
      sortDuration: 'Durée totale',
      asc: 'Ascendant',
      desc: 'Descendant',
      campaigns: 'campagne(s)',
      completedCampaign: 'Terminé',
      readyCampaign: 'Prêt à réclamer',
      activeCampaign: 'Actif',
      availableCampaign: 'Disponible',
      loadingCampaigns: 'Chargement des campagnes...',
      noGames: 'Aucun jeu configuré.',
      noGamesHint: 'Allez dans Paramètres → Drops pour ajouter des jeux.',
      noMatch: 'Aucune campagne ne correspond à votre recherche.',
      searchPlaceholder: 'Rechercher par jeu ou par nom de campagne...',
      durationReq: 'Durée requise'
    },
    points: {
      title: 'Farming de Channel Points',
      subtitle: 'Surveillez vos streamers pour obtenir des points',
      totalPoints: 'Total des points',
      successPings: 'Pings réussis',
      cycles: 'Cycles effectués',
      claimsAvail: 'Réclamations dispo.',
      monitoredChannels: 'Chaînes surveillées',
      channelsCount: 'chaînes',
      noChannels: 'Aucune chaîne configurée. Allez dans Paramètres → Channel Points pour en ajouter.',
      startFarming: 'Démarrer le farming',
      stopFarming: 'Arrêter le farming',
      statusWatching: 'Surveillance',
      statusOffline: 'Hors ligne',
      statusError: 'Erreur',
      statusChecking: 'Vérification...',
      statusClaimed: 'Points réclamés',
      statusClaimFailed: 'Échec de réclamation'
    },
    history: {
      title: 'Historique des réclamations',
      subtitle: 'Historique des drops que vous avez réclamés avec succès.',
      totalClaimed: 'Total réclamés',
      thisWeek: 'Cette semaine',
      uniqueDrops: 'Drops uniques',
      claimedDrops: 'Drops obtenus',
      recentFirst: 'Les plus récents sont affichés en premier.',
      noClaimed: 'Aucun drop réclamé pour le moment.',
      minRequired: 'min requises',
      claimedAt: 'Réclamé le'
    },
    settings: {
      title: 'Configuration',
      tabs: {
        general: 'Général',
        drops: 'Drops Mining',
        points: 'Channel Points',
        schedule: 'Planification'
      },
      general: {
        notifications: 'Notifications',
        notificationsDesc: 'Événements qui déclenchent une notification système.',
        enableNotifications: 'Activer les notifications',
        enableNotificationsDesc: 'Active/désactive toutes les notifications de l\'app.',
        newActiveDrop: 'Nouveau drop actif',
        newActiveDropDesc: 'Quand le mineur commence à accumuler un nouveau drop.',
        dropClaimed: 'Drop réclamé',
        dropClaimedDesc: 'À chaque drop obtenu avec succès.',
        campaignFinish: 'Campagne terminée',
        campaignFinishDesc: 'Quand tous les drops d\'une campagne sont complétés.',
        miner: 'Mineur',
        checkInterval: 'Intervalle de vérification',
        checkIntervalDesc: 'Fréquence à laquelle le mineur interroge Twitch (10–300 s).',
        language: 'Langue',
        languageDesc: 'Langue de l\'interface.',
        system: 'Système & Tray',
        autostart: 'Lancement au démarrage',
        autostartDesc: 'Démarre l\'app automatiquement au démarrage de l\'ordinateur.',
        minToTray: 'Minimiser dans la zone de notification',
        minToTrayDesc: 'Reste actif en arrière-plan à la fermeture.',
        trayIcon: 'Icône dans le tray',
        trayIconDesc: 'Affiche l\'icône dans la barre des tâches système.',
        logsFolder: 'Dossier de logs',
        dangerZone: 'Zone de danger',
        dangerZoneDesc: 'Efface toutes vos configurations et remet les valeurs par défaut. Irréversible.',
        resetButton: 'Réinitialiser la configuration',
        resetConfirmTitle: 'Réinitialiser la configuration ?',
        resetConfirmDesc: 'Toutes vos listes de jeux, streamers, chaînes et préférences seront effacées. Irréversible.',
        resetConfirmBtn: 'Confirmer la réinitialisation'
      },
      drops: {
        games: 'Jeux ciblés',
        gamesDesc: 'Le mineur cherche des drops uniquement pour ces jeux. Sans jeu configuré, aucun drop n\'est miné.',
        gamesPlaceholder: 'Rechercher un jeu...',
        streamers: 'Streamers ciblés',
        streamersDesc: 'Surveille ces chaînes en priorité lorsqu\'elles diffusent un jeu ciblé.',
        streamersPlaceholder: 'Nom du streamer...',
        behavior: 'Comportement',
        autoclaim: 'Autoclaim automatique',
        autoclaimDesc: 'Réclame les drops terminés sans action manuelle dès qu\'ils sont disponibles.',
        autoStartDrops: 'Lancement automatique',
        autoStartDropsDesc: 'Démarre le mineur de drops dès l\'ouverture de l\'app.',
        mineAll: 'Miner tout l\'inventaire',
        mineAllDesc: 'Mine même les drops déjà possédés.'
      },
      points: {
        dailyBonus: 'Watch Bonus (claim quotidien)',
        dailyBonusDesc: 'Regarde ces chaînes pour déclencher le bonus de présence quotidien.',
        channelPlaceholder: 'Nom de la chaîne...',
        autoStartPoints: 'Démarrage automatique',
        autoStartPointsDesc: 'Lance le watch bonus au démarrage de l\'app.',
        monitoredMode: 'Mode de surveillance',
        monitoredModeDesc: 'Séquentiel : une chaîne à la fois. Parallèle : toutes simultanément.',
        sequential: 'Séquentiel',
        parallel: 'Parallèle',
        pointsFarming: 'Farming de channel points',
        pointsFarmingDesc: 'Réclame automatiquement les channel points toutes les heures.',
        autoStartFarming: 'Démarrage automatique',
        maxConcurrent: 'Chaînes simultanées max',
        maxConcurrentDesc: 'Nombre de chaînes surveillées en même temps (1–10).'
      },
      schedule: {
        enableSchedule: 'Activer la planification',
        enableScheduleDesc: 'Suspend le minage en dehors des plages horaires définies.',
        scheduleTimes: 'Plages horaires',
        allDay: 'Toute la journée',
        gameExceptions: 'Exceptions par jeu',
        gameExceptionsDesc: 'Exclure définitivement un jeu du minage. Filtrage optionnel par mot-clé dans le nom de la campagne.',
        gameNamePlaceholder: 'Nom du jeu...',
        campaignKeywordPlaceholder: 'Mot-clé campagne (optionnel)',
        noExceptions: 'Aucune exception configurée.',
        autoExceptions: 'Exceptions automatiques',
        autoExceptionsDesc: 'Ignorer automatiquement les campagnes qui ne valent pas le temps investi.',
        skipShort: 'Ignorer les drops courts',
        skipShortDesc: 'Ignorer si la campagne dure moins de N jours (bêtas, tests limités…).',
        skipExpired: 'Ignorer les drops expirés',
        skipExpiredDesc: 'Ignorer les campagnes terminées depuis plus de 24h.',
        skipOwned: 'Ignorer si déjà possédé',
        skipOwnedDesc: 'Ne mine pas un drop déjà obtenu (sauf si «Miner tout l\'inventaire» est actif).'
      }
    }
  },
  EN: {
    common: {
      active: 'Active',
      idle: 'Idle',
      running: 'Running',
      stopped: 'Stopped',
      error: 'Error',
      loading: 'Loading...',
      cancel: 'Cancel',
      confirm: 'Confirm',
      logout: 'Logout',
      save: 'Save',
      saved: 'Saved',
      refresh: 'Refresh',
      none: 'None',
      minutes: 'minutes',
      days: 'days',
      sec: 's',
      openLogs: 'Open logs',
      reset: 'Reset config',
      ready: 'Ready',
      claimed: 'Claimed',
      claim: 'Claim',
      watching: 'Watching'
    },
    nav: {
      dashboard: 'Dashboard',
      drops: 'Drops',
      points: 'Channel Points',
      history: 'History',
      settings: 'Settings'
    },
    dashboard: {
      welcome: 'Welcome,',
      uptime: 'Uptime',
      claimed: 'Claimed drops',
      points: 'Farmed points',
      status: 'Miner status',
      currentlyMining: 'Currently mining',
      on: 'on',
      stopMiner: 'Stop miner',
      startMiner: 'Start miner',
      activeFarming: 'Farming active',
      noActiveFarming: 'Farming inactive',
      autoclaimOn: 'Autoclaim ON',
      autoclaimOff: 'Autoclaim OFF'
    },
    drops: {
      title: 'Drop Campaigns',
      subtitle: 'Available drops for your selected games',
      currentlyMining: 'Currently mining',
      globalSort: 'Global sort',
      sortAlpha: 'Alphabetical',
      sortDate: 'Expiration date',
      sortDuration: 'Total duration',
      asc: 'Ascending',
      desc: 'Descending',
      campaigns: 'campaign(s)',
      completedCampaign: 'Completed',
      readyCampaign: 'Ready to claim',
      activeCampaign: 'Active',
      availableCampaign: 'Available',
      loadingCampaigns: 'Loading campaigns...',
      noGames: 'No games configured.',
      noGamesHint: 'Go to Settings → Drops to add games.',
      noMatch: 'No campaigns match your search.',
      searchPlaceholder: 'Search by game or campaign name...',
      durationReq: 'Required duration'
    },
    points: {
      title: 'Channel Points Farming',
      subtitle: 'Watch your streamers to get points',
      totalPoints: 'Total points',
      successPings: 'Successful pings',
      cycles: 'Cycles',
      claimsAvail: 'Claims available',
      monitoredChannels: 'Monitored channels',
      channelsCount: 'channels',
      noChannels: 'No channels configured. Go to Settings → Channel Points to add some.',
      startFarming: 'Start farming',
      stopFarming: 'Stop farming',
      statusWatching: 'Watching',
      statusOffline: 'Offline',
      statusError: 'Error',
      statusChecking: 'Checking...',
      statusClaimed: 'Points claimed',
      statusClaimFailed: 'Claim failed'
    },
    history: {
      title: 'Claim History',
      subtitle: 'A history of drops you have successfully claimed.',
      totalClaimed: 'Total claimed',
      thisWeek: 'This week',
      uniqueDrops: 'Unique drops',
      claimedDrops: 'Claimed drops',
      recentFirst: 'Recently claimed drops are shown first.',
      noClaimed: 'No drops claimed yet.',
      minRequired: 'min required',
      claimedAt: 'Claimed on'
    },
    settings: {
      title: 'Configuration',
      tabs: {
        general: 'General',
        drops: 'Drops Mining',
        points: 'Channel Points',
        schedule: 'Schedule'
      },
      general: {
        notifications: 'Notifications',
        notificationsDesc: 'System notifications triggered by miner events.',
        enableNotifications: 'Enable notifications',
        enableNotificationsDesc: 'Enable/disable all system notifications.',
        newActiveDrop: 'New active drop',
        newActiveDropDesc: 'Notify when miner starts accumulating a new drop.',
        dropClaimed: 'Drop claimed',
        dropClaimedDesc: 'Notify when a drop is successfully claimed.',
        campaignFinish: 'Campaign finished',
        campaignFinishDesc: 'Notify when all drops of a campaign are completed.',
        miner: 'Miner',
        checkInterval: 'Check interval',
        checkIntervalDesc: 'Frequency at which the miner checks Twitch (10–300 s).',
        language: 'Language',
        languageDesc: 'Interface language.',
        system: 'System & Tray',
        autostart: 'Start at boot',
        autostartDesc: 'Start app automatically when the computer starts up.',
        minToTray: 'Minimize to tray',
        minToTrayDesc: 'Minimize the app to system tray when closed.',
        trayIcon: 'Tray icon',
        trayIconDesc: 'Show icon in system tray.',
        logsFolder: 'Logs folder',
        dangerZone: 'Danger Zone',
        dangerZoneDesc: 'Erase all your settings and reset to defaults. Irreversible.',
        resetButton: 'Reset configuration',
        resetConfirmTitle: 'Reset configuration?',
        resetConfirmDesc: 'All your game lists, streamers, channels, and preferences will be cleared. Irreversible.',
        resetConfirmBtn: 'Confirm reset'
      },
      drops: {
        games: 'Targeted games',
        gamesDesc: 'The miner will only search for drops of these games. If none, no drops will be mined.',
        gamesPlaceholder: 'Search for a game...',
        streamers: 'Targeted streamers',
        streamersDesc: 'Watch these channels with priority when they stream a targeted game.',
        streamersPlaceholder: 'Streamer name...',
        behavior: 'Behavior',
        autoclaim: 'Automatic autoclaim',
        autoclaimDesc: 'Automatically claims drops when they are completed.',
        autoStartDrops: 'Auto start drops',
        autoStartDropsDesc: 'Start drops miner automatically when app opens.',
        mineAll: 'Mine all inventory',
        mineAllDesc: 'Mine even already owned drops.'
      },
      points: {
        dailyBonus: 'Watch Bonus (daily claim)',
        dailyBonusDesc: 'Watch these channels to trigger the daily watch streak bonus.',
        channelPlaceholder: 'Channel name...',
        autoStartPoints: 'Auto start points',
        autoStartPointsDesc: 'Start daily streak watching when app opens.',
        monitoredMode: 'Monitored mode',
        monitoredModeDesc: 'Sequential: one channel at a time. Parallel: all channels simultaneously.',
        sequential: 'Sequential',
        parallel: 'Parallel',
        pointsFarming: 'Channel points farming',
        pointsFarmingDesc: 'Automatically claim channel points chest every hour.',
        autoStartFarming: 'Auto start farming',
        maxConcurrent: 'Max concurrent channels',
        maxConcurrentDesc: 'Number of channels watched at the same time (1–10).'
      },
      schedule: {
        enableSchedule: 'Enable schedule',
        enableScheduleDesc: 'Suspend mining outside defined timeframes.',
        scheduleTimes: 'Time schedule',
        allDay: 'All day long',
        gameExceptions: 'Exceptions by game',
        gameExceptionsDesc: 'Permanently exclude a game from mining. Optional: filter by campaign keyword.',
        gameNamePlaceholder: 'Game name...',
        campaignKeywordPlaceholder: 'Campaign keyword (optional)',
        noExceptions: 'No exceptions configured.',
        autoExceptions: 'Auto exceptions',
        autoExceptionsDesc: 'Automatically skip campaigns not worth the mining time.',
        skipShort: 'Skip short drops',
        skipShortDesc: 'Skip if the campaign duration is less than N days (betas, tests...).',
        skipExpired: 'Skip expired drops',
        skipExpiredDesc: 'Skip campaigns completed more than 24 hours ago.',
        skipOwned: 'Skip already owned',
        skipOwnedDesc: 'Do not mine a drop already obtained (unless "Mine all inventory" is active).'
      }
    }
  }
}

export function useTranslation() {
  const language = useAppStore((state) => state.language) || 'EN'
  
  // Normalize language name (in case it is "FR" or "EN" or has emoji prefixes)
  let lang = 'EN'
  if (language.includes('FR') || language.includes('Français')) {
    lang = 'FR'
  }

  const t = (key: string, fallback?: string): string => {
    const keys = key.split('.')
    let obj: any = translations[lang as keyof typeof translations] || translations['EN']
    for (const k of keys) {
      if (obj && typeof obj === 'object' && k in obj) {
        obj = obj[k]
      } else {
        // Fallback to EN
        let enObj: any = translations['EN']
        for (const enK of keys) {
          if (enObj && typeof enObj === 'object' && enK in enObj) {
            enObj = enObj[enK]
          } else {
            return fallback || key
          }
        }
        return enObj
      }
    }
    return typeof obj === 'string' ? obj : (fallback || key)
  }

  return { t, lang }
}
