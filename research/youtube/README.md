# Sonde YouTube (prototype de recherche, à jeter)

> **Recherche uniquement.** Ce dossier ne fait pas partie de TwitchMiner : aucun
> `Cargo.toml`, rien n'est compilé ni livré, aucun code de l'application n'en dépend.
> Contexte et conclusions : [`docs/research/YOUTUBE_DROPS.md`](../../docs/research/YOUTUBE_DROPS.md).

`yt_live_probe.py` (Python 3, bibliothèque standard seulement) interroge, **sans
compte**, les points d'entrée publics « innertube » de YouTube pour une vidéo en
direct, et n'affiche que ce qui compterait pour un futur module de drops YouTube :

- statut du direct (`videoDetails.isLive`, `liveBroadcastDetails`, `playabilityStatus`) ;
- paramètres du *heartbeat* (`heartbeatParams`, intervalle) ;
- manifestes proposés (HLS, DASH, SABR) selon le client (WEB, MWEB, TVHTML5, ANDROID_VR) ;
- URL de suivi de lecture que le lecteur appellerait (`api/stats/playback`, `watchtime`, `qoe`, `atr`, `ptracking`) et le nom de leurs paramètres ;
- tout champ qui ressemble à des drops / récompenses / comptes liés dans les réponses `player` et `next` ;
- en option, **un** appel `player/heartbeat` (vérification « le direct est-il en ligne ? »).

Ce qu'il ne fait **pas**, volontairement : aucune connexion à un compte, aucun cookie,
aucun ping `api/stats/*` (ce serait déjà simuler une vue), aucun segment vidéo
téléchargé. Environ 5 à 8 requêtes par exécution, espacées de 1,5 s.

## Utilisation

```bash
# id de vidéo ou @chaîne (résout le direct en cours de la chaîne)
python3 -I research/youtube/yt_live_probe.py @LofiGirl
python3 -I research/youtube/yt_live_probe.py @lolesports --clients web,android_vr --heartbeat
python3 -I research/youtube/yt_live_probe.py <video_id> --dump /tmp/yt-probe   # garde le JSON brut
```

Les versions de clients sont en tête du script (`CLIENTS`) : ce sont les valeurs
volatiles qui iraient, dans une vraie intégration, dans un manifeste comme
`manifest/twitch.json`.

## Ce qu'il a montré (26 octobre 2026, depuis une IP de centre de données)

- La page `watch` et le client **WEB** répondent « *Sign in to confirm you're not a
  bot* » / `UNPLAYABLE` : sans compte ni jeton BotGuard, pas de flux ni d'URL de suivi.
- **TVHTML5** répond « *The page needs to be reloaded* ».
- Seul **ANDROID_VR** renvoie `OK` avec HLS + DASH + SABR et les URL `api/stats/*`
  (`videostatsScheduledFlushWalltimeSeconds = [10, 20, 30]`, puis toutes les 300 s).
- `player/heartbeat` fonctionne sans compte : `status: OK`, `pollDelayMs: 15000`
  (ou `LIVE_STREAM_OFFLINE` avec l'heure prévue) : un bon moyen, peu coûteux, de
  savoir si un direct est en cours.
- Aucun champ « drops » / « récompense » / « compte lié » dans les réponses
  anonymes, y compris pour un direct des Worlds de LoL (partenaire listé par YouTube).
  Si l'indicateur « compte lié » des paramètres du lecteur existe, il n'apparaît
  probablement qu'avec un compte connecté : à vérifier par le propriétaire.
