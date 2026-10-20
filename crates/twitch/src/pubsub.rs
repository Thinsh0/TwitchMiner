//! Twitch PubSub: a self-healing pool of WebSocket connections (50 topics each)
//! with reference-counted topic subscriptions.
//!
//! Consumers call [`PubSub::subscribe`] to get every message and use [`TopicSet`]
//! to declare which topics they need; the pool LISTENs/UNLISTENs as needed and
//! re-LISTENs everything after a reconnect.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::auth::Credentials;
use crate::net::{Backoff, Net};
use crate::ws;

const URL: &str = "wss://pubsub-edge.twitch.tv/v1";
const TOPICS_PER_CONN: usize = 50;
const PING_EVERY: Duration = Duration::from_secs(4 * 60);
const PONG_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct PubSubMessage {
    pub topic: String,
    /// Topic prefix, e.g. `community-points-user-v1`.
    pub topic_kind: String,
    /// Id after the last dot (user or channel id).
    pub target_id: String,
    /// The inner message's `type`.
    pub kind: String,
    /// Inner payload: `data` when present, the whole inner message otherwise.
    pub data: Value,
}

#[derive(Debug, Clone)]
pub enum PubSubEvent {
    Message(Arc<PubSubMessage>),
    Connected {
        connection: usize,
    },
    Disconnected {
        connection: usize,
        reason: String,
    },
    /// A LISTEN was rejected. `ERR_BADAUTH` means the token is no longer valid.
    ListenError {
        topics: Vec<String>,
        error: String,
    },
}

#[derive(Clone)]
pub struct PubSub {
    inner: Arc<Inner>,
}

struct Inner {
    net: Net,
    creds: Arc<ArcSwap<Credentials>>,
    tx: broadcast::Sender<PubSubEvent>,
    cancel: CancellationToken,
    state: Mutex<PoolState>,
}

#[derive(Default)]
struct PoolState {
    refcounts: HashMap<String, usize>,
    conns: Vec<ConnHandle>,
}

struct ConnHandle {
    topics: Arc<Mutex<HashSet<String>>>,
    cmd: mpsc::UnboundedSender<Cmd>,
}

enum Cmd {
    Listen(Vec<String>),
    Unlisten(Vec<String>),
}

impl PubSub {
    pub fn new(net: Net, creds: Arc<ArcSwap<Credentials>>, cancel: CancellationToken) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            inner: Arc::new(Inner {
                net,
                creds,
                tx,
                cancel,
                state: Mutex::default(),
            }),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PubSubEvent> {
        self.inner.tx.subscribe()
    }

    /// A topic set owned by one consumer; dropped sets release their topics.
    pub fn topic_set(&self) -> TopicSet {
        TopicSet {
            pubsub: self.clone(),
            topics: HashSet::new(),
        }
    }

    pub fn active_topics(&self) -> usize {
        self.inner.state.lock().refcounts.len()
    }

    fn listen(&self, topics: Vec<String>) {
        let mut st = self.inner.state.lock();
        let mut fresh = Vec::new();
        for t in topics {
            let c = st.refcounts.entry(t.clone()).or_insert(0);
            *c += 1;
            if *c == 1 {
                fresh.push(t);
            }
        }
        while !fresh.is_empty() {
            let idx = match st
                .conns
                .iter()
                .position(|c| c.topics.lock().len() < TOPICS_PER_CONN)
            {
                Some(i) => i,
                None => {
                    let h = self.spawn_conn(st.conns.len());
                    st.conns.push(h);
                    st.conns.len() - 1
                }
            };
            let conn = &st.conns[idx];
            let room = TOPICS_PER_CONN - conn.topics.lock().len();
            let batch: Vec<String> = fresh.drain(..room.min(fresh.len())).collect();
            conn.topics.lock().extend(batch.iter().cloned());
            let _ = conn.cmd.send(Cmd::Listen(batch));
        }
    }

    fn unlisten(&self, topics: Vec<String>) {
        let mut st = self.inner.state.lock();
        for t in topics {
            let Some(c) = st.refcounts.get_mut(&t) else {
                continue;
            };
            *c -= 1;
            if *c > 0 {
                continue;
            }
            st.refcounts.remove(&t);
            for conn in &st.conns {
                if conn.topics.lock().remove(&t) {
                    let _ = conn.cmd.send(Cmd::Unlisten(vec![t.clone()]));
                    break;
                }
            }
        }
    }

    fn spawn_conn(&self, index: usize) -> ConnHandle {
        let topics = Arc::new(Mutex::new(HashSet::new()));
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let inner = self.inner.clone();
        let t = topics.clone();
        tokio::spawn(async move { run_conn(inner, index, t, cmd_rx).await });
        ConnHandle {
            topics,
            cmd: cmd_tx,
        }
    }
}

/// Declarative topic subscription: call [`TopicSet::set`] with the full wanted set.
pub struct TopicSet {
    pubsub: PubSub,
    topics: HashSet<String>,
}

impl TopicSet {
    pub fn set(&mut self, wanted: impl IntoIterator<Item = String>) {
        let wanted: HashSet<String> = wanted.into_iter().collect();
        let add: Vec<String> = wanted.difference(&self.topics).cloned().collect();
        let remove: Vec<String> = self.topics.difference(&wanted).cloned().collect();
        if !remove.is_empty() {
            self.pubsub.unlisten(remove);
        }
        if !add.is_empty() {
            self.pubsub.listen(add);
        }
        self.topics = wanted;
    }

    pub fn add(&mut self, topic: String) {
        if self.topics.insert(topic.clone()) {
            self.pubsub.listen(vec![topic]);
        }
    }

    pub fn len(&self) -> usize {
        self.topics.len()
    }

    pub fn is_empty(&self) -> bool {
        self.topics.is_empty()
    }
}

impl Drop for TopicSet {
    fn drop(&mut self) {
        let all: Vec<String> = self.topics.drain().collect();
        if !all.is_empty() {
            self.pubsub.unlisten(all);
        }
    }
}

enum Exit {
    Cancelled,
    Reconnect(String),
}

async fn run_conn(
    inner: Arc<Inner>,
    index: usize,
    topics: Arc<Mutex<HashSet<String>>>,
    mut cmd_rx: mpsc::UnboundedReceiver<Cmd>,
) {
    let mut backoff = Backoff::new(Duration::from_secs(1), Duration::from_secs(120));
    loop {
        if inner.cancel.is_cancelled() {
            return;
        }
        let ws = tokio::select! {
            _ = inner.cancel.cancelled() => return,
            r = ws::connect(&inner.net, URL) => r,
        };
        let ws = match ws {
            Ok(ws) => ws,
            Err(e) => {
                let delay = backoff.next_delay();
                tracing::warn!(conn = index, error = %e, ?delay, "pubsub connect failed");
                let _ = inner.tx.send(PubSubEvent::Disconnected {
                    connection: index,
                    reason: e.to_string(),
                });
                tokio::select! {
                    _ = inner.cancel.cancelled() => return,
                    _ = tokio::time::sleep(delay) => continue,
                }
            }
        };
        tracing::debug!(conn = index, "pubsub connected");
        let _ = inner.tx.send(PubSubEvent::Connected { connection: index });
        let started = tokio::time::Instant::now();
        match session(&inner, ws, &topics, &mut cmd_rx).await {
            Exit::Cancelled => return,
            Exit::Reconnect(reason) => {
                tracing::debug!(conn = index, %reason, "pubsub reconnecting");
                let _ = inner.tx.send(PubSubEvent::Disconnected {
                    connection: index,
                    reason,
                });
                if started.elapsed() > Duration::from_secs(60) {
                    backoff.reset();
                }
                let delay = backoff.next_delay();
                tokio::select! {
                    _ = inner.cancel.cancelled() => return,
                    _ = tokio::time::sleep(delay) => {}
                }
            }
        }
    }
}

async fn session(
    inner: &Inner,
    ws: ws::WsStream,
    topics: &Mutex<HashSet<String>>,
    cmd_rx: &mut mpsc::UnboundedReceiver<Cmd>,
) -> Exit {
    let (mut sink, mut stream) = ws.split();
    let mut pending: HashMap<String, Vec<String>> = HashMap::new();

    let initial: Vec<String> = topics.lock().iter().cloned().collect();
    if !initial.is_empty() {
        if let Err(e) = send_listen(inner, &mut sink, "LISTEN", initial, &mut pending).await {
            return Exit::Reconnect(e);
        }
    }

    let jitter = Duration::from_millis(rand::random::<u64>() % 30_000);
    let mut next_ping = tokio::time::Instant::now() + PING_EVERY - jitter;
    let mut pong_deadline: Option<tokio::time::Instant> = None;

    loop {
        let deadline = pong_deadline.unwrap_or(next_ping);
        tokio::select! {
            _ = inner.cancel.cancelled() => {
                let _ = sink.close().await;
                return Exit::Cancelled;
            }
            _ = tokio::time::sleep_until(deadline) => {
                if pong_deadline.is_some() {
                    return Exit::Reconnect("pong timeout".into());
                }
                if sink.send(Message::text(r#"{"type":"PING"}"#)).await.is_err() {
                    return Exit::Reconnect("ping send failed".into());
                }
                pong_deadline = Some(tokio::time::Instant::now() + PONG_TIMEOUT);
                next_ping = tokio::time::Instant::now() + PING_EVERY;
            }
            cmd = cmd_rx.recv() => {
                let (kind, list) = match cmd {
                    Some(Cmd::Listen(t)) => ("LISTEN", t),
                    Some(Cmd::Unlisten(t)) => ("UNLISTEN", t),
                    None => return Exit::Cancelled,
                };
                if let Err(e) = send_listen(inner, &mut sink, kind, list, &mut pending).await {
                    return Exit::Reconnect(e);
                }
            }
            msg = stream.next() => {
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Ping(p))) => { let _ = sink.send(Message::Pong(p)).await; continue; }
                    Some(Ok(Message::Close(_))) | None => return Exit::Reconnect("closed by server".into()),
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Exit::Reconnect(e.to_string()),
                };
                let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                match v.get("type").and_then(Value::as_str).unwrap_or("") {
                    "PONG" => pong_deadline = None,
                    "RECONNECT" => return Exit::Reconnect("server requested reconnect".into()),
                    "RESPONSE" => {
                        let nonce = v.get("nonce").and_then(Value::as_str).unwrap_or("");
                        let topics = pending.remove(nonce).unwrap_or_default();
                        let err = v.get("error").and_then(Value::as_str).unwrap_or("");
                        if !err.is_empty() {
                            tracing::warn!(error = err, ?topics, "pubsub LISTEN rejected");
                            let _ = inner.tx.send(PubSubEvent::ListenError { topics, error: err.to_string() });
                        }
                    }
                    "MESSAGE" => {
                        if let Some(m) = parse_message(&v) {
                            let _ = inner.tx.send(PubSubEvent::Message(Arc::new(m)));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn send_listen<S>(
    inner: &Inner,
    sink: &mut S,
    kind: &str,
    topics: Vec<String>,
    pending: &mut HashMap<String, Vec<String>>,
) -> Result<(), String>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::fmt::Display,
{
    let nonce = format!("{:030x}", rand::random::<u128>() >> 8);
    let token = inner.creds.load().access_token.clone();
    let msg =
        json!({ "type": kind, "nonce": nonce, "data": { "topics": topics, "auth_token": token } });
    pending.insert(nonce, topics);
    sink.send(Message::text(msg.to_string()))
        .await
        .map_err(|e| e.to_string())
}

fn parse_message(v: &Value) -> Option<PubSubMessage> {
    let data = v.get("data")?;
    let topic = data.get("topic")?.as_str()?.to_string();
    let inner: Value = serde_json::from_str(data.get("message")?.as_str()?).ok()?;
    let kind = inner
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let payload = match inner.get("data") {
        Some(d) if d.is_object() => d.clone(),
        _ => inner,
    };
    let (topic_kind, target_id) = topic
        .rsplit_once('.')
        .map(|(a, b)| (a.to_string(), b.to_string()))?;
    Some(PubSubMessage {
        topic,
        topic_kind,
        target_id,
        kind,
        data: payload,
    })
}

/// Well-known topic names.
pub mod topics {
    pub fn community_points_user(user_id: &str) -> String {
        format!("community-points-user-v1.{user_id}")
    }
    pub fn drops_user(user_id: &str) -> String {
        format!("user-drop-events.{user_id}")
    }
    pub fn notifications_user(user_id: &str) -> String {
        format!("onsite-notifications.{user_id}")
    }
    pub fn predictions_user(user_id: &str) -> String {
        format!("predictions-user-v1.{user_id}")
    }
    pub fn playback(channel_id: &str) -> String {
        format!("video-playback-by-id.{channel_id}")
    }
    pub fn broadcast_settings(channel_id: &str) -> String {
        format!("broadcast-settings-update.{channel_id}")
    }
    pub fn raid(channel_id: &str) -> String {
        format!("raid.{channel_id}")
    }
    pub fn moments(channel_id: &str) -> String {
        format!("community-moments-channel-v1.{channel_id}")
    }
    pub fn predictions_channel(channel_id: &str) -> String {
        format!("predictions-channel-v1.{channel_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_points_message() {
        let inner = json!({"type":"points-earned","data":{"channel_id":"1","point_gain":{"total_points":10}}});
        let v = json!({"type":"MESSAGE","data":{"topic":"community-points-user-v1.42","message":inner.to_string()}});
        let m = parse_message(&v).unwrap();
        assert_eq!(m.kind, "points-earned");
        assert_eq!(m.topic_kind, "community-points-user-v1");
        assert_eq!(m.target_id, "42");
        assert_eq!(m.data["channel_id"], "1");
    }

    #[test]
    fn parses_flat_message() {
        let inner = json!({"type":"stream-up","server_time":1.0});
        let v = json!({"type":"MESSAGE","data":{"topic":"video-playback-by-id.7","message":inner.to_string()}});
        let m = parse_message(&v).unwrap();
        assert_eq!(m.kind, "stream-up");
        assert_eq!(m.data["server_time"], 1.0);
    }
}
