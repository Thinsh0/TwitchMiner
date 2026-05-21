# Walkthrough (Log)

## Session Init
- Plan validé par l'utilisateur.
- Initialisation des documents de suivi dans `.antigravity/`.

## Phase 4 - Single Instance
- Plugin `tauri-plugin-single-instance` ajouté via `cargo add`.
- Initialisation du plugin dans `lib.rs` pour focuser la fenêtre principale si une 2ème instance est lancée.

## Phase 1 - System Tray & Minimize to Tray
- Menu contextuel du tray ajouté dans `lib.rs` (Show, Hide, Quit).
- Logique de click sur le tray pour restaurer l'app.
- Interception de `WindowEvent::CloseRequested` pour cacher la fenêtre au lieu de la fermer, en lisant la config `minimize_to_tray` du store.
- Interface `Settings.tsx` mise à jour avec les toggles "Minimize to tray on close" et "Show tray icon".
- Types TS et struct Rust `Settings` mis à jour avec `minimize_to_tray` et `show_tray_icon` (default: true).

## Phase 2 - Autostart avec l'OS
- Plugin `tauri-plugin-autostart` ajouté via `npm run tauri add autostart`.
- Initialisation du plugin dans `lib.rs` avec l'argument `--hidden`.
- Logique ajoutée dans `lib.rs` pour cacher la fenêtre principale au démarrage si l'argument `--hidden` est présent.
- Interface `Settings.tsx` mise à jour avec le toggle "Launch at system startup" gérant le status natif de l'autostart.

## Phase 6.2 - Token Refresh Automatique
- Fonction `refresh_access_token` ajoutée dans `auth.rs` pour appeler l'API `/oauth2/token` avec le `refresh_token` stocké.
- Mise à jour de `api.rs` : `post` et `post_persisted` interceptent désormais les réponses HTTP 401 et les JSON contenant une erreur d'authentification pour déclencher un refresh du token avant de retenter la requête.

## Phase 6.1 - Retry & Backoff
- Implémentation d'une boucle de 3 tentatives avec exponential backoff dans `post` et `post_persisted` de `api.rs`.
- Le token est rafraîchi dynamiquement à l'intérieur de la boucle en cas d'erreur 401, et la requête est relancée.

## Phase 3 - Notifications Natives
- Plugin `tauri-plugin-notification` installé via le CLI.
- Remplacement des appels Web `Notification` par `@tauri-apps/plugin-notification` dans `src/activity.ts`.
- Implémentation de la demande de permission native.

## Phase 5.1 - User Profile dans la Sidebar
- Ajout de la commande `get_user_info` dans `auth.rs` et `lib.rs`.
- Mise à jour de `api.ts` pour inclure la méthode `getUserInfo`.
- Chargement du profil utilisateur dans `App.tsx` et affichage dans le footer de la sidebar.

## Phase 5.2 - Historique des Drops Claimés
- Mise à jour de `miner.rs` pour écouter et stocker les drops complétés dans `config.json` via le `tauri-plugin-store`.
- Ajout de l'événement `drop-claimed` depuis le backend.
- Implémentation du store frontend (`src/store.ts`) pour récupérer l'historique et écouter les nouveaux claims.
- Création du composant `HistoryView.tsx` affichant la liste des drops réclamés avec leur nom et image.
- Intégration de l'onglet "History" dans le menu principal de l'application.

## Phase 6.3 - Logging structuré
- Installation du plugin `tauri-plugin-log`.
- Remplacement de tous les appels standards `println!` par les macros structurées de type `log::info!` dans l'ensemble des fichiers `.rs`.
- Ajout de la dépendance `chrono` pour manipuler le format des dates au besoin et du `started_at`.
- Intégration du bouton "Open Logs Folder" dans la vue "Settings" avec ouverture du dossier via `@tauri-apps/plugin-shell` et `appLogDir()`.

## Phase 5.3 - Statistiques & Uptime
- Ajout des variables `started_at` dans `models/mod.rs` pour `MinerStatus` et `StreakMinerStatus`.
- Mise à jour des valeurs `started_at` avec le temps UTC (`chrono::Utc`) lors du lancement des mineurs.
- Côté frontend, création d'une logique de timer dans le `DashboardView.tsx` pour calculer et afficher l'Uptime (HH:MM:SS) en direct en fusionnant l'état du Drop Miner et du Streak Miner.
- Ajout du nombre total de Drops réclamés sur le Dashboard depuis l'état global Zustand.

## Phase 5.4 - Thème & Apparence
- L'apparence moderne (glassmorphism/gradients) était déjà en place. L'amélioration a visé l'interactivité.
- Installation de `framer-motion` via npm.
- Mise en place d'une transition de vue (Page Transition) fluide dans `App.tsx` en enveloppant le `renderView` d'un composant `AnimatePresence` et d'une `motion.div`.

## Drop Miner Priority Fix
- Résolution du problème où le mineur ciblait tout l'inventaire au lieu des jeux prioritaires.
- Ajout du paramètre `mine_all_inventory` dans les modèles (`mod.rs`, `types.ts`).
- Modification de `build_mineable_candidates` dans `miner.rs` pour filtrer strictement selon les `priority_games` (par défaut).
- Ajout d'un bouton de bascule "Mine all inventory drops" dans `Settings.tsx` pour restaurer l'ancien comportement global si souhaité.
- Ajout d'une alerte bloquante si `mine_all_inventory` est `false` mais qu'aucun jeu prioritaire n'est défini.

## Restructuring & Android Porting
- Création d'une copie de sauvegarde / commit de sécurité (`b7aceb6`) pour sécuriser le code fonctionnel.
- Élaboration du plan d'architecture monorepo : `core` (Rust), `frontend` (responsive React), `desktop` (Tauri Desktop), `mobile` (Tauri Android avec Service et démarrage auto).
- Ajout de l'assistant de configuration mobile (guides de marques comme `dontkillmyapp.com` et indicateurs de permissions vert/rouge interactifs).
- Restructuration physique effectuée :
  - Création du crate partagé `core/` (package `twitch-miner-core`) contenant l'API, les modèles, la config et les mineurs.
  - Déplacement des fichiers frontend (React/Vite/Tailwind) dans `frontend/`.
  - Déplacement de l'application desktop dans `desktop/` avec mise à jour des dépendances Cargo et configurations de chemins Tauri.
  - Vérification de la compilation réussie du package Desktop et du package Core.
- Configuration du module mobile `mobile/src-tauri` :
  - Résolution des erreurs macro-commandes Tauri (`__cmd__check_battery_optimizations` et `__cmd__request_battery_optimization` déclarées en `pub` dans le même fichier `lib.rs`) en retirant le mot-clé `pub` et en renommant les variables inutilisées en `_app` pour éviter les warnings.
  - Configuration de `AndroidManifest.xml` avec les permissions `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_DATA_SYNC`, `POST_NOTIFICATIONS`, `RECEIVE_BOOT_COMPLETED` et `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`.
  - Déclaration de `tauri_plugin_background_service.BackgroundService` avec `foregroundServiceType="dataSync"` et `stopWithTask="false"`.
  - Validation de la compilation réussie du package mobile.

