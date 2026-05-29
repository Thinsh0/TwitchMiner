package com.twitchminer.mobile

import android.content.Intent

class MainActivity : TauriActivity() {
    override fun onNewIntent(intent: Intent) {
        try {
            super.onNewIntent(intent)
        } catch (e: Exception) {
            android.util.Log.e("TwitchMiner", "Caught exception in onNewIntent: ${e.message}")
        }
    }
}