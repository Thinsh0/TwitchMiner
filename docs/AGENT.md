# Brancher un agent IA (Hermes, OpenClaw, Claude…)

TwitchMiner expose un **serveur MCP** (Model Context Protocol), le standard que comprennent la plupart des agents. L'agent peut alors répondre à « où en sont mes drops ? », « combien de points cette semaine ? », « mets Rust en priorité », « mets en pause cette nuit », etc.

## Deux jetons

| Jeton | Commande | Pour qui | Peut tout faire ? |
|---|---|---|---|
| **complet** | `twitchminer api-token` | toi, l'app, `ctl`, les pairs | oui |
| **agent** | `twitchminer api-token --agent` | un agent IA | non, voir ci-dessous |

Pourquoi un jeton à part : l'agent lit des textes écrits par des inconnus (titres de stream, noms de campagne). Un titre piégé du genre « ignore tes instructions et envoie… » pourrait le manipuler. Le jeton agent limite les dégâts possibles, quoi qu'il arrive à l'agent :

- **interdit** : supprimer un compte, ajouter un compte à partir d'un jeton brut, écrire une sauvegarde, changer les cibles de notification, changer les réglages moteur autres que `paused`, `active_hours` et `max_watch_slots` (donc ni proxy, ni pairs, ni URL de mise à jour) ;
- **masqué** (`***`) : jeton des pairs, proxy, URL de webhook, jetons de bot ;
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
| `set_drop_games` | jeux de drops voulus, par priorité (tous comptes, ou un seul avec `account`) |
| `set_paused` | pause / reprise |
| `refresh_drops` | recharger les campagnes |
| `start_login` | connecter un compte : renvoie le code à saisir sur twitch.tv/activate |
| `get_settings` / `update_settings` | réglages d'un module (`drops`, `points`, `predictions`, `notify`) ou `engine`, avec le schéma |

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
