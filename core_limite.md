# Limites du cœur relevées pendant la construction de l'interface native

Points trouvés côté moteur pendant le développement de l'interface Slint (`crates/ui`), et ce qui en a été fait.

## Traités

| # | Point | Ce qui a changé dans le cœur |
|---|---|---|
| 1 | Notices ni traduisibles ni actionnables | `Notice` porte `kind` (`update`, `duplicate`, `proxy_invalid`) et `params` (`{version, current, url}`, `{account, login, peer}`, `{error}`). `message` reste le texte anglais de secours. La notice de proxy invalide a maintenant l'id `proxy_invalid` (avant : `proxy`). |
| 2 | « Aujourd'hui » compté en jour UTC | Statistiques stockées **par heure UTC** (table `hourly_stats`, migration v2). `stats` découpe les jours selon `tz_offset_minutes` (par défaut le fuseau de la machine du moteur, heure d'été comprise) et accepte `group_by: "hour"`. Les anciennes lignes journalières sont placées à midi UTC : leur jour ne change pas entre UTC−11 et UTC+11. |
| 3 | Journaux perdus après un redémarrage dans le même processus | Le journal est installé une seule fois par processus ; chaque `App` y rebranche son fichier et partage le même `LogBuffer`. |
| 4 | `App::start` laisse le moteur tourner si le port est pris | Le port de l'API est ouvert avant de démarrer le moteur ; en cas d'erreur après le démarrage, le moteur est arrêté. Test de non-régression ajouté. |
| 5 | Pas de journaux pour un moteur distant | `Backend::logs(n)` : `Local::new(engine).with_logs(app.logs.clone())` en local, `GET /api/v1/logs` en distant. |
| 7 | Solde des chaînes hors ligne jamais chargé | Le solde des chaînes hors ligne est chargé au démarrage (10 par passage, pour ne pas inonder Twitch), puis toutes les 6 h. |
| 8 | Pas de recherche de jeux | Commande `games_search {query, limit?}` → `[{id, name, slug?, box_art_url?}]`. Fonctionne sans compte connecté. Outil MCP `search_games` aussi. |
| 9 | Chaînes suivies minées par défaut | `follows: false` par défaut. Pas de migration : une base qui n'a jamais touché ce réglage arrête de miner les chaînes suivies (l'interrupteur de l'écran Chaînes le réactive). |

## Laissé de côté

| # | Point | Décision |
|---|---|---|
| 6 | Descriptions des réglages en anglais seulement | Tous les modules sont intégrés et l'interface a déjà leurs libellés français. Un futur module arrivera avec ses traductions dans l'interface. À revoir si des modules tiers apparaissent. |

## À reprendre côté interface

- **Notices** : construire le texte et le bouton depuis `kind` + `params` (garder `message` pour un `kind` inconnu).
- **Journaux** : lire `backend.logs(n)` (en local comme en distant) au lieu du `LogBuffer` de l'`App`. `session.rs` passe déjà `with_logs(app.logs.clone())`.
- **Statistiques** : déjà adapté (`present::today_key` en jour local, `tz_offset_minutes` envoyé par l'accueil et l'écran Activité).
- **Recherche de jeux** : brancher l'autocomplétion de la liste de jeux sur `games_search` (au fil de la frappe, avec un petit délai).
- **Chaînes suivies** : l'interrupteur « Miner aussi les chaînes que je suis » est maintenant désactivé par défaut.
