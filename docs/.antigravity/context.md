# Context & Decisions

## Phase 4 - Single Instance
- **Decision**: Forcer le focus sur la fenêtre principale si une 2ème instance est lancée, plutôt que d'afficher une erreur, pour améliorer l'expérience utilisateur.

## Phase 1 & 2 - System Tray & Autostart
- **Decision**: Lier `minimize_to_tray` à l'interception de `CloseRequested` sur Windows. 
- **Decision**: Utiliser l'argument `--hidden` de `tauri-plugin-autostart` et une vérification manuelle dans `setup` pour éviter les clignotements de la fenêtre au démarrage.

## Phase 6.2 - Token Refresh Automatique
- **Decision**: Au lieu de dépendre d'un timer, on utilise un intercepteur réactif : si on reçoit une erreur 401 ou un tableau `errors` GraphQL avec "unauthenticated", on déclenche le refresh.

## Phase 6.1 - Retry & Backoff
- **Decision**: Implémenter une boucle de 3 itérations avec exponential backoff plus un peu de jitter (aléatoire) pour éviter le Thundering Herd problem. La boucle englobe la requête réseau et gère aussi le rafraîchissement du token et le retry immédiat.

## Phase 3 - Notifications Natives
- **Decision**: Utilisation du plugin officiel Tauri au lieu de l'API Web Notification. Le permission check est géré dynamiquement par le plugin.

## Phase 5.1 - User Profile
- **Decision**: On utilise la query `get_user_info` existante en l'exposant via une nouvelle commande Tauri `auth::get_user_info`, puis le frontend la fetch une fois le checkAuth passé.

## Phase 5.2 - Historique des Drops Claimés
- **Decision**: Utilisation du fichier `config.json` via `tauri-plugin-store` pour stocker la liste des drops obtenus. Emission d'un événement `drop-claimed` depuis le backend pour informer le frontend en temps réel.

## Phase 6.3 - Logging structuré
- **Decision**: Remplacement des `println!` par `tauri-plugin-log` (`log::info!`) pour la robustesse et ajout d'un bouton "Open Logs Folder" dans les paramètres.

## Phase 5.3 - Statistiques & Uptime
- **Decision**: Ajout d'un timer frontend calculé depuis le backend. On transmet la valeur `started_at` via les statuts `MinerStatus` et `StreakMinerStatus` lors du lancement, et on affiche "Miner Uptime" en temps réel sur le `DashboardView`. 

## Phase 5.4 - Thème & Apparence
- **Decision**: Le thème existait déjà (Dark mode, oklch gradients, glassmorphism), l'amélioration porte sur l'UX via `framer-motion` : Ajout d'une `AnimatePresence` avec des transitions (`opacity`, `y`) fluides lors de la navigation entre les vues principales.
