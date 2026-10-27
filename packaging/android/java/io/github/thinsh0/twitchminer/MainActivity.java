package io.github.thinsh0.twitchminer;

import android.Manifest;
import android.app.NativeActivity;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.Settings;

import com.google.firebase.FirebaseApp;
import com.google.firebase.messaging.FirebaseMessaging;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;

/**
 * NativeActivity running the Rust interface, plus what needs Java. The public methods
 * are called from Rust (apps/android) and never block: results are read back by
 * polling ("" or null while waiting).
 */
public class MainActivity extends NativeActivity {
    /** Notification channels, named by the server in each message (notify/fcm.rs). */
    static final String CHANNEL_URGENT = "urgent";
    static final String CHANNEL_EVENTS = "events";

    private static final int ASK_NOTIFY = 1;
    private static final int PICK_FILE = 2;
    /** Largest file read by the chooser (a service account key is ~2.4 KB). */
    private static final int MAX_FILE = 64 * 1024;

    /** This app's Firebase registration token, once known (also set by PushService). */
    static volatile String token = "";
    /** Outcome of pushEnable: "" while waiting, "ok", or "error\n<reason>". */
    private volatile String push = "";
    /** Outcome of pickFile: null while choosing, "ok\n<content>" or "error\n<reason>". */
    private volatile String picked;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        createChannels(this);
        if (firebase()) {
            fetchToken(false);
        }
    }

    static void createChannels(Context context) {
        NotificationManager nm = context.getSystemService(NotificationManager.class);
        if (nm == null) {
            return;
        }
        NotificationChannel urgent = new NotificationChannel(
                CHANNEL_URGENT, "Important", NotificationManager.IMPORTANCE_HIGH);
        urgent.setDescription("Account to reconnect, drops at risk, game account to link");
        nm.createNotificationChannel(urgent);
        NotificationChannel events = new NotificationChannel(
                CHANNEL_EVENTS, "Events", NotificationManager.IMPORTANCE_DEFAULT);
        events.setDescription("Drops claimed, predictions and the other chosen events");
        nm.createNotificationChannel(events);
    }

    /** The APK was built with a Firebase configuration (google-services.json). */
    private boolean firebase() {
        try {
            return !FirebaseApp.getApps(this).isEmpty();
        } catch (Throwable e) {
            return false;
        }
    }

    // ---- Called from Rust ----

    /** Identifies this phone for the server (kept across reinstalls); "" without Firebase. */
    public String pushDevice() {
        if (!firebase()) {
            return "";
        }
        String id = Settings.Secure.getString(getContentResolver(), Settings.Secure.ANDROID_ID);
        return id == null ? "" : id;
    }

    /** The phone's model, shown in the destinations. */
    public String pushName() {
        String maker = Build.MANUFACTURER == null ? "" : Build.MANUFACTURER;
        String model = Build.MODEL == null ? "" : Build.MODEL;
        if (model.toLowerCase().startsWith(maker.toLowerCase()) || maker.isEmpty()) {
            return model;
        }
        return Character.toUpperCase(maker.charAt(0)) + maker.substring(1) + " " + model;
    }

    public String pushToken() {
        return token;
    }

    /** Asks for the permission to notify (Android 13+), then for the token. */
    public void pushEnable() {
        push = "";
        if (!firebase()) {
            push = "error\nunavailable";
            return;
        }
        if (Build.VERSION.SDK_INT >= 33
                && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                        != PackageManager.PERMISSION_GRANTED) {
            runOnUiThread(() -> requestPermissions(
                    new String[] {Manifest.permission.POST_NOTIFICATIONS}, ASK_NOTIFY));
        } else {
            permitted();
        }
    }

    /** "" while waiting, "ok\n<token>" or "error\n<reason>". */
    public String pushState() {
        String p = push;
        return p.equals("ok") ? "ok\n" + token : p;
    }

    @Override
    public void onRequestPermissionsResult(int code, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(code, permissions, results);
        if (code != ASK_NOTIFY) {
            return;
        }
        if (results.length > 0 && results[0] == PackageManager.PERMISSION_GRANTED) {
            permitted();
        } else {
            push = "error\ndenied";
        }
    }

    private void permitted() {
        NotificationManager nm = getSystemService(NotificationManager.class);
        if (nm != null && !nm.areNotificationsEnabled()) {
            push = "error\ndenied";
            return;
        }
        fetchToken(true);
    }

    private void fetchToken(boolean report) {
        try {
            FirebaseMessaging.getInstance().getToken().addOnCompleteListener(task -> {
                if (task.isSuccessful() && task.getResult() != null) {
                    token = task.getResult();
                    if (report) {
                        push = "ok";
                    }
                } else if (report) {
                    Exception e = task.getException();
                    push = "error\n" + (e == null ? "no token" : e.getMessage());
                }
            });
        } catch (RuntimeException e) {
            if (report) {
                push = "error\n" + e.getMessage();
            }
        }
    }

    /** Opens the system's file chooser; the content is then given by pickedFile. */
    public boolean pickFile() {
        picked = null;
        runOnUiThread(() -> {
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("*/*");
            try {
                startActivityForResult(intent, PICK_FILE);
            } catch (RuntimeException e) {
                picked = "error\n" + e.getMessage();
            }
        });
        return true;
    }

    /** null while choosing, then "ok\n<content>" or "error\n<reason>" (given once). */
    public String pickedFile() {
        String p = picked;
        if (p != null) {
            picked = null;
        }
        return p;
    }

    @Override
    protected void onActivityResult(int code, int result, Intent data) {
        super.onActivityResult(code, result, data);
        if (code != PICK_FILE) {
            return;
        }
        Uri uri = data == null ? null : data.getData();
        if (result != RESULT_OK || uri == null) {
            picked = "error\ncancelled";
            return;
        }
        try (InputStream in = getContentResolver().openInputStream(uri)) {
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            byte[] buf = new byte[8192];
            int n;
            while (in != null && (n = in.read(buf)) > 0) {
                out.write(buf, 0, n);
                if (out.size() > MAX_FILE) {
                    picked = "error\nfile too large";
                    return;
                }
            }
            picked = "ok\n" + new String(out.toByteArray(), StandardCharsets.UTF_8);
        } catch (Exception e) {
            picked = "error\n" + e.getMessage();
        }
    }
}
