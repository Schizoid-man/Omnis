package com.omnis.desktop

import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import com.google.firebase.messaging.FirebaseMessaging
import org.json.JSONObject
import java.io.File

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    fetchAndSaveFcmToken()
  }

  private fun fetchAndSaveFcmToken() {
    FirebaseMessaging.getInstance().token.addOnCompleteListener { task ->
      if (!task.isSuccessful) return@addOnCompleteListener
      val token = task.result ?: return@addOnCompleteListener
      try {
        val file = File(filesDir, "fcm_token.json")
        file.writeText(JSONObject().put("token", token).toString())
      } catch (_: Exception) {
        // best effort
      }
    }
  }
}
