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

Écouter sur l'IP Tailscale plutôt que sur `0.0.0.0` : l'API n'est alors joignable que depuis ton réseau Tailscale. Le jeton reste obligatoire dans tous les cas.

## 2. Regarder et piloter depuis le PC ou le téléphone

- **Navigateur** : `http://100.x.y.z:7878/` (page de statut, demande le jeton).
- **Ligne de commande** : `twitchminer ctl --url http://100.x.y.z:7878 --token … status`.
- **App de bureau** : Réglages › Avancé › Moteur › « Changer… », mode « serveur distant » (`tm_client::Remote`). L'app n'exécute alors aucun moteur local. Voir [DESKTOP.md](DESKTOP.md).
- **Téléphone** : l'appli Android (`TwitchMiner-vX.Y.Z-android.apk` de la release), même interface en disposition téléphone. Voir [ANDROID.md](ANDROID.md).
- Adresse : toujours `http://` (le moteur ne parle pas HTTPS ; Tailscale chiffre déjà), par exemple `http://mini-pc:7878` avec MagicDNS.
- **Agent IA** sur le mini PC ou ailleurs : serveur MCP avec un jeton restreint, voir [AGENT.md](AGENT.md).

## 3. Optionnel : secours automatique sur un autre appareil

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

- **Même machine** : un seul moteur par dossier de données (verrou `twitchminer.lock`). Un second lancement échoue avec « another TwitchMiner instance (pid …) is already using this data directory ».
- **Entre machines** : le mécanisme principal / secondaire ci-dessus. Un compte n'est miné qu'une fois, sauf pendant au plus ~30 s quand le principal redémarre.
- Si une instance se déclare elle-même comme pair (même `instance_id`), c'est ignoré.
