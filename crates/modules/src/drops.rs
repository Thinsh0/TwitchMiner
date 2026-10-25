//! Drops: discovers campaigns, picks the best campaign + channel, watches it,
//! tracks progress and claims rewards automatically.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tm_core::event::Event;
use tm_core::settings::EngineSettings;
use tm_core::state::{ChannelMeta, DropAlertInfo, DropAlertKind, DropTarget};
use tm_core::{Module, ModuleContext, ModuleInfo, WatchRequest, priority};
use tm_twitch::models::{Campaign, ClaimResult, ClaimedBenefit, TimedDrop, UserInfo};
use tm_twitch::pubsub::topics;
use tm_twitch::{PubSubEvent, PubSubMessage};
use tokio::time::Instant;

mod plan;

use plan::{Availability, Plan, Status, norm};

pub struct Drops;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(default)]
pub struct DropsSettings {
    pub enabled: bool,
    /// Wanted games in priority order (names, case-insensitive). Empty = every eligible game.
    pub games: Vec<String>,
    pub exclude_games: Vec<String>,
    /// Wanted rewards (part of the reward name, case-insensitive). In a campaign that
    /// has one, the objective stops at it; other campaigns are farmed to their last reward.
    pub wanted_rewards: Vec<String>,
    /// Farm badge/emote-only campaigns (they need no game account link).
    pub badges_emotes: bool,
    /// Skip campaigns whose game account is known not to be linked.
    pub require_linked: bool,
    pub auto_claim: bool,
    pub refresh_minutes: u64,
    /// Channels sampled per game when looking for campaigns without the dashboard.
    pub discovery_channels: u32,
    /// Campaigns limited to fewer channels than this (esports…) are farmed first
    /// whenever one of their channels is live.
    pub restricted_max_channels: u32,
    /// A campaign is "at risk" when `needed × safety_factor + safety_minutes` does not fit
    /// before it ends. The margin never excludes a campaign.
    pub safety_factor: f64,
    pub safety_minutes: u32,
    /// Spend spare time on campaigns that cannot be completed (their first rewards).
    pub fill_partial: bool,
    /// Alerts (event `drop_alert`) for campaigns of the `games` list.
    pub alerts: AlertSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(default)]
pub struct AlertSettings {
    pub at_risk: bool,
    pub will_miss: bool,
    pub not_linked: bool,
    pub new_campaign: bool,
}

impl Default for AlertSettings {
    fn default() -> Self {
        Self {
            at_risk: true,
            will_miss: true,
            not_linked: true,
            new_campaign: true,
        }
    }
}

impl Default for DropsSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            games: Vec::new(),
            exclude_games: Vec::new(),
            wanted_rewards: Vec::new(),
            badges_emotes: true,
            require_linked: true,
            auto_claim: true,
            refresh_minutes: 60,
            discovery_channels: 8,
            restricted_max_channels: 50,
            safety_factor: 1.25,
            safety_minutes: 120,
            fill_partial: true,
            alerts: AlertSettings::default(),
        }
    }
}

/// A campaign the user asked to farm first, until its objective is reached.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Focus {
    campaign_id: String,
}

/// If progress does not move for this long while watching, the channel is skipped.
const STALL_AFTER: Duration = Duration::from_secs(8 * 60);
const CHANNEL_BAN: Duration = Duration::from_secs(30 * 60);
const CAMPAIGN_BAN: Duration = Duration::from_secs(2 * 60 * 60);
const BANS_KEY: &str = "banned_campaigns";
const STALLS_KEY: &str = "stalls";
const FOCUS_KEY: &str = "focus";
const ALERTED_KEY: &str = "alerted";
const KNOWN_KEY: &str = "known_campaigns";
/// Twitch id of each listed game name, so a renamed game keeps matching.
const GAME_IDS_KEY: &str = "game_ids";
const PROGRESS_POLL: Duration = Duration::from_secs(90);
/// How often a better campaign (e.g. an esports channel going live) is looked for.
const RECHECK: Duration = Duration::from_secs(5 * 60);
/// Open campaigns tried per pick (each costs a directory lookup).
const DIRECTORY_LOOKUPS: usize = 10;

#[derive(Debug, Clone)]
struct Target {
    campaign_id: String,
    channel: UserInfo,
    started: Instant,
    last_progress: Instant,
    /// Last progress seen: drop id and its minutes (the next drop restarts at 0).
    last_minutes: Option<(String, u32)>,
}

struct Runtime {
    campaigns: BTreeMap<String, Campaign>,
    claimed: Vec<ClaimedBenefit>,
    /// `norm(listed name) → game id` (persisted).
    game_ids: HashMap<String, String>,
    target: Option<Target>,
    banned_channels: HashMap<String, Instant>,
    /// Persisted: survives restarts so a broken campaign is not retried immediately.
    banned_campaigns: HashMap<String, DateTime<Utc>>,
    stalls: HashMap<String, u32>,
    slugs: HashMap<String, String>,
    tracked: HashSet<String>,
    last_refresh: Option<Instant>,
    plan: Plan,
    focus: Option<Focus>,
    /// Alerts already sent (`kind:campaign` → campaign end), persisted.
    alerted: HashMap<String, DateTime<Utc>>,
    /// Campaign ids already seen (`None` until loaded: the first refresh only records).
    known: Option<HashMap<String, DateTime<Utc>>>,
    /// Drop minutes moved since the plan was last published.
    plan_stale: bool,
}

impl Runtime {
    fn new() -> Self {
        Self {
            campaigns: BTreeMap::new(),
            claimed: Vec::new(),
            game_ids: HashMap::new(),
            target: None,
            banned_channels: HashMap::new(),
            banned_campaigns: HashMap::new(),
            stalls: HashMap::new(),
            slugs: HashMap::new(),
            tracked: HashSet::new(),
            last_refresh: None,
            plan: Plan::default(),
            plan_stale: false,
            focus: None,
            alerted: HashMap::new(),
            known: None,
        }
    }
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
        if !(1.0..=5.0).contains(&s.safety_factor) {
            return Err("safety_factor must be between 1 and 5".into());
        }
        Ok(())
    }

    async fn run(&self, ctx: ModuleContext) -> anyhow::Result<()> {
        let uid = ctx.account_id().to_string();
        let mut settings: DropsSettings = ctx.settings()?;
        let mut rt = Runtime::new();
        if let Ok(Some(stalls)) = ctx.kv_get::<HashMap<String, u32>>(STALLS_KEY).await {
            rt.stalls = stalls;
        }
        rt.focus = ctx
            .kv_get::<Option<Focus>>(FOCUS_KEY)
            .await
            .ok()
            .flatten()
            .flatten();
        if let Ok(Some(alerted)) = ctx.kv_get(ALERTED_KEY).await {
            rt.alerted = alerted;
        }
        rt.known = ctx.kv_get(KNOWN_KEY).await.ok().flatten();
        rt.game_ids = ctx
            .kv_get(GAME_IDS_KEY)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        ctx.set_data("game_ids", json!(rt.game_ids));
        ctx.set_data("focus", json!(rt.focus));
        if let Ok(Some(bans)) = ctx.kv_get::<HashMap<String, DateTime<Utc>>>(BANS_KEY).await {
            rt.banned_campaigns = bans;
            rt.save_bans(&ctx).await;
        }
        let mut topics = ctx.pubsub().topic_set();
        let mut ps = ctx.pubsub().subscribe();
        let mut events = ctx.events();
        let mut progress_poll = tokio::time::interval(PROGRESS_POLL);
        progress_poll.tick().await;
        let mut recheck = tokio::time::interval(RECHECK);
        recheck.tick().await;
        let mut need_refresh = true;
        let mut need_pick = true;
        let mut active_hours = ctx.store().read(EngineSettings::from_state).active_hours;
        // After a failed refresh, the next one waits (Twitch outage: no hammering).
        let mut retry =
            tm_twitch::net::Backoff::new(Duration::from_secs(30), Duration::from_secs(15 * 60));
        let mut retry_at: Option<Instant> = None;

        loop {
            let due = need_refresh
                || rt.last_refresh.is_none_or(|t| {
                    t.elapsed() > Duration::from_secs(60 * settings.refresh_minutes)
                });
            if due && retry_at.is_none_or(|t| Instant::now() >= t) {
                need_refresh = false;
                if let Err(e) = rt.refresh(&ctx, &settings).await {
                    let delay = retry.next_delay();
                    tracing::warn!(error = %e, ?delay, "drops refresh failed");
                    ctx.set_data("last_error", json!(e.to_string()));
                    retry_at = Some(Instant::now() + delay);
                    // Still owed: retried at `retry_at` (keeps that timer meaningful).
                    need_refresh = true;
                } else {
                    ctx.set_data("last_error", Value::Null);
                    retry.reset();
                    retry_at = None;
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
            } else if std::mem::take(&mut rt.plan_stale) {
                // Remaining time and progress of the published plan follow the minutes.
                rt.republish_plan(&ctx, &settings).await;
            }

            let next_trigger = rt.next_time_trigger();
            tokio::select! {
                _ = ctx.cancelled() => break,
                _ = ctx.settings_changed() => {
                    // The watching hours change how much time the plan has.
                    let hours = ctx.store().read(EngineSettings::from_state).active_hours;
                    if hours != active_hours {
                        active_hours = hours;
                        need_pick = true;
                    }
                    let next: DropsSettings = ctx.settings()?;
                    if next != settings {
                        // Game lists drive discovery: rebuild the campaign list.
                        need_refresh |= next.games != settings.games || next.exclude_games != settings.exclude_games;
                        settings = next;
                        need_pick = true;
                    }
                }
                _ = sleep_until_utc(next_trigger) => need_refresh = true,
                _ = tokio::time::sleep_until(retry_at.unwrap_or_else(Instant::now)), if retry_at.is_some() => {}
                _ = tokio::time::sleep(Duration::from_secs(60 * settings.refresh_minutes)) => need_refresh = true,
                _ = progress_poll.tick() => {
                    let (refresh, pick) = rt.poll_progress(&ctx, &settings).await;
                    need_refresh |= refresh;
                    need_pick |= pick;
                }
                _ = recheck.tick() => need_pick |= rt.better_may_exist(),
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
                            && account == &uid && rt.target.as_ref().is_some_and(|t| &t.channel.id == channel_id)
                            // Only a channel problem (not a pause, the active hours or a
                            // reschedule) puts the channel aside.
                            && (cause == "offline" || cause.starts_with("error")) {
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
                        "plan" => Ok(json!(rt.plan.entries)),
                        "focus" => {
                            let r = rt.set_focus(&ctx, &call.args).await;
                            need_pick |= r.is_ok();
                            r
                        }
                        "unfocus" => {
                            rt.save_focus(&ctx, None).await;
                            need_pick = true;
                            Ok(json!({"ok": true}))
                        }
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

/// Watchable time ahead, from the engine's active hours (a pause is temporary: ignored).
fn availability(engine: &EngineSettings, now: DateTime<Utc>) -> Availability {
    if engine.active_hours.is_empty() {
        return Availability::always(now);
    }
    let hours = EngineSettings {
        paused: false,
        ..engine.clone()
    };
    Availability::new(now, |t| {
        hours.watching_allowed(t.with_timezone(&chrono::Local))
    })
}

fn alert_info(kind: DropAlertKind, e: &plan::Entry) -> DropAlertInfo {
    DropAlertInfo {
        kind,
        campaign_id: e.campaign_id.clone(),
        campaign: e.campaign.clone(),
        game: e.game.clone(),
        ends_at: e.ends_at,
        starts_at: e.starts_at,
        missing_minutes: e.missing_minutes,
        link_url: e.link_url.clone(),
        rewards: e.objective.clone(),
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

impl Runtime {
    /// Rebuilds the campaign list from the inventory, the dashboard (when Twitch shows
    /// it) and channel-level discovery, then claims what can be claimed.
    async fn refresh(&mut self, ctx: &ModuleContext, s: &DropsSettings) -> anyhow::Result<()> {
        let gql = ctx.gql();
        let (in_progress, claimed) = gql.inventory().await?;
        self.claimed = claimed;
        // Every reward ever received, most recent first (`module_data.drops.inventory`).
        let mut inventory = self.claimed.clone();
        inventory.sort_by_key(|b| std::cmp::Reverse(b.last_awarded_at));
        ctx.set_data("inventory", json!(inventory));
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

        let seen = campaigns.values().chain(in_progress.iter());
        let mut learned = self.learn_games(s, seen.clone());
        // Discovery through live channels of wanted games (works when the dashboard is hidden,
        // and catches campaigns the dashboard omits).
        let mut games: Vec<String> = plan::resolve_games(&s.games, seen.clone(), &self.game_ids);
        let exclude_games = plan::resolve_games(&s.exclude_games, seen, &self.game_ids);
        if games.is_empty() {
            games = in_progress.iter().map(|c| c.game.name.clone()).collect();
            games.extend(campaigns.values().map(|c| c.game.name.clone()));
            games.sort();
            games.dedup();
        }
        let excluded: HashSet<String> = exclude_games.iter().map(|g| norm(g)).collect();
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
        learned |= self.learn_games(s, campaigns.values());
        if learned {
            if let Err(e) = ctx.kv_put(GAME_IDS_KEY, &self.game_ids).await {
                tracing::warn!(error = %e, "could not save game ids");
            }
            ctx.set_data("game_ids", json!(self.game_ids));
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
        self.detect_new(ctx, s).await;
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
        // The directory's game, as Twitch names it.
        let dir_game = channels
            .iter()
            .find_map(|c| c.stream.as_ref()?.game.clone());
        let mut out: Vec<Campaign> = Vec::new();
        for list in ctx.gql().available_drops(&ids).await? {
            for mut c in list {
                if c.game.id.is_empty()
                    && let Some(g) = &dir_game
                {
                    // Some responses omit the game: it is the directory's.
                    c.game = g.clone();
                }
                let same_game = dir_game.as_ref().is_some_and(|g| g.id == c.game.id)
                    || norm(&c.game.name) == norm(game);
                if same_game && c.game.slug.is_none() {
                    c.game.slug = Some(slug.clone());
                }
                if !out.iter().any(|o| o.id == c.id) {
                    out.push(c);
                }
            }
        }
        Ok(out)
    }

    /// Recomputes the plan (pure: see [`plan`]).
    /// Learns the ids of the listed games shown by `campaigns` (see `plan::learn_game_ids`).
    fn learn_games<'a>(
        &mut self,
        s: &DropsSettings,
        campaigns: impl IntoIterator<Item = &'a Campaign>,
    ) -> bool {
        let names: Vec<String> = s.games.iter().chain(&s.exclude_games).cloned().collect();
        plan::learn_game_ids(&names, campaigns, &mut self.game_ids)
    }

    /// A game list of the settings, renamed games under their current Twitch name.
    fn resolved(&self, names: &[String]) -> Vec<String> {
        plan::resolve_games(names, self.campaigns.values(), &self.game_ids)
    }

    fn replan(&mut self, s: &DropsSettings, engine: &EngineSettings, now: DateTime<Utc>) {
        let games = self.resolved(&s.games);
        let exclude_games = self.resolved(&s.exclude_games);
        let params = plan::Params {
            games: &games,
            exclude_games: &exclude_games,
            wanted_rewards: &s.wanted_rewards,
            badges_emotes: s.badges_emotes,
            require_linked: s.require_linked,
            restricted_max_channels: s.restricted_max_channels as usize,
            safety_factor: s.safety_factor,
            safety_minutes: s.safety_minutes,
            fill_partial: s.fill_partial,
            focus: self.focus.as_ref().map(|f| f.campaign_id.as_str()),
            skipped: &self.banned_campaigns,
        };
        let campaigns: Vec<&Campaign> = self.campaigns.values().collect();
        let current = self.target.as_ref().map(|t| t.campaign_id.as_str());
        self.plan = plan::plan(
            &campaigns,
            &params,
            now,
            &availability(engine, now),
            current,
        );
    }

    /// Campaigns to try, best first.
    fn candidates(&self, now: DateTime<Utc>) -> Vec<Campaign> {
        self.plan
            .order
            .iter()
            .filter_map(|id| self.campaigns.get(id))
            .filter(|c| next_drop(c, now).is_some())
            .cloned()
            .collect()
    }

    /// Whether something ranks above the current target (worth looking for a live channel).
    fn better_may_exist(&self) -> bool {
        self.plan
            .order
            .first()
            .is_some_and(|id| self.target.as_ref().is_none_or(|t| &t.campaign_id != id))
    }

    async fn save_focus(&mut self, ctx: &ModuleContext, focus: Option<Focus>) {
        self.focus = focus;
        if let Err(e) = ctx.kv_put(FOCUS_KEY, &self.focus).await {
            tracing::warn!(error = %e, "could not save drops focus");
        }
        ctx.set_data("focus", json!(self.focus));
    }

    /// `focus {campaign_id}` or `focus {game}` (best campaign of that game in the plan).
    async fn set_focus(&mut self, ctx: &ModuleContext, args: &Value) -> Result<Value, String> {
        let farmable = |e: &plan::Entry| {
            !matches!(
                e.status,
                Status::Done | Status::Upcoming | Status::NotLinked
            )
        };
        let entry = if let Some(id) = args.get("campaign_id").and_then(Value::as_str) {
            self.plan.entries.iter().find(|e| e.campaign_id == id)
        } else if let Some(game) = args.get("game").and_then(Value::as_str) {
            self.plan
                .entries
                .iter()
                .find(|e| norm(&e.game) == norm(game) && farmable(e))
        } else {
            return Err("give `campaign_id` or `game`".into());
        };
        let Some(entry) = entry.cloned() else {
            return Err(
                "no such campaign in the plan (wrong id, finished, or game not wanted)".into(),
            );
        };
        if !farmable(&entry) {
            return Err(format!(
                "campaign cannot be farmed now ({:?})",
                entry.status
            ));
        }
        // An explicit choice overrides an automatic set-aside.
        if self.banned_campaigns.remove(&entry.campaign_id).is_some() {
            self.stalls.remove(&entry.campaign_id);
            self.save_bans(ctx).await;
        }
        self.save_focus(
            ctx,
            Some(Focus {
                campaign_id: entry.campaign_id.clone(),
            }),
        )
        .await;
        Ok(
            json!({"campaign_id": entry.campaign_id, "campaign": entry.campaign, "game": entry.game}),
        )
    }

    /// Recomputes and publishes the plan without choosing again (progress moved).
    async fn republish_plan(&mut self, ctx: &ModuleContext, s: &DropsSettings) {
        let engine = ctx.store().read(EngineSettings::from_state);
        self.replan(s, &engine, Utc::now());
        self.publish_plan(ctx, s).await;
    }

    /// Chooses the campaign and channel to watch and publishes the watch request.
    async fn pick(&mut self, ctx: &ModuleContext, s: &DropsSettings) {
        let now = Utc::now();
        self.banned_channels.retain(|_, t| *t > Instant::now());
        self.banned_campaigns.retain(|_, t| *t > now);
        let engine = ctx.store().read(EngineSettings::from_state);
        self.replan(s, &engine, now);
        // A focus ends with its objective (or when the campaign cannot be farmed any more).
        if let Some(f) = &self.focus
            && !self.plan.order.contains(&f.campaign_id)
        {
            tracing::info!(campaign = %f.campaign_id, "drops focus finished");
            self.save_focus(ctx, None).await;
            self.replan(s, &engine, now);
        }
        let candidates = self.candidates(now);

        // Try campaigns best first; stop at the current target if its channel is fine.
        let current = self
            .target
            .as_ref()
            .filter(|t| !self.banned_channels.contains_key(&t.channel.id))
            .map(|t| t.campaign_id.clone());
        // Restricted campaigns are checked from one batched lookup, so they never use
        // up the directory lookups the open campaigns below them need.
        let acl_live = self.acl_live(ctx, &candidates).await;
        let mut lookups = 0;
        let mut chosen: Option<(Campaign, UserInfo)> = None;
        for c in &candidates {
            if current.as_ref() == Some(&c.id) {
                self.publish_plan(ctx, s).await;
                self.publish(ctx);
                return;
            }
            if c.allowed_channels.is_none() {
                if lookups == DIRECTORY_LOOKUPS {
                    continue;
                }
                lookups += 1;
            }
            match self.find_channel(ctx, c, acl_live.as_ref()).await {
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
        self.publish_plan(ctx, s).await;
        self.publish(ctx);
    }

    /// Publishes the plan (`module_data.drops.plan`) and raises the alerts it calls for.
    async fn publish_plan(&mut self, ctx: &ModuleContext, s: &DropsSettings) {
        let current = self.target.as_ref().map(|t| t.campaign_id.as_str());
        for e in &mut self.plan.entries {
            if e.status == Status::Active && Some(e.campaign_id.as_str()) != current {
                e.status = Status::Planned;
            }
            if Some(e.campaign_id.as_str()) == current {
                e.status = Status::Active;
            }
        }
        ctx.set_data("plan", json!(self.plan.entries));
        let alerts: Vec<DropAlertInfo> = self
            .plan
            .entries
            .iter()
            .filter(|e| e.game_rank.is_some())
            .filter_map(|e| {
                let kind = match e.status {
                    Status::AtRisk if s.alerts.at_risk => DropAlertKind::AtRisk,
                    Status::WillMiss if s.alerts.will_miss => DropAlertKind::WillMiss,
                    Status::NotLinked if s.alerts.not_linked => DropAlertKind::NotLinked,
                    _ => return None,
                };
                Some(alert_info(kind, e))
            })
            .collect();
        for a in alerts {
            self.alert(ctx, a).await;
        }
    }

    /// Emits an alert once per campaign and kind.
    async fn alert(&mut self, ctx: &ModuleContext, a: DropAlertInfo) {
        let key = format!("{:?}:{}", a.kind, a.campaign_id);
        if self.alerted.contains_key(&key) {
            return;
        }
        tracing::info!(kind = ?a.kind, campaign = %a.campaign, "drop alert");
        self.alerted.insert(key, a.ends_at);
        ctx.emit(Event::DropAlert {
            account: ctx.account_id().to_string(),
            alert: a,
        });
        let now = Utc::now();
        self.alerted.retain(|_, end| *end > now);
        if let Err(e) = ctx.kv_put(ALERTED_KEY, &self.alerted).await {
            tracing::warn!(error = %e, "could not save drop alerts");
        }
    }

    /// Alerts on campaigns of wanted games that were never seen before.
    async fn detect_new(&mut self, ctx: &ModuleContext, s: &DropsSettings) {
        let now = Utc::now();
        let listed: HashMap<String, DateTime<Utc>> = self
            .campaigns
            .values()
            .map(|c| (c.id.clone(), c.ends_at))
            .collect();
        let Some(known) = &self.known else {
            // First run: remember what exists without alerting.
            let _ = ctx.kv_put(KNOWN_KEY, &listed).await;
            self.known = Some(listed);
            return;
        };
        let wanted: Vec<String> = self.resolved(&s.games).iter().map(|g| norm(g)).collect();
        let fresh: Vec<Campaign> = self
            .campaigns
            .values()
            .filter(|c| !known.contains_key(&c.id) && now < c.ends_at)
            .filter(|c| wanted.contains(&norm(&c.game.name)))
            .cloned()
            .collect();
        // Discovery can miss a campaign for a while: forget ids only once they ended.
        let mut next = known.clone();
        next.extend(listed);
        next.retain(|_, end| *end > now);
        if &next != known {
            let _ = ctx.kv_put(KNOWN_KEY, &next).await;
        }
        self.known = Some(next);
        if !s.alerts.new_campaign {
            return;
        }
        for c in fresh {
            let (rewards, _, _) = plan::objective(&c, &s.wanted_rewards, now);
            self.alert(
                ctx,
                DropAlertInfo {
                    kind: DropAlertKind::NewCampaign,
                    campaign_id: c.id.clone(),
                    campaign: c.name.clone(),
                    game: c.game.name.clone(),
                    ends_at: c.ends_at,
                    starts_at: c.starts_at,
                    missing_minutes: None,
                    link_url: c.link_url.clone().filter(|_| c.linked == Some(false)),
                    rewards,
                },
            )
            .await;
        }
    }

    /// Channels of every restricted candidate, by id, from one batched lookup.
    /// `None` if it failed: `find_channel` then looks them up campaign by campaign.
    async fn acl_live(
        &self,
        ctx: &ModuleContext,
        candidates: &[Campaign],
    ) -> Option<HashMap<String, UserInfo>> {
        let mut seen = HashSet::new();
        let refs: Vec<(String, Option<String>)> = candidates
            .iter()
            .filter_map(|c| c.allowed_channels.as_ref())
            .flatten()
            .filter(|a| seen.insert(a.id.clone()))
            .map(|a| (a.id.clone(), a.login.clone()))
            .collect();
        if refs.is_empty() {
            return Some(HashMap::new());
        }
        match ctx.gql().users_by_ref(&refs).await {
            Ok(users) => Some(
                users
                    .into_iter()
                    .flatten()
                    .map(|u| (u.id.clone(), u))
                    .collect(),
            ),
            Err(e) => {
                tracing::debug!(error = %e, "restricted channels lookup failed");
                None
            }
        }
    }

    async fn find_channel(
        &self,
        ctx: &ModuleContext,
        c: &Campaign,
        acl_live: Option<&HashMap<String, UserInfo>>,
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
            let mut users: Vec<UserInfo> = match acl_live {
                Some(live) => acl
                    .iter()
                    .filter_map(|a| live.get(&a.id).cloned())
                    .collect(),
                None => {
                    let refs: Vec<(String, Option<String>)> = acl
                        .iter()
                        .map(|a| (a.id.clone(), a.login.clone()))
                        .collect();
                    gql.users_by_ref(&refs)
                        .await?
                        .into_iter()
                        .flatten()
                        .collect()
                }
            };
            users.retain(|u| plays_game(u) && ok(u));
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
        // Make sure the channel really participates in this campaign: Twitch lists it on
        // few of the game's channels, so check them all.
        let ids: Vec<String> = channels.iter().map(|u| u.id.clone()).collect();
        if let Ok(lists) = gql.available_drops(&ids).await {
            for (u, list) in channels.iter().zip(lists) {
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

    /// Polls the current drop session (works without PubSub, claims included).
    /// Returns `(refresh, pick)` like `on_pubsub`.
    async fn poll_progress(&mut self, ctx: &ModuleContext, s: &DropsSettings) -> (bool, bool) {
        let Some(t) = &mut self.target else {
            return (false, false);
        };
        let watching = ctx.store().read(|s| {
            s.accounts
                .get(ctx.account_id())
                .is_some_and(|a| a.watching.contains_key(&t.channel.id))
        });
        if !watching {
            t.last_progress = Instant::now();
            return (false, false);
        }
        // Still live and still on the campaign's game?
        if let Some(c) = self.campaigns.get(&t.campaign_id) {
            let special = ctx
                .gql()
                .manifest()
                .load()
                .special_game_ids
                .contains(&c.game.id);
            if let Ok(mut users) = ctx
                .gql()
                .users_by_ref(&[(t.channel.id.clone(), Some(t.channel.login.clone()))])
                .await
            {
                let stream = users.pop().flatten().and_then(|u| u.stream);
                let on_game = stream
                    .as_ref()
                    .and_then(|s| s.game.as_ref())
                    .is_some_and(|g| {
                        special || g.id == c.game.id || norm(&g.name) == norm(&c.game.name)
                    });
                if !on_game {
                    tracing::info!(channel = %t.channel.login, online = stream.is_some(), "drops channel left the game, switching");
                    self.banned_channels.insert(
                        t.channel.id.clone(),
                        Instant::now() + Duration::from_secs(10 * 60),
                    );
                    self.target = None;
                    return (false, true);
                }
            }
        }
        let Some(t) = &mut self.target else {
            return (false, false);
        };
        let mut completed = false;
        match ctx.gql().current_drop(&t.channel.id).await {
            Ok(Some(cur)) => {
                let progressed = t
                    .last_minutes
                    .as_ref()
                    .is_none_or(|(id, m)| *id != cur.drop_id || cur.current_minutes > *m);
                // A finished drop waiting for its claim is not a stall.
                completed = cur.required_minutes > 0 && cur.current_minutes >= cur.required_minutes;
                if progressed || completed {
                    t.last_minutes = Some((cur.drop_id.clone(), cur.current_minutes));
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
                    if let Some(c) = self.campaigns.get_mut(&campaign_id) {
                        let moved = advance(c, &cur.drop_id, cur.current_minutes, Utc::now());
                        self.plan_stale |= !moved.is_empty();
                        emit_estimates(ctx, c, &moved, &cur.drop_id);
                    }
                }
            }
            Ok(None) => {}
            Err(e) => tracing::debug!(error = %e, "current drop poll failed"),
        }
        // PubSub may have missed `drop-claim`: claim here, then load the next drop.
        if completed && s.auto_claim && !self.claim_all(ctx).await.is_empty() {
            return (true, true);
        }
        let Some(t) = &self.target else {
            return (false, false);
        };
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
            }
            self.target = None;
            self.save_bans(ctx).await;
            return (false, true);
        }
        (false, false)
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
                let moved = advance(c, drop_id, cur, Utc::now());
                self.plan_stale |= !moved.is_empty();
                emit_estimates(ctx, c, &moved, drop_id);
                if let Some(t) = &mut self.target {
                    t.last_progress = Instant::now();
                    t.last_minutes = Some((drop_id.to_string(), cur));
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
        // Completed drops without an instance id: use the id format Twitch accepts.
        let user = ctx.account_id().to_string();
        for c in self.campaigns.values_mut() {
            for d in c
                .drops
                .iter_mut()
                .filter(|d| !d.claimed && d.claim_id.is_none() && d.is_complete())
            {
                d.claim_id = Some(format!("{user}#{}#{}", c.id, d.id));
            }
        }
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
                Ok(ClaimResult::AlreadyClaimed) => {
                    if let Some(d) = self
                        .campaigns
                        .get_mut(&campaign_id)
                        .and_then(|c| c.drops.iter_mut().find(|d| d.id == drop_id))
                    {
                        d.claimed = true;
                    }
                }
                Ok(ClaimResult::Claimed) => {
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
        let _ = ctx.kv_put(STALLS_KEY, &self.stalls).await;
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
                "minutes": t.last_minutes.as_ref().map(|(_, m)| m),
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

/// Sets the minutes Twitch reported for `drop_id` and moves the campaign's other running
/// drops by as much: Twitch counts every unlocked drop of a campaign at once but only
/// reports one. The next refresh replaces these estimates with the real values.
/// Returns the ids of the drops whose minutes changed.
fn advance(c: &mut Campaign, drop_id: &str, minutes: u32, now: DateTime<Utc>) -> Vec<String> {
    let Some(d) = c.drops.iter_mut().find(|d| d.id == drop_id) else {
        return Vec::new();
    };
    if d.current_minutes == minutes {
        return Vec::new();
    }
    // A lower value (new drop, reset) is taken as is, without moving the others.
    let delta = minutes.saturating_sub(d.current_minutes);
    d.current_minutes = minutes;
    let mut moved = vec![drop_id.to_string()];
    if delta == 0 {
        return moved;
    }
    let claimed: HashSet<String> = c
        .drops
        .iter()
        .filter(|d| d.claimed)
        .map(|d| d.id.clone())
        .collect();
    let ids: HashSet<String> = c.drops.iter().map(|d| d.id.clone()).collect();
    for o in c.drops.iter_mut().filter(|o| o.id != drop_id) {
        let running = !o.claimed
            && o.required_minutes > 0
            && o.current_minutes < o.required_minutes
            && o.starts_at <= now
            && now < o.ends_at
            && o.preconditions
                .iter()
                .all(|p| claimed.contains(p) || !ids.contains(p));
        if running {
            o.current_minutes = (o.current_minutes + delta).min(o.required_minutes);
            moved.push(o.id.clone());
        }
    }
    moved
}

/// Publishes the estimated minutes of the drops [`advance`] moved (the reported drop has
/// its own event).
fn emit_estimates(ctx: &ModuleContext, c: &Campaign, moved: &[String], reported: &str) {
    for d in c
        .drops
        .iter()
        .filter(|d| d.id != reported && moved.contains(&d.id))
    {
        ctx.emit(Event::DropProgress {
            account: ctx.account_id().to_string(),
            campaign_id: c.id.clone(),
            drop_id: d.id.clone(),
            current_minutes: d.current_minutes,
            required_minutes: d.required_minutes,
        });
    }
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
        let mut rt = Runtime::new();
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
        let now = Utc::now();
        rt.replan(&s, &EngineSettings::default(), now);
        let ids: Vec<String> = rt.candidates(now).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["r", "w"]);
        let unlinked = rt.plan.entries.iter().find(|e| e.campaign_id == "u");
        assert_eq!(unlinked.map(|e| e.status), Some(Status::NotLinked));
    }

    #[test]
    fn benefits_mark_claimed() {
        let mut c = campaign("c", "g", vec![drop("a", 60, 0, &[])]);
        mark_claimed_from_benefits(
            &mut c,
            &[ClaimedBenefit {
                id: "ba".into(),
                last_awarded_at: Some(Utc::now()),
                ..Default::default()
            }],
        );
        assert!(c.drops[0].claimed);
    }

    #[test]
    fn reported_progress_moves_the_running_drops() {
        let mut c = campaign(
            "c",
            "g",
            vec![
                drop("a", 60, 10, &[]),
                drop("b", 120, 10, &[]),
                drop("locked", 30, 0, &["a"]),
                drop("full", 30, 30, &[]),
            ],
        );
        let moved = advance(&mut c, "a", 15, Utc::now());
        assert_eq!(moved, ["a", "b"]);
        let minutes: Vec<u32> = c.drops.iter().map(|d| d.current_minutes).collect();
        assert_eq!(minutes, [15, 15, 0, 30]);
        // Same value again: nothing moves.
        assert!(advance(&mut c, "a", 15, Utc::now()).is_empty());
        // A lower value is taken as is, without touching the others.
        assert_eq!(advance(&mut c, "a", 2, Utc::now()), ["a"]);
        assert_eq!(c.drops[1].current_minutes, 15);
    }
}
