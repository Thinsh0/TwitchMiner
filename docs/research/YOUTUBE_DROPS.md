# Recherche : prendre en charge les « drops YouTube » ?

> Document de recherche (R&D), pas une fonctionnalité. Rédigé le 26 octobre 2026.
> Rien ici n'est branché dans l'application. Le prototype jetable est dans
> [`research/youtube/`](../../research/youtube/README.md).
> Toutes les observations techniques ont été faites **sans compte** (requêtes publiques
> anonymes, une dizaine au total). Ce qui demande un compte connecté est signalé
> **[non vérifié]**.

## Résumé exécutif

- **Il n'existe pas de « Drops YouTube » au sens de Twitch.** YouTube propose seulement des
  *applications connectées* (`youtube.com/account_sharing`). Une quinzaine d'éditeurs
  (Activision, Blizzard, EA, Epic, Riot, Ubisoft, Supercell, Garena, Moonton, Krafton…) y
  reçoivent les données de visionnage des spectateurs qui ont lié leur compte. **C'est
  l'éditeur qui décide** qui gagne quoi. Les récompenses arrivent seules sur le compte de
  jeu, en 24 h au plus.
- **Il n'y a ni catalogue de campagnes, ni progression, ni bouton « Réclamer », ni API.**
  Les campagnes sont annoncées sur les sites des éditeurs et se limitent à leurs chaînes
  officielles. Le spectateur ne voit au mieux qu'un indicateur « compte lié » dans les
  paramètres du lecteur.
- **Techniquement, c'est bien plus dur que Twitch.**
  - Le crédit dépend de la télémétrie du lecteur (`api/stats/watchtime`…) liée à une session Google connectée.
  - Le client WEB exige des jetons d'attestation BotGuard (*PO tokens*) et le protocole SABR.
  - Les IP de serveur reçoivent « *Sign in to confirm you're not a bot* ».
  - La seule connexion OAuth qu'accepte innertube est celle du client TV, or **l'app TV est exclue des récompenses**. Il resterait les cookies web (renouvelés en permanence) dans un vrai navigateur.
- **Risque élevé.**
  - Les CGU YouTube interdisent l'accès automatisé et tout ce qui fausse les mesures d'audience.
  - Les règlements des éditeurs (ex. CDL 2026) disqualifient les scripts et les comptes multiples.
  - Le compte exposé est le **compte Google entier** (Gmail, Drive…), pas un compte de streaming.
- **Recommandation : no-go pour un module de minage.** Au mieux, on peut ajouter plus tard un
  module « assistant » sans risque : un calendrier des diffusions à récompenses, des alertes,
  et un lien vers la page de liaison, l'utilisateur regardant lui-même. On réévaluera si
  YouTube publie une API de drops, ou si les expériences de la section 7 montrent un
  crédit possible avec un client léger.

---

## 1. Que sont les « drops YouTube » aujourd'hui ?

### 1.1 Le mécanisme de YouTube : les applications connectées

La page d'aide officielle *Connect your account to earn off-platform rewards*
(support.google.com/youtube/answer/9024948, consultée le 26/10/2026) décrit tout le dispositif.

- **Principe** : on lie son compte Google à un compte partenaire. On peut alors « *earn
  off-platform rewards when you watch eligible live streams* ».
- **Partenaires listés** :
  - Activision (Call of Duty) ;
  - Battle.net (Overwatch, Hearthstone ; WoW pour la BlizzCon) ;
  - Electronic Arts (FIFA/FC, Madden) ;
  - Epic Games (Fortnite) ;
  - Garena (Free Fire) ;
  - Moonton (Mobile Legends) ;
  - Krafton et Tencent (PUBG, PUBG Mobile) ;
  - Riot Games (LoL, LoR, TFT) ;
  - Supercell (Clash Royale) ;
  - Ubisoft (Assassin's Creed, Rainbow Six) ;
  - NBA et NFL (récompenses et personnalisation).
- **Éligibilité** :
  - être connecté ;
  - regarder sur **ordinateur, l'app mobile ou le site mobile** (y compris en incrustation) ;
  - « *If you watch on embedded players, the YouTube Smart TV app, or via casting, you won't be eligible for rewards* » ;
  - comptes personnels seulement, pas de comptes de marque ;
  - pas de liaison sur les vidéos « conçues pour les enfants » ;
  - fonction parfois indisponible pour les comptes supervisés.
- **Pas de règle de durée côté YouTube.** La page ne fixe aucun temps minimum, aucune
  région ni aucun âge : c'est l'éditeur qui décide. « *Our partners ultimately figure out
  which eligible viewers will win based on the rules for each stream.* »
- **Livraison** : « *Rewards will be distributed to your partner account after you've won* »,
  en un jour au plus. Il n'y a pas de bouton « Réclamer ».
- **Savoir si une vidéo est éligible** : « *Eligible videos include an option to review your
  linked account state* » dans les paramètres du lecteur. Les comptes liés sont visibles sur
  `myaccount.google.com/accountlinking` : « *As long as this page shows your accounts as
  connected, you're eligible for rewards.* »
- **Données partagées** : YouTube transmet au partenaire l'historique de visionnage et le
  statut d'abonnement.

### 1.2 Comment les éditeurs s'en servent (exemples récents)

| Campagne | Règles côté YouTube | Source |
|---|---|---|
| BlizzCon 2026 (WoW), 12–13/09/2026 | Paliers à 2, 4, 8 et 12 h de visionnage. **Uniquement la chaîne officielle World of Warcraft**, « *for the entirety* ». Livraison automatique en jeu (≤ 24 h). Sur Twitch, n'importe quelle chaîne WoW éligible, réclamation manuelle et une campagne plus longue. | worldofwarcraft.blizzard.com/en-us/news/24295396 |
| Call of Duty League (2023–2026) | Liaison Activision ↔ YouTube. Paliers quotidiens (1 h à 3 h). **Direct seulement** : pas de rediffusion, ni de *watch party* ou de co-stream. Livraison automatique, parfois en une à deux semaines. « *you cannot track your progress on Viewership Rewards* » sur YouTube. | ggrecon.com (15/06/2023), esports.gg (CDL Champs 2025), règlement CDL 2026 Major 2 (PDF callofduty.com) |
| EA SPORTS FC Pro (FC 26) | Liaison via *Connected apps*. « *Watch the stream for the required watch duration* ». La récompense arrive comme **objectif Ultimate Team** à réclamer en jeu (≤ 24 h). | ea.com/games/ea-sports-fc/fc-pro/how-to-earn-rewards |
| Overwatch League (2020–2023, terminée) | Jetons de ligue par heure de direct sur YouTube, avec un Battle.net lié. Nombreux crédits manquants signalés, et l'app TV ne compte pas. | forums Blizzard, articles 2020–2024 |
| LoL Esports (Worlds 2026, en cours) | Les drops Riot passent par **lolesports.com** connecté au compte Riot, pas par YouTube, alors que Riot figure parmi les partenaires et que les Worlds sont aussi diffusés sur YouTube. | dexerto.com (Worlds 2024), guides 2026 |

### 1.3 Deux familles à distinguer

1. **Récompenses YouTube « natives »** : un partenaire des applications connectées reçoit
   les données de visionnage de YouTube, sur des diffusions qu'il a choisies (presque
   toujours ses chaînes officielles). C'est ce qu'on appelle « YouTube drops ».
2. **Programmes « regardez chez nous »** : l'éditeur mesure lui-même le temps passé sur son
   propre site ou lecteur, qui intègre parfois une vidéo YouTube. Exemple : lolesports.com
   et ses *heartbeats* vers l'API de récompenses de Riot. YouTube n'y sert que de
   transport : un lecteur intégré n'est pas éligible côté YouTube, mais l'éditeur compte
   par ses propres moyens.

**Différence de fond avec Twitch.** Sur Twitch, les drops sont un produit de la plateforme :
n'importe quel streamer d'un jeu y participe, la progression est exposée en GQL, la
réclamation est explicite, et il existe une page Inventaire. Sur YouTube, la plateforme ne
fournit qu'un tuyau de données vers l'éditeur. Il n'y a **aucune source de vérité
consultable** pour la progression.

## 2. Comment le temps de visionnage est mesuré techniquement

### 2.1 Les appels du lecteur web (innertube)

| Appel | Rôle | Compte requis ? |
|---|---|---|
| `POST /youtubei/v1/player` | Métadonnées, statut du direct (`videoDetails.isLive`, `microformat…liveBroadcastDetails`), `playabilityStatus`, `streamingData` (HLS / DASH / `serverAbrStreamingUrl` SABR), `heartbeatParams`, `playbackTracking` (URL de télémétrie) | Non, mais voir 2.3 |
| `POST /youtubei/v1/next` | Page de lecture : titre, chaîne, description, chat, recommandations, carte « jeu » | Non |
| `POST /youtubei/v1/player/heartbeat` | Direct seulement : le lecteur le rappelle toutes les `intervalMilliseconds` pour savoir si le direct est en ligne (`LIVE_STREAM_OFFLINE`, heure prévue) | Non (vérifié) |
| `GET s.youtube.com/api/stats/playback` | Début de lecture (« vue ») | Non |
| `GET s.youtube.com/api/stats/watchtime` | Plages réellement regardées (`cpn`, `st`/`et`, `cmt`, `state`, `lact`, `vis`…) envoyées périodiquement | Liées à la session si connecté |
| `POST s.youtube.com/api/stats/qoe` | Qualité d'expérience (mise en mémoire tampon, formats) | — |
| `GET /ptracking`, `api/stats/atr` | Suivi complémentaire (chaîne, attribution) | — |
| `POST *.googlevideo.com/videoplayback` | Segments vidéo. Sur WEB, protocole **SABR/UMP** (en-têtes `application/vnd.yt-ump`) avec jeton PO | Jeton PO sur WEB/MWEB |

Rythme observé (sonde, client ANDROID_VR) : `videostatsScheduledFlushWalltimeSeconds =
[10, 20, 30]`, puis `videostatsDefaultFlushIntervalSeconds = 300`. Le travail
expérimental de Xosrov/YouTube-Playback-Metrics décrit pour le web des envois toutes
les 10 s pendant 30 s, puis toutes les 40 s. Le *heartbeat* est à 15 s sur WEB et 30 s
sur ANDROID_VR ; la réponse donne `pollDelayMs: 15000`.

### 2.2 Ce qui crédite vraiment une récompense : inconnu

YouTube ne documente pas quel signal il transmet aux partenaires. Hypothèse la plus
probable **[non vérifié]** : la même télémétrie que les statistiques d'audience
(`watchtime` liée au compte connecté et au `cpn` de lecture), filtrée par les systèmes
anti-fraude de YouTube. Les plaintes de crédits manquants (OWL, CDL), l'exclusion des
lecteurs intégrés et de l'app TV, et l'obligation de rester sur la chaîne officielle
vont dans ce sens.

**Analogie avec la règle Twitch d'octobre 2026** (HEAD sur chaque segment HLS). Rien ne
dit qu'un ping `watchtime` sans téléchargement de segments suffise. YouTube peut
recouper la télémétrie avec les requêtes `videoplayback` réelles : `cpn` et
`ei`/`plid` relient les deux. Et SABR fait passer le choix des segments par le serveur.
Il faut **supposer qu'un vrai flux (même en 144p) est nécessaire**, à vérifier en 7.

### 2.3 Ce que montre la sonde anonyme (26/10/2026, IP de centre de données)

| Client | Réponse `player` pour un direct en cours |
|---|---|
| WEB | `UNPLAYABLE – Video unavailable`. La page `watch` affiche « *Sign in to confirm you're not a bot* ». Pas de flux, pas de `playbackTracking` |
| MWEB, TVHTML5 | `UNPLAYABLE – The page needs to be reloaded` |
| ANDROID_VR | `OK` : HLS + DASH + SABR, URL `api/stats/*`, `heartbeatParams` |

- `player/heartbeat` (WEB, anonyme) : `OK`, `liveStreamabilityRenderer.pollDelayMs = 15000`.
  Pour un direct programmé (Worlds 2026), on obtient `LIVE_STREAM_OFFLINE` et
  `scheduledStartTime`. C'est l'équivalent bon marché du contrôle « en ligne » de Twitch.
- **Aucun champ lié aux drops, récompenses ou comptes liés** dans `player` ou `next`, y
  compris pour le direct des Worlds de LoL (catégorie *Gaming*, partenaire Riot).
  L'indicateur « compte lié » des paramètres du lecteur n'est donc pas exposé à un
  anonyme. Il est sans doute dans la réponse `player` ou `next` d'une session connectée
  et liée **[non vérifié]**.

### 2.4 Jetons PO, BotGuard et SABR

Sources : wiki yt-dlp *PO Token Guide*, modifié le 12/07/2026 ; issue yt-dlp #12482,
ouverte le 26/02/2025.

- Les jetons PO (*Proof of Origin*) sont produits par des attestations : BotGuard sur le
  web, DroidGuard sur Android, iOSGuard sur iOS. Ils sont exigés pour les requêtes
  `videoplayback` (GVS) des clients `web`, `mweb`, `web_music`, `tv_simply`, et en
  partie `android` et `ios`. Sans eux : erreurs 403, voire « *your account or IP address
  being blocked* ».
- Ils sont le plus souvent liés à l'identifiant vidéo, ou à la session (visiteur ou compte).
- Point utile : « *HLS live streams do not require a PO Token, except on the ios client* ».
  En revanche, le web n'offre plus que du SABR pour les vidéos, et un vrai lecteur web
  produit de toute façon ses jetons BotGuard. Un client qui n'en envoie jamais est
  facile à repérer.
- Seuls les clients sans jeton (ANDROID_VR, TV) passent en anonyme. Or **aucun des deux
  n'est un appareil éligible** aux récompenses : l'aide cite ordinateur, app mobile et
  site mobile.

## 3. Authentification pour une application sans interface

| Option | Utilisable avec innertube ? | Éligible aux récompenses ? | Remarques |
|---|---|---|---|
| **OAuth « appareil » du client YouTube TV** (code à saisir, comme TwitchMiner pour Twitch) | Oui, mais **seulement avec les clients TV** depuis novembre 2024 : « *restricts OAuth to only work with TV clients as originally intended* » (yt-dlp #11462, ouverte le 05/11/2024, close le 16/11/2024 ; OAuth retiré de yt-dlp par #11558) | **Non** : « *YouTube Smart TV app* » exclue | Identifiants du client TV non publics ; les réutiliser revient à usurper l'app officielle |
| **OAuth « appareil » officiel** (notre propre client Google) | Non : jetons pour l'API Data v3, pas pour innertube | Sans objet | Portées autorisées sur ce flux : `youtube` et `youtube.readonly`, entre autres. Il n'y a **aucun appel de visionnage ou de récompense** dans l'API Data. Les règles développeur interdisent d'automatiser des vues |
| **Cookies de session web** (`SID`, `__Secure-3PSID`, `SAPISID`…) + en-tête `SAPISIDHASH` | Oui (client WEB) | Oui, en théorie | yt-dlp : « *YouTube rotates account cookies frequently on open YouTube browser tabs* ». Il faut les exporter depuis une fenêtre privée, et elles expirent quand même. Aucun moyen propre de les obtenir sans navigateur. Risque de défis de sécurité Google (« appareil inconnu », recapture) |
| **Navigateur réel piloté** (Chromium sans interface + profil) | Oui (c'est le vrai lecteur) | Oui | Méthode du seul outil connu qui ait marché (ow-league-tokens v2). Lourd (centaines de Mo de RAM), incompatible avec le binaire statique et l'Android actuel de TwitchMiner. Google « *may log you out* » |

Durée de vie des jetons OAuth Google (developers.google.com/identity/protocols/oauth2) :

- un jeton de rafraîchissement inutilisé expire au bout de 6 mois ;
- il dure 7 jours si le projet est en statut « Testing » ;
- 100 jetons au plus par compte et par client (le plus ancien est révoqué en silence) ;
- révocation possible à tout moment par l'utilisateur.

Conclusion : **aucune voie d'authentification à la fois propre, durable et éligible.** Le
modèle Twitch de TwitchMiner (code appareil du client Android TV, jeton durable, GQL)
n'a pas d'équivalent ici.

## 4. Projets existants

Aucun outil maintenu ne cible les « drops YouTube » en général. On trouve seulement des
outils propres à un éditeur, tous à l'arrêt.

| Projet | Langage | Approche | État |
|---|---|---|---|
| [ucarno/ow-league-tokens](https://github.com/ucarno/ow-league-tokens) | Python | v1 : simulait le site de l'OWL. **v2 « The YouTube Update »** : vrai Chrome/Brave piloté par `undetected-chromedriver`, sans interface en option. Connexion Google manuelle dans la fenêtre, profils conservés, liaison Battle.net via `youtube.com/account_sharing` | Dernière version v2.0.6 (15/07/2023), dernier commit le 26/10/2024. Mort avec l'OWL. Problèmes connus : « *Google may log you out* », Firefox écarté car « *Google will be able to detect automation* », plantages du pilote, Docker difficile |
| [LeagueOfPoro/CapsuleFarmerEvolved](https://github.com/LeagueOfPoro/CapsuleFarmerEvolved) | Python | Programme « regardez chez nous » : simule le trafic de lolesports.com (*heartbeats* vers Riot), sans navigateur ni YouTube | Dernier commit le 15/03/2023. Bandeau : « *RIOT TOOK ACTION AGAINST CAPSULE FARMER EVOLVED! IT STILL WORKS BUT YOUR ACCOUNT MIGHT GET SHADOWBANNED* » |
| [MShawon/YouTube-Viewer](https://github.com/MShawon/YouTube-Viewer) et similaires | Python | Robots de vues Selenium (gonflement d'audience) | Hors sujet. C'est exactement ce que les CGU visent |
| [DevilXD/TwitchDropsMiner](https://github.com/DevilXD/TwitchDropsMiner), [rdavydov/Twitch-Channel-Points-Miner-v2](https://github.com/rdavydov/Twitch-Channel-Points-Miner-v2) | Python | Twitch seulement. Aucune trace de prise en charge de YouTube dans leurs tickets | Actifs |
| [yt-dlp](https://github.com/yt-dlp/yt-dlp), [LuanRT/googlevideo](https://jsr.io/@luanrt/googlevideo) | Python, TS | Pas des mineurs, mais la meilleure documentation vivante d'innertube : clients, jetons PO, SABR/UMP | Très actifs, ce qui montre la vitesse à laquelle YouTube change |

Ce qu'on en retient :

- le seul outil YouTube qui ait marché utilisait un **vrai navigateur connecté** ;
- personne n'a publié de reproduction « protocole seul » de la télémétrie avec crédit de récompenses ;
- les éditeurs réagissent (Riot contre CapsuleFarmer).

## 5. Conditions d'utilisation et risques

**CGU YouTube** (version du 15/12/2023, youtube.com/static?template=terms), *Permissions and
Restrictions*. Il est interdit :

- d'« *access the Service using any automated means (such as robots, botnets or scrapers)* »,
  sauf moteurs de recherche ou accord écrit ;
- de « *cause or encourage any inaccurate measurements of genuine user engagement with the
  Service* » ;
- de « *circumvent, disable, fraudulently engage with, or otherwise interfere with any part
  of the Service* ».

YouTube peut suspendre ou résilier **le compte Google** ou l'accès au service.

**Règles développeur YouTube** (mises à jour le 15/10/2026,
developers.google.com/youtube/terms/developer-policies) :

- « *you must not automate or trigger views* » (III.I.2) ;
- « *must not use undocumented APIs without express permission* » (III.D.7, innertube l'est) ;
- pas de lecteur en arrière-plan (III.I.9) ;
- pas de récompense en échange d'engagement (III.F.3).

**Politique d'engagement artificiel** (support.google.com/youtube/answer/3399767) : elle
vise surtout les créateurs (« *using automatic systems* »), mais précise que le trafic
artificiel n'est pas compté. Les visionnages d'un robot risquent donc simplement de ne
rien créditer.

**Règlements des éditeurs** : le règlement CDL 2026 Major 2 (et CODNEXT 2023) annule toutes
les participations en cas de « *multiple accounts, or any other methods including, without
limitation, script, macro or any automated or other means which subverts the intended
operation of the Promotion* ».

**Clairement interdit** : comptes multiples pour multiplier les récompenses, robots de vues,
contournement de BotGuard ou des jetons PO, réutilisation des identifiants OAuth du client TV.

**Zone grise** : laisser un vrai navigateur ouvert sur un direct, onglet en arrière-plan.
C'est ce que font beaucoup de spectateurs, et c'est techniquement indiscernable si
c'est vraiment un navigateur sur une machine personnelle.

**Comparaison avec Twitch.**

- Les CGU Twitch interdisent aussi l'accès par « *robot, spider, scraper… or other automated
  means* ».
- Twitch tolère de fait les mineurs depuis des années : ils sont nombreux, publics,
  dans les tutoriels de Docker et d'Unraid, et les sanctions sont rares.
- Sur YouTube, l'enjeu est différent :
  1. le compte exposé est le compte Google principal, pas un compte de streaming jetable ;
  2. Google a une détection d'automatisation agressive (BotGuard, contrôles d'IP, défis de connexion) ;
  3. l'éditeur peut retirer les récompenses ou bannir le compte de jeu.

## 6. Faisabilité pour TwitchMiner

### 6.1 Ce que demanderait l'architecture

TwitchMiner est aujourd'hui **mono-plateforme jusque dans le cœur**.

- `AccountInfo` est une identité Twitch.
- `ModuleContext` expose `gql()` et `pubsub()` de `tm-twitch`.
- L'ordonnanceur (`crates/core/src/scheduler.rs`) lance `tm_twitch::watch`.
- `ChannelMeta` n'a pas de notion de plateforme.

Ajouter YouTube n'est donc pas « un module de plus ». Il faudrait :

1. **`tm-youtube`** (nouveau crate, à côté de `tm-twitch`) : client innertube (`player`,
   `next`, `player/heartbeat`), résolution `@chaîne` → direct en cours, état du direct,
   et un *watcher* (flux HLS de plus basse qualité + télémétrie) **si** les expériences
   montrent qu'il crédite.
2. **`manifest/youtube.json`** (ou une section du manifeste), versionné avec `revision`
   comme pour Twitch :
   - noms et versions de clients, *user agents* ;
   - liste curée des campagnes : éditeur, chaînes officielles (`channelId`), dates,
     paliers, « direct seulement ».

   Comme YouTube n'a pas d'API de campagnes, cette liste serait **maintenue à la main**.
3. **Le cœur** :
   - une notion de `platform` sur les comptes, les chaînes et les `WatchRequest` ;
   - des créneaux de visionnage par plateforme ;
   - des identifiants YouTube stockés comme ceux de Twitch (jamais dans `State`) ;
   - un flux de connexion différent (cookies ou navigateur, pas de code appareil).

   C'est un changement transversal de `tm-core`, de `tm-server` (API, `access.rs` : les
   cookies Google sont des secrets) et de `tm-ui`.
4. **Le module `youtube_rewards`** :
   - choisir la diffusion à suivre dans le manifeste ;
   - vérifier qu'elle est en direct (*heartbeat*) ;
   - demander un créneau ;
   - compter le temps **localement**, faute de progression côté serveur ;
   - publier par `set_data`.

   Pas de réclamation, puisque la livraison est automatique côté éditeur.
5. **Événements** : `DropProgress` et `DropClaimed` ne conviennent pas, car il n'y a ni
   identifiant de drop ni confirmation. Il faudrait au plus un `YoutubeWatchProgress`
   (temps local estimé) ou rester sur `set_data`. On ne pourrait jamais afficher
   « récompense obtenue » de façon fiable.
6. **Interface** : une section YouTube (liaison des comptes, diffusions à venir, temps
   estimé), traduite en `fr`. Disposition compacte à vérifier.

### 6.2 Effort estimé

| Variante | Contenu | Effort | Risque |
|---|---|---|---|
| **A. Assistant (recommandé si on fait quelque chose)** | Module `youtube_schedule` : manifeste curé des diffusions à récompenses, état en direct par `player/heartbeat` anonyme, alertes par `notify`, liens vers `youtube.com/account_sharing` et la diffusion. **Aucun visionnage simulé, aucun compte Google** | 3–5 jours | Faible (lecture publique ; rester sous quelques requêtes par minute) |
| B. Mineur « protocole » | A + cookies web + client WEB + BotGuard/PO tokens + SABR + télémétrie | 4–8 semaines, puis maintenance continue (YouTube change tous les mois, cf. yt-dlp) | Très élevé : peut-être impossible sans exécuter BotGuard ; crédit non garanti ; compte Google exposé |
| C. Mineur « navigateur » | A + Chromium sans interface piloté (CDP), profil persistant | 2–3 semaines | Élevé : dépendance lourde, pas d'Android ni de musl statique, déconnexions Google, CGU |

### 6.3 Plus grandes inconnues

1. Quel signal crédite vraiment : télémétrie seule, ou télémétrie + segments + BotGuard ?
2. L'éligibilité est-elle visible dans `player`/`next` pour une session connectée et
   liée ? C'est l'indicateur « compte lié » des paramètres du lecteur.
3. Durée de vie réelle de cookies web utilisés par un client non navigateur, et fréquence
   des défis de sécurité.
4. Le client mobile (app Android ou iOS, éligible) est-il imitable ? Il exige des jetons
   DroidGuard ou iOSGuard : a priori non.
5. Comment les éditeurs repèrent-ils les abus (comptes multiples, sessions sans
   interaction), et avec quelles sanctions ?

### 6.4 Recommandation

**No-go pour un mineur YouTube (variantes B et C).** Les raisons :

- il n'y a pas de progression ni de réclamation à automatiser ;
- le gain est faible (quelques campagnes d'éditeurs par an, sur leurs seules chaînes officielles) ;
- l'interdiction est nette (CGU, règles développeur, règlements des éditeurs) ;
- le risque touche le compte Google principal ;
- la seule authentification durable (TV) est inéligible ;
- l'approche « navigateur » casserait les principes du projet : binaire natif léger,
  Android, un seul dossier de données.

**Go conditionnel pour la variante A**, si le propriétaire veut couvrir YouTube :

- un module isolé, sans compte, qui aide l'utilisateur à ne rien rater et à vérifier sa liaison ;
- il ne demande aucune refonte multi-plateforme du cœur : un module peut faire ses propres
  requêtes publiques et publier par `set_data` / `notify`.

**Conditions pour rouvrir B** :

- une API officielle (progression ou éligibilité) ;
- **ou** des expériences de la section 7 concluantes sur un compte de test, avec un client
  léger qui crédite sans BotGuard ;
- **et** un avis assumé du propriétaire sur le risque CGU.

## 7. Questions ouvertes et expériences à mener avec un vrai compte

À faire par le propriétaire, **sur un compte Google de test** (jamais le compte principal),
lié à un compte de jeu de test, pendant une vraie campagne (ex. prochaine CDL ou BlizzCon),
**en regardant soi-même** dans un navigateur normal :

1. **Indicateur d'éligibilité** :
   - ouvrir la diffusion connecté et lié ;
   - dans les outils de développement, enregistrer les réponses `youtubei/v1/player` et
     `next` (HAR) ;
   - chercher le champ qui alimente « *review your linked account state* » (paramètres du
     lecteur) ;
   - comparer avec la même vidéo hors connexion (sonde `research/youtube/`).
2. **Télémétrie réelle** : noter le rythme et les paramètres de `api/stats/watchtime` et `qoe`,
   et des requêtes `videoplayback` (SABR) pendant 10 min ; onglet visible puis caché
   (`vis`), lecteur en sourdine.
3. **Seuils** : vérifier si le temps est compté par session continue ou cumulé, et ce qui se
   passe sur une coupure du direct ou un rechargement.
4. **Délai de livraison** : heure du palier contre heure d'arrivée en jeu.
5. **Visibilité de la progression** : chercher sur le site de l'éditeur une API ou une page
   « progression » (Activision et EA ont parfois des pages compte) qui servirait de
   source de vérité.
6. **Ne pas tester** de client non navigateur, de cookies exportés vers un script, ni de
   télémétrie forgée : c'est précisément ce que les CGU interdisent. Si le propriétaire veut
   aller plus loin malgré tout, c'est une décision explicite, documentée à part.

Questions sans réponse publique à ce jour :

- YouTube prévoit-il un vrai produit « drops » (catalogue, progression) ? Aucune annonce
  trouvée au 26/10/2026.
- Liste à jour des partenaires : la page d'aide date de plusieurs années et cite encore FIFA
  et l'OWL.
- Régions et âges : propres à chaque règlement d'éditeur (ex. la Corée éligible côté YouTube
  pour la BlizzCon 2026, pas côté Twitch).

## 8. Sources (consultées le 26/10/2026)

Officielles :

- YouTube Help, *Connect your account to earn off-platform rewards & improve your YouTube experience* : https://support.google.com/youtube/answer/9024948
- YouTube Terms of Service (en vigueur le 15/12/2023) : https://www.youtube.com/static?template=terms
- YouTube API Services Developer Policies (mises à jour le 15/10/2026) : https://developers.google.com/youtube/terms/developer-policies
- YouTube Help, *Fake engagement policy* : https://support.google.com/youtube/answer/3399767
- Google, *OAuth 2.0 for TV and Limited-Input Device Applications* : https://developers.google.com/identity/protocols/oauth2/limited-input-device
- Google, *Using OAuth 2.0* (expiration des jetons) : https://developers.google.com/identity/protocols/oauth2#expiration
- Blizzard, *Now Live! Watch and Earn New BlizzCon 2026 Rewards* : http://worldofwarcraft.blizzard.com/en-us/news/24295396
- EA, *FC Pro – How to earn rewards* : https://www.ea.com/games/ea-sports-fc/fc-pro/how-to-earn-rewards
- Activision, *2026 Major 2 Viewership Reward Rules* (PDF ; clause anti-automatisation lue via l'extrait du moteur de recherche, le PDF n'a pas pu être téléchargé) : https://www.callofduty.com/content/dam/atvi/callofduty/esports-new/2026-cdl-programs/2026_Major_2_Viewership_Reward_Rules.pdf
- Activision, *Call of Duty: NEXT Viewership Incentive – Official Rules* (2023) : https://www.callofduty.com/be/fr/modernwarfare3/CODNEXT2023-WatchRewards
- Blizzard Support, *Didn't Receive World of Warcraft YouTube Drops* : https://us.support.blizzard.com/en/article/313397

Secondaires, techniques et projets :

- yt-dlp wiki, *PO Token Guide* (modifié le 12/07/2026) : https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide
- yt-dlp wiki, *Extractors* (YouTube : cookies, risques de bannissement, limites) : https://github.com/yt-dlp/yt-dlp/wiki/Extractors
- yt-dlp #11462, OAuth limité aux clients TV (05–16/11/2024) : https://github.com/yt-dlp/yt-dlp/issues/11462
- yt-dlp #12482, *web only has SABR formats* (26/02/2025) : https://github.com/yt-dlp/yt-dlp/issues/12482
- Client `yt-dlp/extractor/youtube/_base.py` (noms et versions de clients innertube) : https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/youtube/_base.py
- LuanRT/googlevideo (SABR/UMP) : https://jsr.io/@luanrt/googlevideo
- Xosrov, *YouTube-Playback-Metrics* (paramètres `watchtime`, expérimental) : https://github.com/Xosrov/YouTube-Playback-Metrics
- ucarno/ow-league-tokens : https://github.com/ucarno/ow-league-tokens
- LeagueOfPoro/CapsuleFarmerEvolved : https://github.com/LeagueOfPoro/CapsuleFarmerEvolved
- GGRecon, *How to get the CDL Champs Drops in MW2* (15/06/2023) : https://www.ggrecon.com/guides/mw2-cdl-champs-drops-how-to-get/
- esports.gg, *How to earn COD Champs 2025 viewership rewards on YouTube* : https://esports.gg/news/call-of-duty/how-to-earn-cod-champs-2025-viewership-rewards-on-youtube/
- Dexerto, *How to claim LoL Worlds 2024 drops* : https://www.dexerto.com/league-of-legends/how-to-claim-lol-worlds-2024-drops-2892970
- Mesures de première main : `research/youtube/yt_live_probe.py`, exécuté le 26/10/2026 sur `@LofiGirl` (direct en cours) et `@lolesports` (Worlds 2026, direct programmé).
