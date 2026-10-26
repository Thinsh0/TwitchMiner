# En ligne de commande

Le daemon `twitchminer` fait tourner le moteur sans interface (serveur, Raspberry Pi, Termux) et se pilote par son API, en local ou à distance. Les binaires sont dans les archives des [Releases](https://github.com/Thinsh0/TwitchMiner/releases/latest) (`twitchminer-…-linux-musl.tar.gz` sont statiques, sans dépendance) et dans l'image Docker.

## Lancer le moteur

```bash
twitchminer run                       # écoute sur 127.0.0.1:7878
twitchminer run --bind 100.x.y.z:7878 # sur l'adresse Tailscale de la machine
#   No account yet. Open https://www.twitch.tv/activate and enter the code: ABCDEFGH
```

Au premier lancement, sans compte, il affiche un code de connexion Twitch. Les données (comptes, réglages, historique) sont dans un seul dossier : `--data-dir` ou `$TM_DATA_DIR`, sinon le dossier de données de la plateforme.

Page de statut : **http://127.0.0.1:7878/** (demande le jeton).

## Le piloter

```bash
twitchminer api-token                 # jeton de l'API (pour les applis)
twitchminer ctl status                # résumé lisible
twitchminer ctl login                 # ajouter un compte (code à valider sur Twitch)
twitchminer ctl set module:drops '{"games": ["Rust", "World of Warcraft"]}'   # ordre = priorité
twitchminer ctl add-channel xqc       # chaîne à suivre pour les points
twitchminer ctl call drops plan       # le plan des drops
twitchminer ctl call drops refresh    # relire les campagnes maintenant
twitchminer ctl events                # flux d'événements en direct
twitchminer ctl history               # historique
twitchminer ctl backup                # copie cohérente de la base
twitchminer ctl raw '{"cmd": "guest_invite", "name": "Sam"}'   # n'importe quelle commande de l'API
```

À distance : `--url http://<serveur>:7878 --token <jeton>` (ou `TM_API_URL`, `TM_API_TOKEN`). Avec Docker : `docker exec twitchminer twitchminer ctl status`.

`twitchminer --help` et `twitchminer ctl --help` listent tout ; les commandes et réglages sont décrits dans [API.md](API.md).

## Agents IA

`twitchminer mcp` sert le protocole MCP sur l'entrée standard, relié au daemon avec le jeton restreint des agents. Voir [AGENT.md](AGENT.md).
