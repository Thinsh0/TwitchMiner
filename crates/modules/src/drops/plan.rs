//! Drop planning: decides in which order campaigns are farmed.
//!
//! Pure functions only (no I/O), so every rule is unit-tested.
//!
//! Rules, in order:
//! 1. a focused campaign (user request) goes first;
//! 2. campaigns restricted to a few channels (esports…) are farmed whenever one of
//!    their channels is live: their time is not predictable, so they are not planned;
//! 3. open campaigns are admitted in the user's game order, each one only if every
//!    admitted objective still fits before its deadline; admitted campaigns are then
//!    farmed earliest deadline first (EDF), which meets every deadline when they fit;
//! 4. campaigns that cannot fit are farmed last, for their intermediate rewards
//!    (`fill_partial`).
//!
//! An objective is the campaign's last reward (or the user's wanted rewards):
//! the best rewards usually need the most watch time.

use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use tm_twitch::models::{Campaign, TimedDrop};

/// Availability timeline resolution.
const STEP_MIN: i64 = 5;
/// Planning horizon (campaigns ending later are treated as ending there).
const HORIZON_DAYS: i64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Being farmed right now.
    Active,
    /// Focused by the user: farmed first.
    Focus,
    /// Restricted to a few channels: farmed whenever one of them is live.
    Restricted,
    /// Fits, starts around `start_at`.
    Planned,
    /// Fits, but with less than the safety margin.
    AtRisk,
    /// Cannot be completed before it ends (a higher-priority objective needs the time).
    WillMiss,
    /// Has not started yet.
    Upcoming,
    /// Game account not linked.
    NotLinked,
    /// Temporarily skipped (progress stalled).
    Skipped,
    /// Objective reached.
    Done,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    pub campaign_id: String,
    pub campaign: String,
    pub game: String,
    pub status: Status,
    pub ends_at: DateTime<Utc>,
    /// Rewards the objective aims at.
    pub objective: Vec<String>,
    /// Watch minutes still needed for the objective.
    pub remaining_minutes: u32,
    /// Estimated start and completion (planned campaigns only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta: Option<DateTime<Utc>>,
    /// Minutes that would be missing (`will_miss`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing_minutes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_until: Option<DateTime<Utc>>,
    /// Position in the user's game list (`None` = not listed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_rank: Option<usize>,
}

/// What the planner needs from the settings.
#[derive(Debug, Clone)]
pub struct Params<'a> {
    pub games: &'a [String],
    pub exclude_games: &'a [String],
    pub wanted_rewards: &'a [String],
    pub badges_emotes: bool,
    pub require_linked: bool,
    pub restricted_max_channels: usize,
    pub safety_factor: f64,
    pub safety_minutes: u32,
    pub fill_partial: bool,
    pub focus: Option<&'a str>,
    pub skipped: &'a HashMap<String, DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// Campaign ids to try, best first (the first one with a live channel is farmed).
    pub order: Vec<String>,
    /// Every campaign of a wanted game, for display.
    pub entries: Vec<Entry>,
}

/// Cumulative watchable minutes, sampled every `STEP_MIN` from `start`.
pub struct Availability {
    start: DateTime<Utc>,
    cum: Vec<u32>,
}

impl Availability {
    /// `allowed(t)`: whether watching is allowed at `t` (active hours).
    pub fn new(start: DateTime<Utc>, allowed: impl Fn(DateTime<Utc>) -> bool) -> Self {
        let steps = (HORIZON_DAYS * 24 * 60 / STEP_MIN) as usize;
        let mut cum = Vec::with_capacity(steps + 1);
        let mut total = 0u32;
        cum.push(0);
        for i in 0..steps {
            if allowed(start + Duration::minutes(i as i64 * STEP_MIN)) {
                total += STEP_MIN as u32;
            }
            cum.push(total);
        }
        Self { start, cum }
    }

    pub fn always(start: DateTime<Utc>) -> Self {
        Self::new(start, |_| true)
    }

    fn idx(&self, t: DateTime<Utc>) -> usize {
        let i = (t - self.start).num_minutes().max(0) / STEP_MIN;
        (i as usize).min(self.cum.len() - 1)
    }

    /// Watchable minutes between the start and `t`.
    pub fn until(&self, t: DateTime<Utc>) -> u32 {
        self.cum[self.idx(t)]
    }

    /// The moment the cumulative watchable time reaches `minutes`.
    pub fn time_at(&self, minutes: u32) -> DateTime<Utc> {
        let i = self.cum.partition_point(|&c| c < minutes);
        self.start + Duration::minutes(i as i64 * STEP_MIN)
    }
}

pub fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

/// Whether a campaign can only be watched on a short channel list.
pub fn is_restricted(c: &Campaign, max_channels: usize) -> bool {
    c.allowed_channels
        .as_ref()
        .is_some_and(|a| !a.is_empty() && a.len() < max_channels)
}

fn drop_open(d: &TimedDrop, now: DateTime<Utc>) -> bool {
    d.required_minutes > 0 && !d.claimed && now < d.ends_at
}

fn drop_remaining(d: &TimedDrop) -> u32 {
    if d.claimed {
        0
    } else {
        d.required_minutes.saturating_sub(d.current_minutes)
    }
}

/// Minutes to earn `d`, including its unfinished precondition chain
/// (chained drops progress one after the other).
fn chain_remaining(c: &Campaign, d: &TimedDrop, seen: &mut HashSet<String>) -> u32 {
    if !seen.insert(d.id.clone()) {
        return 0;
    }
    let pre: u32 = d
        .preconditions
        .iter()
        .filter_map(|p| c.drops.iter().find(|x| &x.id == p))
        .map(|p| chain_remaining(c, p, seen))
        .sum();
    pre + drop_remaining(d)
}

fn reward_names(d: &TimedDrop) -> Vec<String> {
    if d.benefits.is_empty() {
        vec![d.name.clone()]
    } else {
        d.benefits.iter().map(|b| b.name.clone()).collect()
    }
}

/// The objective of a campaign: `(reward names, remaining minutes, deadline)`.
/// Independent drops progress together, so the objective costs its longest chain.
pub fn objective(
    c: &Campaign,
    wanted_rewards: &[String],
    now: DateTime<Utc>,
) -> (Vec<String>, u32, DateTime<Utc>) {
    let wanted: Vec<String> = wanted_rewards.iter().map(|w| norm(w)).collect();
    let matches = |d: &TimedDrop| {
        let names = reward_names(d);
        wanted.iter().any(|w| {
            norm(&d.name).contains(w.as_str()) || names.iter().any(|n| norm(n).contains(w.as_str()))
        })
    };
    let wanted_drops: Vec<&TimedDrop> = c.drops.iter().filter(|d| matches(d)).collect();
    let targets: Vec<&TimedDrop> = if wanted.is_empty() || wanted_drops.is_empty() {
        c.drops.iter().filter(|d| drop_open(d, now)).collect()
    } else {
        wanted_drops
            .into_iter()
            .filter(|d| drop_open(d, now))
            .collect()
    };
    let mut names = Vec::new();
    let mut remaining = 0;
    let mut deadline = c.ends_at;
    for d in &targets {
        let r = chain_remaining(c, d, &mut HashSet::new());
        if r == 0 {
            continue;
        }
        remaining = remaining.max(r);
        deadline = deadline.min(d.ends_at);
        names.extend(reward_names(d));
    }
    let mut seen = BTreeSet::new();
    names.retain(|n| seen.insert(n.clone()));
    (names, remaining, deadline)
}

struct Job {
    id: String,
    remaining: u32,
    deadline: DateTime<Utc>,
}

/// EDF feasibility: for each deadline, all work due by then fits in the watchable time.
fn feasible(jobs: &[&Job], avail: &Availability, factor: f64, extra: u32) -> bool {
    let mut sorted: Vec<&&Job> = jobs.iter().collect();
    sorted.sort_by_key(|j| j.deadline);
    let mut demand = 0u64;
    for j in sorted {
        demand += (f64::from(j.remaining) * factor).ceil() as u64 + u64::from(extra);
        if demand > u64::from(avail.until(j.deadline)) {
            return false;
        }
    }
    true
}

/// Largest shortfall (minutes) of an EDF schedule of `jobs`.
fn overload(jobs: &[&Job], avail: &Availability) -> u32 {
    let mut sorted: Vec<&&Job> = jobs.iter().collect();
    sorted.sort_by_key(|j| j.deadline);
    let mut demand = 0u32;
    let mut worst = 0u32;
    for j in sorted {
        demand += j.remaining;
        worst = worst.max(demand.saturating_sub(avail.until(j.deadline)));
    }
    worst
}

pub fn plan(
    campaigns: &[&Campaign],
    p: &Params,
    now: DateTime<Utc>,
    avail: &Availability,
    current: Option<&str>,
) -> Plan {
    let wanted: Vec<String> = p.games.iter().map(|g| norm(g)).collect();
    let excluded: HashSet<String> = p.exclude_games.iter().map(|g| norm(g)).collect();
    let rank = |c: &Campaign| wanted.iter().position(|g| *g == norm(&c.game.name));

    let mut entries: Vec<Entry> = Vec::new();
    let mut focus: Option<String> = None;
    let mut restricted: Vec<(&Campaign, Option<usize>)> = Vec::new();
    let mut open: Vec<(&Campaign, Option<usize>, u32, DateTime<Utc>)> = Vec::new();

    for &c in campaigns {
        if excluded.contains(&norm(&c.game.name)) {
            continue;
        }
        let r = rank(c);
        if !wanted.is_empty() && r.is_none() {
            continue;
        }
        let (names, remaining, deadline) = objective(c, p.wanted_rewards, now);
        let mut e = Entry {
            campaign_id: c.id.clone(),
            campaign: c.name.clone(),
            game: c.game.name.clone(),
            status: Status::Planned,
            ends_at: c.ends_at,
            objective: names,
            remaining_minutes: remaining,
            start_at: None,
            eta: None,
            missing_minutes: None,
            starts_at: c.starts_at,
            link_url: None,
            skipped_until: None,
            game_rank: r,
        };
        let badge_only = c.badge_or_emote_only();
        if c.status == "EXPIRED" || now >= c.ends_at {
            continue;
        }
        if badge_only && !p.badges_emotes {
            continue;
        }
        if c.starts_at.is_some_and(|s| s > now) || c.status == "UPCOMING" {
            e.status = Status::Upcoming;
        } else if remaining == 0 {
            e.status = Status::Done;
        } else if !badge_only && p.require_linked && c.linked == Some(false) {
            e.status = Status::NotLinked;
            e.link_url = c.link_url.clone();
        } else if let Some(until) = p.skipped.get(&c.id).filter(|t| **t > now) {
            e.status = Status::Skipped;
            e.skipped_until = Some(*until);
        } else if p.focus == Some(c.id.as_str()) {
            e.status = Status::Focus;
            focus = Some(c.id.clone());
        } else if is_restricted(c, p.restricted_max_channels) {
            e.status = Status::Restricted;
            restricted.push((c, r));
        } else {
            open.push((c, r, remaining, deadline));
        }
        entries.push(e);
    }

    // Admission in priority order (unlisted games last), then EDF among admitted jobs.
    open.sort_by_key(|(c, r, _, deadline)| (r.unwrap_or(usize::MAX), *deadline, c.id.clone()));
    let horizon = now + Duration::days(HORIZON_DAYS);
    let jobs: Vec<Job> = open
        .iter()
        .map(|(c, _, remaining, deadline)| Job {
            id: c.id.clone(),
            remaining: *remaining,
            deadline: (*deadline).min(horizon),
        })
        .collect();
    let mut admitted: Vec<&Job> = Vec::new();
    let mut missed: Vec<&Job> = Vec::new();
    for j in &jobs {
        admitted.push(j);
        if !feasible(&admitted, avail, 1.0, 0) {
            admitted.pop();
            missed.push(j);
        }
    }
    // Deadlines in the same hour count as equal: the user's priority decides.
    let rank_of = |id: &str| {
        open.iter()
            .find(|(c, ..)| c.id == id)
            .and_then(|(_, r, ..)| *r)
            .unwrap_or(usize::MAX)
    };
    let mut edf = admitted.clone();
    edf.sort_by_key(|j| (j.deadline.timestamp() / 3600, rank_of(&j.id), j.deadline));

    // Estimated schedule and safety check, in EDF order.
    let mut used = 0u32;
    let mut prefix: Vec<&Job> = Vec::new();
    for j in &edf {
        prefix.push(j);
        let safe = feasible(&prefix, avail, p.safety_factor, p.safety_minutes);
        let start = avail.time_at(used);
        used += j.remaining;
        let eta = avail.time_at(used);
        if let Some(e) = entries.iter_mut().find(|e| e.campaign_id == j.id) {
            e.status = if safe {
                Status::Planned
            } else {
                Status::AtRisk
            };
            e.start_at = Some(start.max(now));
            e.eta = Some(eta);
        }
    }
    for j in &missed {
        let mut with: Vec<&Job> = admitted.clone();
        with.push(j);
        if let Some(e) = entries.iter_mut().find(|e| e.campaign_id == j.id) {
            e.status = Status::WillMiss;
            e.missing_minutes = Some(overload(&with, avail).clamp(1, j.remaining));
        }
    }

    let mut order: Vec<String> = Vec::new();
    order.extend(focus);
    restricted.sort_by_key(|(c, r)| (r.unwrap_or(usize::MAX), c.ends_at));
    order.extend(restricted.iter().map(|(c, _)| c.id.clone()));
    order.extend(edf.iter().map(|j| j.id.clone()));
    if p.fill_partial {
        order.extend(missed.iter().map(|j| j.id.clone()));
    }

    if let Some(cur) = current
        && let Some(e) = entries.iter_mut().find(|e| e.campaign_id == cur)
        && order.iter().any(|id| id == cur)
    {
        e.status = Status::Active;
    }
    let pos = |e: &Entry| {
        order
            .iter()
            .position(|id| *id == e.campaign_id)
            .unwrap_or(usize::MAX)
    };
    entries.sort_by_key(|e| (pos(e), e.game_rank.unwrap_or(usize::MAX), e.ends_at));
    Plan { order, entries }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tm_twitch::models::{Benefit, ChannelRef, Game};

    fn drop(id: &str, req: u32, cur: u32, pre: &[&str]) -> TimedDrop {
        TimedDrop {
            id: id.into(),
            name: id.into(),
            starts_at: DateTime::<Utc>::MIN_UTC,
            ends_at: DateTime::<Utc>::MAX_UTC,
            required_minutes: req,
            current_minutes: cur,
            claimed: false,
            claim_id: None,
            preconditions: pre.iter().map(|s| s.to_string()).collect(),
            benefits: vec![Benefit {
                id: format!("b{id}"),
                name: format!("reward {id}"),
                image_url: None,
                kind: "DIRECT_ENTITLEMENT".into(),
            }],
        }
    }

    fn campaign(id: &str, game: &str, hours_left: i64, drops: Vec<TimedDrop>) -> Campaign {
        Campaign {
            id: id.into(),
            name: id.into(),
            game: Game {
                id: game.into(),
                name: game.into(),
                slug: None,
                box_art_url: None,
            },
            status: "ACTIVE".into(),
            starts_at: None,
            ends_at: Utc::now() + Duration::hours(hours_left),
            image_url: None,
            details_url: None,
            link_url: None,
            linked: Some(true),
            allowed_channels: None,
            drops,
        }
    }

    fn params<'a>(games: &'a [String], skipped: &'a HashMap<String, DateTime<Utc>>) -> Params<'a> {
        Params {
            games,
            exclude_games: &[],
            wanted_rewards: &[],
            badges_emotes: true,
            require_linked: true,
            restricted_max_channels: 50,
            safety_factor: 1.25,
            safety_minutes: 120,
            fill_partial: true,
            focus: None,
            skipped,
        }
    }

    fn status(p: &Plan, id: &str) -> Status {
        p.entries
            .iter()
            .find(|e| e.campaign_id == id)
            .unwrap()
            .status
    }

    #[test]
    fn short_campaign_goes_first_when_both_fit() {
        // Rust (priority 1): 10 h left to do, ends in 7 days.
        // ARC (priority 2): 4 h left to do, ends tomorrow.
        let rust = campaign("rust", "Rust", 24 * 7, vec![drop("r", 600, 0, &[])]);
        let arc = campaign("arc", "ARC", 24, vec![drop("a", 240, 0, &[])]);
        let games = ["rust".to_string(), "arc".to_string()];
        let skipped = HashMap::new();
        let now = Utc::now();
        let p = plan(
            &[&rust, &arc],
            &params(&games, &skipped),
            now,
            &Availability::always(now),
            None,
        );
        assert_eq!(p.order, ["arc", "rust"]);
        assert_eq!(status(&p, "rust"), Status::Planned);
    }

    #[test]
    fn priority_wins_when_only_one_fits() {
        // Both end in 24 h; 20 h + 10 h do not fit: the higher game wins.
        let a = campaign("a", "A", 24, vec![drop("x", 20 * 60, 0, &[])]);
        let b = campaign("b", "B", 24, vec![drop("y", 10 * 60, 0, &[])]);
        let games = ["b".to_string(), "a".to_string()];
        let skipped = HashMap::new();
        let now = Utc::now();
        let p = plan(
            &[&a, &b],
            &params(&games, &skipped),
            now,
            &Availability::always(now),
            None,
        );
        assert_eq!(p.order, ["b", "a"], "a is kept as filler");
        assert_eq!(status(&p, "a"), Status::WillMiss);
        let missing = p
            .entries
            .iter()
            .find(|e| e.campaign_id == "a")
            .unwrap()
            .missing_minutes;
        assert!(missing.unwrap() >= 5 * 60);
    }

    #[test]
    fn same_deadline_follows_priority() {
        let a = campaign("a", "A", 48, vec![drop("x", 60, 0, &[])]);
        let b = campaign("b", "B", 48, vec![drop("y", 60, 0, &[])]);
        let games = ["b".to_string(), "a".to_string()];
        let skipped = HashMap::new();
        let now = Utc::now();
        let p = plan(
            &[&a, &b],
            &params(&games, &skipped),
            now,
            &Availability::always(now),
            None,
        );
        assert_eq!(p.order, ["b", "a"]);
    }

    #[test]
    fn restricted_campaigns_first_and_unplanned() {
        let mut esports = campaign("owcs", "Overwatch", 48, vec![drop("o", 120, 0, &[])]);
        esports.allowed_channels = Some(vec![ChannelRef {
            id: "1".into(),
            login: Some("ow_esports".into()),
            display_name: None,
        }]);
        let rust = campaign("rust", "Rust", 24, vec![drop("r", 60, 0, &[])]);
        let games = ["rust".to_string(), "overwatch".to_string()];
        let skipped = HashMap::new();
        let now = Utc::now();
        let p = plan(
            &[&rust, &esports],
            &params(&games, &skipped),
            now,
            &Availability::always(now),
            None,
        );
        assert_eq!(p.order, ["owcs", "rust"]);
        assert_eq!(status(&p, "owcs"), Status::Restricted);
    }

    #[test]
    fn objective_is_the_last_reward_or_the_wanted_one() {
        // Chain a(60) -> b(120); independent c(90). Objective: chain a+b = 180.
        let c = campaign(
            "c",
            "G",
            48,
            vec![
                drop("a", 60, 30, &[]),
                drop("b", 120, 0, &["a"]),
                drop("c", 90, 0, &[]),
            ],
        );
        let now = Utc::now();
        let (names, remaining, _) = objective(&c, &[], now);
        assert_eq!(remaining, 30 + 120);
        assert_eq!(names.len(), 3);
        let (names, remaining, _) = objective(&c, &["reward c".into()], now);
        assert_eq!((names, remaining), (vec!["reward c".to_string()], 90));
    }

    #[test]
    fn active_hours_reduce_capacity() {
        // 3 h of work, ends in 10 h, but only 1 h in 2 is watchable: 5 h available.
        let c = campaign("c", "G", 10, vec![drop("x", 6 * 60, 0, &[])]);
        let games: [String; 0] = [];
        let skipped = HashMap::new();
        let now = Utc::now();
        let half = Availability::new(now, |t| (t - now).num_hours() % 2 == 0);
        let p = plan(&[&c], &params(&games, &skipped), now, &half, None);
        assert_eq!(status(&p, "c"), Status::WillMiss);
    }

    #[test]
    fn margin_flags_tight_campaigns() {
        // 9 h of work in 10 h: fits, but not with 1.25x + 2 h.
        let c = campaign("c", "G", 10, vec![drop("x", 9 * 60, 0, &[])]);
        let games: [String; 0] = [];
        let skipped = HashMap::new();
        let now = Utc::now();
        let p = plan(
            &[&c],
            &params(&games, &skipped),
            now,
            &Availability::always(now),
            Some("c"),
        );
        assert_eq!(p.order, ["c"]);
        assert_eq!(status(&p, "c"), Status::Active);
        let p = plan(
            &[&c],
            &params(&games, &skipped),
            now,
            &Availability::always(now),
            None,
        );
        assert_eq!(status(&p, "c"), Status::AtRisk);
    }

    #[test]
    fn focus_unlinked_and_skipped() {
        let a = campaign("a", "A", 48, vec![drop("x", 60, 0, &[])]);
        let b = campaign("b", "B", 48, vec![drop("y", 60, 0, &[])]);
        let mut unlinked = campaign("u", "A", 48, vec![drop("z", 60, 0, &[])]);
        unlinked.linked = Some(false);
        unlinked.link_url = Some("https://link".into());
        let games: [String; 0] = [];
        let mut skipped = HashMap::new();
        let now = Utc::now();
        skipped.insert("a".to_string(), now + Duration::hours(1));
        let mut p = params(&games, &skipped);
        p.focus = Some("b");
        let out = plan(
            &[&a, &b, &unlinked],
            &p,
            now,
            &Availability::always(now),
            None,
        );
        assert_eq!(out.order, ["b"]);
        assert_eq!(status(&out, "a"), Status::Skipped);
        assert_eq!(status(&out, "u"), Status::NotLinked);
        assert_eq!(status(&out, "b"), Status::Focus);
    }
}
