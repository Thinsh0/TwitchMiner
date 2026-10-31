# Application de bureau (Windows, Linux)

`twitchminer-desktop` : l'interface native (Slint) et son intégration au bureau. L'interface elle-même est dans `crates/ui` (crate `tm-ui`), partagée avec l'appli Android (`apps/android`, voir [ANDROID.md](ANDROID.md)) ; `apps/desktop` n'ajoute que ce qui est propre au bureau. Sous 720 px de large, elle passe en disposition compacte (celle du téléphone) : navigation en bas, actions sous les textes, réglages sur une colonne.

```bash
cargo run -p twitchminer-desktop                 # fenêtre + moteur local
cargo run -p twitchminer-desktop -- --background # démarre dans la barre système
```

## Moteur local ou distant

Au premier lancement, l'app démarre un moteur local (données dans le dossier de `Paths`, comme le daemon). Dans **Réglages › Avancé › Moteur**, « Changer… » permet de piloter un TwitchMiner distant (URL + jeton, `twitchminer api-token` sur la machine du moteur) : l'app ne fait alors tourner aucun moteur.

Si le dossier de données est déjà utilisé (daemon lancé sur ce PC), l'app propose de s'y connecter avec l'adresse et le jeton déjà remplis.

Un moteur distant déjà choisi qui ne répond pas au démarrage est gardé : l'app réessaie seule, et un bandeau au-dessus de chaque écran dit qu'il est injoignable (avec « Changer… »). Pendant ce temps, les actions sont refusées tout de suite plutôt qu'exécutées au retour de la connexion.

**Accès à distance (API)** : dans Réglages › Avancé › Réseau, l'app peut servir l'API de son moteur local (par défaut `127.0.0.1:7878`, comme le daemon) pour un téléphone, un autre PC ou un agent IA. Mettre l'adresse locale ou Tailscale pour l'ouvrir aux autres appareils. Si l'adresse est prise ou pas encore disponible (Tailscale pas encore connecté), le moteur démarre quand même, sans l'API, et le réglage dit pourquoi.

Le **journal** (Réglages › Avancé) est celui du moteur, local ou distant.

Si le serveur ne répond pas, l'écran de connexion dit pourquoi en clair : adresse en `https://` alors que le moteur ne parle que HTTP (écrire `http://`, Tailscale chiffre déjà), jeton refusé, rien n'écoute sur ce port (moteur arrêté, mauvais port), nom inconnu (faute de frappe, Tailscale/VPN pas connecté), pas de réponse. Une adresse tapée sans `http://` est acceptée.

## Mises à jour

L'app vérifie la dernière version publiée sur GitHub au démarrage puis toutes les 6 heures (sauf si « Vérifier les mises à jour » est coupé dans les réglages du moteur), et quand on clique « Rechercher des mises à jour » (Réglages › Avancé › À propos). Une nouvelle version ajoute une carte dans « À faire » :

- **Windows** : « Mettre à jour » télécharge `TwitchMiner-vX.Y.Z-windows.exe` et sa signature `TwitchMiner-vX.Y.Z-windows.exe.sig`, vérifie sa taille, son empreinte SHA-256 (publiée par GitHub) et sa **signature ed25519** avec la clé publique intégrée au programme, remplace l'exe en place (l'ancien est renommé `.old` puis supprimé au démarrage suivant) et relance l'app (`--updated` : la nouvelle attend que l'ancienne soit fermée). Le dossier de l'exe doit être modifiable par l'utilisateur. S'il manque quoi que ce soit (signature, empreinte, clé publique dans cette version) ou si une vérification échoue, rien n'est installé : seule la page de la version est proposée.
- **Linux** : « Télécharger » ouvre la page de la version.

### Signature des mises à jour (mainteneur)

Une mise à jour n'est installée automatiquement que si sa signature correspond à la clé publique `RELEASE_PUBLIC_KEY` de `crates/ui/src/update.rs`. Tant que cette constante est vide (cas du dépôt tel quel), l'app ne s'installe jamais elle-même : elle propose la page de la version. Pour l'activer, une seule fois :

1. Créer la paire de clés, **hors du dépôt** (jamais commitée) :

   ```bash
   openssl genpkey -algorithm ed25519 -out release-signing.pem
   chmod 600 release-signing.pem
   # clé publique : les 32 octets bruts, en hexadécimal (64 caractères)
   openssl pkey -in release-signing.pem -pubout -outform DER | tail -c 32 | od -An -tx1 | tr -d ' \n'; echo
   ```

2. Ajouter le secret GitHub **`RELEASE_SIGNING_KEY`** (Settings › Secrets and variables › Actions) avec tout le contenu de `release-signing.pem` (lignes `-----BEGIN PRIVATE KEY-----` comprises). Garder une copie hors ligne : perdue, les versions déjà installées ne pourront plus se mettre à jour seules.
3. Coller la clé publique (64 caractères hexadécimaux) dans `RELEASE_PUBLIC_KEY`, puis publier une version comme d'habitude.

Le workflow **Release** échoue si le secret manque, ou si la clé publique du code n'est pas celle du secret. Il signe chaque fichier publié (`<fichier>.sig`, 64 octets bruts). Seules les versions construites avec la clé publique sauront vérifier (et donc installer) les suivantes : la première version qui l'intègre s'installe encore à la main.

Vérifier un fichier à la main :

```bash
python3 -c "import sys; sys.stdout.buffer.write(bytes.fromhex('302a300506032b6570032100' + sys.argv[1]))" "<clé publique hex>" > release.der
openssl pkey -pubin -inform DER -in release.der -out release.pub
openssl pkeyutl -verify -pubin -inkey release.pub -rawin -in TwitchMiner-vX.Y.Z-windows.exe -sigfile TwitchMiner-vX.Y.Z-windows.exe.sig
```

Avec un moteur distant, l'avis « mise à jour » du moteur concerne le serveur : avec Docker, récupérer la nouvelle image (bouton *Update* de Dockge, ou `docker compose pull && docker compose up -d`). En local, le moteur fait partie de l'app : seule la carte de l'app est montrée.

## Bureau

- **Barre système** : l'icône résume l'activité (chaînes regardées, drop en cours, pause, action requise). Clic gauche : ouvrir. Menu : ouvrir, pause / reprise, quitter.
- **Fermer la fenêtre** la masque : le moteur continue. « Quitter » dans le menu de l'icône arrête tout proprement.
- **Notifications système** dès que la fenêtre n'a pas le focus (masquée, ou ouverte derrière une autre) : drop récupéré, compte à reconnecter, alertes des drops (campagne juste, manquée, compte de jeu à lier, nouvelle campagne). Un compte à reconnecter est notifié même quand la fenêtre a le focus : la carte dans l'interface passe facilement inaperçue.
- **Lancer avec la session** (Réglages › Démarrage) : inscrit l'app au démarrage de la session de l'utilisateur (registre `Run` sous Windows, `~/.config/autostart` sous Linux), avec `--background` : elle démarre dans la barre système sans ouvrir de fenêtre.
- **Retour sur la fenêtre** (sortie de la barre système, Alt+Tab, clic) : l'app redemande les campagnes de drops (inventaire, progression de chaque drop) et les totaux du jour, au plus toutes les 2 minutes. Le reste (drop en cours, plan, chaînes) est déjà en direct.
- **Une seule instance** par utilisateur et dossier de données : relancer l'app affiche la fenêtre déjà ouverte.

Sous Linux, l'icône utilise le protocole StatusNotifierItem (KDE, et GNOME avec l'extension AppIndicator).

## Windows

- **Pas de console** : l'exécutable est une application graphique, même compilé en mode debug. Les journaux restent dans `logs/` du dossier de données.
- **Menu Démarrer et liste des applications** : à chaque démarrage, l'app crée ou met à jour son raccourci (`%APPDATA%\Microsoft\Windows\Start Menu\Programs\TwitchMiner.lnk`) et son entrée dans Paramètres › Applications › Applications installées (`HKCU\…\Uninstall\TwitchMiner`). Il n'y a pas d'installeur : il suffit de placer l'exe où on veut le garder et de le lancer une fois. S'il est déplacé, le prochain démarrage met les deux à jour. Rien n'est inscrit quand l'app est lancée par `cargo run`.
- **Désinstaller** depuis la liste des applications lance `twitchminer-desktop.exe --uninstall` : après confirmation, il retire le raccourci, l'entrée et le lancement avec la session. Les données et l'exe restent ; le dossier est à supprimer soi-même.
- L'icône et les informations de version sont intégrées à l'exe (`apps/desktop/build.rs`, compilateur de ressources du SDK Windows ou `windres`). Sans compilateur de ressources, la compilation continue sans icône (avertissement).

## Préférences de l'interface

Thème (sombre, clair, OLED), langue (français, anglais) et dernier serveur distant sont gardés dans la base du dossier de données (`tm_app::read_ui_prefs` / `write_ui_prefs`), pas dans un fichier à part.

## Développement

Variables d'environnement utiles pour travailler sur l'interface :

| Variable | Effet |
|---|---|
| `TM_LANG=en` | interface en anglais |
| `TM_UI_DEMO=1` (ou `paused`) | état fictif réaliste, sans compte Twitch ni moteur |
| `TM_UI_SNAPSHOT=capture.bmp` | fenêtre hors de l'écran, sans focus ni icône, capture écrite chaque seconde |
| `TM_UI_SCREEN=drops` | écran de départ (`home`, `drops`, `channels`, `activity`, `settings`) |
| `TM_UI_THEME=light` | thème de départ (`dark`, `light`, `oled`) |
| `TM_UI_SIZE=1180x1000` | taille de la fenêtre |
| `TM_UI_EXPAND=1` | sections repliées ouvertes |

Les textes de l'interface sont écrits en anglais dans les fichiers `.slint` (`@tr("…")`) et traduits dans `crates/ui/translations/fr/LC_MESSAGES/tm-ui.po` ; un test vérifie que chaque texte a sa traduction française.
