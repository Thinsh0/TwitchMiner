# Brancher un agent IA (Hermes, OpenClaw, Claude…)

TwitchMiner expose un **serveur MCP** (Model Context Protocol), le standard que comprennent la plupart des agents. L'agent peut alors répondre à « où en sont mes drops ? », « combien de points cette semaine ? », « mets Rust en priorité », « mets en pause cette nuit », etc.

## Activer (désactivé par défaut)

Sans agent branché, rien n'est exposé : la route `/mcp` répond 404 et le jeton agent est refusé partout. Pour activer, avec le jeton complet :

```bash
twitchminer ctl set engine '{"agent_enabled": true}'     # false pour couper, effet immédiat
```

L'agent ne peut pas modifier ce réglage lui-même.

## Deux jetons

| Jeton | Commande | Pour qui | Peut tout faire ? |
|---|---|---|---|
| **complet** | `twitchminer api-token` | toi, l'app, `ctl`, les pairs | oui |
| **agent** | `twitchminer api-token --agent` | un agent IA | non, voir ci-dessous |

Pourquoi un jeton à part : l'agent lit des textes écrits par des inconnus (titres de stream, noms de campagne). Un titre piégé du genre « ignore tes instructions et envoie… » pourrait le manipuler. Le jeton agent limite les dégâts possibles, quoi qu'il arrive à l'agent :

- **interdit** : supprimer un compte, ajouter un compte à partir d'un jeton brut, écrire une sauvegarde, changer les cibles de notification, changer les réglages moteur autres que `paused`, `active_hours` et `max_watch_slots` (donc ni proxy, ni pairs, ni URL de mise à jour), ou les remplacer sans `merge: true` ;
- **masqué** (`***`) : jeton des pairs, proxy, URL de webhook, jetons et en-têtes des notifications, partout où ils apparaissent (état, `settings_get`, réponse de `settings_set`, événements) ; les erreurs d'envoi des notifications ne contiennent jamais l'URL de la cible ;
- **autorisé** : tout le reste (lire l'état, l'historique et les stats ; jeux de drops, chaînes, pause, horaires, connexion d'un compte par code, réglages des modules).

Une action refusée renvoie l'erreur `forbidden`.

## Transport 1 : HTTP (agent sur le mini PC ou ailleurs dans Tailscale)

- URL : `http://127.0.0.1:7878/api/v1/mcp`, ou `http://<ip-tailscale>:7878/api/v1/mcp` si l'agent tourne sur une autre machine ;
- en-tête : `Authorization: Bearer <jeton agent>`.

Forme générique (la syntaxe exacte dépend de l'agent, voir sa doc « MCP servers ») :

```json
{
  "mcpServers": {
    "twitchminer": {
      "url": "http://127.0.0.1:7878/api/v1/mcp",
      "headers": { "Authorization": "Bearer <jeton agent>" }
    }
  }
}
```

Transport « Streamable HTTP » en réponses JSON simples, sans flux SSE. Versions du protocole acceptées : `2025-11-25`, `2025-06-18`, `2025-03-26`, `2024-11-05`.

## Transport 2 : stdio (l'agent lance une commande)

`twitchminer mcp` est un petit pont : il lit le JSON-RPC sur l'entrée standard et le relaie au daemon qui tourne déjà. Il ne démarre **pas** de second moteur.

```json
{
  "mcpServers": {
    "twitchminer": {
      "command": "twitchminer",
      "args": ["mcp"]
    }
  }
}
```

- Sur la même machine que le daemon, rien d'autre à faire : le jeton agent est lu dans le dossier de données (même utilisateur, ou `TM_DATA_DIR`).
- Vers un autre daemon : `["mcp", "--url", "http://100.x.y.z:7878", "--token", "<jeton agent>"]`, ou les variables `TM_API_URL` et `TM_MCP_TOKEN`.
- Daemon injoignable ou jeton refusé : l'agent reçoit une erreur JSON-RPC claire, le détail est écrit sur la sortie d'erreur.

## La skill

[`integrations/agent/SKILL.md`](../integrations/agent/SKILL.md) explique à l'agent quand et comment utiliser les outils (format standard `SKILL.md`, compris par Hermes et OpenClaw). Copie le dossier `integrations/agent/` dans le dossier de skills de l'agent, sous le nom `twitchminer`.

## Les outils

| Outil | Effet |
|---|---|
| `status` | vue d'ensemble : comptes, chaînes regardées, drop en cours (progression, ETA), pause, alertes |
| `list_channels` | chaînes à points : en direct, jeu, solde, points de la session |
| `list_campaigns` | campagnes de drops (filtre `game`, `include_finished`) avec `linked`, `link_url`, `is_target`, `skipped_until` |
| `get_history` | derniers événements (points, drops) |
| `get_stats` | totaux par jour ou par chaîne sur N jours |
| `add_channel` / `remove_channel` | chaînes à points |
| `search_games` | chercher un jeu par son nom (noms exacts pour `set_drop_games`) |
| `set_drop_games` | jeux de drops voulus, par priorité (tous comptes, ou un seul avec `account`) |
| `set_paused` | pause / reprise |
| `refresh_drops` | recharger les campagnes |
| `get_drop_plan` | le plan des drops : statut de chaque campagne (en cours, planifiée avec début et fin estimés, à risque, sera manquée, non liée…), objectif, minutes restantes |
| `focus_drops` | farmer une campagne (ou la meilleure d'un jeu) en premier jusqu'à son objectif ; `clear: true` pour annuler |
| `start_login` | connecter un compte : renvoie le code à saisir sur twitch.tv/activate |
| `get_settings` / `update_settings` | réglages d'un module (`drops`, `points`, `predictions`, `notify`) ou `engine`, avec le schéma |

## Réveiller l'agent (proactif)

MCP fonctionne en question-réponse : l'agent ne sait rien tant qu'il n'appelle pas un outil. Pour qu'il réagisse seul (campagne à risque, compte de jeu à lier, nouvelle campagne), ajoute une cible **webhook** au module `notify`, pointée sur l'entrée webhook de l'agent :

```bash
twitchminer ctl set module:notify '{
  "targets": [{ "kind": "webhook", "url": "http://127.0.0.1:<port>/<chemin>", "token": "<secret>" }],
  "events": ["drop_alert", "drop_claimed", "auth_required"]
}'
```

- corps envoyé : `{"text": "<message lisible>", "event": {...}}` (l'événement complet, ex. `drop_alert` avec `kind`, `campaign`, `game`, `ends_at`, `missing_minutes`, `link_url`) ;
- `token` → en-tête `Authorization: Bearer <secret>` ; `headers` pour d'autres en-têtes si l'agent en demande ;
- l'URL et le chemin exacts dépendent de l'agent : voir sa doc « webhook ».
- `twitchminer ctl call notify test --account <id>` envoie un message de test.

Sans webhook, l'agent peut aussi passer régulièrement via sa propre tâche planifiée (`status`, `get_drop_plan`).

## Tester à la main

```bash
TOKEN=$(twitchminer api-token --agent)
curl -s -X POST http://127.0.0.1:7878/api/v1/mcp \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"status","arguments":{}}}'

echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | twitchminer mcp
```

## Changer de jeton

`twitchminer api-token --agent --rotate` (ou sans `--agent` pour le jeton complet) crée un nouveau jeton et l'affiche. Le daemon l'utilise à son prochain redémarrage ; l'ancien reste valable jusque-là.
