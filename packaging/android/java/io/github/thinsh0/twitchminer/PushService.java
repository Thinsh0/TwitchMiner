package io.github.thinsh0.twitchminer;

import android.app.Notification;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Intent;

import com.google.firebase.messaging.FirebaseMessagingService;
import com.google.firebase.messaging.RemoteMessage;

/**
 * Notifications pushed by the server. While the app is in the background Android shows
 * them by itself; this shows the ones arriving while it is open.
 */
public class PushService extends FirebaseMessagingService {
    @Override
    public void onNewToken(String token) {
        // The interface sees it changed and gives it to the server.
        MainActivity.token = token;
    }

    @Override
    public void onMessageReceived(RemoteMessage message) {
        RemoteMessage.Notification n = message.getNotification();
        if (n == null) {
            return;
        }
        MainActivity.createChannels(this);
        String channel = n.getChannelId() != null ? n.getChannelId() : MainActivity.CHANNEL_EVENTS;
        Intent open = new Intent(this, MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        PendingIntent tap = PendingIntent.getActivity(this, 0, open, PendingIntent.FLAG_IMMUTABLE);
        String body = n.getBody() == null ? "" : n.getBody();
        Notification shown = new Notification.Builder(this, channel)
                .setSmallIcon(R.drawable.ic_notification)
                .setColor(getColor(R.color.accent))
                .setContentTitle(n.getTitle() == null ? "TwitchMiner" : n.getTitle())
                .setContentText(body)
                .setStyle(new Notification.BigTextStyle().bigText(body))
                .setContentIntent(tap)
                .setAutoCancel(true)
                .build();
        NotificationManager nm = getSystemService(NotificationManager.class);
        if (nm != null) {
            nm.notify((int) System.currentTimeMillis(), shown);
        }
    }
}
