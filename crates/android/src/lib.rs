//! JNI bridge: lets an Android foreground service host the engine.
//!
//! Kotlin side (see `kotlin/Native.kt`):
//! ```kotlin
//! val h = Native.start(filesDir.absolutePath, null)        // or "0.0.0.0:7878" for remote control
//! val reply = Native.execute(h, """{"cmd":"summary"}""")   // {"ok":true,"data":{...}}
//! Native.stop(h)
//! ```
//! Every call exchanges the JSON documented in `docs/API.md`.

use std::path::PathBuf;

use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{jlong, jstring};
use serde_json::json;
use tm_core::Command;

/// Engine + the runtime driving it, owned by the Java side through an opaque handle.
pub struct Host {
    runtime: tokio::runtime::Runtime,
    app: Option<tm_app::App>,
}

impl Host {
    pub fn start(data_dir: PathBuf, api_bind: Option<String>) -> Result<Self, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("twitchminer")
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let api = match api_bind {
            Some(b) => Some(tm_app::ApiOptions {
                bind: b.parse().map_err(|e| format!("bad bind address: {e}"))?,
                token: None,
            }),
            None => None,
        };
        let app = runtime
            .block_on(tm_app::App::start(tm_app::AppOptions {
                data_dir: Some(data_dir),
                log_stdout: false,
                api,
                ..Default::default()
            }))
            .map_err(|e| format!("{e:#}"))?;
        Ok(Self {
            runtime,
            app: Some(app),
        })
    }

    /// Executes a JSON command and returns `{"ok":true,"data":…}` / `{"ok":false,"error":…}`.
    pub fn execute_json(&self, command: &str) -> String {
        let Some(app) = &self.app else {
            return json!({"ok": false, "error": {"code": "unavailable", "message": "stopped"}})
                .to_string();
        };
        let reply = match serde_json::from_str::<Command>(command) {
            Ok(cmd) => match self.runtime.block_on(app.engine.execute(cmd)) {
                Ok(data) => json!({"ok": true, "data": data}),
                Err(e) => json!({"ok": false, "error": e}),
            },
            Err(e) => {
                json!({"ok": false, "error": {"code": "bad_command", "message": e.to_string()}})
            }
        };
        reply.to_string()
    }

    pub fn summary_json(&self) -> String {
        self.execute_json(r#"{"cmd":"summary"}"#)
    }

    pub fn api_token(&self) -> Option<String> {
        self.app.as_ref().map(|a| a.api_token.clone())
    }

    pub fn stop(mut self) {
        if let Some(app) = self.app.take() {
            self.runtime.block_on(app.shutdown());
        }
        self.runtime
            .shutdown_timeout(std::time::Duration::from_secs(5));
    }
}

fn read(env: &mut JNIEnv, s: &JString) -> Option<String> {
    if s.is_null() {
        return None;
    }
    env.get_string(s).ok().map(Into::into)
}

fn out(env: &mut JNIEnv, s: String) -> jstring {
    env.new_string(s)
        .map(|j| j.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// `handle` must come from `start` and not have been passed to `stop`.
unsafe fn host<'a>(handle: jlong) -> Option<&'a Host> {
    unsafe { (handle as *const Host).as_ref() }
}

/// `Native.start(dataDir: String, apiBind: String?): Long` — 0 on failure (see `lastError`).
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_thinsh0_twitchminer_Native_start(
    mut env: JNIEnv,
    _class: JClass,
    data_dir: JString,
    api_bind: JString,
) -> jlong {
    let Some(dir) = read(&mut env, &data_dir) else {
        return 0;
    };
    let bind = read(&mut env, &api_bind);
    match Host::start(PathBuf::from(dir), bind) {
        Ok(h) => Box::into_raw(Box::new(h)) as jlong,
        Err(e) => {
            let _ = env.throw_new("java/lang/IllegalStateException", e);
            0
        }
    }
}

/// `Native.execute(handle: Long, commandJson: String): String`
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_thinsh0_twitchminer_Native_execute(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    command: JString,
) -> jstring {
    // SAFETY: the Kotlin wrapper only passes handles returned by `start`.
    let Some(h) = (unsafe { host(handle) }) else {
        return std::ptr::null_mut();
    };
    let cmd = read(&mut env, &command).unwrap_or_default();
    let reply = h.execute_json(&cmd);
    out(&mut env, reply)
}

/// `Native.summary(handle: Long): String` — shortcut for the notification / Now Bar.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_thinsh0_twitchminer_Native_summary(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    // SAFETY: see `execute`.
    let Some(h) = (unsafe { host(handle) }) else {
        return std::ptr::null_mut();
    };
    let reply = h.summary_json();
    out(&mut env, reply)
}

/// `Native.apiToken(handle: Long): String?`
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_thinsh0_twitchminer_Native_apiToken(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    // SAFETY: see `execute`.
    let Some(t) = (unsafe { host(handle) }).and_then(Host::api_token) else {
        return std::ptr::null_mut();
    };
    out(&mut env, t)
}

/// `Native.stop(handle: Long)` — the handle is invalid afterwards.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_thinsh0_twitchminer_Native_stop(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    // SAFETY: ownership goes back to Rust exactly once (Kotlin zeroes its handle).
    let h = unsafe { Box::from_raw(handle as *mut Host) };
    h.stop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_roundtrip_without_jvm() {
        let dir = std::env::temp_dir().join(format!("tm-android-{}", std::process::id()));
        let h = Host::start(dir.clone(), None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&h.summary_json()).unwrap();
        assert_eq!(v["ok"], true);
        let bad: serde_json::Value = serde_json::from_str(&h.execute_json("{nope")).unwrap();
        assert_eq!(bad["error"]["code"], "bad_command");
        h.stop();
        let _ = std::fs::remove_dir_all(dir);
    }
}
