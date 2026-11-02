package com.irandawn.toloo;

import android.app.NativeActivity;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;

import androidx.core.splashscreen.SplashScreen;

public class TolooActivity extends NativeActivity {
    private static final long SAFETY_RELEASE_MS = 1500;
    private static volatile boolean sNativeReady;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        SplashScreen splash = SplashScreen.installSplashScreen(this);
        super.onCreate(savedInstanceState);
        splash.setKeepOnScreenCondition(() -> !sNativeReady);
        new Handler(Looper.getMainLooper()).postDelayed(
                () -> sNativeReady = true, SAFETY_RELEASE_MS);
    }

    public static void nativeReady() {
        sNativeReady = true;
    }
}
