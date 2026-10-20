# Brief pour l'interface (desktop + Android)

Le backend est terminé et testé. Ce document dit **quoi construire** et **avec quelles données**. Le contrat exact est dans [API.md](API.md).

## Pile recommandée

- **Slint** (Rust natif, rendu GPU/Skia, desktop + Android, langage déclaratif `.slint`). Alternatives : `egui` (plus simple, moins joli) ou `iced`.
- Démarrage : `tm_app::App::start(...)` puis `tm_client::Local::new(app.engine.clone())`.
- Toute l'UI lit `backend.state()` (`watch::Receiver<Arc<State>>`) et agit par `backend.execute(Command)`. **Aucune autre source de données.**
- **Dès le début** : un écran « Se connecter à un serveur » qui crée un `tm_client::Remote` au lieu de `Local`. Le reste de l'UI ne change pas. En mode distant, l'app ne démarre **aucun moteur local**. Pour un secours automatique, voir [SERVER.md](SERVER.md) (rôle `secondary`).
- Si `App::start` échoue avec `EngineError::Lock`, un autre moteur (le daemon) utilise déjà ce dossier : proposer de s'y connecter en distant.

## Écrans

| Écran | Données (`State`, voir API.md) | Actions (`Command`) |
|---|---|---|
| **Accueil / tableau de bord** | `view::summary()` (ou `Command::Summary`) : totaux de session, comptes, chaînes regardées, drop en cours (`progress`, `eta`), `watching_allowed`/`paused` | bouton Pause/Reprise : `settings_set engine {"paused": bool}` |
| **Connexion** | `logins[]` (code + URL), événement `login_finished` | `login_start`, `login_cancel`. Afficher le code en très grand, ouvrir l'URL, attendre `login_finished` |
| **Comptes** | `accounts[].info`, `.status` (`auth_required` ⇒ bouton « Se reconnecter » ; `standby` ⇒ « Miné par <peer> »), `.modules` (santé de chaque module) | `account_set_enabled`, `account_remove`, `login_start` |
| **Drops** | `accounts[].campaigns` (jeu, image, fin, `linked`, `drops[]` avec progression), `drop_target`, `module_data.drops.skipped_campaigns` | jeux voulus : `settings_set module:drops {"games": [...]}` ; `module_call drops refresh` / `claim_all` |
| **Chaînes / points** | `accounts[].channels` (avatar, live, jeu, spectateurs, solde, `session_points`), `watching` | `module_call points add_channel/remove_channel {login}` ; ordre : `settings_set ... {"channels": [...]}` |
| **Historique** | `history` (paginé par `before_id`), types filtrables | — |
| **Statistiques** | `stats` par jour / par chaîne (points, minutes, drops) | graphique simple |
| **Réglages** | `modules_list` → `settings_schema` + `settings_get` (`effective`, `defaults`, `schema`) | `settings_set` (merge). **Générer les formulaires depuis le JSON Schema** : booléens, nombres, enums, listes de chaînes |
| **Journaux** | `GET /api/v1/logs` ou `app.logs.recent(n)` / `.subscribe()` | — |
| **À propos / mises à jour** | `notices.update`, `version`, `manifest_revision` | `update_check`, `manifest_refresh`, `backup` |

`notices` (map) : bandeau en haut de l'app (`level` info/warning/error), fermeture avec `notice_dismiss`.

## Événements utiles pour l'animation et les toasts (`backend.events()`)

- `points_earned` (raison `CLAIM` = coffre, `WATCH_STREAK`, `RAID`…) → petit toast « +50 ».
- `drop_claimed` → toast/notification système.
- `account_status_changed{auth_required}` → alerte.
- `watch_started` / `watch_stopped` → transition dans la liste « en cours ».

Ne pas recalculer l'état à partir des événements : le miroir `state()` est déjà à jour. Les événements ne servent qu'à l'effet visuel.

## Plateforme

- **Desktop** : icône dans la barre système (tooltip = `summary`), démarrage avec la session (crate `auto-launch`), fermer la fenêtre = réduire dans la barre. Le moteur continue de tourner.
- **Android** : voir [ANDROID.md](ANDROID.md) (service de premier plan `specialUse`, exemption batterie, guide OneUI « Applications jamais en veille », notification avec progression → Now Bar sur OneUI 8).
- Données : ne rien écrire hors du dossier de `Paths` (la base contient déjà tout).

## Pièges

- `execute` est asynchrone et peut prendre jusqu'à plusieurs secondes (`login_start`, `module_call`) : ne pas bloquer le thread UI.
- `campaigns` peut contenir des dizaines d'entrées : virtualiser les listes.
- `channels[].points` peut être `null` (inconnu) : afficher « — », pas 0.
- `linked: null` = inconnu, pas « non lié ».
- Les identifiants (comptes, chaînes, campagnes) sont des chaînes de caractères, pas des nombres.
