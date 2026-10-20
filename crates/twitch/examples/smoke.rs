//! Live smoke test against Twitch.
//!
//! `TM_TOKEN=<oauth token> cargo run -p tm-twitch --example smoke -- [channel_login]`

use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use tm_twitch::auth::{self, Credentials, TokenResponse};
use tm_twitch::pubsub::topics;
use tm_twitch::watch::{self, WatchTarget};
use tm_twitch::{Gql, Manifest, Net, NetConfig, PubSub, PubSubEvent, manifest};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow_like::Result {
    let token = std::env::var("TM_TOKEN").expect("TM_TOKEN");
    let net = Net::new(&NetConfig::default())?;
    let m = Manifest::embedded();
    let (creds, v): (Credentials, _) = auth::credentials_from_token(
        &net,
        &m.default_profile.clone(),
        TokenResponse {
            access_token: token,
            refresh_token: None,
            expires_in: None,
        },
        None,
    )
    .await?;
    println!(
        "validated: {} ({}) expires_in={}",
        v.login, v.user_id, v.expires_in
    );
    let creds = Arc::new(ArcSwap::from_pointee(creds));
    let gql = Gql::new(net.clone(), manifest::shared(m), creds.clone());

    let (inv, claimed) = gql.inventory().await?;
    println!(
        "inventory: {} in progress, {} claimed benefits",
        inv.len(),
        claimed.len()
    );
    println!("dashboard: {:?}", gql.dashboard().await?.map(|d| d.len()));

    let channel = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "zackrawrr".into());
    let user = gql
        .users_by_login(std::slice::from_ref(&channel))
        .await?
        .pop()
        .flatten()
        .expect("channel exists");
    println!(
        "channel {} online={} game={:?}",
        user.login,
        user.stream.is_some(),
        user.stream
            .as_ref()
            .and_then(|s| s.game.as_ref())
            .map(|g| &g.name)
    );
    let ctx = gql.channel_points_context(&channel).await?;
    println!("points ctx: {ctx:?}");
    let drops = gql.available_drops(std::slice::from_ref(&user.id)).await?;
    println!(
        "available drops campaigns: {:?}",
        drops[0].iter().map(|c| &c.name).collect::<Vec<_>>()
    );
    println!("current drop: {:?}", gql.current_drop(&user.id).await?);
    println!("follows: {}", gql.follows().await?.len());

    let cancel = CancellationToken::new();
    let ps = PubSub::new(net.clone(), creds.clone(), cancel.clone());
    let mut rx = ps.subscribe();
    let mut set = ps.topic_set();
    set.set([
        topics::community_points_user(&v.user_id),
        topics::drops_user(&v.user_id),
        topics::playback(&user.id),
    ]);

    let (tx, mut reports) = tokio::sync::mpsc::channel(8);
    tokio::spawn(watch::run(
        gql.clone(),
        WatchTarget {
            channel_id: user.id.clone(),
            login: user.login.clone(),
        },
        tx,
        cancel.clone(),
    ));

    let secs: u64 = std::env::var("TM_SMOKE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(70);
    let deadline = tokio::time::sleep(Duration::from_secs(secs));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            r = reports.recv() => println!("watch: {r:?}"),
            e = rx.recv() => match e {
                Ok(PubSubEvent::Message(m)) => println!("pubsub {} {} {}", m.topic, m.kind, m.data.to_string().chars().take(200).collect::<String>()),
                Ok(other) => println!("pubsub {other:?}"),
                Err(e) => println!("pubsub lag {e}"),
            },
        }
    }
    cancel.cancel();
    Ok(())
}

mod anyhow_like {
    pub type Result = std::result::Result<(), Box<dyn std::error::Error>>;
}
