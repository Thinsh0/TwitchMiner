//! System notifications while the window does not have the focus (hidden, or behind
//! another one): what to say for an engine event (the views' translations do not reach
//! here, so both languages are written out). Important ones are sent even while the
//! window has the focus: a card in the interface is easy to miss.

use chrono::{DateTime, Local, Utc};
use tm_core::state::{AccountStatus, DropAlertKind};
use tm_core::{Event, State};

/// A notification for the platform to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

/// The notification for `event`, if it is worth one. `fr`: interface in French.
pub fn compose(event: &Event, state: &State, fr: bool) -> Option<Notification> {
    let n = |title: String, body: String| Some(Notification { title, body });
    match event {
        Event::DropClaimed {
            drop_name, game, ..
        } => {
            if fr {
                n("Drop récupéré".into(), format!("{drop_name} ({game})"))
            } else {
                n("Drop claimed".into(), format!("{drop_name} ({game})"))
            }
        }
        Event::AccountStatusChanged {
            account,
            status: AccountStatus::AuthRequired,
        } => {
            let name = state
                .accounts
                .get(account)
                .and_then(|a| a.info.as_ref())
                .map(|i| i.display_name.clone())
                .unwrap_or_else(|| account.clone());
            if fr {
                n(
                    format!("{name} doit se reconnecter"),
                    "Twitch n'accepte plus sa session : rien n'est miné pour ce compte. Ouvre TwitchMiner pour le reconnecter.".into(),
                )
            } else {
                n(
                    format!("{name} must sign in again"),
                    "Twitch no longer accepts its session: nothing is mined for it. Open TwitchMiner to reconnect.".into(),
                )
            }
        }
        Event::DropAlert { alert: a, .. } => {
            let ends = day_time(a.ends_at);
            let game = &a.game;
            let campaign = &a.campaign;
            match (a.kind, fr) {
                (DropAlertKind::AtRisk, true) => n(
                    format!("Drops {game} : c'est juste"),
                    format!("« {campaign} » finit le {ends}. « Faire en premier » le garantit."),
                ),
                (DropAlertKind::AtRisk, false) => n(
                    format!("{game} drops: tight"),
                    format!("“{campaign}” ends {ends}. “Do it first” secures it."),
                ),
                (DropAlertKind::WillMiss, true) => n(
                    format!("Drops {game} : ne sera pas fini"),
                    format!(
                        "« {campaign} » finit le {ends}, il manquera ~{}.",
                        hours(a.missing_minutes)
                    ),
                ),
                (DropAlertKind::WillMiss, false) => n(
                    format!("{game} drops will not be finished"),
                    format!(
                        "“{campaign}” ends {ends}, about {} would be missing.",
                        hours(a.missing_minutes)
                    ),
                ),
                (DropAlertKind::NotLinked, true) => n(
                    format!("Lie ton compte {game}"),
                    format!("« {campaign} » ne donne ses récompenses qu'aux comptes liés au jeu."),
                ),
                (DropAlertKind::NotLinked, false) => n(
                    format!("Link your {game} account"),
                    format!("“{campaign}” only rewards accounts linked to the game."),
                ),
                (DropAlertKind::NewCampaign, true) => n(
                    format!("Nouvelle campagne {game}"),
                    format!("« {campaign} », jusqu'au {ends}."),
                ),
                (DropAlertKind::NewCampaign, false) => n(
                    format!("New {game} campaign"),
                    format!("“{campaign}”, until {ends}."),
                ),
            }
        }
        _ => None,
    }
}

/// Sent even while the window has the focus: something stopped and needs the user.
pub fn important(event: &Event) -> bool {
    matches!(
        event,
        Event::AccountStatusChanged {
            status: AccountStatus::AuthRequired,
            ..
        }
    )
}

fn day_time(t: DateTime<Utc>) -> String {
    t.with_timezone(&Local).format("%d/%m %H:%M").to_string()
}

fn hours(minutes: Option<u32>) -> String {
    let m = minutes.unwrap_or(0);
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{:.1} h", f64::from(m) / 60.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_required_names_the_account() {
        let mut state = State::default();
        let acc = state.accounts.entry("7".into()).or_default();
        acc.info = Some(tm_core::state::AccountInfo {
            id: "7".into(),
            login: "someone".into(),
            display_name: "Someone".into(),
            avatar_url: None,
            enabled: true,
        });
        let e = Event::AccountStatusChanged {
            account: "7".into(),
            status: AccountStatus::AuthRequired,
        };
        let n = compose(&e, &state, false).unwrap();
        assert_eq!(n.title, "Someone must sign in again");
        let other = Event::AccountStatusChanged {
            account: "7".into(),
            status: AccountStatus::Running,
        };
        assert!(compose(&other, &state, true).is_none());
        assert!(important(&e));
        assert!(!important(&other));
    }
}
