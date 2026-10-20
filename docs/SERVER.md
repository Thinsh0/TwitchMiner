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

```bash
# binaire Linux statique (artefact CI "x86_64-unknown-linux-musl") dans ~/.local/bin
twitchminer run --bind 100.x.y.z:7878      # IP Tailscale du mini PC (tailscale ip -4)
twitchminer api-token                      # jeton à copier sur les autres appareils
```

En service : `packaging/systemd/twitchminer.service`, avec `Environment=TM_API_BIND=100.x.y.z:7878`. Docker fonctionne aussi (voir README).

Écouter sur l'IP Tailscale plutôt que sur `0.0.0.0` : l'API n'est alors joignable que depuis ton réseau Tailscale. Le jeton reste obligatoire dans tous les cas.

## 2. Regarder et piloter depuis le PC ou le téléphone

- **Navigateur** : `http://100.x.y.z:7878/` (page de statut, demande le jeton).
- **Ligne de commande** : `twitchminer ctl --url http://100.x.y.z:7878 --token … status`.
- **App native** (à venir) : mode « serveur distant », via `tm_client::Remote`. L'app n'exécute alors aucun moteur local.

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
