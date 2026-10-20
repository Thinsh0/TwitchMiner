# Application de bureau (Windows, Linux)

`twitchminer-desktop` : l'interface native (Slint) et son intégration au bureau. L'interface elle-même est dans `crates/ui` (crate `tm-ui`), partagée avec la future app Android ; `apps/desktop` n'ajoute que ce qui est propre au bureau.

```bash
cargo run -p twitchminer-desktop                 # fenêtre + moteur local
cargo run -p twitchminer-desktop -- --background # démarre dans la barre système
```

## Moteur local ou distant

Au premier lancement, l'app démarre un moteur local (données dans le dossier de `Paths`, comme le daemon). Dans **Réglages › Avancé › Moteur**, « Changer… » permet de piloter un TwitchMiner distant (URL + jeton, `twitchminer api-token` sur la machine du moteur) : l'app ne fait alors tourner aucun moteur.

Si le dossier de données est déjà utilisé (daemon lancé sur ce PC), l'app propose de s'y connecter avec l'adresse et le jeton déjà remplis.

**Accès à distance (API)** : dans Réglages › Avancé › Réseau, l'app peut servir l'API de son moteur local (par défaut `127.0.0.1:7878`, comme le daemon) pour un téléphone, un autre PC ou un agent IA. Mettre l'adresse locale ou Tailscale pour l'ouvrir aux autres appareils.

## Bureau

- **Barre système** : l'icône résume l'activité (chaînes regardées, drop en cours, pause, action requise). Clic gauche : ouvrir. Menu : ouvrir, pause / reprise, quitter.
- **Fermer la fenêtre** la masque : le moteur continue. « Quitter » dans le menu de l'icône arrête tout proprement.
- **Lancer avec la session** (Réglages › Démarrage) : inscrit l'app au démarrage de la session de l'utilisateur (registre `Run` sous Windows, `~/.config/autostart` sous Linux), avec `--background` : elle démarre dans la barre système sans ouvrir de fenêtre.
- **Une seule instance** par utilisateur et dossier de données : relancer l'app affiche la fenêtre déjà ouverte.

Sous Linux, l'icône utilise le protocole StatusNotifierItem (KDE, et GNOME avec l'extension AppIndicator).

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
