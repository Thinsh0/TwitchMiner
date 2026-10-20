//! Channel points: watches channels, claims bonus chests, follows raids, claims
//! moments and keeps watch streaks alive.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tm_core::event::Event;
use tm_core::module::validate_as;
use tm_core::state::ChannelMeta;
use tm_core::{Module, ModuleContext, ModuleInfo, WatchRequest, priority};
use tm_twitch::models::{StreamInfo, UserInfo};
use tm_twitch::pubsub::topics;
use tm_twitch::{PubSubEvent, PubSubMessage};
use tokio::time::Instant;

pub struct Points;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(default)]
pub struct PointsSettings {
    pub enabled: bool,
    /// Channel logins, highest priority first.
    pub channels: Vec<String>,
    /// Also farm followed channels (after `channels`).
    pub follows: bool,
    /// Logins never farmed (useful with `follows`).
    pub exclude: Vec<String>,
    pub claim_bonus: bool,
    pub join_raids: bool,
    pub claim_moments: bool,
    /// Prioritise channels whose watch streak is not secured yet for the current stream.
    pub watch_streak: bool,
    /// Online-status polling interval (PubSub covers most changes in real time).
    pub refresh_minutes: u64,
    /// How to rank channels competing for the watch slots (after streaks).
    pub order_by: OrderBy,
}

impl Default for PointsSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            channels: Vec::new(),
            follows: true,
            exclude: Vec::new(),
            claim_bonus: true,
            join_raids: true,
            claim_moments: true,
            watch_streak: true,
            refresh_minutes: 5,
            order_by: OrderBy::Config,
        }
    }
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum OrderBy {
    /// `channels` order, then followed channels.
    #[default]
    Config,
    /// Highest points multiplier first (subscriptions earn more), then config order.
    Multiplier,
    /// Channels where you have the fewest points first.
    FewestPoints,
    /// Channels where you have the most points first.
    MostPoints,
}

/// Minutes to watch for a streak to count (Twitch requires a few minutes).
const STREAK_MINUTES: u64 = 7;
const ONLINE_RECHECK: Duration = Duration::from_secs(60);
const CONTEXT_REFRESH: Duration = Duration::from_secs(30 * 60);
/// Watched channels are re-checked often so bonus chests are claimed even if
/// PubSub misses the `claim-available` message.
const WATCHED_CONTEXT_REFRESH: Duration = Duration::from_secs(4 * 60);
const FOLLOWS_REFRESH: Duration = Duration::from_secs(60 * 60);

/// Per-broadcast progress persisted across restarts (streak tracking).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Memory {
    stream_id: String,
    minutes: u64,
    streak_done: bool,
}

const MEMORY_KEY: &str = "broadcasts";

#[derive(Debug, Clone)]
struct ChannelRt {
    meta: ChannelMeta,
    rank: usize,
    stream: Option<StreamInfo>,
    /// Stream id for which the streak was secured.
    streak_done: Option<String>,
    /// Minutes credited on the current broadcast.
    broadcast_minutes: u64,
    last_context: Option<Instant>,
}

#[async_trait::async_trait]
impl Module for Points {
    fn info(&self) -> ModuleInfo {
        ModuleInfo::new(
            "points",
            "Channel points",
            "Watches your channels to earn channel points, claims bonus chests, joins raids, claims moments and keeps watch streaks.",
            json!(PointsSettings::default()),
        )
        .with_schema::<PointsSettings>()
    }

    fn validate_settings(&self, v: &Value) -> Result<(), String> {
        let s: PointsSettings = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
        if s.refresh_minutes == 0 {
            return Err("refresh_minutes must be at least 1".into());
        }
        validate_as::<PointsSettings>(v)
    }

    async fn run(&self, ctx: ModuleContext) -> anyhow::Result<()> {
        let uid = ctx.account_id().to_string();
        let mut rt = Runtime {
            channels: BTreeMap::new(),
            recheck: HashMap::new(),
            raids: HashSet::new(),
        };
        let mut memory: HashMap<String, Memory> = ctx
            .kv_get(MEMORY_KEY)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        let mut topics = ctx.pubsub().topic_set();
        let mut ps = ctx.pubsub().subscribe();
        let mut events = ctx.events();
        let mut settings: PointsSettings = ctx.settings()?;
        let mut follows: Vec<String> = Vec::new();
        let mut follows_at: Option<Instant> = None;
        let mut resolve = true;
        let mut poll =
            tokio::time::interval(Duration::from_secs(60 * settings.refresh_minutes.max(1)));
        poll.tick().await;

        loop {
            if resolve {
                resolve = false;
                if settings.follows && follows_at.is_none_or(|t| t.elapsed() > FOLLOWS_REFRESH) {
                    match ctx.gql().follows().await {
                        Ok(f) => {
                            follows = f.into_iter().map(|c| c.id).collect();
                            follows_at = Some(Instant::now());
                        }
                        Err(e) => tracing::warn!(error = %e, "could not load follows"),
                    }
                }
                let users = resolve_channels(&ctx, &settings, &follows).await?;
                rt.sync(&ctx, users);
                rt.restore(std::mem::take(&mut memory));
                let mut wanted = vec![topics::community_points_user(&uid)];
                for id in rt.channels.keys() {
                    wanted.push(topics::playback(id));
                    if settings.join_raids {
                        wanted.push(topics::raid(id));
                    }
                    if settings.claim_moments {
                        wanted.push(topics::moments(id));
                    }
                }
                topics.set(wanted);
                rt.refresh_contexts(&ctx, &settings).await;
                ctx.set_data("channels", json!(rt.channels.len()));
            }
            let balances = ctx.store().read(|st| {
                st.accounts
                    .get(ctx.account_id())
                    .map(|a| {
                        a.channels
                            .values()
                            .map(|c| (c.id.clone(), (c.points.unwrap_or(0), c.multiplier)))
                            .collect()
                    })
                    .unwrap_or_default()
            });
            ctx.set_watch_requests(rt.requests(&settings, &balances));

            let next_recheck = rt.recheck.values().min().copied();
            tokio::select! {
                _ = ctx.cancelled() => break,
                _ = ctx.settings_changed() => {
                    let next: PointsSettings = ctx.settings()?;
                    if next != settings {
                        if next.refresh_minutes != settings.refresh_minutes {
                            poll = tokio::time::interval(Duration::from_secs(60 * next.refresh_minutes.max(1)));
                            poll.tick().await;
                        }
                        settings = next;
                        resolve = true;
                    }
                }
                _ = poll.tick() => {
                    rt.save(&ctx).await;
                    rt.poll_status(&ctx).await;
                    rt.refresh_contexts(&ctx, &settings).await;
                    if follows_at.is_some_and(|t| t.elapsed() > FOLLOWS_REFRESH) {
                        resolve = true;
                    }
                }
                _ = sleep_until_opt(next_recheck) => {
                    let now = Instant::now();
                    let due: Vec<String> = rt.recheck.iter().filter(|(_, t)| **t <= now).map(|(k, _)| k.clone()).collect();
                    for id in &due { rt.recheck.remove(id); }
                    rt.check(&ctx, &due).await;
                    rt.refresh_contexts(&ctx, &settings).await;
                }
                ev = ps.recv() => match ev {
                    Ok(PubSubEvent::Message(m)) => rt.on_pubsub(&ctx, &settings, &m).await,
                    Ok(PubSubEvent::Connected { .. }) => {}
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => rt.poll_status(&ctx).await,
                    Err(_) => break,
                },
                ev = events.recv() => {
                    if let Ok(env) = ev
                        && let Event::WatchMinute { account, channel_id } = &env.event
                            && account == &uid
                                && let Some(ch) = rt.channels.get_mut(channel_id) {
                                    ch.broadcast_minutes += 1;
                                }
                }
                call = ctx.next_call() => {
                    let Some(call) = call else { break };
                    let reply = handle_call(&ctx, &call.name, &call.args, &rt).await;
                    if reply.is_ok() && matches!(call.name.as_str(), "add_channel" | "remove_channel" | "refresh") {
                        settings = ctx.settings()?;
                        resolve = true;
                    }
                    let _ = call.reply.send(reply);
                }
            }
        }
        rt.save(&ctx).await;
        for id in rt.channels.keys() {
            ctx.release_channel(id);
        }
        Ok(())
    }
}

async fn sleep_until_opt(t: Option<Instant>) {
    match t {
        Some(t) => tokio::time::sleep_until(t).await,
        None => std::future::pending().await,
    }
}

async fn handle_call(
    ctx: &ModuleContext,
    name: &str,
    args: &Value,
    rt: &Runtime,
) -> Result<Value, String> {
    let login = || {
        args.get("login")
            .and_then(Value::as_str)
            .map(|l| l.trim().trim_start_matches('@').to_lowercase())
            .filter(|l| !l.is_empty())
            .ok_or_else(|| "missing `login`".to_string())
    };
    let scope = tm_core::settings::account_module_scope(ctx.account_id(), "points");
    let current = || -> PointsSettings { ctx.settings().unwrap_or_default() };
    match name {
        "add_channel" => {
            let login = login()?;
            let users = ctx
                .gql()
                .users_by_login(std::slice::from_ref(&login))
                .await
                .map_err(|e| e.to_string())?;
            if users.into_iter().next().flatten().is_none() {
                return Err(format!("channel `{login}` not found"));
            }
            let mut channels = current().channels;
            if !channels.contains(&login) {
                channels.push(login);
            }
            set_account_settings(ctx, &scope, json!({ "channels": channels })).await
        }
        "remove_channel" => {
            let login = login()?;
            let mut s = current();
            s.channels.retain(|c| c != &login);
            set_account_settings(ctx, &scope, json!({ "channels": s.channels })).await
        }
        "refresh" => Ok(json!({ "ok": true })),
        "status" => Ok(json!({
            "channels": rt.channels.values().map(|c| json!({
                "id": c.meta.id, "login": c.meta.login, "online": c.stream.is_some(),
                "broadcast_minutes": c.broadcast_minutes,
                "streak_done": c.streak_done.is_some() && c.streak_done == c.stream.as_ref().map(|s| s.id.clone()),
            })).collect::<Vec<_>>()
        })),
        other => Err(format!("unknown call `{other}`")),
    }
}

/// Writes an account-level override directly (modules may own their settings).
async fn set_account_settings(
    ctx: &ModuleContext,
    scope: &str,
    patch: Value,
) -> Result<Value, String> {
    let mut doc = ctx
        .store()
        .read(|s| s.settings.get(scope).cloned())
        .unwrap_or_else(|| json!({}));
    tm_twitch::manifest::merge_json(&mut doc, &patch);
    ctx.db()
        .put_settings(scope.to_string(), doc.clone())
        .await
        .map_err(|e| e.to_string())?;
    ctx.emit(Event::SettingsChanged {
        scope: scope.to_string(),
        value: doc.clone(),
    });
    Ok(doc)
}

async fn resolve_channels(
    ctx: &ModuleContext,
    s: &PointsSettings,
    follows: &[String],
) -> anyhow::Result<Vec<UserInfo>> {
    let exclude: HashSet<String> = s.exclude.iter().map(|l| l.to_lowercase()).collect();
    let logins: Vec<String> = s
        .channels
        .iter()
        .map(|l| l.trim().trim_start_matches('@').to_lowercase())
        .filter(|l| !l.is_empty() && !exclude.contains(l))
        .collect();
    let mut out: Vec<UserInfo> = Vec::new();
    for u in ctx
        .gql()
        .users_by_login(&logins)
        .await?
        .into_iter()
        .flatten()
    {
        if !out.iter().any(|o| o.id == u.id) {
            out.push(u);
        }
    }
    if s.follows && !follows.is_empty() {
        let missing: Vec<String> = follows
            .iter()
            .filter(|id| !out.iter().any(|o| &o.id == *id))
            .cloned()
            .collect();
        for u in ctx.gql().users_by_id(&missing).await?.into_iter().flatten() {
            if !exclude.contains(&u.login) {
                out.push(u);
            }
        }
    }
    Ok(out)
}

struct Runtime {
    channels: BTreeMap<String, ChannelRt>,
    /// Channels to re-check after a PubSub hint (stream-up arrives before the API knows).
    recheck: HashMap<String, Instant>,
    raids: HashSet<String>,
}

impl Runtime {
    /// Re-applies persisted per-broadcast progress to channels still on the same broadcast.
    fn restore(&mut self, memory: HashMap<String, Memory>) {
        for (id, m) in memory {
            let Some(ch) = self.channels.get_mut(&id) else {
                continue;
            };
            if ch.stream.as_ref().is_some_and(|s| s.id == m.stream_id) {
                ch.broadcast_minutes = ch.broadcast_minutes.max(m.minutes);
                if m.streak_done {
                    ch.streak_done = Some(m.stream_id);
                }
            }
        }
    }

    async fn save(&self, ctx: &ModuleContext) {
        let memory: HashMap<String, Memory> = self
            .channels
            .values()
            .filter_map(|c| {
                let s = c.stream.as_ref()?;
                Some((
                    c.meta.id.clone(),
                    Memory {
                        stream_id: s.id.clone(),
                        minutes: c.broadcast_minutes,
                        streak_done: c.streak_done.as_deref() == Some(s.id.as_str()),
                    },
                ))
            })
            .collect();
        if let Err(e) = ctx.kv_put(MEMORY_KEY, &memory).await {
            tracing::debug!(error = %e, "could not save broadcast progress");
        }
    }

    fn sync(&mut self, ctx: &ModuleContext, users: Vec<UserInfo>) {
        let keep: HashSet<String> = users.iter().map(|u| u.id.clone()).collect();
        let removed: Vec<String> = self
            .channels
            .keys()
            .filter(|id| !keep.contains(*id))
            .cloned()
            .collect();
        for id in removed {
            self.channels.remove(&id);
            ctx.release_channel(&id);
        }
        for (rank, u) in users.into_iter().enumerate() {
            let meta = ChannelMeta {
                id: u.id.clone(),
                login: u.login.clone(),
                display_name: u.display_name.clone(),
                avatar_url: u.avatar_url.clone(),
            };
            ctx.track_channel(meta.clone());
            let entry = self
                .channels
                .entry(u.id.clone())
                .or_insert_with(|| ChannelRt {
                    meta: meta.clone(),
                    rank,
                    stream: None,
                    streak_done: None,
                    broadcast_minutes: 0,
                    last_context: None,
                });
            entry.rank = rank;
            entry.meta = meta;
            set_stream(ctx, entry, u.stream);
        }
    }

    async fn poll_status(&mut self, ctx: &ModuleContext) {
        let ids: Vec<String> = self.channels.keys().cloned().collect();
        self.check(ctx, &ids).await;
    }

    async fn check(&mut self, ctx: &ModuleContext, ids: &[String]) {
        if ids.is_empty() {
            return;
        }
        match ctx.gql().users_by_id(ids).await {
            Ok(users) => {
                for (id, u) in ids.iter().zip(users) {
                    if let Some(ch) = self.channels.get_mut(id) {
                        set_stream(ctx, ch, u.and_then(|u| u.stream));
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "status poll failed"),
        }
    }

    /// Loads balance / pending bonus for online channels not refreshed recently.
    async fn refresh_contexts(&mut self, ctx: &ModuleContext, s: &PointsSettings) {
        let watched: HashSet<String> = ctx.store().read(|st| {
            st.accounts
                .get(ctx.account_id())
                .map(|a| a.watching.keys().cloned().collect())
                .unwrap_or_default()
        });
        for ch in self.channels.values_mut() {
            let every = if watched.contains(&ch.meta.id) {
                WATCHED_CONTEXT_REFRESH
            } else {
                CONTEXT_REFRESH
            };
            if ch.stream.is_none() || ch.last_context.is_some_and(|t| t.elapsed() < every) {
                continue;
            }
            ch.last_context = Some(Instant::now());
            match ctx.gql().channel_points_context(&ch.meta.login).await {
                Ok(pc) => {
                    ctx.emit(Event::PointsBalance {
                        account: ctx.account_id().to_string(),
                        channel_id: ch.meta.id.clone(),
                        balance: pc.balance,
                        multiplier: pc.multiplier,
                    });
                    if let (Some(claim), true) = (pc.available_claim, s.claim_bonus) {
                        claim_bonus(ctx, &ch.meta, &claim).await;
                    }
                }
                Err(e) => {
                    tracing::debug!(channel = %ch.meta.login, error = %e, "points context failed")
                }
            }
        }
    }

    /// `balances`: channel id → (points, multiplier), from the state.
    fn requests(
        &self,
        s: &PointsSettings,
        balances: &HashMap<String, (u64, f64)>,
    ) -> Vec<WatchRequest> {
        let mut online: Vec<&ChannelRt> = self
            .channels
            .values()
            .filter(|c| c.stream.is_some())
            .collect();
        let bal = |c: &ChannelRt| balances.get(&c.meta.id).copied().unwrap_or((0, 1.0));
        match s.order_by {
            OrderBy::Config => online.sort_by_key(|c| c.rank),
            OrderBy::Multiplier => online.sort_by(|a, b| {
                bal(b)
                    .1
                    .partial_cmp(&bal(a).1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.rank.cmp(&b.rank))
            }),
            OrderBy::FewestPoints => online.sort_by_key(|c| (bal(c).0, c.rank)),
            OrderBy::MostPoints => online.sort_by_key(|c| (std::cmp::Reverse(bal(c).0), c.rank)),
        }
        let n = online.len();
        online
            .into_iter()
            .enumerate()
            .map(|(pos, c)| {
                let streak_pending = s.watch_streak
                    && c.broadcast_minutes < STREAK_MINUTES
                    && c.streak_done.as_deref() != c.stream.as_ref().map(|st| st.id.as_str());
                let boost = (n - pos).min(99) as i32;
                let (prio, reason) = if streak_pending {
                    (priority::STREAK + boost, "points: watch streak")
                } else {
                    (priority::POINTS + boost, "points")
                };
                WatchRequest {
                    channel_id: c.meta.id.clone(),
                    login: c.meta.login.clone(),
                    priority: prio,
                    reason: reason.into(),
                }
            })
            .collect()
    }

    async fn on_pubsub(&mut self, ctx: &ModuleContext, s: &PointsSettings, m: &PubSubMessage) {
        let account = ctx.account_id().to_string();
        match m.topic_kind.as_str() {
            "community-points-user-v1" => {
                let d = &m.data;
                match m.kind.as_str() {
                    "points-earned" => {
                        let channel_id = str_at(d, "/point_gain/channel_id")
                            .or_else(|| str_at(d, "/channel_id"))
                            .unwrap_or_default();
                        let amount = d
                            .pointer("/point_gain/total_points")
                            .and_then(Value::as_i64)
                            .unwrap_or(0);
                        let reason = str_at(d, "/point_gain/reason_code").unwrap_or_default();
                        let balance = d
                            .pointer("/balance/balance")
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        if reason == "WATCH_STREAK"
                            && let Some(ch) = self.channels.get_mut(&channel_id)
                        {
                            ch.streak_done = ch.stream.as_ref().map(|st| st.id.clone());
                        }
                        let name = ctx
                            .store()
                            .read(|st| {
                                st.accounts
                                    .get(ctx.account_id())
                                    .and_then(|a| a.channels.get(&channel_id))
                                    .map(|c| c.login.clone())
                            })
                            .unwrap_or_else(|| channel_id.clone());
                        tracing::info!(channel = %name, amount, %reason, balance, "points earned");
                        ctx.emit(Event::PointsEarned {
                            account,
                            channel_id,
                            amount,
                            reason,
                            balance,
                        });
                    }
                    "points-spent" => {
                        let channel_id = str_at(d, "/balance/channel_id").unwrap_or_default();
                        let balance = d
                            .pointer("/balance/balance")
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        let amount = d
                            .pointer("/point_gain/total_points")
                            .and_then(Value::as_i64)
                            .unwrap_or(0);
                        ctx.emit(Event::PointsSpent {
                            account,
                            channel_id,
                            amount,
                            balance,
                        });
                    }
                    "claim-available" if s.claim_bonus => {
                        let channel_id = str_at(d, "/claim/channel_id").unwrap_or_default();
                        let claim_id = str_at(d, "/claim/id").unwrap_or_default();
                        let meta = self
                            .channels
                            .get(&channel_id)
                            .map(|c| c.meta.clone())
                            .unwrap_or(ChannelMeta {
                                id: channel_id.clone(),
                                login: channel_id.clone(),
                                display_name: String::new(),
                                avatar_url: None,
                            });
                        claim_bonus(ctx, &meta, &claim_id).await;
                    }
                    _ => {}
                }
            }
            "video-playback-by-id" => {
                let id = &m.target_id;
                let Some(ch) = self.channels.get_mut(id) else {
                    return;
                };
                match m.kind.as_str() {
                    "stream-up" => {
                        self.recheck
                            .insert(id.clone(), Instant::now() + ONLINE_RECHECK);
                    }
                    "stream-down" => {
                        set_stream(ctx, ch, None);
                    }
                    "viewcount" => {
                        if ch.stream.is_none() {
                            self.recheck
                                .entry(id.clone())
                                .or_insert_with(|| Instant::now() + Duration::from_secs(5));
                        } else if let Some(v) = m.data.get("viewers").and_then(Value::as_u64)
                            && viewers_moved(ch.stream.as_ref().map_or(0, |s| s.viewers), v)
                        {
                            if let Some(st) = ch.stream.as_mut() {
                                st.viewers = v;
                            }
                            ctx.emit(Event::ViewersChanged {
                                account,
                                channel_id: id.clone(),
                                viewers: v,
                            });
                        }
                    }
                    _ => {}
                }
            }
            "raid" if s.join_raids => {
                if m.kind != "raid_update_v2" {
                    return;
                }
                let (Some(raid_id), Some(target)) = (
                    str_at(&m.data, "/raid/id"),
                    str_at(&m.data, "/raid/target_login"),
                ) else {
                    return;
                };
                if !self.raids.insert(raid_id.clone()) {
                    return;
                }
                match ctx.gql().join_raid(&raid_id).await {
                    Ok(()) => {
                        tracing::info!(from = %m.target_id, to = %target, "joined raid");
                        ctx.emit(Event::RaidJoined {
                            account,
                            channel_id: m.target_id.clone(),
                            target_login: target,
                        });
                    }
                    Err(e) => tracing::warn!(error = %e, "join raid failed"),
                }
            }
            "community-moments-channel-v1" if s.claim_moments => {
                if m.kind != "active" {
                    return;
                }
                let Some(moment) = str_at(&m.data, "/moment_id") else {
                    return;
                };
                match ctx.gql().claim_moment(&moment).await {
                    Ok(()) => {
                        ctx.emit(Event::MomentClaimed {
                            account,
                            channel_id: m.target_id.clone(),
                        });
                    }
                    Err(e) => tracing::warn!(error = %e, "claim moment failed"),
                }
            }
            _ => {}
        }
    }
}

/// Viewer counts change every few seconds; only publish meaningful moves (≥ 2 %).
fn viewers_moved(old: u64, new: u64) -> bool {
    old.abs_diff(new) * 50 >= old.max(1)
}

fn set_stream(ctx: &ModuleContext, ch: &mut ChannelRt, stream: Option<StreamInfo>) {
    let new_broadcast = match (&ch.stream, &stream) {
        (Some(a), Some(b)) => a.id != b.id,
        (None, Some(_)) => true,
        _ => false,
    };
    if new_broadcast {
        ch.broadcast_minutes = 0;
        ch.last_context = None;
    }
    if stream.is_none() {
        ch.broadcast_minutes = 0;
    }
    ch.stream = stream.clone();
    ctx.emit(Event::StreamChanged {
        account: ctx.account_id().to_string(),
        channel_id: ch.meta.id.clone(),
        stream,
    });
}

async fn claim_bonus(ctx: &ModuleContext, ch: &ChannelMeta, claim_id: &str) {
    match ctx.gql().claim_points(&ch.id, claim_id).await {
        Ok(()) => tracing::info!(channel = %ch.login, "bonus claimed"),
        Err(e) => tracing::warn!(channel = %ch.login, error = %e, "bonus claim failed"),
    }
}

fn str_at(v: &Value, ptr: &str) -> Option<String> {
    v.pointer(ptr).and_then(|x| {
        x.as_str()
            .map(str::to_string)
            .or_else(|| x.as_u64().map(|n| n.to_string()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(id: &str, rank: usize, live: bool) -> ChannelRt {
        ChannelRt {
            meta: ChannelMeta {
                id: id.into(),
                login: id.into(),
                display_name: id.into(),
                avatar_url: None,
            },
            rank,
            stream: live.then(|| StreamInfo {
                id: format!("s{id}"),
                title: String::new(),
                game: None,
                viewers: 0,
                started_at: None,
            }),
            streak_done: None,
            broadcast_minutes: 0,
            last_context: None,
        }
    }

    #[test]
    fn streak_outranks_order_and_offline_is_skipped() {
        let mut rt = Runtime {
            channels: BTreeMap::new(),
            recheck: HashMap::new(),
            raids: HashSet::new(),
        };
        rt.channels.insert("a".into(), ch("a", 0, true));
        rt.channels.insert("b".into(), ch("b", 1, true));
        rt.channels.insert("c".into(), ch("c", 2, false));
        rt.channels.get_mut("a").unwrap().broadcast_minutes = 30;
        let s = PointsSettings::default();
        let r = rt.requests(&s, &HashMap::new());
        assert_eq!(r.len(), 2);
        let a = r.iter().find(|x| x.channel_id == "a").unwrap();
        let b = r.iter().find(|x| x.channel_id == "b").unwrap();
        assert!(b.priority > a.priority, "b still needs its streak");
    }

    #[test]
    fn order_by_multiplier_and_points() {
        let mut rt = Runtime {
            channels: BTreeMap::new(),
            recheck: HashMap::new(),
            raids: HashSet::new(),
        };
        rt.channels.insert("a".into(), ch("a", 0, true));
        rt.channels.insert("b".into(), ch("b", 1, true));
        let balances: HashMap<String, (u64, f64)> =
            [("a".into(), (500, 1.0)), ("b".into(), (100, 2.0))]
                .into_iter()
                .collect();
        let top = |order_by| {
            let s = PointsSettings {
                watch_streak: false,
                order_by,
                ..Default::default()
            };
            let r = rt.requests(&s, &balances);
            r.iter()
                .max_by_key(|x| x.priority)
                .unwrap()
                .channel_id
                .clone()
        };
        assert_eq!(top(OrderBy::Config), "a");
        assert_eq!(top(OrderBy::Multiplier), "b");
        assert_eq!(top(OrderBy::FewestPoints), "b");
        assert_eq!(top(OrderBy::MostPoints), "a");
    }
}
