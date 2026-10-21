//! Plain data types returned by the API layer. They are serializable so the core can
//! store them in its state as-is.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Game {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub box_art_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamInfo {
    pub id: String,
    pub title: String,
    pub game: Option<Game>,
    pub viewers: u64,
    pub started_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserInfo {
    pub id: String,
    pub login: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub stream: Option<StreamInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelRef {
    pub id: String,
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Benefit {
    pub id: String,
    pub name: String,
    pub image_url: Option<String>,
    /// Twitch `distributionType`: `BADGE`, `EMOTE`, `DIRECT_ENTITLEMENT`, …
    pub kind: String,
}

impl Benefit {
    pub fn is_badge_or_emote(&self) -> bool {
        matches!(self.kind.as_str(), "BADGE" | "EMOTE")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimedDrop {
    pub id: String,
    pub name: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub required_minutes: u32,
    pub current_minutes: u32,
    pub claimed: bool,
    pub claim_id: Option<String>,
    pub preconditions: Vec<String>,
    pub benefits: Vec<Benefit>,
}

impl TimedDrop {
    pub fn is_complete(&self) -> bool {
        self.required_minutes > 0 && self.current_minutes >= self.required_minutes
    }
    /// Twitch only hands out a `dropInstanceID` once the drop is earned, so its
    /// presence is enough (minute counters can lag behind).
    pub fn can_claim(&self) -> bool {
        !self.claimed && self.claim_id.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Campaign {
    pub id: String,
    pub name: String,
    pub game: Game,
    /// `ACTIVE`, `UPCOMING`, `EXPIRED` (empty if unknown).
    pub status: String,
    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: DateTime<Utc>,
    pub image_url: Option<String>,
    pub details_url: Option<String>,
    pub link_url: Option<String>,
    /// Whether the Twitch account is linked to the game account. `None` = unknown.
    pub linked: Option<bool>,
    /// Channel allow-list. `None` = any channel streaming the game.
    pub allowed_channels: Option<Vec<ChannelRef>>,
    pub drops: Vec<TimedDrop>,
}

impl Campaign {
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        self.status != "EXPIRED" && self.starts_at.is_none_or(|s| s <= now) && now < self.ends_at
    }

    pub fn badge_or_emote_only(&self) -> bool {
        let mut benefits = self.drops.iter().flat_map(|d| &d.benefits).peekable();
        benefits.peek().is_some() && benefits.all(Benefit::is_badge_or_emote)
    }

    /// Merges `other` (more detailed or more recent) into `self`, keeping known values.
    pub fn merge(&mut self, other: Campaign) {
        let Campaign {
            name,
            game,
            status,
            starts_at,
            ends_at,
            image_url,
            details_url,
            link_url,
            linked,
            allowed_channels,
            drops,
            ..
        } = other;
        if !name.is_empty() {
            self.name = name;
        }
        if !game.id.is_empty() {
            let slug = self.game.slug.take();
            let art = self.game.box_art_url.take();
            self.game = game;
            self.game.slug = self.game.slug.take().or(slug);
            self.game.box_art_url = self.game.box_art_url.take().or(art);
        }
        if !status.is_empty() {
            self.status = status;
        }
        self.starts_at = starts_at.or(self.starts_at);
        self.ends_at = ends_at;
        self.image_url = image_url.or(self.image_url.take());
        self.details_url = details_url.or(self.details_url.take());
        self.link_url = link_url.or(self.link_url.take());
        self.linked = linked.or(self.linked);
        self.allowed_channels = allowed_channels.or(self.allowed_channels.take());
        for d in drops {
            match self.drops.iter_mut().find(|x| x.id == d.id) {
                Some(x) => {
                    x.current_minutes = x.current_minutes.max(d.current_minutes);
                    x.claimed |= d.claimed;
                    x.claim_id = d.claim_id.or(x.claim_id.take());
                    if !d.benefits.is_empty() {
                        x.benefits = d.benefits;
                    }
                    if !d.preconditions.is_empty() {
                        x.preconditions = d.preconditions;
                    }
                    x.required_minutes = d.required_minutes.max(x.required_minutes);
                }
                None => self.drops.push(d),
            }
        }
    }
}

/// A reward of the account's inventory (every drop reward ever received).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClaimedBenefit {
    pub id: String,
    pub last_awarded_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub game: Option<String>,
    /// Times it was received (some rewards stack).
    #[serde(default)]
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PointsContext {
    pub channel_id: String,
    pub balance: u64,
    pub available_claim: Option<String>,
    pub multiplier: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CurrentDrop {
    pub drop_id: String,
    pub channel_id: Option<String>,
    pub current_minutes: u32,
    pub required_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectoryStream {
    pub channel: UserInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClaimResult {
    Claimed,
    AlreadyClaimed,
    Failed(String),
}

// ---------------------------------------------------------------- parsing helpers

pub(crate) fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

pub(crate) fn date(v: &Value, k: &str) -> Option<DateTime<Utc>> {
    v.get(k)
        .and_then(Value::as_str)
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|d| d.with_timezone(&Utc))
}

pub(crate) fn u(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(|x| {
        x.as_u64()
            .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
    })
}

pub(crate) fn arr<'a>(v: &'a Value, k: &str) -> impl Iterator<Item = &'a Value> {
    v.get(k).and_then(Value::as_array).into_iter().flatten()
}

pub(crate) fn parse_game(v: &Value) -> Option<Game> {
    if !v.is_object() {
        return None;
    }
    Some(Game {
        id: s(v, "id")?,
        name: s(v, "displayName")
            .or_else(|| s(v, "name"))
            .unwrap_or_default(),
        slug: s(v, "slug"),
        box_art_url: s(v, "boxArtURL"),
    })
}

pub(crate) fn parse_user(v: &Value) -> Option<UserInfo> {
    if !v.is_object() {
        return None;
    }
    let stream = v
        .get("stream")
        .filter(|s| s.is_object())
        .map(|st| StreamInfo {
            id: s(st, "id").unwrap_or_default(),
            title: s(st, "title")
                .or_else(|| v.get("broadcastSettings").and_then(|b| s(b, "title")))
                .unwrap_or_default(),
            game: st.get("game").and_then(parse_game).or_else(|| {
                v.get("broadcastSettings")
                    .and_then(|b| b.get("game"))
                    .and_then(parse_game)
            }),
            viewers: u(st, "viewersCount").unwrap_or(0),
            started_at: date(st, "createdAt"),
        });
    Some(UserInfo {
        id: s(v, "id")?,
        login: s(v, "login").unwrap_or_default(),
        display_name: s(v, "displayName")
            .or_else(|| s(v, "login"))
            .unwrap_or_default(),
        avatar_url: s(v, "profileImageURL"),
        stream,
    })
}

fn parse_benefits(drop: &Value) -> Vec<Benefit> {
    arr(drop, "benefitEdges")
        .filter_map(|e| e.get("benefit"))
        .filter_map(|b| {
            Some(Benefit {
                id: s(b, "id")?,
                name: s(b, "name").unwrap_or_default(),
                image_url: s(b, "imageAssetURL"),
                kind: s(b, "distributionType").unwrap_or_else(|| "UNKNOWN".into()),
            })
        })
        .collect()
}

pub(crate) fn parse_drop(d: &Value) -> Option<TimedDrop> {
    let me = d.get("self").filter(|x| x.is_object());
    let required = u(d, "requiredMinutesWatched").unwrap_or(0) as u32;
    let claimed = me
        .and_then(|m| m.get("isClaimed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(TimedDrop {
        id: s(d, "id")?,
        name: s(d, "name").unwrap_or_default(),
        starts_at: date(d, "startAt").unwrap_or(DateTime::<Utc>::MIN_UTC),
        ends_at: date(d, "endAt").unwrap_or(DateTime::<Utc>::MAX_UTC),
        required_minutes: required,
        current_minutes: if claimed {
            required
        } else {
            me.and_then(|m| u(m, "currentMinutesWatched")).unwrap_or(0) as u32
        },
        claimed,
        claim_id: me.and_then(|m| s(m, "dropInstanceID")),
        preconditions: arr(d, "preconditionDrops")
            .filter_map(|p| s(p, "id"))
            .collect(),
        benefits: parse_benefits(d),
    })
}

pub(crate) fn parse_campaign(c: &Value) -> Option<Campaign> {
    if !c.is_object() {
        return None;
    }
    let allow = c.get("allow").filter(|a| a.is_object());
    let allowed_channels = allow
        .filter(|a| a.get("isEnabled").and_then(Value::as_bool).unwrap_or(true))
        .and_then(|a| a.get("channels"))
        .and_then(Value::as_array)
        .filter(|chs| !chs.is_empty())
        .map(|chs| {
            chs.iter()
                .filter_map(|ch| {
                    Some(ChannelRef {
                        id: s(ch, "id")?,
                        login: s(ch, "name"),
                        display_name: s(ch, "displayName"),
                    })
                })
                .collect()
        });
    Some(Campaign {
        id: s(c, "id")?,
        name: s(c, "name").unwrap_or_default(),
        game: c.get("game").and_then(parse_game).unwrap_or_default(),
        status: s(c, "status").unwrap_or_default(),
        starts_at: date(c, "startAt"),
        ends_at: date(c, "endAt").unwrap_or(DateTime::<Utc>::MAX_UTC),
        image_url: s(c, "imageURL"),
        details_url: s(c, "detailsURL"),
        link_url: s(c, "accountLinkURL"),
        linked: c
            .get("self")
            .and_then(|m| m.get("isAccountConnected"))
            .and_then(Value::as_bool),
        allowed_channels,
        drops: arr(c, "timeBasedDrops").filter_map(parse_drop).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_available_drops_campaign() {
        let v = json!({"id":"c1","name":"Cuddly","game":{"id":"18122","name":"World of Warcraft"},
            "endAt":"2026-11-03T17:59:59.999Z","imageURL":"x",
            "timeBasedDrops":[{"id":"d1","name":"Cuddly","startAt":"2026-10-13T17:00:00Z","endAt":"2026-11-03T17:59:59.999Z",
              "benefitEdges":[{"benefit":{"id":"b1","name":"Pet","imageAssetURL":"i"}}],"requiredMinutesWatched":240}]});
        let c = parse_campaign(&v).unwrap();
        assert_eq!(c.game.name, "World of Warcraft");
        assert_eq!(c.drops[0].required_minutes, 240);
        assert_eq!(c.linked, None);
        assert!(c.allowed_channels.is_none());
    }

    #[test]
    fn merge_keeps_progress() {
        let mut a = parse_campaign(&json!({"id":"c","endAt":"2030-01-01T00:00:00Z",
            "timeBasedDrops":[{"id":"d","requiredMinutesWatched":60,"self":{"currentMinutesWatched":30}}]})).unwrap();
        let b = parse_campaign(
            &json!({"id":"c","name":"N","status":"ACTIVE","endAt":"2030-01-01T00:00:00Z",
            "self":{"isAccountConnected":true},
            "timeBasedDrops":[{"id":"d","requiredMinutesWatched":60}]}),
        )
        .unwrap();
        a.merge(b);
        assert_eq!(a.drops[0].current_minutes, 30);
        assert_eq!(a.linked, Some(true));
        assert_eq!(a.name, "N");
    }
}
