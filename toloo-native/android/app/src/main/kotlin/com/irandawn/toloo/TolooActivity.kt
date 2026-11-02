package com.irandawn.toloo

import android.app.NativeActivity
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen

class TolooActivity : NativeActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        val splash = installSplashScreen()
        super.onCreate(savedInstanceState)
        splash.setKeepOnScreenCondition { !ready }
        Handler(Looper.getMainLooper()).postDelayed(
            { ready = true },
            SAFETY_RELEASE_MS,
        )
    }

    companion object {
        private const val SAFETY_RELEASE_MS = 1500L

        @Volatile
        private var ready = false

        @JvmStatic
        fun nativeReady() {
            ready = true
        }
    }
}
