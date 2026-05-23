# Context & Decisions

- **Notifications Image**: Ajout du support de l'image via `attachments` / `icon` dans les notifications Tauri, afin de fournir un visuel du drop lorsqu'il est trouvé ou récupéré.
- **Mobile Setup**: Le widget de configuration Android est désormais complètement masqué (pas de bannière verte) une fois que tout a été approuvé.
- **Stealth & Jitter**: La boucle principale du miner (`miner.rs` et `streak_miner.rs`) a été modifiée pour introduire un délai aléatoire (jitter) de 1 à 10 secondes additionnelles aux 60 secondes classiques. Le but est de réduire la probabilité de détection robotique (comportement non déterministe).
- **Point Miner & Offline**: La fonction `process_single_channel` lit explicitement si le `stream` GraphQL est `None` et renvoie une erreur "Stream is offline" pour forcer l'affichage correct du statut dans l'interface UI (et éviter de pinger inutilement une chaîne fermée).
- **Coffres Verts (Green Chests)**: La boucle `process_channels_sequential` appelle déjà `get_channel_points` toutes les 60 secondes, ce qui suffit largement à capter les bonus périodiques de Twitch (généralement toutes les 15 minutes).
