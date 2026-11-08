package com.irandawn.toloo

import android.Manifest
import android.app.NativeActivity
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import java.lang.ref.WeakReference

class TolooActivity : NativeActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        startedAt = SystemClock.elapsedRealtime()
        activity = WeakReference(this)
        val splash = installSplashScreen()
        super.onCreate(savedInstanceState)
        splash.setKeepOnScreenCondition { !ready }
        Handler(Looper.getMainLooper()).postDelayed(
            { ready = true },
            SAFETY_RELEASE_MS,
        )
        createNotificationChannel(this)
        Log.i(TAG, "onCreate")
    }

    override fun onResume() {
        super.onResume()
        foreground = true
    }

    override fun onPause() {
        foreground = false
        super.onPause()
    }

    companion object {
        private const val TAG = "Toloo"
        private const val SAFETY_RELEASE_MS = 1500L
        private const val CHANNEL_ID = "toloo_messages"
        private const val NOTIFICATION_REQUEST = 1001

        @Volatile
        private var ready = false

        @Volatile
        private var foreground = false

        private var notificationId = 1

        private var startedAt = 0L

        private var activity: WeakReference<TolooActivity>? = null

        private fun createNotificationChannel(context: Context) {
            val manager =
                context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            val channel = NotificationChannel(
                CHANNEL_ID,
                "Messages",
                NotificationManager.IMPORTANCE_DEFAULT,
            ).apply {
                description = "New Toloo messages"
            }
            manager.createNotificationChannel(channel)
        }

        @JvmStatic
        fun requestNotifications() {
            val current = activity?.get() ?: return
            Handler(Looper.getMainLooper()).post {
                createNotificationChannel(current)
                if (
                    Build.VERSION.SDK_INT >= 33 &&
                    current.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) !=
                    PackageManager.PERMISSION_GRANTED
                ) {
                    current.requestPermissions(
                        arrayOf(Manifest.permission.POST_NOTIFICATIONS),
                        NOTIFICATION_REQUEST,
                    )
                }
            }
        }

        @JvmStatic
        fun notifyMessage(title: String, body: String) {
            val current = activity?.get() ?: return
            Handler(Looper.getMainLooper()).post {
                if (foreground) return@post
                if (
                    Build.VERSION.SDK_INT >= 33 &&
                    current.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) !=
                    PackageManager.PERMISSION_GRANTED
                ) {
                    return@post
                }
                val manager =
                    current.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
                val intent = Intent(current, TolooActivity::class.java)
                val pending = PendingIntent.getActivity(
                    current,
                    0,
                    intent,
                    PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                )
                val notification = Notification.Builder(current, CHANNEL_ID)
                    .setSmallIcon(R.mipmap.ic_launcher)
                    .setContentTitle(title)
                    .setContentText(body)
                    .setStyle(Notification.BigTextStyle().bigText(body))
                    .setContentIntent(pending)
                    .setAutoCancel(true)
                    .build()
                manager.notify(notificationId++, notification)
            }
        }

        @JvmStatic
        fun nativeReady() {
            ready = true
            Log.i(TAG, "first frame after ${SystemClock.elapsedRealtime() - startedAt}ms")
        }

        @JvmStatic
        fun contentReady() {
            activity?.get()?.reportFullyDrawn()
            Log.i(TAG, "content ready after ${SystemClock.elapsedRealtime() - startedAt}ms")
        }

        @JvmStatic
        fun startRelayService(address: String) {
            val current = activity?.get() ?: return
            Handler(Looper.getMainLooper()).post {
                val intent = Intent(current, RelayHostService::class.java)
                    .putExtra("address", address)
                current.startForegroundService(intent)
            }
        }

        @JvmStatic
        fun stopRelayService() {
            val current = activity?.get() ?: return
            Handler(Looper.getMainLooper()).post {
                current.stopService(Intent(current, RelayHostService::class.java))
            }
        }

        @JvmStatic
        fun copyToClipboard(text: String) {
            val current = activity?.get() ?: return
            Handler(Looper.getMainLooper()).post {
                val clipboard =
                    current.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                clipboard.setPrimaryClip(ClipData.newPlainText("Toloo", text))
            }
        }

        @JvmStatic
        fun shareText(text: String, title: String) {
            val current = activity?.get() ?: return
            Handler(Looper.getMainLooper()).post {
                val send = Intent(Intent.ACTION_SEND).apply {
                    type = "text/plain"
                    putExtra(Intent.EXTRA_TEXT, text)
                    putExtra(Intent.EXTRA_TITLE, title)
                }
                current.startActivity(Intent.createChooser(send, title))
            }
        }
    }
}
