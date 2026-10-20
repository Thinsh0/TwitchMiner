# Limites du cœur relevées pendant la construction de l'interface native

Points trouvés côté moteur (`tm-core`, `tm-app`, modules) pendant le développement de l'interface Slint (`crates/ui`). L'interface ne les contourne pas : chaque point attend une décision et un changement côté cœur, en respectant les règles de `CLAUDE.md`.

## 1. Les notices ne sont ni traduisibles ni actionnables

`Notice { id, level, message, at }` ne porte qu'un texte libre en anglais (ex. « TwitchMiner 0.2.0 is available. »). L'interface ne peut donc pas :
- l'afficher en français (ou dans une autre langue) ;
- proposer l'action qui va avec (« Télécharger » pour une mise à jour, « Voir le pair » pour un doublon…).

**Proposition** : ajouter à `Notice` deux champs optionnels, rétrocompatibles :
- `kind` : type stable (`update`, `duplicate`, `manifest`…) ;
- `params` : objet JSON (`{"version": "0.2.0", "url": "…"}`, `{"account": "…", "peer": "…"}`).

`message` reste comme texte de secours. À documenter dans `docs/API.md`.

## 2. « Aujourd'hui » compte en jour UTC

`daily_stats.day` est calculé avec `env.ts.format("%Y-%m-%d")`, donc en UTC (`crates/core/src/db.rs`). En France, le compteur « Aujourd'hui » de l'accueil repasse à zéro vers 1 h ou 2 h du matin, et l'historique par jour décale les sessions nocturnes.

**Proposition** : ranger les statistiques par jour local, ou stocker l'heure UTC et laisser `stats` grouper selon un fuseau passé en paramètre (`tz_offset_minutes`). Attention aux données déjà enregistrées (migration ou simple décalage accepté).

## 3. Journaux du moteur non capturés après un redémarrage dans le même processus

`tm_core::logging::init` utilise `try_init` : si l'interface arrête puis redémarre un moteur local dans le même processus (passage local → distant → local), le premier abonné global reste en place. Conséquences pour le second `App` :
- son `LogBuffer` ne reçoit rien : les journaux affichés dans l'interface sont vides ;
- l'écrivain de fichier du premier abonné a été arrêté avec le premier `App` (son `WorkerGuard` est détruit) : plus rien n'est écrit dans `logs/` jusqu'au redémarrage du processus.

**Proposition** : initialiser le journal une seule fois par processus (`OnceLock`) et partager le même `LogBuffer` entre les `App` successifs.
