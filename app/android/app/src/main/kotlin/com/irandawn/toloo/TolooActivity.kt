package com.irandawn.toloo

import android.Manifest
import android.app.Activity
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
    private var pendingSaveText: String? = null

    private external fun nativeOnFilePicked(text: String)
    private external fun nativeOnFileStatus(message: String)

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

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_OPEN_FILE && requestCode != REQUEST_SAVE_FILE) {
            return
        }
        if (resultCode != Activity.RESULT_OK) {
            pendingSaveText = null
            nativeOnFileStatus("File selection cancelled")
            return
        }
        val uri = data?.data
        if (uri == null) {
            pendingSaveText = null
            nativeOnFileStatus("No file selected")
            return
        }
        when (requestCode) {
            REQUEST_OPEN_FILE -> {
                try {
                    val input = contentResolver.openInputStream(uri)
                        ?: throw IllegalStateException("Could not open file")
                    input.use { stream ->
                        val buffer = ByteArray(MAX_FILE_BYTES + 1)
                        var total = 0
                        while (total < buffer.size) {
                            val read = stream.read(buffer, total, buffer.size - total)
                            if (read < 0) break
                            total += read
                        }
                        if (total > MAX_FILE_BYTES) {
                            nativeOnFileStatus("Selected file is too large")
                            return
                        }
                        nativeOnFilePicked(String(buffer, 0, total, Charsets.UTF_8))
                    }
                } catch (error: Exception) {
                    nativeOnFileStatus("Could not read file: ${error.message}")
                }
            }
            REQUEST_SAVE_FILE -> {
                val text = pendingSaveText
                pendingSaveText = null
                if (text == null) {
                    nativeOnFileStatus("Nothing to save")
                    return
                }
                try {
                    contentResolver.openOutputStream(uri)?.use { stream ->
                        stream.write(text.toByteArray(Charsets.UTF_8))
                    } ?: throw IllegalStateException("Could not open destination")
                    nativeOnFileStatus("File saved")
                } catch (error: Exception) {
                    nativeOnFileStatus("Could not save file: ${error.message}")
                }
            }
        }
    }

    companion object {
        private const val TAG = "Toloo"
        private const val SAFETY_RELEASE_MS = 1500L
        private const val CHANNEL_ID = "toloo_messages"
        private const val NOTIFICATION_REQUEST = 1001
        private const val REQUEST_OPEN_FILE = 2001
        private const val REQUEST_SAVE_FILE = 2002
        private const val MAX_FILE_BYTES = 8 * 1024 * 1024

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
        fun openTextFile() {
            val current = activity?.get() ?: return
            Handler(Looper.getMainLooper()).post {
                val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                    addCategory(Intent.CATEGORY_OPENABLE)
                    type = "*/*"
                }
                current.startActivityForResult(intent, REQUEST_OPEN_FILE)
            }
        }

        @JvmStatic
        fun saveTextFile(name: String, text: String) {
            val current = activity?.get() ?: return
            Handler(Looper.getMainLooper()).post {
                current.pendingSaveText = text
                val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                    addCategory(Intent.CATEGORY_OPENABLE)
                    type = "text/plain"
                    putExtra(Intent.EXTRA_TITLE, name)
                }
                current.startActivityForResult(intent, REQUEST_SAVE_FILE)
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
