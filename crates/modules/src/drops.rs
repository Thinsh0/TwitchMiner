//! Drops: discovers campaigns, picks the best campaign + channel, watches it,
//! tracks progress and claims rewards automatically.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tm_core::event::Event;
use tm_core::state::{ChannelMeta, DropTarget};
use tm_core::{Module, ModuleContext, ModuleInfo, WatchRequest, priority};
use tm_twitch::models::{Campaign, ClaimResult, ClaimedBenefit, TimedDrop, UserInfo};
use tm_twitch::pubsub::topics;
use tm_twitch::{PubSubEvent, PubSubMessage};
use tokio::time::Instant;

pub struct Drops;

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DropsMode {
    /// Follow the `games` order, then campaigns ending soonest.
    #[default]
    Priority,
    /// Campaigns ending soonest first, regardless of `games` order.
    EndingSoonest,
    /// Campaigns closest to completion first.
    LeastRemaining,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(default)]
pub struct DropsSettings {
    pub enabled: bool,
    /// Wanted games (names, case-insensitive). Empty = every eligible game.
    pub games: Vec<String>,
    pub exclude_games: Vec<String>,
    pub mode: DropsMode,
    /// Farm badge/emote-only campaigns (they need no game account link).
    pub badges_emotes: bool,
    /// Skip campaigns whose game account is known not to be linked.
    pub require_linked: bool,
    pub auto_claim: bool,
    pub refresh_minutes: u64,
    /// Channels sampled per game when looking for campaigns without the dashboard.
    pub discovery_channels: u32,
}

impl Default for DropsSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            games: Vec::new(),
            exclude_games: Vec::new(),
            mode: DropsMode::Priority,
            badges_emotes: true,
            require_linked: true,
            auto_claim: true,
            refresh_minutes: 60,
            discovery_channels: 8,
        }
    }
}

/// If progress does not move for this long while watching, the channel is skipped.
const STALL_AFTER: Duration = Duration::from_secs(8 * 60);
const CHANNEL_BAN: Duration = Duration::from_secs(30 * 60);
const CAMPAIGN_BAN: Duration = Duration::from_secs(2 * 60 * 60);
const BANS_KEY: &str = "banned_campaigns";
const PROGRESS_POLL: Duration = Duration::from_secs(90);

#[derive(Debug, Clone)]
struct Target {
    campaign_id: String,
    channel: UserInfo,
    started: Instant,
    last_progress: Instant,
    last_minutes: Option<u32>,
}

struct Runtime {
    campaigns: BTreeMap<String, Campaign>,
    claimed: Vec<ClaimedBenefit>,
    target: Option<Target>,
    banned_channels: HashMap<String, Instant>,
    /// Persisted: survives restarts so a broken campaign is not retried immediately.
    banned_campaigns: HashMap<String, DateTime<Utc>>,
    stalls: HashMap<String, u32>,
    slugs: HashMap<String, String>,
    tracked: HashSet<String>,
    last_refresh: Option<Instant>,
}

#[async_trait::async_trait]
impl Module for Drops {
    fn info(&self) -> ModuleInfo {
        ModuleInfo::new(
            "drops",
            "Drops",
            "Finds drop campaigns, watches eligible streams and claims rewards automatically.",
            json!(DropsSettings::default()),
        )
        .with_schema::<DropsSettings>()
    }

    fn validate_settings(&self, v: &Value) -> Result<(), String> {
        let s: DropsSettings = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
        if s.refresh_minutes < 5 {
            return Err("refresh_minutes must be at least 5".into());
        }
        Ok(())
    }

    async fn run(&self, ctx: ModuleContext) -> anyhow::Result<()> {
        let uid = ctx.account_id().to_string();
        let mut settings: DropsSettings = ctx.settings()?;
        let mut rt = Runtime {
            campaigns: BTreeMap::new(),
            claimed: Vec::new(),
            target: None,
            banned_channels: HashMap::new(),
            banned_campaigns: HashMap::new(),
            stalls: HashMap::new(),
            slugs: HashMap::new(),
            tracked: HashSet::new(),
            last_refresh: None,
        };
        if let Ok(Some(bans)) = ctx.kv_get::<HashMap<String, DateTime<Utc>>>(BANS_KEY).await {
            rt.banned_campaigns = bans;
            rt.save_bans(&ctx).await;
        }
        let mut topics = ctx.pubsub().topic_set();
        let mut ps = ctx.pubsub().subscribe();
        let mut events = ctx.events();
        let mut progress_poll = tokio::time::interval(PROGRESS_POLL);
        progress_poll.tick().await;
        let mut need_refresh = true;
        let mut need_pick = true;

        loop {
            if need_refresh
                || rt.last_refresh.is_none_or(|t| {
                    t.elapsed() > Duration::from_secs(60 * settings.refresh_minutes)
                })
            {
                need_refresh = false;
                if let Err(e) = rt.refresh(&ctx, &settings).await {
                    tracing::warn!(error = %e, "drops refresh failed");
                    ctx.set_data("last_error", json!(e.to_string()));
                } else {
                    ctx.set_data("last_error", Value::Null);
                }
                need_pick = true;
            }
            if need_pick {
                need_pick = false;
                rt.pick(&ctx, &settings).await;
                let mut wanted = vec![topics::drops_user(&uid)];
                if let Some(t) = &rt.target {
                    wanted.push(topics::playback(&t.channel.id));
                }
                topics.set(wanted);
            }

            let next_trigger = rt.next_time_trigger();
            tokio::select! {
                _ = ctx.cancelled() => break,
                _ = ctx.settings_changed() => {
                    let next: DropsSettings = ctx.settings()?;
                    if next != settings {
                        // Game lists drive discovery: rebuild the campaign list.
                        need_refresh |= next.games != settings.games || next.exclude_games != settings.exclude_games;
                        settings = next;
                        need_pick = true;
                    }
                }
                _ = sleep_until_utc(next_trigger) => need_refresh = true,
                _ = tokio::time::sleep(Duration::from_secs(60 * settings.refresh_minutes)) => need_refresh = true,
                _ = progress_poll.tick() => {
                    if rt.poll_progress(&ctx).await {
                        need_pick = true;
                    }
                }
                ev = ps.recv() => match ev {
                    Ok(PubSubEvent::Message(m)) => {
                        let (refresh, pick) = rt.on_pubsub(&ctx, &settings, &m).await;
                        need_refresh |= refresh;
                        need_pick |= pick;
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => need_refresh = true,
                    Err(_) => break,
                },
                ev = events.recv() => {
                    if let Ok(env) = ev
                        && let Event::WatchStopped { account, channel_id, cause } = &env.event
                            && account == &uid && rt.target.as_ref().is_some_and(|t| &t.channel.id == channel_id) && cause != "rescheduled" {
                                tracing::info!(%cause, "drops channel stopped, picking another");
                                rt.banned_channels.insert(channel_id.clone(), Instant::now() + Duration::from_secs(5 * 60));
                                need_pick = true;
                            }
                }
                call = ctx.next_call() => {
                    let Some(call) = call else { break };
                    let reply = match call.name.as_str() {
                        "refresh" => { need_refresh = true; Ok(json!({"ok": true})) }
                        "claim_all" => Ok(json!({ "claimed": rt.claim_all(&ctx).await })),
                        "status" => Ok(rt.status()),
                        other => Err(format!("unknown call `{other}`")),
                    };
                    let _ = call.reply.send(reply);
                }
            }
        }
        ctx.emit(Event::DropTargetChanged {
            account: uid,
            target: None,
        });
        for id in rt.tracked.drain() {
            ctx.release_channel(&id);
        }
        Ok(())
    }
}

async fn sleep_until_utc(t: Option<DateTime<Utc>>) {
    match t {
        Some(t) => {
            let d = (t - Utc::now()).to_std().unwrap_or(Duration::ZERO) + Duration::from_secs(5);
            tokio::time::sleep(d).await
        }
        None => std::future::pending().await,
    }
}

fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

impl Runtime {
    /// Rebuilds the campaign list from the inventory, the dashboard (when Twitch shows
    /// it) and channel-level discovery, then claims what can be claimed.
    async fn refresh(&mut self, ctx: &ModuleContext, s: &DropsSettings) -> anyhow::Result<()> {
        let gql = ctx.gql();
        let (in_progress, claimed) = gql.inventory().await?;
        self.claimed = claimed;
        let mut campaigns: BTreeMap<String, Campaign> = BTreeMap::new();

        match gql.dashboard().await {
            Ok(Some(list)) => {
                for c in list.into_iter().filter(|c| c.status != "EXPIRED") {
                    campaigns.insert(c.id.clone(), c);
                }
            }
            Ok(None) => tracing::debug!("drops dashboard hidden, using discovery"),
            Err(e) => tracing::warn!(error = %e, "dashboard failed"),
        }

        // Discovery through live channels of wanted games (works when the dashboard is hidden,
        // and catches campaigns the dashboard omits).
        let mut games: Vec<String> = s.games.clone();
        if games.is_empty() {
            games = in_progress.iter().map(|c| c.game.name.clone()).collect();
            games.extend(campaigns.values().map(|c| c.game.name.clone()));
            games.sort();
            games.dedup();
        }
        let excluded: HashSet<String> = s.exclude_games.iter().map(|g| norm(g)).collect();
        if campaigns.is_empty() || !s.games.is_empty() {
            for game in games
                .iter()
                .filter(|g| !excluded.contains(&norm(g)))
                .take(15)
            {
                match self.discover(ctx, game, s.discovery_channels).await {
                    Ok(found) => {
                        for c in found {
                            match campaigns.get_mut(&c.id) {
                                Some(existing) => existing.merge(c),
                                None => {
                                    campaigns.insert(c.id.clone(), c);
                                }
                            }
                        }
                    }
                    Err(e) => tracing::debug!(%game, error = %e, "discovery failed"),
                }
            }
        }

        // Details (ACL, link status, personal progress) for every campaign.
        let ids: Vec<String> = campaigns.keys().cloned().collect();
        if !ids.is_empty() {
            match gql.campaign_details(&ids).await {
                Ok(details) => {
                    for d in details.into_iter().flatten() {
                        if let Some(c) = campaigns.get_mut(&d.id) {
                            c.merge(d);
                        }
                    }
                }
                Err(e) => tracing::debug!(error = %e, "campaign details failed"),
            }
        }
        for c in in_progress {
            match campaigns.get_mut(&c.id) {
                Some(existing) => existing.merge(c),
                None => {
                    campaigns.insert(c.id.clone(), c);
                }
            }
        }
        for c in campaigns.values_mut() {
            mark_claimed_from_benefits(c, &self.claimed);
        }
        self.campaigns = campaigns;
        self.last_refresh = Some(Instant::now());
        tracing::info!(
            campaigns = self.campaigns.len(),
            "drops campaigns refreshed"
        );
        ctx.emit(Event::CampaignsReplaced {
            account: ctx.account_id().to_string(),
            campaigns: self.campaigns.values().cloned().collect(),
        });
        if s.auto_claim {
            self.claim_all(ctx).await;
        }
        Ok(())
    }

    async fn slug(&mut self, ctx: &ModuleContext, game: &str) -> anyhow::Result<Option<String>> {
        if let Some(s) = self.slugs.get(&norm(game)) {
            return Ok(Some(s.clone()));
        }
        let slug = ctx.gql().game_slug(game).await?;
        if let Some(s) = &slug {
            self.slugs.insert(norm(game), s.clone());
        }
        Ok(slug)
    }

    async fn discover(
        &mut self,
        ctx: &ModuleContext,
        game: &str,
        sample: u32,
    ) -> anyhow::Result<Vec<Campaign>> {
        let Some(slug) = self.slug(ctx, game).await? else {
            return Ok(Vec::new());
        };
        let channels = ctx
            .gql()
            .game_directory(&slug, sample.clamp(1, 30), true)
            .await?;
        let ids: Vec<String> = channels.iter().map(|c| c.id.clone()).collect();
        let mut out: Vec<Campaign> = Vec::new();
        for list in ctx.gql().available_drops(&ids).await? {
            for mut c in list {
                if c.game.slug.is_none() {
                    c.game.slug = Some(slug.clone());
                }
                if !out.iter().any(|o| o.id == c.id) {
                    out.push(c);
                }
            }
        }
        Ok(out)
    }

    fn eligible(&self, s: &DropsSettings, now: DateTime<Utc>) -> Vec<&Campaign> {
        let wanted: Vec<String> = s.games.iter().map(|g| norm(g)).collect();
        let excluded: HashSet<String> = s.exclude_games.iter().map(|g| norm(g)).collect();
        let banned = |id: &str| self.banned_campaigns.get(id).is_some_and(|t| *t > now);
        let mut list: Vec<&Campaign> = self
            .campaigns
            .values()
            .filter(|c| c.is_active(now) && !banned(&c.id))
            .filter(|c| !excluded.contains(&norm(&c.game.name)))
            .filter(|c| wanted.is_empty() || wanted.contains(&norm(&c.game.name)))
            .filter(|c| {
                let badge_only = c.badge_or_emote_only();
                if badge_only {
                    return s.badges_emotes;
                }
                !(s.require_linked && c.linked == Some(false))
            })
            .filter(|c| next_drop(c, now).is_some())
            .collect();
        let game_rank = |c: &Campaign| {
            wanted
                .iter()
                .position(|g| *g == norm(&c.game.name))
                .unwrap_or(usize::MAX)
        };
        match s.mode {
            DropsMode::Priority => list.sort_by_key(|c| (game_rank(c), c.ends_at)),
            DropsMode::EndingSoonest => list.sort_by_key(|c| c.ends_at),
            DropsMode::LeastRemaining => list.sort_by_key(|c| {
                next_drop(c, now)
                    .map(|d| d.required_minutes.saturating_sub(d.current_minutes))
                    .unwrap_or(u32::MAX)
            }),
        }
        list
    }

    /// Chooses the campaign and channel to watch and publishes the watch request.
    async fn pick(&mut self, ctx: &ModuleContext, s: &DropsSettings) {
        let now = Utc::now();
        self.banned_channels.retain(|_, t| *t > Instant::now());
        self.banned_campaigns.retain(|_, t| *t > now);
        let candidates: Vec<Campaign> = self.eligible(s, now).into_iter().cloned().collect();

        // Keep the current target if it is still the best eligible campaign and its channel is fine.
        if let Some(t) = &self.target {
            let still_best = candidates.first().is_some_and(|c| c.id == t.campaign_id);
            if still_best && !self.banned_channels.contains_key(&t.channel.id) {
                self.publish(ctx);
                return;
            }
        }

        let mut chosen: Option<(Campaign, UserInfo)> = None;
        for c in candidates.iter().take(10) {
            match self.find_channel(ctx, c).await {
                Ok(Some(ch)) => {
                    chosen = Some((c.clone(), ch));
                    break;
                }
                Ok(None) => tracing::debug!(campaign = %c.name, "no live channel"),
                Err(e) => tracing::debug!(campaign = %c.name, error = %e, "channel lookup failed"),
            }
        }
        self.target = chosen.map(|(c, ch)| {
            tracing::info!(game = %c.game.name, campaign = %c.name, channel = %ch.login, "drops target");
            Target { campaign_id: c.id, channel: ch, started: Instant::now(), last_progress: Instant::now(), last_minutes: None }
        });
        if self.target.is_none() && !candidates.is_empty() {
            tracing::info!(
                eligible = candidates.len(),
                "no live channel for eligible campaigns"
            );
        }
        self.publish(ctx);
    }

    async fn find_channel(
        &self,
        ctx: &ModuleContext,
        c: &Campaign,
    ) -> anyhow::Result<Option<UserInfo>> {
        let gql = ctx.gql();
        let special = gql.manifest().load().special_game_ids.contains(&c.game.id);
        let plays_game = |u: &UserInfo| {
            special
                || u.stream
                    .as_ref()
                    .and_then(|s| s.game.as_ref())
                    .is_some_and(|g| g.id == c.game.id || norm(&g.name) == norm(&c.game.name))
        };
        let ok = |u: &UserInfo| !self.banned_channels.contains_key(&u.id);
        if let Some(acl) = &c.allowed_channels {
            let ids: Vec<String> = acl.iter().map(|a| a.id.clone()).collect();
            let mut users: Vec<UserInfo> = gql
                .users_by_id(&ids)
                .await?
                .into_iter()
                .flatten()
                .filter(|u| plays_game(u) && ok(u))
                .collect();
            users.sort_by_key(|u| {
                std::cmp::Reverse(u.stream.as_ref().map(|s| s.viewers).unwrap_or(0))
            });
            return Ok(users.into_iter().next());
        }
        let slug = match &c.game.slug {
            Some(s) => s.clone(),
            None => match gql.game_slug(&c.game.name).await? {
                Some(s) => s,
                None => return Ok(None),
            },
        };
        let channels: Vec<UserInfo> = gql
            .game_directory(&slug, 20, true)
            .await?
            .into_iter()
            .filter(ok)
            .collect();
        // Make sure the channel really participates in this campaign.
        let sample: Vec<UserInfo> = channels.iter().take(6).cloned().collect();
        let ids: Vec<String> = sample.iter().map(|u| u.id.clone()).collect();
        if let Ok(lists) = gql.available_drops(&ids).await {
            for (u, list) in sample.iter().zip(lists) {
                if list.iter().any(|x| x.id == c.id) {
                    return Ok(Some(u.clone()));
                }
            }
        }
        Ok(None)
    }

    fn publish(&mut self, ctx: &ModuleContext) {
        let account = ctx.account_id().to_string();
        let now = Utc::now();
        let wanted: HashSet<String> = self.target.iter().map(|t| t.channel.id.clone()).collect();
        for id in self
            .tracked
            .difference(&wanted)
            .cloned()
            .collect::<Vec<_>>()
        {
            ctx.release_channel(&id);
            self.tracked.remove(&id);
        }
        match &self.target {
            Some(t) => {
                let c = self.campaigns.get(&t.campaign_id);
                // Idempotent: unchanged events are dropped by the store.
                self.tracked.insert(t.channel.id.clone());
                ctx.track_channel(ChannelMeta {
                    id: t.channel.id.clone(),
                    login: t.channel.login.clone(),
                    display_name: t.channel.display_name.clone(),
                    avatar_url: t.channel.avatar_url.clone(),
                });
                if ctx.store().read(|s| {
                    s.accounts
                        .get(ctx.account_id())
                        .and_then(|a| a.channels.get(&t.channel.id))
                        .is_some_and(|c| c.stream.is_none())
                }) {
                    ctx.emit(Event::StreamChanged {
                        account: account.clone(),
                        channel_id: t.channel.id.clone(),
                        stream: t.channel.stream.clone(),
                    });
                }
                let game = c.map(|c| c.game.name.clone()).unwrap_or_default();
                ctx.emit(Event::DropTargetChanged {
                    account,
                    target: Some(DropTarget {
                        campaign_id: t.campaign_id.clone(),
                        drop_id: c.and_then(|c| next_drop(c, now)).map(|d| d.id.clone()),
                        channel_id: t.channel.id.clone(),
                        game: game.clone(),
                    }),
                });
                ctx.set_watch_requests(vec![WatchRequest {
                    channel_id: t.channel.id.clone(),
                    login: t.channel.login.clone(),
                    priority: priority::DROPS,
                    reason: format!("drops: {game}"),
                }]);
            }
            None => {
                ctx.emit(Event::DropTargetChanged {
                    account,
                    target: None,
                });
                ctx.set_watch_requests(Vec::new());
            }
        }
    }

    /// Polls the current drop session. Returns `true` when a new pick is needed.
    async fn poll_progress(&mut self, ctx: &ModuleContext) -> bool {
        let Some(t) = &mut self.target else {
            return false;
        };
        let watching = ctx.store().read(|s| {
            s.accounts
                .get(ctx.account_id())
                .is_some_and(|a| a.watching.contains_key(&t.channel.id))
        });
        if !watching {
            t.last_progress = Instant::now();
            return false;
        }
        match ctx.gql().current_drop(&t.channel.id).await {
            Ok(Some(cur)) => {
                if t.last_minutes.is_none_or(|m| cur.current_minutes > m) {
                    t.last_minutes = Some(cur.current_minutes);
                    t.last_progress = Instant::now();
                }
                let campaign_id = self
                    .campaigns
                    .values()
                    .find(|c| c.drops.iter().any(|d| d.id == cur.drop_id))
                    .map(|c| c.id.clone());
                if let Some(campaign_id) = campaign_id {
                    ctx.emit(Event::DropProgress {
                        account: ctx.account_id().to_string(),
                        campaign_id: campaign_id.clone(),
                        drop_id: cur.drop_id.clone(),
                        current_minutes: cur.current_minutes,
                        required_minutes: cur.required_minutes,
                    });
                    if let Some(d) = self
                        .campaigns
                        .get_mut(&campaign_id)
                        .and_then(|c| c.drops.iter_mut().find(|d| d.id == cur.drop_id))
                    {
                        d.current_minutes = cur.current_minutes;
                    }
                }
            }
            Ok(None) => {}
            Err(e) => tracing::debug!(error = %e, "current drop poll failed"),
        }
        let Some(t) = &self.target else { return false };
        if t.last_progress.elapsed() > STALL_AFTER && t.started.elapsed() > STALL_AFTER {
            let stalls = self.stalls.entry(t.campaign_id.clone()).or_insert(0);
            *stalls += 1;
            tracing::warn!(channel = %t.channel.login, stalls = *stalls, "drop progress stalled, switching channel");
            self.banned_channels
                .insert(t.channel.id.clone(), Instant::now() + CHANNEL_BAN);
            if *stalls >= 2 {
                tracing::warn!(campaign = %t.campaign_id, "campaign does not progress (account not linked?), skipping it");
                let until =
                    Utc::now() + chrono::Duration::from_std(CAMPAIGN_BAN).unwrap_or_default();
                self.banned_campaigns.insert(t.campaign_id.clone(), until);
                self.stalls.remove(&t.campaign_id.clone());
                self.save_bans(ctx).await;
            }
            self.target = None;
            return true;
        }
        false
    }

    /// Returns `(refresh, pick)`.
    async fn on_pubsub(
        &mut self,
        ctx: &ModuleContext,
        s: &DropsSettings,
        m: &PubSubMessage,
    ) -> (bool, bool) {
        match (m.topic_kind.as_str(), m.kind.as_str()) {
            ("user-drop-events", "drop-progress") => {
                let drop_id = m
                    .data
                    .get("drop_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let cur = m
                    .data
                    .get("current_progress_min")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32;
                let req = m
                    .data
                    .get("required_progress_min")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32;
                let Some(c) = self
                    .campaigns
                    .values_mut()
                    .find(|c| c.drops.iter().any(|d| d.id == drop_id))
                else {
                    return (true, false);
                };
                if let Some(d) = c.drops.iter_mut().find(|d| d.id == drop_id) {
                    d.current_minutes = cur;
                }
                if let Some(t) = &mut self.target {
                    t.last_progress = Instant::now();
                    t.last_minutes = Some(cur);
                }
                self.stalls.remove(&c.id);
                ctx.emit(Event::DropProgress {
                    account: ctx.account_id().to_string(),
                    campaign_id: c.id.clone(),
                    drop_id: drop_id.to_string(),
                    current_minutes: cur,
                    required_minutes: req,
                });
                (false, false)
            }
            ("user-drop-events", "drop-claim") => {
                let drop_id = m
                    .data
                    .get("drop_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let instance = m
                    .data
                    .get("drop_instance_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if let Some(d) = self
                    .campaigns
                    .values_mut()
                    .flat_map(|c| c.drops.iter_mut())
                    .find(|d| d.id == drop_id)
                {
                    d.claim_id = Some(instance);
                    d.current_minutes = d.current_minutes.max(d.required_minutes);
                }
                if s.auto_claim {
                    self.claim_all(ctx).await;
                }
                // The next drop becomes available a few seconds after a claim.
                tokio::time::sleep(Duration::from_secs(5)).await;
                (true, true)
            }
            ("video-playback-by-id", "stream-down") => {
                let is_target = self
                    .target
                    .as_ref()
                    .is_some_and(|t| t.channel.id == m.target_id);
                if is_target {
                    self.banned_channels.insert(
                        m.target_id.clone(),
                        Instant::now() + Duration::from_secs(5 * 60),
                    );
                    self.target = None;
                }
                (false, is_target)
            }
            _ => (false, false),
        }
    }

    async fn claim_all(&mut self, ctx: &ModuleContext) -> Vec<String> {
        let mut done = Vec::new();
        let claimable: Vec<(String, String, String, String, String)> = self
            .campaigns
            .values()
            .flat_map(|c| {
                c.drops.iter().filter(|d| d.can_claim()).map(move |d| {
                    (
                        c.id.clone(),
                        d.id.clone(),
                        d.name.clone(),
                        c.game.name.clone(),
                        d.claim_id.clone().unwrap_or_default(),
                    )
                })
            })
            .collect();
        for (campaign_id, drop_id, name, game, claim_id) in claimable {
            match ctx.gql().claim_drop(&claim_id).await {
                Ok(ClaimResult::Claimed | ClaimResult::AlreadyClaimed) => {
                    tracing::info!(%game, drop = %name, "drop claimed");
                    if let Some(d) = self
                        .campaigns
                        .get_mut(&campaign_id)
                        .and_then(|c| c.drops.iter_mut().find(|d| d.id == drop_id))
                    {
                        d.claimed = true;
                    }
                    ctx.emit(Event::DropClaimed {
                        account: ctx.account_id().to_string(),
                        campaign_id,
                        drop_id,
                        drop_name: name.clone(),
                        game,
                    });
                    done.push(name);
                }
                Ok(ClaimResult::Failed(why)) => {
                    tracing::warn!(drop = %name, %why, "drop claim failed")
                }
                Err(e) => tracing::warn!(drop = %name, error = %e, "drop claim error"),
            }
        }
        done
    }

    /// Persists campaign bans and publishes them for the UI (`module_data.drops.skipped_campaigns`).
    async fn save_bans(&mut self, ctx: &ModuleContext) {
        let now = Utc::now();
        self.banned_campaigns.retain(|_, t| *t > now);
        if let Err(e) = ctx.kv_put(BANS_KEY, &self.banned_campaigns).await {
            tracing::warn!(error = %e, "could not save campaign bans");
        }
        ctx.set_data("skipped_campaigns", json!(self.banned_campaigns));
    }

    fn next_time_trigger(&self) -> Option<DateTime<Utc>> {
        let now = Utc::now();
        self.campaigns
            .values()
            .flat_map(|c| {
                let mut t = vec![c.ends_at];
                t.extend(c.starts_at);
                t.extend(c.drops.iter().flat_map(|d| [d.starts_at, d.ends_at]));
                t
            })
            .filter(|t| *t > now && *t < now + chrono::Duration::days(7))
            .min()
    }

    fn status(&self) -> Value {
        json!({
            "campaigns": self.campaigns.len(),
            "target": self.target.as_ref().map(|t| json!({
                "campaign_id": t.campaign_id,
                "channel": t.channel.login,
                "minutes": t.last_minutes,
                "since_progress_s": t.last_progress.elapsed().as_secs(),
            })),
            "banned_channels": self.banned_channels.len(),
            "banned_campaigns": self.banned_campaigns.keys().collect::<Vec<_>>(),
        })
    }
}

/// The drop that watching would currently advance.
fn next_drop(c: &Campaign, now: DateTime<Utc>) -> Option<&TimedDrop> {
    let claimed: HashSet<&str> = c
        .drops
        .iter()
        .filter(|d| d.claimed)
        .map(|d| d.id.as_str())
        .collect();
    c.drops
        .iter()
        .filter(|d| !d.claimed && d.required_minutes > 0 && d.current_minutes < d.required_minutes)
        .filter(|d| d.starts_at <= now && now < d.ends_at)
        .filter(|d| {
            d.preconditions
                .iter()
                .all(|p| claimed.contains(p.as_str()) || !c.drops.iter().any(|x| &x.id == p))
        })
        .min_by_key(|d| d.required_minutes - d.current_minutes)
}

/// Marks drops as claimed when all their benefits were awarded during the drop window
/// (the inventory lists claimed benefits, not claimed drops).
fn mark_claimed_from_benefits(c: &mut Campaign, claimed: &[ClaimedBenefit]) {
    for d in &mut c.drops {
        if d.claimed || d.benefits.is_empty() {
            continue;
        }
        let all = d.benefits.iter().all(|b| {
            claimed.iter().any(|cb| {
                cb.id == b.id
                    && cb
                        .last_awarded_at
                        .is_none_or(|t| t >= d.starts_at && t < d.ends_at)
            })
        });
        if all {
            d.claimed = true;
            d.current_minutes = d.current_minutes.max(d.required_minutes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tm_twitch::models::{Benefit, Game};

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
                name: id.into(),
                image_url: None,
                kind: "DIRECT_ENTITLEMENT".into(),
            }],
        }
    }

    fn campaign(id: &str, game: &str, drops: Vec<TimedDrop>) -> Campaign {
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
            ends_at: Utc::now() + chrono::Duration::days(1),
            image_url: None,
            details_url: None,
            link_url: None,
            linked: Some(true),
            allowed_channels: None,
            drops,
        }
    }

    #[test]
    fn preconditions_gate_next_drop() {
        let c = campaign(
            "c",
            "g",
            vec![drop("a", 60, 10, &[]), drop("b", 30, 0, &["a"])],
        );
        assert_eq!(next_drop(&c, Utc::now()).unwrap().id, "a");
    }

    #[test]
    fn eligibility_respects_link_and_games() {
        let mut rt = Runtime {
            campaigns: BTreeMap::new(),
            claimed: vec![],
            target: None,
            banned_channels: HashMap::new(),
            banned_campaigns: HashMap::new(),
            stalls: HashMap::new(),
            slugs: HashMap::new(),
            tracked: HashSet::new(),
            last_refresh: None,
        };
        let mut unlinked = campaign("u", "Rust", vec![drop("x", 60, 0, &[])]);
        unlinked.linked = Some(false);
        rt.campaigns.insert("u".into(), unlinked);
        rt.campaigns.insert(
            "w".into(),
            campaign("w", "WoW", vec![drop("y", 60, 0, &[])]),
        );
        rt.campaigns.insert(
            "r".into(),
            campaign("r", "Rust", vec![drop("z", 60, 0, &[])]),
        );
        let s = DropsSettings {
            games: vec!["rust".into(), "wow".into()],
            ..Default::default()
        };
        let ids: Vec<&str> = rt
            .eligible(&s, Utc::now())
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(ids, ["r", "w"]);
    }

    #[test]
    fn benefits_mark_claimed() {
        let mut c = campaign("c", "g", vec![drop("a", 60, 0, &[])]);
        mark_claimed_from_benefits(
            &mut c,
            &[ClaimedBenefit {
                id: "ba".into(),
                last_awarded_at: Some(Utc::now()),
            }],
        );
        assert!(c.drops[0].claimed);
    }
}
