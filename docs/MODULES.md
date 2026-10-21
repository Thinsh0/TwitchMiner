# Écrire un module

Une fonctionnalité = un fichier dans `crates/modules/src/` + une ligne dans `tm_modules::builtin()`.

```rust
use serde::{Deserialize, Serialize};
use serde_json::json;
use tm_core::{Module, ModuleContext, ModuleInfo, event::Event};

pub struct Greeter;

#[derive(Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
struct Settings { enabled: bool, message: String }

#[async_trait::async_trait]
impl Module for Greeter {
    fn info(&self) -> ModuleInfo {
        ModuleInfo::new("greeter", "Greeter", "Exemple", json!({"enabled": false, "message": "hello"}))
    }

    fn validate_settings(&self, v: &serde_json::Value) -> Result<(), String> {
        tm_core::module::validate_as::<Settings>(v)
    }

    async fn run(&self, ctx: ModuleContext) -> anyhow::Result<()> {
        let mut events = ctx.events();
        loop {
            tokio::select! {
                _ = ctx.cancelled() => return Ok(()),
                _ = ctx.settings_changed() => {}
                Ok(env) = events.recv() => {
                    if let Event::DropClaimed { drop_name, .. } = &env.event {
                        let s: Settings = ctx.settings()?;
                        ctx.set_data("last", json!(format!("{} {drop_name}", s.message)));
                    }
                }
            }
        }
    }
}
```

## Ce que fournit `ModuleContext`

| Méthode | Usage |
|---|---|
| `account_id()`, `login()` | compte pour lequel le module tourne |
| `gql()` | client Twitch authentifié (`tm_twitch::Gql`, API typée dans `api.rs`) |
| `pubsub()` | `subscribe()` pour tous les messages, `topic_set()` pour déclarer ses topics (libérés automatiquement) |
| `settings::<T>()`, `settings_value()`, `settings_changed()` | réglages effectifs, notification de changement |
| `emit(Event)` | modifier l'état (seule façon de le faire) |
| `events()` | écouter tous les événements, y compris ceux des autres modules |
| `set_data(key, json)` | publier des données libres dans `state.accounts[id].module_data[module]`, sans toucher au cœur |
| `track_channel(meta)` / `release_channel(id)` | faire apparaître une chaîne dans l'état |
| `set_watch_requests(vec)` | demander des créneaux de visionnage (priorités dans `tm_core::priority`) |
| `kv_get` / `kv_put` | stockage persistant privé au module (SQLite) |
| `next_call()` | recevoir les `module_call` des interfaces et y répondre |
| `cancelled()`, `sleep(d)` | arrêt propre |

## Règles

- `run` doit rendre la main quand `ctx.cancelled()` se déclenche, en moins de 10 s.
- Retourner une erreur, ou paniquer, redémarre le module avec backoff. Pas besoin de tout rattraper soi-même.
- Un module tourne seulement quand le compte est connecté. Pour tourner aussi quand le jeton est refusé (le compte attend une nouvelle connexion), répondre `false` à `needs_login()` : c'est le cas des notifications, qui préviennent ainsi qu'il faut se reconnecter. Les appels Twitch échouent pendant ce temps.
- Ne jamais garder d'état « vrai » ailleurs que dans le `State` s'il doit être vu par une interface : publiez-le avec `emit` ou `set_data`.
- Les événements sans effet sont ignorés par le store : on peut émettre de façon idempotente.
- Un module qui a besoin d'un nouvel appel Twitch l'ajoute dans `tm-twitch` (`api.rs`) et son hash dans `manifest/twitch.json`.
- Si un module a besoin d'un nouveau type d'événement partagé, on l'ajoute dans `event.rs` avec sa branche du réducteur dans `state.rs`. Sinon, `set_data` suffit.
