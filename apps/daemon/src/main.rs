//! `twitchminer`: headless daemon + control client.
//!
//! ```text
//! twitchminer run                 # start mining (asks for a login if there is no account)
//! twitchminer ctl state           # talk to a running daemon
//! twitchminer ctl add-channel xqc
//! twitchminer ctl set module:drops '{"games":["Rust"]}'
//! twitchminer ctl events          # live event stream
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use tm_core::event::Event;
use tm_core::logging::{self, LogOptions};
use tm_core::{Command, Engine, EngineConfig, Paths};

#[derive(Parser)]
#[command(
    name = "twitchminer",
    version,
    about = "Twitch drops & channel points miner"
)]
struct Cli {
    /// Data directory (default: platform data dir, or $TM_DATA_DIR).
    #[arg(long, global = true, env = "TM_DATA_DIR")]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the miner (default).
    Run {
        /// API listen address. Use 0.0.0.0:7878 to allow remote control (token required).
        #[arg(long, default_value = "127.0.0.1:7878", env = "TM_API_BIND")]
        bind: SocketAddr,
        /// Disable the HTTP/WebSocket API.
        #[arg(long)]
        no_api: bool,
        /// Log filter, e.g. `info` or `debug,tm_twitch=trace`.
        #[arg(long, env = "RUST_LOG")]
        log: Option<String>,
    },
    /// Control a running daemon through its API.
    Ctl {
        #[arg(long, default_value = "http://127.0.0.1:7878", env = "TM_API_URL")]
        url: String,
        /// API token (default: read from the data directory).
        #[arg(long, env = "TM_API_TOKEN")]
        token: Option<String>,
        #[command(subcommand)]
        action: Ctl,
    },
    /// Print the API token (creating it if needed).
    ApiToken,
}

#[derive(Subcommand)]
enum Ctl {
    /// Print the full state as JSON.
    State,
    /// Short human summary.
    Status,
    /// Start a device-code login and wait for it.
    Login,
    /// Add an account from an OAuth token.
    AddToken {
        token: String,
        #[arg(long)]
        refresh: Option<String>,
    },
    /// Add a channel to farm (points module) for an account (default: first account).
    AddChannel {
        login: String,
        #[arg(long)]
        account: Option<String>,
    },
    RemoveChannel {
        login: String,
        #[arg(long)]
        account: Option<String>,
    },
    /// Read a settings scope (`engine`, `module:drops`, `account:<id>/module:points`).
    Get {
        scope: String,
        #[arg(long)]
        account: Option<String>,
    },
    /// Merge JSON into a settings scope.
    Set {
        scope: String,
        json: String,
        #[arg(long)]
        replace: bool,
    },
    /// Call a module action, e.g. `call drops refresh`.
    Call {
        module: String,
        name: String,
        #[arg(default_value = "{}")]
        args: String,
        #[arg(long)]
        account: Option<String>,
    },
    History {
        #[arg(long, default_value_t = 30)]
        limit: u32,
    },
    Stats {
        #[arg(long)]
        by_channel: bool,
    },
    /// Stream live events.
    Events,
    /// Send a raw command JSON, e.g. '{"cmd":"modules_list"}'.
    Raw { json: String },
}

const TOKEN_KV: (&str, &str) = ("server", "token");

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tm_twitch::tls::install_crypto_provider();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        match cli.command.unwrap_or(Cmd::Run {
            bind: "127.0.0.1:7878".parse()?,
            no_api: false,
            log: None,
        }) {
            Cmd::Run { bind, no_api, log } => run(cli.data_dir, bind, no_api, log).await,
            Cmd::Ctl { url, token, action } => ctl(cli.data_dir, url, token, action).await,
            Cmd::ApiToken => {
                let paths = Paths::resolve(cli.data_dir)?;
                println!("{}", api_token(&paths).await?);
                Ok(())
            }
        }
    })
}

async fn api_token(paths: &Paths) -> anyhow::Result<String> {
    let db = tm_core::db::Db::open(&paths.db_file())?;
    if let Some(Value::String(t)) = db.kv_get(TOKEN_KV.0.into(), TOKEN_KV.1.into()).await? {
        return Ok(t);
    }
    let t = tm_server::generate_token();
    db.kv_put(TOKEN_KV.0.into(), TOKEN_KV.1.into(), json!(t))
        .await?;
    Ok(t)
}

async fn run(
    data_dir: Option<PathBuf>,
    bind: SocketAddr,
    no_api: bool,
    log: Option<String>,
) -> anyhow::Result<()> {
    let paths = Paths::resolve(data_dir)?;
    let logs = logging::init(
        &paths,
        LogOptions {
            filter: log,
            stdout: true,
        },
    );
    let token = api_token(&paths).await?;
    let engine = Engine::start(EngineConfig {
        paths: paths.clone(),
        modules: tm_modules::builtin(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
    .await?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), data = %paths.root.display(), "TwitchMiner started");

    let shutdown = Arc::new(tokio::sync::Notify::new());
    if !no_api {
        let cfg = tm_server::ServerConfig {
            bind,
            token: Some(token),
            logs: Some(logs.buffer.clone()),
        };
        let engine = engine.clone();
        let stop = shutdown.clone();
        tokio::spawn(async move {
            if let Err(e) =
                tm_server::serve(engine, cfg, async move { stop.notified().await }).await
            {
                tracing::error!(error = %e, "API server failed");
            }
        });
    }

    // Headless first run: ask for a login right here.
    if engine.store().read(|s| s.accounts.is_empty()) {
        let flow = engine
            .execute(Command::LoginStart { profile: None })
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        println!(
            "\n  No account yet. Open {} and enter the code: {}\n",
            flow["verification_uri"]
                .as_str()
                .unwrap_or("https://www.twitch.tv/activate"),
            flow["user_code"].as_str().unwrap_or("?")
        );
    }

    wait_for_signal().await;
    tracing::info!("shutting down");
    shutdown.notify_waiters();
    engine.shutdown().await;
    Ok(())
}

async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("signal handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

// ---------------------------------------------------------------------- client

struct Client {
    http: reqwest::Client,
    url: String,
    token: String,
}

impl Client {
    async fn call(&self, cmd: Value) -> anyhow::Result<Value> {
        let resp: Value = self
            .http
            .post(format!("{}/api/v1/command", self.url))
            .bearer_auth(&self.token)
            .json(&cmd)
            .send()
            .await
            .context("daemon unreachable (is `twitchminer run` started?)")?
            .json()
            .await?;
        if resp["ok"].as_bool() == Some(true) {
            Ok(resp["data"].clone())
        } else {
            bail!("{}", resp["error"])
        }
    }

    async fn first_account(&self, account: Option<String>) -> anyhow::Result<String> {
        if let Some(a) = account {
            return Ok(a);
        }
        let state = self.call(json!({"cmd": "snapshot"})).await?;
        state["accounts"]
            .as_object()
            .and_then(|m| m.keys().next().cloned())
            .context("no account configured")
    }
}

async fn ctl(
    data_dir: Option<PathBuf>,
    url: String,
    token: Option<String>,
    action: Ctl,
) -> anyhow::Result<()> {
    let token = match token {
        Some(t) => t,
        None => api_token(&Paths::resolve(data_dir)?).await?,
    };
    let url = url.trim_end_matches('/').to_string();
    let http = reqwest::Client::builder().no_proxy().build()?;
    let c = Client {
        http,
        url: url.clone(),
        token: token.clone(),
    };
    let print = |v: Value| println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    match action {
        Ctl::State => print(c.call(json!({"cmd": "snapshot"})).await?),
        Ctl::Status => status(&c.call(json!({"cmd": "snapshot"})).await?),
        Ctl::Login => {
            let flow = c.call(json!({"cmd": "login_start"})).await?;
            println!(
                "Open {} and enter: {}",
                flow["verification_uri"].as_str().unwrap_or(""),
                flow["user_code"].as_str().unwrap_or("")
            );
            println!(
                "(the daemon finishes the login on its own; watch with `twitchminer ctl events`)"
            );
        }
        Ctl::AddToken { token, refresh } => print(
            c.call(
                json!({"cmd": "login_with_token", "access_token": token, "refresh_token": refresh}),
            )
            .await?,
        ),
        Ctl::AddChannel { login, account } => {
            let account = c.first_account(account).await?;
            print(c.call(json!({"cmd": "module_call", "account": account, "module": "points", "name": "add_channel", "args": {"login": login}})).await?)
        }
        Ctl::RemoveChannel { login, account } => {
            let account = c.first_account(account).await?;
            print(c.call(json!({"cmd": "module_call", "account": account, "module": "points", "name": "remove_channel", "args": {"login": login}})).await?)
        }
        Ctl::Get { scope, account } => print(
            c.call(json!({"cmd": "settings_get", "scope": scope, "account": account}))
                .await?,
        ),
        Ctl::Set {
            scope,
            json: value,
            replace,
        } => {
            let value: Value = serde_json::from_str(&value).context("invalid JSON")?;
            print(c.call(json!({"cmd": "settings_set", "scope": scope, "value": value, "merge": !replace})).await?)
        }
        Ctl::Call {
            module,
            name,
            args,
            account,
        } => {
            let account = c.first_account(account).await?;
            let args: Value = serde_json::from_str(&args).context("invalid JSON args")?;
            print(c.call(json!({"cmd": "module_call", "account": account, "module": module, "name": name, "args": args})).await?)
        }
        Ctl::History { limit } => {
            let rows = c.call(json!({"cmd": "history", "limit": limit})).await?;
            for r in rows.as_array().into_iter().flatten().rev() {
                println!(
                    "{}  {:<22} {}",
                    r["ts"].as_str().unwrap_or(""),
                    r["kind"].as_str().unwrap_or(""),
                    r["event"]
                );
            }
        }
        Ctl::Stats { by_channel } => print(
            c.call(json!({"cmd": "stats", "group_by": if by_channel { "channel" } else { "day" }}))
                .await?,
        ),
        Ctl::Raw { json: raw } => print(
            c.call(serde_json::from_str(&raw).context("invalid JSON")?)
                .await?,
        ),
        Ctl::Events => events(&url, &token).await?,
    }
    Ok(())
}

fn status(state: &Value) {
    println!(
        "TwitchMiner {} (seq {})",
        state["version"].as_str().unwrap_or("?"),
        state["seq"]
    );
    for (_, a) in state["accounts"].as_object().into_iter().flatten() {
        let info = &a["info"];
        println!(
            "\n● {} — {}",
            info["display_name"].as_str().unwrap_or("?"),
            a["status"]["state"].as_str().unwrap_or("?")
        );
        let s = &a["session"];
        println!(
            "  session: +{} points, {} min watched, {} drops, {} bonuses, {} raids",
            s["points_earned"],
            s["watch_minutes"],
            s["drops_claimed"],
            s["bonuses_claimed"],
            s["raids_joined"]
        );
        let modules: Vec<String> = a["modules"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| format!("{k}:{}", v["state"].as_str().unwrap_or("?")))
            .collect();
        println!("  modules: {}", modules.join(", "));
        for (id, w) in a["watching"].as_object().into_iter().flatten() {
            let name = a["channels"][id]["display_name"].as_str().unwrap_or(id);
            println!(
                "  ▶ watching {name} ({}) — {} min",
                w["reason"].as_str().unwrap_or(""),
                w["minutes"]
            );
        }
        if let Some(t) = a["drop_target"].as_object() {
            let camp = &a["campaigns"][t["campaign_id"].as_str().unwrap_or("")];
            println!(
                "  🎁 drops: {} — {}",
                t["game"].as_str().unwrap_or(""),
                camp["name"].as_str().unwrap_or("")
            );
            for d in camp["drops"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|d| d["claimed"] != true)
            {
                println!(
                    "     · {} {}/{} min",
                    d["name"].as_str().unwrap_or(""),
                    d["current_minutes"],
                    d["required_minutes"]
                );
            }
        }
        let online = a["channels"]
            .as_object()
            .map(|m| m.values().filter(|c| !c["stream"].is_null()).count())
            .unwrap_or(0);
        println!(
            "  channels: {} tracked, {} live, {} campaigns",
            a["channels"].as_object().map_or(0, |m| m.len()),
            online,
            a["campaigns"].as_object().map_or(0, |m| m.len())
        );
    }
    for (_, n) in state["notices"].as_object().into_iter().flatten() {
        println!("⚠ {}", n["message"].as_str().unwrap_or(""));
    }
}

async fn events(url: &str, token: &str) -> anyhow::Result<()> {
    use futures_util::StreamExt;
    let ws_url = format!("{}/api/v1/ws?token={token}", url.replacen("http", "ws", 1));
    let net = tm_twitch::Net::new(&tm_twitch::NetConfig::default())?;
    let net = tm_twitch::Net {
        ws_proxy: None,
        ..net
    };
    let mut ws = tm_twitch::ws::connect(&net, &ws_url).await?;
    while let Some(Ok(msg)) = ws.next().await {
        let Ok(text) = msg.into_text() else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        match v["type"].as_str() {
            Some("snapshot") => println!("[snapshot] seq {}", v["state"]["seq"]),
            Some("event") => {
                let e = &v["event"];
                let parsed: Option<Event> = serde_json::from_value(e.clone()).ok();
                let ts = e["ts"].as_str().unwrap_or("").get(11..19).unwrap_or("");
                match parsed {
                    Some(ev) => println!("{ts} {:<24} {}", ev.kind(), compact(e)),
                    None => println!("{ts} {e}"),
                }
            }
            _ => println!("{text}"),
        }
    }
    Ok(())
}

fn compact(e: &Value) -> String {
    let mut e = e.clone();
    if let Some(o) = e.as_object_mut() {
        for k in ["seq", "ts", "type", "account"] {
            o.remove(k);
        }
    }
    let s = e.to_string();
    if s.len() > 200 {
        format!("{}…", &s[..s.floor_char_boundary(200)])
    } else {
        s
    }
}
