package com.omnis.desktop

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import org.json.JSONObject
import java.io.File

class OmnisFcmService : FirebaseMessagingService() {

    override fun onNewToken(token: String) {
        super.onNewToken(token)
        saveToken(token)
    }

    override fun onMessageReceived(remoteMessage: RemoteMessage) {
        // Server sends wake-only pushes (no payload content).
        // The app polls for new messages on resume; nothing to do here.
    }

    private fun saveToken(token: String) {
        try {
            val file = File(filesDir, "fcm_token.json")
            file.writeText(JSONObject().put("token", token).toString())
        } catch (_: Exception) {
            // best effort
        }
    }
}
