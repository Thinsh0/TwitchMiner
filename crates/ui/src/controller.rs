//! Glue between the window and the backend: opens the session, mirrors the state into
//! the views, and turns callbacks into commands (run on the tokio runtime, never on
//! the UI thread).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use tm_client::Backend;
use tm_core::{ApiResult, Command, Event};
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use crate::present::{self, Input, Last};
use crate::session::{self, OpenError, Session, Target};
use crate::{Bridge, Conn, MainWindow, SetupReason, ToastItem, ToastKind, Tone};

thread_local! {
    /// Last state seen, on the UI thread (re-rendered by the timer).
    static LAST: RefCell<Last> = RefCell::new(Last::default());
}

/// How long a toast stays on screen.
const TOAST_TTL: Duration = Duration::from_secs(6);

#[derive(Clone)]
pub struct Controller(Arc<Inner>);

struct Inner {
    rt: Handle,
    ui: slint::Weak<MainWindow>,
    data_dir: Option<PathBuf>,
    backend: Mutex<Option<Arc<dyn Backend>>>,
    session: tokio::sync::Mutex<Option<Session>>,
    watcher: Mutex<Option<JoinHandle<()>>>,
}

impl Controller {
    pub fn new(rt: Handle, ui: &MainWindow, data_dir: Option<PathBuf>) -> Self {
        Self(Arc::new(Inner {
            rt,
            ui: ui.as_weak(),
            data_dir,
            backend: Mutex::new(None),
            session: tokio::sync::Mutex::new(None),
            watcher: Mutex::new(None),
        }))
    }

    pub fn bind(&self, ui: &MainWindow) {
        let b = ui.global::<Bridge>();
        b.set_toasts(ModelRc::from(Rc::new(VecModel::<ToastItem>::default())));

        let c = self.clone();
        b.on_login_start(move || c.login_start());
        let c = self.clone();
        b.on_login_cancel(move |flow_id| {
            c.command(Command::LoginCancel {
                flow_id: flow_id.into(),
            })
        });
        b.on_open_url(|url| open_url(&url));
        let c = self.clone();
        b.on_connect_remote(move |url, token| {
            c.open(
                Target::Remote {
                    url: url.trim().trim_end_matches('/').to_string(),
                    token: token.trim().to_string(),
                },
                true,
            )
        });
        let c = self.clone();
        b.on_use_local(move || c.open(Target::Local, true));
        let c = self.clone();
        b.on_show_setup(move || c.show_setup());
        let weak = ui.as_weak();
        b.on_cancel_setup(move || {
            if let Some(ui) = weak.upgrade() {
                ui.global::<Bridge>().set_conn(Conn::Ready);
            }
        });
        let c = self.clone();
        b.on_dismiss_notice(move |id| c.command(Command::NoticeDismiss { id: id.into() }));
        let weak = ui.as_weak();
        b.on_dismiss_toast(move |id| {
            if let Some(ui) = weak.upgrade() {
                remove_toast(&ui, id);
            }
        });
        let c = self.clone();
        b.on_set_account_enabled(move |account, enabled| {
            c.command(Command::AccountSetEnabled {
                account: account.into(),
                enabled,
            })
        });
        let c = self.clone();
        b.on_remove_account(move |account| {
            c.command(Command::AccountRemove {
                account: account.into(),
            })
        });
        let weak = ui.as_weak();
        b.on_select_account(move |id| {
            if let Some(ui) = weak.upgrade() {
                ui.global::<Bridge>().set_account(id);
                render(&ui);
            }
        });
        b.on_theme_changed(|_| {
            // Not persisted yet: see the note on interface preferences.
        });
    }

    /// Opens a session on `target`. `from_setup`: asked from the "choose an engine"
    /// screen, which stays visible (with the error) if it fails.
    pub fn open(&self, target: Target, from_setup: bool) {
        let me = self.clone();
        me.on_ui(move |ui| {
            let b = ui.global::<Bridge>();
            if from_setup {
                b.set_connecting(true);
            } else {
                b.set_conn(Conn::Starting);
            }
        });
        let inner = self.0.clone();
        self.0.rt.spawn(async move {
            // Already on the requested local engine: nothing to restart.
            let same_local = target == Target::Local
                && inner
                    .session
                    .lock()
                    .await
                    .as_ref()
                    .is_some_and(|s| s.target == Target::Local);
            if same_local {
                me.on_ui(|ui| {
                    let b = ui.global::<Bridge>();
                    b.set_connecting(false);
                    b.set_conn(Conn::Ready);
                });
                return;
            }
            // A local engine must release the data directory before another one opens it.
            if target == Target::Local {
                me.close().await;
            }
            match Session::open(target.clone(), inner.data_dir.clone()).await {
                Ok(new) => {
                    let backend = new.backend.clone();
                    let old = inner.session.lock().await.replace(new);
                    me.watch(backend, &target);
                    if let Some(old) = old {
                        old.close().await;
                    }
                    me.on_ui(|ui| {
                        let b = ui.global::<Bridge>();
                        b.set_connecting(false);
                        b.set_setup_reason(SetupReason::None);
                        b.set_conn(Conn::Ready);
                    });
                }
                Err(e) => {
                    let has_session = inner.session.lock().await.is_some();
                    let (reason, detail, url, token) = match e {
                        OpenError::Busy { pid } => (
                            SetupReason::Busy,
                            pid,
                            session::LOCAL_DAEMON_URL.to_string(),
                            session::local_daemon_token(inner.data_dir.clone())
                                .await
                                .unwrap_or_default(),
                        ),
                        OpenError::Failed(msg) => {
                            let (url, token) = match &target {
                                Target::Remote { url, token } => (url.clone(), token.clone()),
                                Target::Local => Default::default(),
                            };
                            (SetupReason::Failed, msg, url, token)
                        }
                    };
                    me.on_ui(move |ui| {
                        let b = ui.global::<Bridge>();
                        b.set_connecting(false);
                        b.set_setup_reason(reason);
                        b.set_setup_detail(detail.into());
                        b.set_setup_url(url.into());
                        b.set_setup_token(token.into());
                        b.set_setup_can_cancel(has_session);
                        b.set_conn(Conn::Setup);
                    });
                }
            }
        });
    }

    fn show_setup(&self) {
        let remote_url = LAST.with(|l| l.borrow().remote.as_ref().map(|(u, _)| u.clone()));
        self.on_ui(move |ui| {
            let b = ui.global::<Bridge>();
            b.set_setup_reason(SetupReason::None);
            b.set_setup_url(remote_url.unwrap_or_default().into());
            b.set_setup_token(SharedString::new());
            b.set_setup_can_cancel(true);
            b.set_conn(Conn::Setup);
        });
    }

    /// Mirrors the backend's state, link and notable events into the window.
    fn watch(&self, backend: Arc<dyn Backend>, target: &Target) {
        let remote_url = match target {
            Target::Remote { url, .. } => Some(url.clone()),
            Target::Local => None,
        };
        *self.0.backend.lock().expect("backend lock") = Some(backend.clone());
        let me = self.clone();
        let task = self.0.rt.spawn(async move {
            let mut state = backend.state();
            let mut link = backend.link();
            let mut events = backend.events();
            // A local engine's link never changes (its sender is gone): stop polling it.
            let mut link_open = true;
            loop {
                let s = state.borrow_and_update().clone();
                let l = link.borrow_and_update().clone();
                let remote = remote_url.clone().map(|u| (u, l));
                me.on_ui(move |ui| {
                    LAST.with(|last| *last.borrow_mut() = Last { state: s, remote });
                    render(ui);
                });
                // Wait for the next change, handling events (toasts) in between.
                loop {
                    tokio::select! {
                        r = state.changed() => { if r.is_err() { return; } break; }
                        r = link.changed(), if link_open => {
                            if r.is_err() { link_open = false; continue; }
                            break;
                        }
                        r = events.recv() => match r {
                            Ok(env) => me.on_event(&env.event),
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                            Err(_) => return,
                        },
                    }
                }
            }
        });
        if let Some(old) = self.0.watcher.lock().expect("watcher lock").replace(task) {
            old.abort();
        }
    }

    fn on_event(&self, event: &Event) {
        if let Event::LoginFinished { error, account, .. } = event {
            match error {
                Some(e) => self.toast(ToastKind::Error, Tone::Danger, e.clone()),
                None if account.is_some() => {
                    let id = account.clone().unwrap_or_default();
                    self.on_ui(move |ui| {
                        let name = LAST.with(|l| {
                            l.borrow()
                                .state
                                .accounts
                                .get(&id)
                                .and_then(|a| a.info.as_ref())
                                .map(|i| i.display_name.clone())
                        });
                        let b = ui.global::<Bridge>();
                        b.set_account(id.into());
                        b.set_screen(crate::Screen::Home);
                        if let Some(name) = name {
                            push_toast(ui, ToastKind::LoginOk, Tone::Success, name);
                        }
                    });
                }
                None => {}
            }
        }
    }

    fn login_start(&self) {
        self.on_ui(|ui| ui.global::<Bridge>().set_login_busy(true));
        let me = self.clone();
        self.run(Command::LoginStart { profile: None }, move |r| {
            me.on_ui(|ui| ui.global::<Bridge>().set_login_busy(false));
            if let Ok(v) = r
                && let Some(url) = v["verification_uri"].as_str()
            {
                open_url(url);
            }
        });
    }

    /// Fire-and-forget command; failures become a toast.
    fn command(&self, cmd: Command) {
        self.run(cmd, |_| {});
    }

    fn run(&self, cmd: Command, done: impl FnOnce(&ApiResult) + Send + 'static) {
        let Some(backend) = self.0.backend.lock().expect("backend lock").clone() else {
            return;
        };
        let me = self.clone();
        self.0.rt.spawn(async move {
            let r = backend.execute(cmd).await;
            if let Err(e) = &r {
                me.toast(ToastKind::Error, Tone::Danger, e.message.clone());
            }
            done(&r);
        });
    }

    fn toast(&self, kind: ToastKind, tone: Tone, detail: String) {
        self.on_ui(move |ui| push_toast(ui, kind, tone, detail));
    }

    fn on_ui(&self, f: impl FnOnce(&MainWindow) + Send + 'static) {
        let _ = self.0.ui.upgrade_in_event_loop(move |ui| f(&ui));
    }

    /// Stops watching and closes the session (local engine shut down cleanly).
    pub async fn close(&self) {
        if let Some(w) = self.0.watcher.lock().expect("watcher lock").take() {
            w.abort();
        }
        *self.0.backend.lock().expect("backend lock") = None;
        if let Some(s) = self.0.session.lock().await.take() {
            s.close().await;
        }
    }
}

/// Re-renders every view from the last state (UI thread).
pub fn render(ui: &MainWindow) {
    LAST.with(|last| {
        let last = last.borrow();
        let input = Input {
            state: &last.state,
            remote: last.remote.as_ref().map(|(u, l)| (u.as_str(), l)),
        };
        present::apply(ui, &input);
    });
}

fn push_toast(ui: &MainWindow, kind: ToastKind, tone: Tone, detail: String) {
    thread_local!(static NEXT: std::cell::Cell<i32> = const { std::cell::Cell::new(1) });
    let id = NEXT.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    let toasts = ui.global::<Bridge>().get_toasts();
    let Some(model) = toasts.as_any().downcast_ref::<VecModel<ToastItem>>() else {
        return;
    };
    // Same message already shown: do not stack it.
    if model.iter().any(|t| t.kind == kind && t.detail == detail) {
        return;
    }
    model.push(ToastItem {
        id,
        kind,
        tone,
        detail: detail.into(),
    });
    let weak = ui.as_weak();
    slint::Timer::single_shot(TOAST_TTL, move || {
        if let Some(ui) = weak.upgrade() {
            remove_toast(&ui, id);
        }
    });
}

fn remove_toast(ui: &MainWindow, id: i32) {
    let toasts = ui.global::<Bridge>().get_toasts();
    if let Some(model) = toasts.as_any().downcast_ref::<VecModel<ToastItem>>()
        && let Some(i) = model.iter().position(|t| t.id == id)
    {
        model.remove(i);
    }
}

fn open_url(url: &str) {
    // Only web links: never let a value from the state launch a local program.
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }
    if let Err(e) = open::that_detached(url) {
        tracing::warn!(error = %e, url, "cannot open the browser");
    }
}
