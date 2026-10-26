//! TwitchMiner on Android: the shared interface (`tm-ui`) in a NativeActivity, as a
//! client of a TwitchMiner running elsewhere (home server, PC). No engine runs on the
//! phone. Built into an APK by `packaging/android/build-apk.sh`.

#[cfg(target_os = "android")]
mod android {
    use std::ffi::{CString, c_char, c_int};
    use std::io::Write;

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

    fn init_logs() {
        let _ = tracing_subscriber::fmt()
            .with_writer(|| Logcat)
            .with_ansi(false)
            .without_time()
            .with_env_filter("info")
            .try_init();
        std::panic::set_hook(Box::new(|info| logcat(6, &format!("panic: {info}"))));
    }

    struct Phone {
        app: AndroidApp,
    }

    impl tm_ui::Platform for Phone {
        fn open_url(&self, url: &str) -> bool {
            match view(&self.app, url) {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!(error = %e, url, "cannot open the browser");
                    false
                }
            }
        }
    }

    /// `startActivity(new Intent(ACTION_VIEW, Uri.parse(url)))`.
    fn view(app: &AndroidApp, url: &str) -> jni::errors::Result<()> {
        // SAFETY: both pointers come from the running activity and stay valid while it
        // runs; the VM is never destroyed by this handle.
        let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast())? };
        let activity = unsafe { jni::objects::JObject::from_raw(app.activity_as_ptr().cast()) };
        let mut env = vm.attach_current_thread()?;
        let result = (|| {
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
                &activity,
                "startActivity",
                "(Landroid/content/Intent;)V",
                &[(&intent).into()],
            )?;
            Ok(())
        })();
        // No browser: ActivityNotFoundException must not stay pending.
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        result
    }

    #[unsafe(no_mangle)]
    fn android_main(app: AndroidApp) {
        init_logs();
        let data_dir = app.internal_data_path();
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
            ..Default::default()
        };
        if let Err(e) = tm_ui::run(runtime.handle().clone(), opts, Box::new(Phone { app })) {
            logcat(6, &format!("interface stopped: {e:#}"));
        }
        runtime.shutdown_timeout(std::time::Duration::from_secs(2));
    }
}
