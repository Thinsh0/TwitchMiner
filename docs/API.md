# API : le contrat pour les interfaces

Toute interface (desktop, Android, web, bot…) parle au moteur avec **trois primitives** :

| Primitive | En Rust (in-process) | En réseau (`tm-server`) |
|---|---|---|
| Lire l'état complet | `engine.store().snapshot()` | `GET /api/v1/state` ou 1er message WS |
| Suivre les changements | `engine.store().subscribe()` | messages `event` du WebSocket |
| Agir | `engine.execute(Command)` | `POST /api/v1/command` ou message WS |

Les types sont les mêmes des deux côtés : `tm_core::{State, Envelope, Event, Command, ApiError}`. Une interface Rust peut désérialiser directement le JSON réseau dans ces types.

## Pour une interface Rust : `tm-client`

Le plus simple est de ne pas gérer tout ça à la main :

```rust
use tm_client::{Backend, Local, Remote};
let app = tm_app::App::start(tm_app::AppOptions { log_stdout: false, ..Default::default() }).await?;
let backend: Arc<dyn Backend> = Arc::new(Local::new(app.engine.clone()).with_logs(app.logs.clone()));
// ou bien, pour piloter un daemon distant (PC, serveur, téléphone) :
// let backend: Arc<dyn Backend> = Arc::new(Remote::connect("http://192.168.1.10:7878", &token).await?);

let mut state = backend.state();          // watch::Receiver<Arc<State>>, toujours à jour
let mut link = backend.link();            // Connecting / Connected / Disconnected (Remote)
let mut events = backend.events();        // flux brut, pour toasts et animations
backend.execute(Command::Summary).await?; // mêmes commandes partout
backend.logs(200).await?;                 // journaux du moteur, local ou distant (à relire pendant l'affichage)
```

L'interface est identique, que le moteur tourne dans le même processus ou sur une autre machine. Les rafales d'événements sont regroupées (publication au plus toutes les 50 ms).

## Synchronisation sans trou

```rust
let (mut state, mut rx) = engine.store().snapshot_and_subscribe();
while let Ok(env) = rx.recv().await {
    if env.seq <= state.seq { continue; }            // déjà inclus dans le snapshot
    tm_core::state::apply_envelope(&mut state, &env); // même réducteur que le moteur
    redraw(&state);
}
```

Si le client prend trop de retard (`RecvError::Lagged`), il reprend un snapshot. Le WebSocket le fait automatiquement en renvoyant un message `snapshot`.

**Compatibilité de versions** :
- `api_version` ne change que sur une rupture du protocole.
- Les champs inconnus sont ignorés à la désérialisation, donc un client plus ancien lit l'état d'un daemon plus récent.
- `tm_client::Remote` se resynchronise sur un snapshot s'il voit un trou dans les `seq` ou un type d'événement qu'il ne connaît pas. Les événements reçus pendant la resynchronisation sont appliqués ensuite.

**Connexion distante** : `Remote` publie son état au plus toutes les 50 ms, comme `Local`. Il envoie un ping WebSocket toutes les 20 s et considère la connexion perdue après 60 s sans réponse (veille, réseau changé). Le serveur fait de même avec ses clients (ping toutes les 30 s, fermeture après 90 s de silence). Tant que le lien n'est pas `Connected`, `execute` répond tout de suite `unavailable` : une commande n'est jamais exécutée plus tard, au retour de la connexion.

## Serveur HTTP / WebSocket

Adresse par défaut : `127.0.0.1:7878` (`--bind`). Authentification par jeton :
- en-tête `Authorization: Bearer <token>`, partout (WebSocket compris : c'est ce que fait `tm_client::Remote`) ;
- un navigateur, qui ne peut pas mettre d'en-tête sur un WebSocket, ouvre `/api/v1/ws` sans jeton et envoie d'abord `{"type": "auth", "token": "<token>"}` (dans les 10 s) ; voir [Messages WebSocket](#messages-websocket) ;
- `?token=<token>` dans l'URL reste accepté pour les anciens clients, mais est **déconseillé** : les adresses finissent dans les journaux, l'historique du navigateur et les proxys. Le serveur note une fois dans son journal qu'un client l'utilise (sans le jeton) ;
- après 10 mauvais jetons depuis une même adresse IP, elle est refusée 30 s (`429`, code `rate_limited`, en-tête `Retry-After`), puis le double à chaque récidive, jusqu'à 15 min. Jamais pour `127.0.0.1` / `::1` ;
- un moteur intégré sans jeton (`ServerConfig.token: None`) ne démarre que sur une adresse de bouclage ;
- le jeton est généré au premier lancement et stocké dans la base. Pour le lire : `twitchminer api-token`.
- un second jeton, **agent** (`twitchminer api-token --agent`), donne un accès restreint pour les agents IA : certaines commandes sont refusées (`forbidden`) et les secrets sont masqués (`***`) dans l'état, les réglages et les événements. Détails : [AGENT.md](AGENT.md).
- des jetons **invité** (`guest_invite`), un par personne : l'invité n'utilise le moteur que pour ses propres comptes Twitch (un ami sur ton serveur). Il ne reçoit que ce qui les concerne : l'état, les événements et les réponses sont filtrés par le serveur (comptes, connexions en cours, avis, réglages), pas seulement masqués par l'interface. Il peut ajouter des comptes (`login_start` : ils deviennent les siens), les configurer (`settings_set` sur `account:<le sien>/module:*` uniquement), les mettre en pause ou les retirer, lire leur historique et leurs statistiques. Tout le reste est refusé (`forbidden`) : autres comptes, réglages partagés et du moteur, invités, journal (`/logs` 403), sauvegardes, mises à jour, `/mcp`. Il lit les réglages partagés des modules (les valeurs par défaut de ses comptes), sauf les destinations de notifications, et ceux du moteur sans secrets. Un jeton révoqué (`guest_remove`) répond `unauthorized`.

| Route | Description |
|---|---|
| `GET /api/v1/health` | `{ok, version, api_version, seq, instance_id}`, sans authentification |
| `GET /api/v1/state` | le `State` complet |
| `POST /api/v1/command` | corps = une `Command`, avec `Content-Type: application/json` (sinon `415`). Réponse `{"ok":true,"data":…}` ou `{"ok":false,"error":{"code","message"}}` |
| `GET /api/v1/logs?n=200` | dernières lignes de journal `[{ts, level, target, message}]` |
| `GET /api/v1/ws[?logs=1]` | flux temps réel, voir ci-dessous |
| `POST /api/v1/mcp` | serveur MCP (JSON-RPC 2.0, Streamable HTTP en réponses JSON, `Content-Type: application/json`) pour les agents IA, voir [AGENT.md](AGENT.md) |

**Navigateurs et autres sites** (toutes les routes) :
- l'en-tête `Host` doit être une adresse IP, `localhost`, un nom local (un seul mot comme `mini-pc`, `*.local`, `*.lan`, `*.home.arpa`, `*.internal`, `*.ts.net`) ou un nom de `engine.api_allowed_hosts`, sinon `403` (protection contre le *DNS rebinding*) ;
- une requête qui agit (tout sauf `GET`/`HEAD`, et l'ouverture d'un WebSocket) avec un en-tête `Origin` d'un autre site est refusée (`403`). Même site = même nom et même port que `Host`, ou un nom de `api_allowed_hosts`. Les clients natifs n'envoient pas d'`Origin` ;
- les en-têtes CORS (`Access-Control-Allow-Origin`, avec `Vary: Origin`) ne sont renvoyés qu'à ces origines ; une requête préliminaire `OPTIONS` d'une autre origine reçoit `403`.

### Messages WebSocket

Du serveur vers le client :
```jsonc
{"type": "snapshot", "api_version": 1, "access": "full", "state": { /* State */ }}  // à la connexion et après un retard
// access : full | agent | guest. Pour un invité, les `seq` des événements ont des trous (ceux des
// autres comptes ne sont pas envoyés) : appliquer tout événement de seq supérieur ; un nouveau
// snapshot arrive quand ses comptes changent.
{"type": "event", "event": {"seq": 54, "ts": "…", "type": "points_earned", "account": "…", …}}
{"type": "reply", "id": 7, "ok": true, "data": { … }}      // réponse à une commande
{"type": "log", "line": {"ts", "level", "target", "message"}} // si ?logs=1
```
Du client vers le serveur :
```json
{"type": "auth", "token": "<token>"}
{"id": 7, "cmd": {"cmd": "settings_set", "scope": "module:drops", "value": {"games": ["Rust"]}, "merge": true}}
```
`auth` : seulement si le jeton n'a pas été donné à l'ouverture (en-tête ou ancien `?token=`), et en premier message, dans les 10 s. Accepté, le serveur répond par le `snapshot` ; refusé, par `{"type": "error", "error": {"code": "unauthorized" | "rate_limited" | "forbidden", "message"}}`, puis ferme la connexion (code 1008). Un `auth` envoyé alors que la connexion est déjà authentifiée est ignoré.
Les commandes s'exécutent en parallèle : utilisez `id` pour associer chaque réponse à sa commande.

## Commandes (`Command`, champ `cmd`)

| `cmd` | Champs | Réponse |
|---|---|---|
| `snapshot` | — | `State` |
| `summary` | — | vue dérivée compacte (comptes, chaînes regardées, drop en cours avec progression et ETA, totaux) pour icône de barre système, notifications et widgets. En Rust : `tm_core::view::summary(&state, now)` |
| `modules_list` | — | `[{id, name, description, default_settings, settings_schema}]` |
| `login_start` | `profile?`, `guest?` (le compte ajouté appartiendra à cet invité ; imposé par le serveur pour un jeton invité) | `{flow_id, user_code, verification_uri, verification_uri_complete, expires_at}`. `verification_uri_complete` est la même page avec le code déjà rempli. La fin arrive en événement `login_finished`. Pour un invité : au plus `guest_limits.max_pending_logins` connexions en attente (`forbidden`) ; à la fin, un compte Twitch déjà présent sur le serveur (le propriétaire ou un autre invité) est refusé sans toucher à ses identifiants (`login_finished` avec `error`, sans `account`), de même au-delà de `guest_limits.max_accounts` comptes. Ses propres comptes peuvent toujours se reconnecter |
| `login_cancel` | `flow_id` | `{cancelled}` |
| `login_with_token` | `access_token`, `refresh_token?`, `profile?` | `{account}` |
| `account_remove` | `account` | `{removed}` |
| `account_set_enabled` | `account`, `enabled` | `{enabled}` |
| `settings_get` | `scope`, `account?` | `{scope, raw, effective, defaults, schema}` |
| `settings_set` | `scope`, `value`, `merge?` | document enregistré (`value: null` supprime la portée) |
| `history` | `account?`, `channel_id?`, `kinds?[]`, `before_id?`, `since?`, `limit?` | `[{id, seq, ts, account, kind, channel_id, event}]`, du plus récent au plus ancien |
| `stats` | `account?`, `channel_id?`, `from?`, `to?` (jours locaux `YYYY-MM-DD`), `group_by: "day"\|"hour"\|"channel"`, `tz_offset_minutes?` | `[{key, points, watch_minutes, drops}]`. Stocké par heure UTC ; les jours sont découpés selon `tz_offset_minutes` (décalage de celui qui demande, ex. `120` pour UTC+2), par défaut le fuseau de la machine du moteur. Une interface distante doit envoyer le sien |
| `games_search` | `query`, `limit?` (10, max 25) | `[{id, name, slug?, box_art_url?}]`, meilleure correspondance d'abord (autocomplétion de la liste de jeux). Moins de 2 caractères : `[]`. Un invité : au plus `guest_limits.games_search_per_minute` par minute (`rate_limited`) |
| `module_call` | `account`, `module`, `name`, `args?` | défini par le module |
| `manifest_refresh` | — | `{updated, revision}` |
| `update_check` | — | `{current, latest, update_available, url}`. Une notice `update` est aussi levée si besoin |
| `notice_dismiss` | `id` | `{}` |
| `guest_invite` | `name` | `{id, name, token}`. Crée un invité ; le jeton n'est donné qu'ici (seule son empreinte SHA-256 est gardée). Jeton complet uniquement |
| `guest_remove` | `id` | `{removed}`. Révoque le jeton ; les comptes de l'invité restent. Jeton complet uniquement |
| `backup` | `path?` | `{path}`. Copie cohérente de la base, faisable à chaud. Par défaut dans `<data>/backups/`. `path` doit être **absolu** (sinon `invalid`) et ne pas désigner un fichier non vide. Sous Unix la copie est créée en `0600` (elle contient les jetons Twitch) et `backups/` en `0700`. Pour restaurer : arrêter, puis remplacer `twitchminer.db` |

Codes d'erreur : `not_found`, `invalid`, `auth`, `module`, `unavailable`, `timeout`, `manifest`, `internal`, `bad_command`, `bad_request` (corps illisible, `Content-Type` manquant), `unauthorized`, `forbidden` (jeton agent ou invité, autre site, nom d'hôte inconnu), `rate_limited` (trop de mauvais jetons, trop de recherches d'un invité).

### Actions des modules (`module_call`)

| Module | `name` | `args` | Effet |
|---|---|---|---|
| `points` | `add_channel` | `{login}` | ajoute la chaîne aux réglages du compte |
| `points` | `remove_channel` | `{login}` | la retire |
| `points` | `status` | — | état interne (minutes par diffusion, série…) |
| `drops` | `refresh` | — | recharge les campagnes maintenant |
| `drops` | `claim_all` | — | récupère tout ce qui peut l'être |
| `drops` | `status` | — | cible, chaînes/campagnes écartées |
| `drops` | `plan` | — | le plan (comme `module_data.drops.plan`) |
| `drops` | `focus` | `{campaign_id}` ou `{game}` | farmer cette campagne (ou la meilleure de ce jeu) en premier jusqu'à son objectif, puis reprendre le plan |
| `drops` | `unfocus` | — | annule le focus |
| `notify` | `test` | — | envoie une notification de test |

## Réglages

Portées :
- `engine` : `max_watch_slots` (1-2), `proxy`, `manifest_url`, `manifest_auto_update`, `history_retention_days`, `update_check`, `releases_url`, `paused`, `active_hours`, `agent_enabled`, `api_allowed_hosts`, `guest_limits` ;
  - **Noms du serveur** : `api_allowed_hosts` (défaut `[]`), noms auxquels l'API répond en plus des adresses IP, de `localhost` et des noms locaux, par exemple le nom public d'un reverse proxy : `["tm.example.com"]`, ou `["*.example.com"]` pour ses sous-domaines. Sans schéma ni port. Les pages web de ces noms peuvent aussi agir sur l'API (CORS). Voir [SERVER.md](SERVER.md#exposer-le-serveur-hors-de-tailscale).
  - **Limites des invités** : `guest_limits` = `{"max_pending_logins": 2, "max_accounts": 5, "max_notify_targets": 10, "games_search_per_minute": 60}` (chacune ≥ 1). Par invité ; le propriétaire n'a aucune limite.
  - **Agents IA** : `agent_enabled` (défaut `false`). Désactivé, la route `/mcp` répond 404 et le jeton agent est refusé partout (403), y compris les WebSocket déjà ouverts. Seul le jeton complet peut le changer. Voir [AGENT.md](AGENT.md).
  - **Pause globale** : `settings_set engine {"paused": true}`. Rien n'est regardé (`watch_stopped{cause:"paused"}`), mais les modules continuent de suivre et de récupérer les bonus.
  - **Plusieurs instances** : `role` (`primary` | `secondary`) et `peers: [{name?, url, token?}]`. Un secondaire passe en `standby` les comptes qu'un pair mine déjà, et les reprend si le pair disparaît. Voir [SERVER.md](SERVER.md).
  - **Plages horaires** : `{"active_hours": [{"days": [1,2,3,4,5], "start": "18:00", "end": "02:00"}]}`. Heure locale, `days` en ISO (1 = lundi), une fin avant le début passe minuit, `days` vide = tous les jours. Hors plage : `cause:"outside active hours"`.
- `guests` : `{"<id>": {name, token_sha256, accounts: [ids], created_at}}`, écrite par `guest_invite`, `guest_remove`, la fin d'un `login_start` d'invité (le compte s'ajoute à `accounts`, et reçoit `account:<id>/module:notify` `{"targets": []}` pour ne pas hériter des destinations du propriétaire) et `account_remove`. Jamais envoyée aux agents ni aux invités.
- **Notifications d'un invité** (`settings_set` sur `account:<le sien>/module:notify`) : le serveur calcule le document complet (fusion comprise) et l'enregistre tel quel (`merge` ignoré). Il garde toujours une liste `targets` (sinon les destinations du propriétaire seraient héritées ; `value: null` devient `{"targets": []}`), jamais de `firebase_key` (refusée si envoyée, ses téléphones utilisent celle du serveur). Chaque destination `discord`, `ntfy` ou `webhook` doit être une adresse `https://` publique : pas d'IP privée, de bouclage, link-local, CGNAT `100.64.0.0/10` (Tailscale), ULA `fc00::/7`, ni de nom local (`localhost`, un seul mot, `*.local`, `*.ts.net`…) ; pas d'`headers` ; au plus `guest_limits.max_notify_targets` destinations ; un type inconnu est refusé (`forbidden`). À l'envoi, le module revérifie l'adresse, refuse un nom qui se résout vers une adresse privée et ne suit aucune redirection ; l'invité ne voit qu'une erreur générique.
- `module:<id>` : réglages partagés par tous les comptes ;
- `account:<account_id>/module:<id>` : surcharge pour un compte.

Valeur effective = `défauts du module ⊕ module:<id> ⊕ account:<id>/module:<id>` (fusion profonde).

Chaque portée a un **JSON Schema** (`settings_get(...).schema`, ou `modules_list[].settings_schema`) généré depuis les structures Rust : types, valeurs d'énumération et descriptions des champs. Une interface peut construire ses formulaires de réglages automatiquement, y compris pour les modules ajoutés plus tard. Chaque module a un booléen `enabled` qui le démarre ou l'arrête à chaud. `settings_set` valide le document effectif avant de l'enregistrer.

<details><summary>Défauts des modules intégrés</summary>

```jsonc
// drops (voir « Plan des drops » ci-dessous)
{"enabled": true, "games": [], "exclude_games": [], "wanted_rewards": [],
 "badges_emotes": true, "require_linked": true, "auto_claim": true, "refresh_minutes": 60, "discovery_channels": 8,
 "restricted_max_channels": 50, "safety_factor": 1.25, "safety_minutes": 120, "fill_partial": true,
 "alerts": {"at_risk": true, "will_miss": true, "not_linked": true, "new_campaign": true}}
// points
{"enabled": true, "channels": [], "follows": false, "exclude": [], "claim_bonus": true, "join_raids": true,
 "claim_moments": true, "watch_streak": true, "refresh_minutes": 5,
 "order_by": "config"}                                                    // config | multiplier | fewest_points | most_points
// predictions (désactivé par défaut)
{"enabled": false, "strategy": "smart", "percent": 5.0, "max_points": 50000, "min_balance": 2000,
 "min_voters": 20, "seconds_before_close": 6, "channels": [],           // most_voted | most_points | high_odds | smart
 "min_odds": 0, "max_odds": 0,          // gain (pot total / pot de la réponse) hors de ces bornes : pas de pari ; 0 = sans limite
 "stealth": false}                      // jamais plus que le plus gros pari déjà placé sur la réponse
// module_data.predictions.summary : {"bets", "wins", "losses", "refunds", "net"} (net = gagné - misé),
//   repris de l'historique au premier démarrage ; aussi dans module_call predictions status
// notify
{"enabled": true, "targets": [], "events": ["drop_claimed", "drop_alert", "auth_required", "prediction_result", "notice_raised"], "min_points": 0,
 "all_urgent": false, "language": "fr",                                 // fr | en
 "firebase_key": ""}                    // contenu JSON de la clé de compte de service Firebase (cibles fcm)
// urgent (ntfy « Priority: high », FCM priorité HIGH et canal « urgent ») : le test, la
//   confirmation d'une nouvelle destination (envoyée dès qu'elle apparaît dans targets),
//   auth_required et les alertes at_risk / will_miss / not_linked ; pour tout si all_urgent
// targets: {"kind":"discord","webhook_url"} | {"kind":"ntfy","url","token?"} | {"kind":"telegram","bot_token","chat_id"} | {"kind":"webhook","url","token?","headers?"}
//   bot_token : <chiffres>:<lettres, chiffres, - ou _> (sinon settings_set refusé)
//   | {"kind":"fcm","token","device","name"} : un téléphone avec l'appli, ajouté par l'appli elle-même
//   (jeton Firebase ; device = ANDROID_ID : un nouveau jeton du même téléphone remplace l'ancien)
// fcm : API HTTP v1 de Firebase Cloud Messaging, avec firebase_key (JWT RS256 → jeton d'accès
//   Google, gardé ~55 min) ; message « notification » (affiché par Android même appli fermée),
//   canal urgent ou events. firebase_key invalide → settings_set refusé (aussi : project_id hors
//   [a-z0-9-], token_uri ailleurs que sur https://oauth2.googleapis.com/). Seule la clé du projet
//   Firebase avec lequel l'APK a été construit peut l'atteindre (UNREGISTERED : appli désinstallée
//   ou données effacées, ré-ajouter le téléphone). firebase_key est masquée ("***") pour l'agent et,
//   héritée du propriétaire, pour les invités
// webhook : POST {"text", "event"} ; token → "Authorization: Bearer …" ; headers = en-têtes en plus (réveil d'un agent IA)
// 3 essais par cible ; un 429 attend son Retry-After (ou le retry_after de Discord / Telegram), au plus 1 min
```
</details>

### Plan des drops

Le module `drops` ne se contente pas de trier : il **planifie** (`module_data.drops.plan`, recalculé à chaque choix de cible et à chaque minute de progression d'un drop : temps restant et avancement suivent le visionnage). Twitch ne signale que le drop en cours, alors qu'il compte en même temps tous les drops débloqués d'une campagne : le module avance les autres d'autant (événements `drop_progress` estimés), et la prochaine actualisation remet les vraies valeurs.

1. **Focus** (`module_call drops focus`) : en premier, jusqu'à son objectif.
2. **Campagnes à chaînes définies** (moins de `restricted_max_channels` chaînes autorisées, ex. esport) : priorité maximale dès qu'une de leurs chaînes est en direct sur le jeu. Leur temps n'est pas prévisible, elles ne sont pas planifiées.
3. **Campagnes ouvertes** : admises dans l'ordre de `games`, chacune seulement si **tous** les objectifs admis tiennent encore avant leur fin (temps disponible = `active_hours` ; la pause, temporaire, est ignorée). Les admises sont farmées **échéance la plus proche d'abord** ; à échéance égale (même heure), l'ordre de `games` décide. Une priorité haute n'est donc jamais sacrifiée pour une plus basse.
4. **Campagnes impossibles à finir** : en dernier, pour leurs premières récompenses (`fill_partial`).

**Objectif** d'une campagne : sa dernière récompense (les plus longues sont souvent les meilleures), ou les récompenses de `wanted_rewards` qu'elle contient (nom partiel, sans casse). Les drops indépendants avancent ensemble ; un drop chaîné compte avec ses prérequis. La **marge** (`× safety_factor + safety_minutes`) n'exclut jamais : elle fait seulement passer une campagne en `at_risk`.

```jsonc
// module_data.drops.plan : meilleure d'abord
[{"campaign_id", "campaign", "game", "status", "ends_at", "objective": ["récompenses"], "remaining_minutes",
  "start_at?", "eta?",          // estimations (planned / at_risk)
  "missing_minutes?",           // will_miss
  "starts_at?", "link_url?", "skipped_until?", "game_rank?"}]
// status : active | focus | restricted | planned | at_risk | will_miss | upcoming | not_linked | skipped | done
// module_data.drops.focus : {"campaign_id"} ou null
// module_data.drops.game_ids : {"<nom de la liste, en minuscules>": "<id du jeu>"} ; un jeu renommé
//   par Twitch reste reconnu par son id (le nom de la liste n'a pas besoin de changer)
// module_data.drops.inventory : toutes les récompenses reçues, plus récente d'abord
[{"id", "name?", "image_url?", "game?", "count", "last_awarded_at?"}]
```

Seuls les jeux de `games` (tous si la liste est vide) apparaissent dans le plan. Les **alertes** (`drop_alert`) ne concernent que les jeux de `games`, une seule fois par campagne et par type.

## L'état (`State`)

```jsonc
{
  "seq": 53,                        // version : seq du dernier événement appliqué
  "version": "0.1.0",
  "instance_id": "uuid",            // identifiant stable de cette instance
  "started_at": "…",
  "manifest_revision": 1,
  "watching_allowed": true,         // jugé par le moteur dans son fuseau (absent : moteur plus ancien)
  "settings": { "<scope>": { … } },
  "notices": { "<id>": {"id", "level": "info|warning|error", "message", "at", "kind?", "params?"} },
  // kind (+ params), pour un texte traduit et le bon bouton :
  //   "update"        {version, current, url}     → « Télécharger »
  //   "duplicate"     {account, login, peer}      → compte miné par deux instances principales
  //   "proxy_invalid" {error}                     → ouvrir les réglages réseau
  //   "realtime_down" {account, login}            → PubSub injoignable depuis 10 min : drops et points
  //                                                 continuent, raids, moments et prédictions attendent
  // message : texte anglais de secours
  "logins":  { "<flow_id>": {"flow_id", "user_code", "verification_uri", "verification_uri_complete?", "expires_at", "guest?"} },
  "accounts": {
    "<user_id>": {
      "info":   {"id", "login", "display_name", "avatar_url", "enabled"},
      "status": {"state": "stopped|starting|running|auth_required|error|standby", "message?", "peer?"},
      "modules": { "<module>": {"state": "disabled|running|stopped|restarting", "error?", "attempt?"} },
      "channels": {
        "<channel_id>": {
          "id", "login", "display_name", "avatar_url",
          "stream": null | {"id", "title", "viewers", "started_at", "game": {"id","name","slug","box_art_url"}},
          "points": 1234, "multiplier": 1.0,
          "sources": ["points", "drops"],   // modules qui suivent la chaîne
          "session_points": 50, "session_minutes": 12
        }
      },
      "watching": { "<channel_id>": {"channel_id", "reason", "since", "minutes"} },
      "campaigns": {
        "<campaign_id>": {
          "id", "name", "status", "starts_at", "ends_at", "image_url", "details_url", "link_url",
          "linked": true | false | null,
          "game": {"id","name","slug","box_art_url"},
          "allowed_channels": null | [{"id","login","display_name"}],
          "drops": [{"id","name","starts_at","ends_at","required_minutes","current_minutes","claimed","claim_id",
                     "preconditions": ["<drop_id>"], "benefits": [{"id","name","image_url","kind"}]}]
        }
      },
      "drop_target": null | {"campaign_id", "drop_id", "channel_id", "game"},
      "session": {"points_earned", "watch_minutes", "drops_claimed", "bonuses_claimed", "raids_joined", "moments_claimed"},
      "module_data": { "<module>": { "<key>": <json> } }   // données libres publiées par les modules
    }
  }
}
```

## Événements (`Event`, champ `type`)

Chaque événement arrive enveloppé : `{seq, ts, type, …champs}`. Ceux marqués 📜 sont aussi enregistrés dans l'historique.

| `type` | Champs |
|---|---|
| `engine_started` | `version`, `instance_id` |
| `settings_changed` | `scope`, `value` (`null` = supprimé) |
| `manifest_updated` | `revision` |
| `watching_allowed` | `allowed` : le moteur peut regarder en ce moment (pas en pause, dans les plages horaires, jugées dans **son** fuseau horaire) |
| `notice_raised` 📜 / `notice_cleared` | `notice` / `id` |
| `login_started` / `login_finished` 📜 | `flow` / `flow_id`, `account?`, `error?` |
| `account_upserted` / `account_removed` | `info` / `account` |
| `account_status_changed` 📜 | `account`, `status` |
| `module_health_changed` | `account`, `module`, `health` |
| `module_data_set` | `account`, `module`, `key`, `value` (`null` = supprimé) |
| `channel_tracked` / `channel_released` | `account`, `channel` (meta) / `channel_id`, `source` |
| `stream_changed` | `account`, `channel_id`, `stream` |
| `viewers_changed` | `account`, `channel_id`, `viewers` |
| `points_balance` | `account`, `channel_id`, `balance`, `multiplier` |
| `points_earned` 📜 | `account`, `channel_id`, `amount`, `reason` (`WATCH`, `CLAIM`, `WATCH_STREAK`, `RAID`…), `balance` |
| `points_spent` 📜 | `account`, `channel_id`, `amount`, `balance` |
| `raid_joined` 📜 | `account`, `channel_id`, `target_login` |
| `moment_claimed` 📜 | `account`, `channel_id` |
| `prediction_placed` 📜 | `account`, `channel_id`, `event_id`, `title`, `outcome`, `points` |
| `prediction_result` 📜 | `account`, `channel_id`, `event_id`, `result` (`WIN`/`LOSE`/`REFUND`), `points_won` (net) |
| `watch_started` 📜 / `watch_stopped` 📜 | `account`, `channel_id`, `reason` / `cause` (`offline`, `rescheduled`, `paused`, `outside active hours`, `error: …`) |
| `watch_minute` | `account`, `channel_id` (une minute créditée) |
| `campaigns_replaced` | `account`, `campaigns[]` |
| `drop_progress` | `account`, `campaign_id`, `drop_id`, `current_minutes`, `required_minutes` |
| `drop_claimed` 📜 | `account`, `campaign_id`, `drop_id`, `drop_name`, `game` |
| `drop_target_changed` | `account`, `target` |
| `drop_alert` 📜 | `account`, `alert` : `{kind, campaign_id, campaign, game, ends_at, starts_at?, missing_minutes?, link_url?, rewards[]}`, `kind` = `at_risk` \| `will_miss` \| `not_linked` \| `new_campaign` |

## Exemple de parcours « premier lancement »

1. `snapshot` : `accounts` est vide, l'interface affiche « Se connecter ».
2. `login_start` : afficher `user_code` et ouvrir `verification_uri_complete` (code déjà rempli), ou à défaut `verification_uri`.
3. Attendre l'événement `login_finished` : `account` est rempli, sinon afficher `error`.
4. `account_upserted` puis `account_status_changed{running}` arrivent ; les modules démarrent.
5. Proposer d'ajouter des chaînes (`module_call points add_channel`) et des jeux (`settings_set module:drops {"games": [...]}`).
