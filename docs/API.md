# API : le contrat pour les interfaces

Toute interface (desktop, Android, web, bot…) parle au moteur avec **trois primitives** :

| Primitive | En Rust (in-process) | En réseau (`tm-server`) |
|---|---|---|
| Lire l'état complet | `engine.store().snapshot()` | `GET /api/v1/state` ou 1er message WS |
| Suivre les changements | `engine.store().subscribe()` | messages `event` du WebSocket |
| Agir | `engine.execute(Command)` | `POST /api/v1/command` ou message WS |

Les types sont les mêmes des deux côtés : `tm_core::{State, Envelope, Event, Command, ApiError}`. Une interface Rust peut désérialiser directement le JSON réseau dans ces types.

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

## Serveur HTTP / WebSocket

Adresse par défaut : `127.0.0.1:7878` (`--bind`). Authentification par jeton :
- en-tête `Authorization: Bearer <token>`, ou paramètre `?token=<token>` (pratique pour le WebSocket d'un navigateur) ;
- le jeton est généré au premier lancement et stocké dans la base. Pour le lire : `twitchminer api-token`.

| Route | Description |
|---|---|
| `GET /api/v1/health` | `{ok, version, seq}`, sans authentification |
| `GET /api/v1/state` | le `State` complet |
| `POST /api/v1/command` | corps = une `Command`. Réponse `{"ok":true,"data":…}` ou `{"ok":false,"error":{"code","message"}}` |
| `GET /api/v1/logs?n=200` | dernières lignes de journal `[{ts, level, target, message}]` |
| `GET /api/v1/ws[?logs=1]` | flux temps réel, voir ci-dessous |

### Messages WebSocket

Du serveur vers le client :
```jsonc
{"type": "snapshot", "state": { /* State */ }}            // à la connexion et après un retard
{"type": "event", "event": {"seq": 54, "ts": "…", "type": "points_earned", "account": "…", …}}
{"type": "reply", "id": 7, "ok": true, "data": { … }}      // réponse à une commande
{"type": "log", "line": {"ts", "level", "target", "message"}} // si ?logs=1
```
Du client vers le serveur :
```json
{"id": 7, "cmd": {"cmd": "settings_set", "scope": "module:drops", "value": {"games": ["Rust"]}, "merge": true}}
```
Les commandes s'exécutent en parallèle : utilisez `id` pour associer chaque réponse à sa commande.

## Commandes (`Command`, champ `cmd`)

| `cmd` | Champs | Réponse |
|---|---|---|
| `snapshot` | — | `State` |
| `summary` | — | vue dérivée compacte (comptes, chaînes regardées, drop en cours avec progression et ETA, totaux) pour icône de barre système, notifications et widgets. En Rust : `tm_core::view::summary(&state, now)` |
| `modules_list` | — | `[{id, name, description, default_settings, settings_schema}]` |
| `login_start` | `profile?` | `{flow_id, user_code, verification_uri, expires_at}`. La fin arrive en événement `login_finished` |
| `login_cancel` | `flow_id` | `{cancelled}` |
| `login_with_token` | `access_token`, `refresh_token?`, `profile?` | `{account}` |
| `account_remove` | `account` | `{removed}` |
| `account_set_enabled` | `account`, `enabled` | `{enabled}` |
| `settings_get` | `scope`, `account?` | `{scope, raw, effective, defaults, schema}` |
| `settings_set` | `scope`, `value`, `merge?` | document enregistré (`value: null` supprime la portée) |
| `history` | `account?`, `channel_id?`, `kinds?[]`, `before_id?`, `since?`, `limit?` | `[{id, seq, ts, account, kind, channel_id, event}]`, du plus récent au plus ancien |
| `stats` | `account?`, `channel_id?`, `from?`, `to?` (`YYYY-MM-DD`), `group_by: "day"\|"channel"` | `[{key, points, watch_minutes, drops}]` |
| `module_call` | `account`, `module`, `name`, `args?` | défini par le module |
| `manifest_refresh` | — | `{updated, revision}` |
| `update_check` | — | `{current, latest, update_available, url}`. Une notice `update` est aussi levée si besoin |
| `notice_dismiss` | `id` | `{}` |

Codes d'erreur : `not_found`, `invalid`, `auth`, `module`, `unavailable`, `timeout`, `manifest`, `internal`, `bad_command`, `unauthorized`.

### Actions des modules (`module_call`)

| Module | `name` | `args` | Effet |
|---|---|---|---|
| `points` | `add_channel` | `{login}` | ajoute la chaîne aux réglages du compte |
| `points` | `remove_channel` | `{login}` | la retire |
| `points` | `status` | — | état interne (minutes par diffusion, série…) |
| `drops` | `refresh` | — | recharge les campagnes maintenant |
| `drops` | `claim_all` | — | récupère tout ce qui peut l'être |
| `drops` | `status` | — | cible, chaînes/campagnes écartées |
| `notify` | `test` | — | envoie une notification de test |

## Réglages

Portées :
- `engine` : `max_watch_slots` (1-2), `proxy`, `manifest_url`, `manifest_auto_update`, `history_retention_days`, `update_check`, `releases_url` ;
- `module:<id>` : réglages partagés par tous les comptes ;
- `account:<account_id>/module:<id>` : surcharge pour un compte.

Valeur effective = `défauts du module ⊕ module:<id> ⊕ account:<id>/module:<id>` (fusion profonde).

Chaque portée a un **JSON Schema** (`settings_get(...).schema`, ou `modules_list[].settings_schema`) généré depuis les structures Rust : types, valeurs d'énumération et descriptions des champs. Une interface peut construire ses formulaires de réglages automatiquement, y compris pour les modules ajoutés plus tard. Chaque module a un booléen `enabled` qui le démarre ou l'arrête à chaud. `settings_set` valide le document effectif avant de l'enregistrer.

<details><summary>Défauts des modules intégrés</summary>

```jsonc
// drops
{"enabled": true, "games": [], "exclude_games": [], "mode": "priority",   // priority | ending_soonest | least_remaining
 "badges_emotes": true, "require_linked": true, "auto_claim": true, "refresh_minutes": 60, "discovery_channels": 8}
// points
{"enabled": true, "channels": [], "follows": true, "exclude": [], "claim_bonus": true, "join_raids": true,
 "claim_moments": true, "watch_streak": true, "refresh_minutes": 5}
// predictions (désactivé par défaut)
{"enabled": false, "strategy": "smart", "percent": 5.0, "max_points": 50000, "min_balance": 2000,
 "min_voters": 20, "seconds_before_close": 6, "channels": []}           // most_voted | most_points | high_odds | smart
// notify
{"enabled": true, "targets": [], "events": ["drop_claimed", "auth_required", "prediction_result", "notice_raised"], "min_points": 0}
// targets: {"kind":"discord","webhook_url"} | {"kind":"ntfy","url","token?"} | {"kind":"telegram","bot_token","chat_id"} | {"kind":"webhook","url"}
```
</details>

## L'état (`State`)

```jsonc
{
  "seq": 53,                        // version : seq du dernier événement appliqué
  "version": "0.1.0",
  "started_at": "…",
  "manifest_revision": 1,
  "settings": { "<scope>": { … } },
  "notices": { "<id>": {"id", "level": "info|warning|error", "message", "at"} },
  "logins":  { "<flow_id>": {"flow_id", "user_code", "verification_uri", "expires_at"} },
  "accounts": {
    "<user_id>": {
      "info":   {"id", "login", "display_name", "avatar_url", "enabled"},
      "status": {"state": "stopped|starting|running|auth_required|error", "message?"},
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
| `engine_started` | `version` |
| `settings_changed` | `scope`, `value` (`null` = supprimé) |
| `manifest_updated` | `revision` |
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
| `watch_started` 📜 / `watch_stopped` 📜 | `account`, `channel_id`, `reason` / `cause` (`offline`, `rescheduled`, `error: …`) |
| `watch_minute` | `account`, `channel_id` (une minute créditée) |
| `campaigns_replaced` | `account`, `campaigns[]` |
| `drop_progress` | `account`, `campaign_id`, `drop_id`, `current_minutes`, `required_minutes` |
| `drop_claimed` 📜 | `account`, `campaign_id`, `drop_id`, `drop_name`, `game` |
| `drop_target_changed` | `account`, `target` |

## Exemple de parcours « premier lancement »

1. `snapshot` : `accounts` est vide, l'interface affiche « Se connecter ».
2. `login_start` : afficher `user_code` et ouvrir `verification_uri`.
3. Attendre l'événement `login_finished` : `account` est rempli, sinon afficher `error`.
4. `account_upserted` puis `account_status_changed{running}` arrivent ; les modules démarrent.
5. Proposer d'ajouter des chaînes (`module_call points add_channel`) et des jeux (`settings_set module:drops {"games": [...]}`).
