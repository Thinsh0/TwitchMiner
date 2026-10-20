//! Derived, presentation-friendly views of the [`State`]: what a tray icon, a
//! notification, a widget or Android's ongoing "Live Update" needs, computed in
//! one place so every front-end shows the same thing.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::state::{AccountStatus, State};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Summary {
    pub accounts: Vec<AccountSummary>,
    /// Session totals across accounts.
    pub points_earned: i64,
    pub drops_claimed: u64,
    pub watch_minutes: u64,
    /// Number of accounts needing attention (login required, errors).
    pub attention: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountSummary {
    pub id: String,
    pub name: String,
    pub status: String,
    pub watching: Vec<WatchingSummary>,
    pub drop: Option<DropSummary>,
    pub points_earned: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WatchingSummary {
    pub channel_id: String,
    pub channel: String,
    pub game: Option<String>,
    pub reason: String,
    pub minutes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DropSummary {
    pub game: String,
    pub campaign: String,
    pub drop: String,
    pub image_url: Option<String>,
    pub current_minutes: u32,
    pub required_minutes: u32,
    /// 0.0 ..= 1.0
    pub progress: f32,
    /// Estimated completion if watching continues uninterrupted.
    pub eta: Option<DateTime<Utc>>,
    /// Remaining drops in the campaign after this one.
    pub remaining_drops: usize,
}

pub fn summary(state: &State, now: DateTime<Utc>) -> Summary {
    let mut out = Summary::default();
    for (id, acc) in &state.accounts {
        let name = acc
            .info
            .as_ref()
            .map(|i| i.display_name.clone())
            .unwrap_or_else(|| id.clone());
        let status = match &acc.status {
            Some(AccountStatus::Running) => "running",
            Some(AccountStatus::Starting) => "starting",
            Some(AccountStatus::Stopped) | None => "stopped",
            Some(AccountStatus::AuthRequired) => {
                out.attention += 1;
                "auth_required"
            }
            Some(AccountStatus::Error { .. }) => {
                out.attention += 1;
                "error"
            }
        };
        let watching = acc
            .watching
            .values()
            .map(|w| {
                let ch = acc.channels.get(&w.channel_id);
                WatchingSummary {
                    channel_id: w.channel_id.clone(),
                    channel: ch
                        .map(|c| c.display_name.clone())
                        .unwrap_or_else(|| w.channel_id.clone()),
                    game: ch
                        .and_then(|c| c.stream.as_ref())
                        .and_then(|s| s.game.as_ref())
                        .map(|g| g.name.clone()),
                    reason: w.reason.clone(),
                    minutes: w.minutes,
                }
            })
            .collect();
        let drop = acc.drop_target.as_ref().and_then(|t| {
            let c = acc.campaigns.get(&t.campaign_id)?;
            let d = t
                .drop_id
                .as_ref()
                .and_then(|id| c.drops.iter().find(|d| &d.id == id))
                .or_else(|| c.drops.iter().find(|d| !d.claimed))?;
            let remaining = d.required_minutes.saturating_sub(d.current_minutes);
            let watching_target = acc.watching.contains_key(&t.channel_id);
            Some(DropSummary {
                game: c.game.name.clone(),
                campaign: c.name.clone(),
                drop: d.name.clone(),
                image_url: d
                    .benefits
                    .first()
                    .and_then(|b| b.image_url.clone())
                    .or_else(|| c.image_url.clone()),
                current_minutes: d.current_minutes,
                required_minutes: d.required_minutes,
                progress: if d.required_minutes == 0 {
                    0.0
                } else {
                    (d.current_minutes as f32 / d.required_minutes as f32).clamp(0.0, 1.0)
                },
                eta: watching_target.then(|| now + Duration::minutes(i64::from(remaining))),
                remaining_drops: c
                    .drops
                    .iter()
                    .filter(|x| !x.claimed && x.id != d.id)
                    .count(),
            })
        });
        out.points_earned += acc.session.points_earned;
        out.drops_claimed += acc.session.drops_claimed;
        out.watch_minutes += acc.session.watch_minutes;
        out.accounts.push(AccountSummary {
            id: id.clone(),
            name,
            status: status.to_string(),
            watching,
            drop,
            points_earned: acc.session.points_earned,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::state::{AccountInfo, DropTarget, apply};
    use tm_twitch::models::{Campaign, Game, TimedDrop};

    #[test]
    fn drop_progress_and_eta() {
        let now = Utc::now();
        let mut s = State::default();
        let info = AccountInfo {
            id: "1".into(),
            login: "me".into(),
            display_name: "Me".into(),
            avatar_url: None,
            enabled: true,
        };
        apply(&mut s, &Event::AccountUpserted { info }, now);
        let drop = TimedDrop {
            id: "d".into(),
            name: "Skin".into(),
            starts_at: now,
            ends_at: now,
            required_minutes: 120,
            current_minutes: 30,
            claimed: false,
            claim_id: None,
            preconditions: vec![],
            benefits: vec![],
        };
        let campaign = Campaign {
            id: "c".into(),
            name: "Camp".into(),
            game: Game {
                id: "g".into(),
                name: "Game".into(),
                slug: None,
                box_art_url: None,
            },
            status: "ACTIVE".into(),
            starts_at: None,
            ends_at: now,
            image_url: None,
            details_url: None,
            link_url: None,
            linked: Some(true),
            allowed_channels: None,
            drops: vec![drop],
        };
        apply(
            &mut s,
            &Event::CampaignsReplaced {
                account: "1".into(),
                campaigns: vec![campaign],
            },
            now,
        );
        let target = DropTarget {
            campaign_id: "c".into(),
            drop_id: Some("d".into()),
            channel_id: "9".into(),
            game: "Game".into(),
        };
        apply(
            &mut s,
            &Event::DropTargetChanged {
                account: "1".into(),
                target: Some(target),
            },
            now,
        );
        apply(
            &mut s,
            &Event::WatchStarted {
                account: "1".into(),
                channel_id: "9".into(),
                reason: "drops".into(),
            },
            now,
        );
        let sum = summary(&s, now);
        let d = sum.accounts[0].drop.as_ref().unwrap();
        assert_eq!(d.progress, 0.25);
        assert_eq!(d.eta, Some(now + Duration::minutes(90)));
    }
}
