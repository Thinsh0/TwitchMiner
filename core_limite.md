# Limites du cœur relevées pendant la construction de l'interface native

Points trouvés côté moteur (`tm-core`, `tm-app`, modules) pendant le développement de l'interface Slint (`crates/ui`). L'interface ne les contourne pas : chaque point attend une décision et un changement côté cœur, en respectant les règles de `CLAUDE.md`.

## 1. Les notices ne sont ni traduisibles ni actionnables

`Notice { id, level, message, at }` ne porte qu'un texte libre en anglais (ex. « TwitchMiner 0.2.0 is available. »). L'interface ne peut donc pas :
- l'afficher en français (ou dans une autre langue) ;
- proposer l'action qui va avec (« Télécharger » pour une mise à jour, « Voir le pair » pour un doublon…).

**Proposition** : ajouter à `Notice` deux champs optionnels, rétrocompatibles :
- `kind` : type stable (`update`, `duplicate`, `manifest`…) ;
- `params` : objet JSON (`{"version": "0.2.0", "url": "…"}`, `{"account": "…", "peer": "…"}`).

`message` reste comme texte de secours. À documenter dans `docs/API.md`.

## 2. « Aujourd'hui » compte en jour UTC

`daily_stats.day` est calculé avec `env.ts.format("%Y-%m-%d")`, donc en UTC (`crates/core/src/db.rs`). En France, le compteur « Aujourd'hui » de l'accueil repasse à zéro vers 1 h ou 2 h du matin, et l'historique par jour décale les sessions nocturnes.

**Proposition** : ranger les statistiques par jour local, ou stocker l'heure UTC et laisser `stats` grouper selon un fuseau passé en paramètre (`tz_offset_minutes`). Attention aux données déjà enregistrées (migration ou simple décalage accepté).

## 3. Journaux du moteur non capturés après un redémarrage dans le même processus

`tm_core::logging::init` utilise `try_init` : si l'interface arrête puis redémarre un moteur local dans le même processus (passage local → distant → local), le premier abonné global reste en place. Conséquences pour le second `App` :
- son `LogBuffer` ne reçoit rien : les journaux affichés dans l'interface sont vides ;
- l'écrivain de fichier du premier abonné a été arrêté avec le premier `App` (son `WorkerGuard` est détruit) : plus rien n'est écrit dans `logs/` jusqu'au redémarrage du processus.

Ce cas arrive aussi quand on active ou change l'option « Accès à distance (API) » des Réglages avancés : l'interface redémarre le moteur local pour l'appliquer.

**Proposition** : initialiser le journal une seule fois par processus (`OnceLock`) et partager le même `LogBuffer` entre les `App` successifs.

## 4. `App::start` laisse le moteur tourner si le port de l'API est pris

Dans `crates/app/src/lib.rs`, `App::start` démarre le moteur (`Engine::start`, qui prend le verrou du dossier de données et lance les comptes) **avant** d'ouvrir le port de l'API (`TcpListener::bind(...).await?`). Si le port est déjà utilisé (daemon lancé, autre programme), `App::start` renvoie une erreur, mais le moteur déjà démarré n'est jamais arrêté (`engine.shutdown()` n'est pas appelé) : ses tâches continuent et le verrou du dossier peut rester pris jusqu'à la fin du processus.

Côté interface : l'option « Accès réseau (API) » des Réglages avancés peut déclencher ce cas. L'erreur s'affiche, mais un nouvel essai en local peut ensuite échouer avec « dossier déjà utilisé ».

**Proposition** : ouvrir le port avant `Engine::start`, ou appeler `engine.shutdown().await` avant de renvoyer l'erreur.

## 5. Pas de journaux pour un moteur distant

`tm_client::Backend` n'expose que l'état, les événements, le lien et `execute`. Le serveur a pourtant `GET /api/v1/logs` et le flux WebSocket `?logs=1`, mais `tm_client::Remote` ne s'en sert pas. Les Réglages avancés n'affichent donc le journal que pour un moteur local ; en distant, ils renvoient vers la machine du moteur.

**Proposition** : ajouter à `Backend` une méthode `logs(n) -> Vec<LogLine>` (et éventuellement un flux), implémentée par `Local` (le `LogBuffer` de l'`App`) et par `Remote` (`GET /api/v1/logs`, ou le WebSocket avec `?logs=1`).

## 6. Descriptions des réglages en anglais uniquement

Les JSON Schema (`settings_schema`, `settings_get(...).schema`) tirent leurs `description` des commentaires Rust, en anglais. L'interface a ses propres libellés et aides en français pour les réglages qu'elle connaît. Pour un module ajouté plus tard, elle affiche le nom de la clé et la description anglaise.

**Proposition** : permettre aux modules de fournir des libellés traduits, par exemple un champ `title` par réglage (`#[schemars(title = "…")]`) et un dictionnaire de traductions publié avec `modules_list` (`{"fr": {"refresh_minutes": {"title", "description"}}}`).

## 7. Solde de points inconnu transmis comme 0

`Gql::channel_points_context` (`crates/twitch/src/api.rs`) fait `balance: u(&cp, "balance").unwrap_or(0)` : quand Twitch ne renvoie pas `community.channel.self.communityPoints` (points de chaîne désactivés, réponse partielle, `self` nul…), le module `points` émet `PointsBalance { balance: 0 }`. L'état contient alors `points: 0` au lieu de `null`, et l'interface affiche « 0 » à la place de « — » : l'utilisateur croit avoir perdu ses points (constaté en test).

**Proposition** : `PointsContext.balance: Option<u64>`, et ne pas émettre `PointsBalance` (ou émettre `null`) quand le solde est absent. Vérifier aussi que le hash de `ChannelPointsContext` du manifeste renvoie bien `communityPoints`.

## 8. Solde des chaînes hors ligne jamais chargé

`PointsModule::refresh_contexts` ne charge le solde que des chaînes en direct (`if ch.stream.is_none() … continue`). Une chaîne hors ligne depuis le démarrage du moteur reste à `points: null` (« — » dans l'écran Chaînes), même si l'utilisateur y a des points.

**Proposition** : charger une fois le solde de chaque chaîne suivie au démarrage (et toutes les quelques heures), même hors ligne : `ChannelPointsContext` fonctionne aussi pour une chaîne hors ligne.

## 9. Pas de recherche de jeux

Pour ajouter un jeu à la liste des drops, l'interface ne peut proposer que les jeux qu'elle connaît déjà : ceux des campagnes (`accounts[].campaigns[].game`) et des chaînes en direct suivies. Un jeu sans campagne en cours doit être tapé exactement.

**Proposition** : une commande `games_search { query }` (GQL `SearchCategories` ou équivalent, hash dans `manifest/twitch.json`) qui renvoie `[{id, name, box_art_url}]`, utilisable pour l'autocomplétion. À refuser ou non au jeton agent selon `access.rs` (trafic sortant).
