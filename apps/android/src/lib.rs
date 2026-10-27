//! TwitchMiner on Android: the shared interface (`tm-ui`) in a NativeActivity, as a
//! client of a TwitchMiner running elsewhere (home server, PC). No engine runs on the
//! phone. Built into an APK by `packaging/android/build-apk.sh`, with a little Java
//! (`packaging/android/java`: notifications pushed by the server, file chooser) called
//! through JNI.

#[cfg(target_os = "android")]
mod android {
    use std::cell::{Cell, OnceCell};
    use std::ffi::{CString, c_char, c_int};
    use std::io::Write;
    use std::path::{Path, PathBuf};

    use slint::android::AndroidApp;

    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
    }

    /// One line in logcat (`adb logcat -s TwitchMiner`), priority INFO (4) or ERROR (6).
    fn logcat(prio: c_int, text: &str) {
        let text = CString::new(text.replace('\0', " ")).unwrap_or_default();
        // SAFETY: both strings are NUL-terminated and outlive the call.
        unsafe { __android_log_write(prio, c"TwitchMiner".as_ptr(), text.as_ptr()) };
    }

    /// `tracing` output, line by line, to logcat.
    struct Logcat;

    impl Write for Logcat {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let text = String::from_utf8_lossy(buf);
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                logcat(4, line);
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Where a panic is written, to be shown at the next start (no logcat on a phone
    /// without a computer).
    const CRASH_FILE: &str = "last-crash.txt";

    fn init_logs(data_dir: Option<PathBuf>) {
        let _ = tracing_subscriber::fmt()
            .with_writer(|| Logcat)
            .with_ansi(false)
            .without_time()
            .with_env_filter("info")
            .try_init();
        std::panic::set_hook(Box::new(move |info| {
            let text = format!("panic: {info}");
            logcat(6, &text);
            if let Some(dir) = &data_dir {
                let _ = std::fs::write(dir.join(CRASH_FILE), &text);
            }
        }));
    }

    /// The panic of the previous run, if any (read once).
    fn last_crash(data_dir: Option<&Path>) -> Option<String> {
        let path = data_dir?.join(CRASH_FILE);
        let text = std::fs::read_to_string(&path).ok()?;
        let _ = std::fs::remove_file(&path);
        Some(text.chars().take(600).collect())
    }

    struct Phone {
        app: AndroidApp,
        /// `pushEnable` was called: `push_address` reports its outcome.
        enabling: Cell<bool>,
        /// Device id and model, asked once (the interface reads them at every update).
        device: OnceCell<Option<(String, String)>>,
    }

    impl Phone {
        fn device(&self) -> Option<&(String, String)> {
            self.device
                .get_or_init(|| {
                    let id = java_string(&self.app, "pushDevice").filter(|d| !d.is_empty())?;
                    let name = java_string(&self.app, "pushName").unwrap_or_default();
                    Some((id, name))
                })
                .as_ref()
        }
    }

    impl tm_ui::Platform for Phone {
        fn open_url(&self, url: &str) -> bool {
            // No browser: ActivityNotFoundException, cleared by `with_activity`.
            with_activity(&self.app, |env, activity| view(env, activity, url)).is_some()
        }

        fn push_device(&self) -> Option<String> {
            self.device().map(|(id, _)| id.clone())
        }

        fn push_enable(&self) {
            self.enabling.set(true);
            java_void(&self.app, "pushEnable");
        }

        fn push_address(&self) -> Option<Result<tm_ui::PushAddress, String>> {
            let address = |token: String| {
                let (device, name) = self.device().cloned().unwrap_or_default();
                Ok(tm_ui::PushAddress {
                    token,
                    device,
                    name,
                })
            };
            if !self.enabling.get() {
                let token = java_string(&self.app, "pushToken")?;
                return (!token.is_empty()).then(|| address(token));
            }
            let state = java_string(&self.app, "pushState")?;
            let (kind, detail) = state.split_once('\n')?;
            self.enabling.set(false);
            match kind {
                "ok" if !detail.is_empty() => Some(address(detail.to_string())),
                _ => Some(Err(detail.to_string())),
            }
        }

        fn pick_file(&self) -> bool {
            with_activity(&self.app, |env, activity| {
                env.call_method(activity, "pickFile", "()Z", &[])?.z()
            })
            .unwrap_or(false)
        }

        fn picked_file(&self) -> Option<Result<String, String>> {
            let picked = java_string(&self.app, "pickedFile")?;
            let (kind, detail) = picked.split_once('\n').unwrap_or((&picked, ""));
            Some(match kind {
                "ok" => Ok(detail.to_string()),
                _ => Err(detail.to_string()),
            })
        }
    }

    /// Runs `f` with the activity (`MainActivity.java`); `None`, logged, when it fails.
    /// A Java exception is cleared, never left pending.
    fn with_activity<R>(
        app: &AndroidApp,
        f: impl FnOnce(&mut jni::JNIEnv, &jni::objects::JObject) -> jni::errors::Result<R>,
    ) -> Option<R> {
        // SAFETY: both pointers come from the running activity and stay valid while it
        // runs; the VM is never destroyed by this handle.
        let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) }.ok()?;
        let activity = unsafe { jni::objects::JObject::from_raw(app.activity_as_ptr().cast()) };
        let mut env = vm.attach_current_thread().ok()?;
        let result = f(&mut env, &activity);
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        match result {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::warn!(error = %e, "call to the Java side failed");
                None
            }
        }
    }

    /// `activity.<method>()` returning a `String` (`None` for `null`).
    fn java_string(app: &AndroidApp, method: &str) -> Option<String> {
        with_activity(app, |env, activity| {
            let s = env
                .call_method(activity, method, "()Ljava/lang/String;", &[])?
                .l()?;
            if s.is_null() {
                return Ok(None);
            }
            let s = jni::objects::JString::from(s);
            let text: String = env.get_string(&s)?.into();
            Ok(Some(text))
        })
        .flatten()
    }

    fn java_void(app: &AndroidApp, method: &str) {
        with_activity(app, |env, activity| {
            env.call_method(activity, method, "()V", &[])?.v()
        });
    }

    /// `startActivity(new Intent(ACTION_VIEW, Uri.parse(url)))`.
    fn view(
        env: &mut jni::JNIEnv,
        activity: &jni::objects::JObject,
        url: &str,
    ) -> jni::errors::Result<()> {
        let url = env.new_string(url)?;
        let uri = env
            .call_static_method(
                "android/net/Uri",
                "parse",
                "(Ljava/lang/String;)Landroid/net/Uri;",
                &[(&url).into()],
            )?
            .l()?;
        let action = env.new_string("android.intent.action.VIEW")?;
        let intent = env.new_object(
            "android/content/Intent",
            "(Ljava/lang/String;Landroid/net/Uri;)V",
            &[(&action).into(), (&uri).into()],
        )?;
        env.call_method(
            activity,
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[(&intent).into()],
        )?;
        Ok(())
    }

    #[unsafe(no_mangle)]
    fn android_main(app: AndroidApp) {
        let data_dir = app.internal_data_path();
        init_logs(data_dir.clone());
        let last_crash = last_crash(data_dir.as_deref());
        if let Err(e) = slint::android::init(app.clone()) {
            logcat(6, &format!("cannot start the interface: {e}"));
            return;
        }
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("tm-worker")
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                logcat(6, &format!("cannot start the runtime: {e}"));
                return;
            }
        };
        let opts = tm_ui::Options {
            data_dir,
            remote_only: true,
            last_crash,
            ..Default::default()
        };
        if let Err(e) = tm_ui::run(
            runtime.handle().clone(),
            opts,
            Box::new(Phone {
                app,
                enabling: Cell::new(false),
                device: OnceCell::new(),
            }),
        ) {
            logcat(6, &format!("interface stopped: {e:#}"));
        }
        runtime.shutdown_timeout(std::time::Duration::from_secs(2));
        // The activity is gone. Android may start a new one in this same process, and
        // the interface cannot be set up twice: end the process so it starts afresh.
        std::process::exit(0);
    }
}
