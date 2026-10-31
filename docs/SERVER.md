# Serveur maison + PC + téléphone

Exemple : un mini PC allumé en permanence, un PC et un téléphone, tous dans le même réseau **Tailscale**.

```
                Tailscale (chiffré)
  ┌────────────┐                ┌──────────────────────────┐
  │  mini PC   │◄─── API ───────│  PC / téléphone           │
  │  primary   │                │  secondary (peer = mini PC)│
  │  mine tout │                │  en veille, reprend si le │
  └────────────┘                │  mini PC tombe             │
                                └──────────────────────────┘
```

## 1. Le mini PC (instance principale)

### Avec Docker (le plus simple)

Image publiée à chaque release, pour `amd64` et `arm64` (Raspberry Pi, mini PC ARM) :

```bash
docker run -d --name twitchminer --restart unless-stopped \
  -v twitchminer:/data -p 7878:7878 ghcr.io/thinsh0/twitchminer
docker logs twitchminer                          # code de connexion Twitch au premier lancement
docker exec twitchminer twitchminer api-token    # jeton à copier sur les autres appareils
docker exec twitchminer twitchminer ctl status
```

- Les données (comptes, réglages, historique) sont dans le volume `twitchminer` : elles survivent aux mises à jour.
- Mettre à jour : `docker pull ghcr.io/thinsh0/twitchminer && docker rm -f twitchminer`, puis relancer la commande `docker run` ci-dessus.
- Pour n'écouter que sur Tailscale : `-p 100.x.y.z:7878:7878` au lieu de `-p 7878:7878`.
- **Dockge / Portainer / Compose** : une pile avec ce `compose.yaml`, puis le bouton *Update* (ou `docker compose pull && docker compose up -d`) à chaque nouvelle version :

  ```yaml
  services:
    twitchminer:
      image: ghcr.io/thinsh0/twitchminer:latest
      restart: unless-stopped
      environment:
        - TZ=Europe/Paris        # plages horaires à l'heure locale
      volumes:
        - data:/data
      ports:
        - 7878:7878
  volumes:
    data:
  ```

### Sans Docker

```bash
# binaire Linux statique de la release (x86_64- ou aarch64-unknown-linux-musl) dans ~/.local/bin
twitchminer run --bind 100.x.y.z:7878      # IP Tailscale du mini PC (tailscale ip -4)
twitchminer api-token                      # jeton à copier sur les autres appareils
```

En service : `packaging/systemd/twitchminer.service`, avec `Environment=TM_API_BIND=100.x.y.z:7878`.

Écouter sur l'IP Tailscale plutôt que sur `0.0.0.0` : l'API n'est alors joignable que depuis ton réseau Tailscale. Le jeton reste obligatoire dans tous les cas. Voir aussi [Garde-fous](#garde-fous).

## 2. Regarder et piloter depuis le PC ou le téléphone

- **Navigateur** : `http://100.x.y.z:7878/` (page de statut, demande le jeton).
- **Ligne de commande** : `twitchminer ctl --url http://100.x.y.z:7878 --token … status`.
- **App de bureau** : Réglages › Avancé › Moteur › « Changer… », mode « serveur distant » (`tm_client::Remote`). L'app n'exécute alors aucun moteur local. Voir [DESKTOP.md](DESKTOP.md).
- **Téléphone** : l'appli Android (`TwitchMiner-vX.Y.Z-android.apk` de la release), même interface en disposition téléphone. Voir [ANDROID.md](ANDROID.md).
- Adresse : toujours `http://` (le moteur ne parle pas HTTPS ; Tailscale chiffre déjà), par exemple `http://mini-pc:7878` avec MagicDNS.
- **Agent IA** sur le mini PC ou ailleurs : serveur MCP avec un jeton restreint, voir [AGENT.md](AGENT.md).

## 3. Optionnel : miner pour un ami

Le serveur peut miner aussi le compte Twitch d'un ami, sans qu'il voie les tiens :

1. Dans l'app (jeton complet), **Réglages › Invités** : entrer son nom, **Inviter**. Un jeton s'affiche, une seule fois : le lui envoyer avec l'adresse du serveur.
2. Il installe l'app (Windows ou Android), choisit « Serveur distant » et colle l'adresse et son jeton, puis **Connecter un compte Twitch** : le compte devient le sien.
3. Il ne voit et ne règle que ses comptes (drops, chaînes, notifications à lui, pause de son compte). Les tiens, les réglages du moteur, le journal et les autres invités n'existent pas pour lui : le serveur ne les lui envoie jamais. Tu vois son compte (marqué « Invité : … ») et peux le supprimer ; **Révoquer** coupe son accès.

Réseau : avec Tailscale, partager seulement la machine du serveur avec lui (console Tailscale › Machines › … › **Share**) : il l'atteint par son nom sans voir le reste de ton réseau. Ne jamais ouvrir le port 7878 sur Internet.

Il lit les réglages partagés des modules (ta liste de jeux et de chaînes sert de valeur par défaut à son compte tant qu'il n'a pas la sienne), jamais tes destinations de notifications. Détails : [API.md](API.md).

Ce qu'un invité ne peut pas faire, même en appelant l'API à la main :
- **se connecter avec un compte Twitch déjà présent** sur le serveur (le tien ou celui d'un autre invité) : refusé, les identifiants existants ne sont pas touchés. Il peut reconnecter ses propres comptes ;
- **viser ton réseau avec ses notifications** : ses destinations doivent être des adresses `https://` publiques (ni IP privée, ni Tailscale `100.x`, ni `localhost`, ni nom local), vérifiées à l'enregistrement puis à chaque envoi après résolution DNS, sans suivre de redirection ; pas d'en-têtes HTTP en plus ni de clé Firebase à lui (ses téléphones utilisent la tienne, qu'il ne voit pas). Les erreurs d'envoi qu'il voit restent vagues ;
- **dépasser les limites** `guest_limits` des réglages du moteur (défauts : 2 connexions Twitch en attente, 5 comptes, 10 destinations par compte, 60 recherches de jeux par minute). Toi, tu n'as aucune limite.

## 4. Optionnel : secours automatique sur un autre appareil

Si tu veux que le téléphone (ou le PC) prenne le relais quand le mini PC est éteint, fais tourner un moteur dessus en **secondaire** :

```bash
twitchminer ctl set engine '{
  "role": "secondary",
  "peers": [{ "name": "mini-pc", "url": "http://100.x.y.z:7878", "token": "<jeton du mini PC>" }]
}'
```

Comportement, vérifié en conditions réelles :

| Situation | Résultat |
|---|---|
| Le mini PC mine le compte | le secondaire passe le compte en `standby` (≤ 30 s, et dès le démarrage s'il voit le pair) |
| Le mini PC devient injoignable | le secondaire reprend après 2 contrôles sans réponse (≈ 1 min) |
| Le mini PC revient | il reprend (principal) et le secondaire repasse en `standby` (≤ 30 s) |
| Deux **principaux** minent le même compte | notice `duplicate:<compte>` sur l'instance qui connaît l'autre comme pair |

Pendant `standby`, le compte n'est pas miné du tout sur le secondaire : aucun visionnage, aucune connexion PubSub. Le statut indique quel pair s'en occupe.

## Garde-fous

### Plusieurs instances

- **Même machine** : un seul moteur par dossier de données (verrou `twitchminer.lock`). Un second lancement échoue avec « another TwitchMiner instance (pid …) is already using this data directory ».
- **Entre machines** : le mécanisme principal / secondaire ci-dessus. Un compte n'est miné qu'une fois, sauf pendant au plus ~30 s quand le principal redémarre.
- Si une instance se déclare elle-même comme pair (même `instance_id`), c'est ignoré.

### Sécurité : modèle de menace

TwitchMiner est pensé pour un **réseau privé** (Tailscale, réseau local) : l'API parle HTTP simple, sans TLS. Tailscale chiffre le trafic et choisit qui peut joindre la machine ; le jeton décide de ce que chacun peut faire. On veut se protéger :
- d'un appareil du réseau qui n'a pas de jeton (ou un mauvais) ;
- d'un **invité** (jeton invité) qui essaierait d'utiliser le serveur contre ton réseau ou tes comptes ;
- d'un **agent IA** manipulé par un texte piégé (titres de stream…), voir [AGENT.md](AGENT.md) ;
- d'une **page web** ouverte dans ton navigateur qui tenterait de parler à l'API (CSRF, DNS rebinding).

Hors de portée : quelqu'un qui a déjà ton jeton complet, ou un accès à ton compte sur la machine du serveur (il peut lire la base).

### Ce qui est protégé par défaut

- **Jeton obligatoire** sur toute l'API, sauf `GET /api/v1/health` et la page d'accueil (statique, elle demande le jeton). Un moteur lancé sans jeton (usage intégré, `ServerConfig.token: None`) refuse de démarrer sur une autre adresse que `127.0.0.1` / `::1`.
- **Trois niveaux de jeton** : complet (toi, tes appareils, les pairs), agent (restreint, désactivé par défaut), invité (ses comptes seulement). Voir [API.md](API.md).
- **Mauvais jetons répétés** : après 10 échecs depuis une même adresse IP, elle est refusée 30 s (réponse `429`, en-tête `Retry-After`), puis le double à chaque récidive, jusqu'à 15 min. Une connexion réussie remet le compteur à zéro. Les connexions locales (`127.0.0.1`, `::1` : l'app et `ctl` sur la même machine) ne sont jamais bloquées. Pendant un blocage, même le bon jeton attend : corriger le jeton de l'app, puis patienter.
- **Navigateur** :
  - l'en-tête `Host` doit être une adresse IP, `localhost` ou un nom local (un seul mot comme `mini-pc`, `*.local`, `*.lan`, `*.home.arpa`, `*.internal`, `*.ts.net` de MagicDNS). Un autre nom est refusé (`403`) : protège du *DNS rebinding* (un site qui ferait pointer son nom vers ton serveur). Pour un nom public (reverse proxy), l'ajouter à `api_allowed_hosts` ;
  - une requête qui agit (`POST`, ouverture d'un WebSocket) envoyée par une page d'un autre site (en-tête `Origin` différent) est refusée (`403`). Les clients natifs (app, `ctl`, agents) n'envoient pas d'`Origin` ;
  - plus de `Access-Control-Allow-Origin: *` : les en-têtes CORS ne sont donnés qu'à la même origine ou aux noms de `api_allowed_hosts` ;
  - `POST /api/v1/command` et `/api/v1/mcp` exigent `Content-Type: application/json` (un formulaire HTML ne peut pas l'envoyer à un autre site) ;
  - le jeton n'est plus mis dans l'URL du WebSocket : l'app l'envoie en en-tête, la page d'accueil dans le premier message. `?token=` reste accepté pour les anciens clients, mais déconseillé (les adresses finissent dans les journaux et l'historique).
- **Invités** : destinations de notification limitées aux adresses `https://` publiques, comptes déjà présents refusés, limites `guest_limits` (voir plus haut).
- **Données** : sous Unix, le dossier de données est en `0700` et les sauvegardes (`backup`) en `0600`, dans un dossier `backups/` en `0700` ; un chemin de sauvegarde explicite doit être absolu. Sous Windows, le dossier par défaut (`%APPDATA%\Thinsh0\TwitchMiner\data`) hérite des droits du profil utilisateur (toi, SYSTEM, les administrateurs) ; TwitchMiner ne les restreint pas davantage. En mode portable ou avec `--data-dir`, choisir un dossier que les autres comptes de la machine ne lisent pas.
- **Manifeste Twitch** (téléchargé toutes les 6 h) : adresses `https://` de `twitch.tv` uniquement, au plus 256 Kio, et une révision au plus 1000 au-dessus de l'actuelle (une révision énorme bloquerait les corrections suivantes). Il n'est pas encore signé : la confiance repose sur HTTPS et sur ce dépôt GitHub.
- **Mises à jour de l'app Windows** : installées seulement si leur signature ed25519 est valide (voir [DESKTOP.md](DESKTOP.md#mises-à-jour)).

### Exposer le serveur hors de Tailscale

À éviter. Si c'est nécessaire :
- **Jamais le port 7878 directement sur Internet** : HTTP simple, le jeton passerait en clair. Avec Docker, `-p 7878:7878` publie sur toutes les interfaces : préférer `-p 100.x.y.z:7878:7878` (Tailscale) ou `-p 127.0.0.1:7878:7878` (derrière un proxy local).
- Mettre un **reverse proxy avec TLS** devant (Caddy, nginx, Traefik), le moteur n'écoutant que sur `127.0.0.1`, et déclarer le nom public :

  ```bash
  twitchminer run --bind 127.0.0.1:7878
  twitchminer ctl set engine '{"api_allowed_hosts": ["tm.example.com"]}'
  ```

  ```caddyfile
  tm.example.com {
      reverse_proxy 127.0.0.1:7878    # WebSocket compris
  }
  ```

  Le proxy doit transmettre les WebSocket (`Upgrade`) et garder l'en-tête `Host` d'origine (comportement par défaut de Caddy ; avec nginx : `proxy_set_header Host $host;`, `proxy_set_header Upgrade $http_upgrade;`, `proxy_set_header Connection "upgrade";`).
- Derrière un proxy local, toutes les requêtes arrivent de `127.0.0.1` : le blocage après mauvais jetons ne s'applique plus (les en-têtes `X-Forwarded-For`, falsifiables, ne sont pas lus). Faire limiter le débit par le proxy (`rate_limit` de Caddy, `limit_req` de nginx…).
- Changer de jeton après toute fuite possible : `twitchminer api-token --rotate` (aussi `--agent`), et `guest_remove` pour un invité.

### Limites connues

- Sans Tailscale (réseau local seul), le trafic n'est pas chiffré : quelqu'un qui écoute le réseau voit le jeton.
- Avec un proxy sortant (`engine.proxy`), les destinations des invités sont vérifiées par une résolution DNS juste avant l'envoi, mais c'est le proxy qui résout pour de bon.
- Le manifeste Twitch n'est pas signé (voir plus haut).
- Signaler une faille : [SECURITY.md](../SECURITY.md).
