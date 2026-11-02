package com.irandawn.toloo;

import android.app.NativeActivity;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;

import androidx.core.splashscreen.SplashScreen;

public class TolooActivity extends NativeActivity {
    private static final long MIN_SPLASH_MS = 600;
    private static final long SAFETY_RELEASE_MS = 1500;
    private static volatile boolean sNativeReady;

    private final long startedAt = SystemClock.uptimeMillis();

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        SplashScreen splash = SplashScreen.installSplashScreen(this);
        super.onCreate(savedInstanceState);
        splash.setKeepOnScreenCondition(
                () -> !sNativeReady
                        || SystemClock.uptimeMillis() - startedAt < MIN_SPLASH_MS);
        new Handler(Looper.getMainLooper()).postDelayed(
                () -> sNativeReady = true, SAFETY_RELEASE_MS);
    }

    public static void nativeReady() {
        sNativeReady = true;
    }
}
