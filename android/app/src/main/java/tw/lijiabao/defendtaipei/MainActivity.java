package tw.lijiabao.defendtaipei;

import android.app.ActivityManager;
import android.content.ClipData;
import android.content.Intent;
import android.media.AudioManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Debug;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.Looper;
import android.os.PowerManager;
import android.os.SystemClock;
import android.util.Log;
import android.view.Display;
import android.view.View;
import android.view.WindowManager;

import androidx.core.content.FileProvider;

import com.google.androidgamesdk.GameActivity;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.Collections;

/** GameActivity lifecycle and local platform diagnostics; gameplay remains in Rust. */
public class MainActivity extends GameActivity {
    private static final long SNAPSHOT_INTERVAL_MS = 2_000;
    // Compatible with the conservative Android game Thermal API guidance.
    // Do not poll more often when a listener or lifecycle event fires.
    private static final long HEAVY_SAMPLE_INTERVAL_MS = 10_000;
    private static final long MAX_REPORT_BYTES = 4_194_304;
    private static final String REPORT_NAME = "perf-report.json";

    static {
        System.loadLibrary("defend_taipei_android");
    }

    private static native void nativePublishDeviceSnapshot(String json);
    private static native int nativeRequestedFrameRate();
    private static native boolean nativeTakeReportShareRequest();
    private static native void nativeReportShareResult(int result);

    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private HandlerThread diagnosticsThread;
    private Handler diagnosticsHandler;
    private PowerManager powerManager;
    private ActivityManager activityManager;
    private AudioManager audioManager;
    private volatile boolean resumed;
    private volatile boolean focused;
    private volatile boolean destroyed;
    private volatile Integer thermalStatus;
    private volatile float appliedRefreshHint;
    private boolean thermalListenerRegistered;
    // The following samples belong to the diagnostics worker only.
    private long lastHeavySampleMs = -HEAVY_SAMPLE_INTERVAL_MS;
    private Long pssKb;
    private Float thermalHeadroom;

    private final PowerManager.OnThermalStatusChangedListener thermalListener = status -> {
        thermalStatus = validThermalStatus(status);
        publishSoon(false);
    };

    private final Runnable monitor = new Runnable() {
        @Override
        public void run() {
            if (destroyed || !resumed) {
                return;
            }
            publishDeviceSnapshot(true);
            mainHandler.post(() -> {
                if (destroyed) {
                    return;
                }
                applyRefreshPreference(resumed && focused ? nativeRequestedFrameRate() : 0);
                if (resumed && focused && nativeTakeReportShareRequest()) {
                    openReportChooser();
                }
            });
            if (!destroyed && resumed) {
                diagnosticsHandler.postDelayed(this, SNAPSHOT_INTERVAL_MS);
            }
        }
    };

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        powerManager = getSystemService(PowerManager.class);
        activityManager = getSystemService(ActivityManager.class);
        audioManager = getSystemService(AudioManager.class);
        diagnosticsThread = new HandlerThread("DefendTaipei-device");
        diagnosticsThread.start();
        diagnosticsHandler = new Handler(diagnosticsThread.getLooper());
        if (powerManager != null) {
            try {
                thermalStatus = validThermalStatus(powerManager.getCurrentThermalStatus());
                powerManager.addThermalStatusListener(getMainExecutor(), thermalListener);
                thermalListenerRegistered = true;
            } catch (RuntimeException unavailable) {
                thermalStatus = null;
                Log.i("DTDEVICE", "thermal_listener=unavailable");
            }
        }
        publishSoon(false);
    }

    @Override
    protected void onResume() {
        super.onResume();
        resumed = true;
        if (diagnosticsHandler != null) {
            diagnosticsHandler.removeCallbacks(monitor);
            diagnosticsHandler.post(monitor);
        }
    }

    @Override
    protected void onPause() {
        resumed = false;
        if (diagnosticsHandler != null) {
            diagnosticsHandler.removeCallbacks(monitor);
        }
        publishSoon(false);
        applyRefreshPreference(0);
        super.onPause();
    }

    @Override
    protected void onDestroy() {
        destroyed = true;
        resumed = false;
        if (thermalListenerRegistered && powerManager != null) {
            try {
                powerManager.removeThermalStatusListener(thermalListener);
            } catch (RuntimeException ignored) {
                Log.i("DTDEVICE", "thermal_listener_cleanup=unavailable");
            }
        }
        if (diagnosticsHandler != null) {
            diagnosticsHandler.removeCallbacksAndMessages(null);
        }
        mainHandler.removeCallbacksAndMessages(null);
        if (diagnosticsThread != null) {
            diagnosticsThread.quitSafely();
        }
        super.onDestroy();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        focused = hasFocus;
        publishSoon(false);
        if (hasFocus) {
            hideSystemUi();
        }
    }

    private void publishSoon(boolean allowHeavySamples) {
        if (!destroyed && diagnosticsHandler != null) {
            diagnosticsHandler.post(() -> publishDeviceSnapshot(allowHeavySamples));
        }
    }

    private static Integer validThermalStatus(int value) {
        return value >= PowerManager.THERMAL_STATUS_NONE
                && value <= PowerManager.THERMAL_STATUS_SHUTDOWN ? value : null;
    }

    private static String label(String value) {
        if (value == null) {
            return "";
        }
        StringBuilder result = new StringBuilder();
        value.codePoints().limit(96).filter(point -> !Character.isISOControl(point))
                .forEach(result::appendCodePoint);
        return result.toString();
    }

    private static void put(JSONObject data, String key, Object value) throws JSONException {
        data.put(key, value == null ? JSONObject.NULL : value);
    }

    private void sampleHeavyValues(long now) {
        if (now - lastHeavySampleMs < HEAVY_SAMPLE_INTERVAL_MS) {
            return;
        }
        // Set the deadline before API calls so failure does not cause a hot retry loop.
        lastHeavySampleMs = now;
        pssKb = null;
        thermalHeadroom = null;
        try {
            long value = Debug.getPss();
            if (value > 0) {
                pssKb = value;
            }
        } catch (RuntimeException ignored) {
            // Unknown process PSS is not zero memory use.
        }
        if (powerManager != null) {
            try {
                float value = powerManager.getThermalHeadroom(0);
                if (Float.isFinite(value) && value >= 0) {
                    thermalHeadroom = value;
                }
            } catch (RuntimeException ignored) {
                // Unsupported thermal headroom is JSON null, never a fake temperature.
            }
        }
    }

    private void publishDeviceSnapshot(boolean allowHeavySamples) {
        if (destroyed) {
            return;
        }
        long now = SystemClock.elapsedRealtime();
        if (allowHeavySamples && resumed) {
            sampleHeavyValues(now);
        }
        try {
            JSONObject data = new JSONObject();
            data.put("schema_version", 1);
            data.put("platform", "android");
            data.put("manufacturer", label(Build.MANUFACTURER));
            data.put("model", label(Build.MODEL));
            // Build.DEVICE is the product codename, not a unique device identifier.
            data.put("device", label(Build.DEVICE));
            data.put("sdk_int", Build.VERSION.SDK_INT);
            data.put("sampled_at_elapsed_ms", now);
            data.put("foreground", resumed && focused);
            put(data, "thermal_status", thermalStatus);
            put(data, "thermal_headroom", thermalHeadroom);
            Long heavyAge = lastHeavySampleMs < 0 ? null : now - lastHeavySampleMs;
            put(data, "thermal_headroom_age_ms", heavyAge);
            put(data, "process_pss_kb", pssKb);
            put(data, "process_pss_age_ms", heavyAge);
            if (powerManager != null) {
                try {
                    data.put("power_save", powerManager.isPowerSaveMode());
                } catch (RuntimeException ignored) {
                    put(data, "power_save", null);
                }
                try {
                    // Status callbacks can be absent on some implementations.
                    thermalStatus = validThermalStatus(powerManager.getCurrentThermalStatus());
                } catch (RuntimeException ignored) {
                    // Do not re-stamp an old cached status as a fresh reading.
                    thermalStatus = null;
                }
                put(data, "thermal_status", thermalStatus);
            }
            if (activityManager != null) {
                try {
                    ActivityManager.MemoryInfo memory = new ActivityManager.MemoryInfo();
                    activityManager.getMemoryInfo(memory);
                    data.put("total_memory_bytes", memory.totalMem);
                    data.put("available_memory_bytes", memory.availMem);
                    data.put("low_memory", memory.lowMemory);
                    data.put("low_ram", activityManager.isLowRamDevice());
                } catch (RuntimeException ignored) {
                    // Omitted optional values decode as unknown.
                }
            }
            Display display = getDisplay();
            ArrayList<Float> rates = sameResolutionRates(display);
            JSONArray supported = new JSONArray();
            for (float rate : rates) {
                supported.put(rate);
            }
            data.put("supported_refresh_hz", supported);
            if (display != null) {
                float actual = display.getRefreshRate();
                if (Float.isFinite(actual) && actual > 0) {
                    data.put("current_refresh_hz", actual);
                }
            }
            data.put("requested_frame_rate", nativeRequestedFrameRate());
            put(data, "applied_refresh_hint_hz", appliedRefreshHint > 0 ? appliedRefreshHint : null);
            if (audioManager != null) {
                try {
                    data.put("media_volume", audioManager.getStreamVolume(AudioManager.STREAM_MUSIC));
                    data.put("media_max_volume", audioManager.getStreamMaxVolume(AudioManager.STREAM_MUSIC));
                    data.put("media_muted", audioManager.isStreamMute(AudioManager.STREAM_MUSIC));
                } catch (RuntimeException ignored) {
                    // OS volume readback is diagnostic only; never change the user's volume.
                }
            }
            nativePublishDeviceSnapshot(data.toString());
        } catch (JSONException | RuntimeException error) {
            // Broken vendor diagnostics must not take down the game or flood logcat.
            Log.i("DTDEVICE", "snapshot=unavailable reason=" + error.getClass().getSimpleName());
        }
    }

    private static ArrayList<Float> sameResolutionRates(Display display) {
        ArrayList<Float> rates = new ArrayList<>();
        if (display == null) {
            return rates;
        }
        Display.Mode current = display.getMode();
        for (Display.Mode mode : display.getSupportedModes()) {
            if (mode.getPhysicalWidth() != current.getPhysicalWidth()
                    || mode.getPhysicalHeight() != current.getPhysicalHeight()) {
                continue;
            }
            float rate = mode.getRefreshRate();
            if (!Float.isFinite(rate) || rate <= 0 || rate > 1000) {
                continue;
            }
            boolean duplicate = false;
            for (float existing : rates) {
                if (Math.abs(existing - rate) < 0.1f) {
                    duplicate = true;
                    break;
                }
            }
            if (!duplicate && rates.size() < 64) {
                rates.add(rate);
            }
        }
        Collections.sort(rates);
        return rates;
    }

    /** Select a supported same-resolution rate. Prefer the lowest exact multiple
     * of the desired app cadence, then the closest higher rate, then the maximum.
     * This is only an OS preference; Rust owns app pacing and actual diagnostics.
     */
    private void applyRefreshPreference(int desiredFps) {
        try {
            float chosen = 0;
            if (desiredFps > 0) {
                ArrayList<Float> rates = sameResolutionRates(getDisplay());
                for (float rate : rates) {
                    float multiple = rate / desiredFps;
                    if (rate + 0.5f >= desiredFps
                            && Math.abs(multiple - Math.round(multiple)) < 0.015f) {
                        chosen = rate;
                        break;
                    }
                }
                if (chosen == 0) {
                    for (float rate : rates) {
                        chosen = rate;
                        if (rate + 0.5f >= desiredFps) {
                            break;
                        }
                    }
                }
            }
            WindowManager.LayoutParams params = getWindow().getAttributes();
            if (Math.abs(params.preferredRefreshRate - chosen) > 0.1f) {
                // Do not request a mode ID or resolution change. All Android
                // versions we support accept this measured supported refresh.
                params.preferredRefreshRate = chosen;
                getWindow().setAttributes(params);
            }
            appliedRefreshHint = chosen;
        } catch (RuntimeException unavailable) {
            appliedRefreshHint = 0;
        }
    }

    private void openReportChooser() {
        if (destroyed || !resumed || !focused) {
            nativeReportShareResult(4);
            return;
        }
        File report = new File(getFilesDir(), REPORT_NAME);
        if (!report.isFile() || report.length() <= 0 || report.length() > MAX_REPORT_BYTES) {
            nativeReportShareResult(3);
            return;
        }
        try {
            // Restrict FileProvider to a report-only cache directory, never the
            // save/settings directory. The snapshot is stable while being shared.
            File directory = new File(getCacheDir(), "performance_reports");
            Files.createDirectories(directory.toPath());
            // Each chooser gets its own immutable snapshot. A later export does
            // not replace a file that another chosen app is still reading.
            File shared = File.createTempFile("perf-report-", ".json", directory);
            Files.copy(report.toPath(), shared.toPath(), StandardCopyOption.REPLACE_EXISTING);
            Uri uri = FileProvider.getUriForFile(this, getPackageName() + ".performance_reports", shared);
            Intent send = new Intent(Intent.ACTION_SEND);
            send.setType("application/json");
            send.putExtra(Intent.EXTRA_STREAM, uri);
            send.setClipData(ClipData.newRawUri("Defend Taipei performance report", uri));
            send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
            startActivity(Intent.createChooser(send, "分享效能報告 / Share performance report"));
            // A chooser was opened; this is deliberately not a "sent" result.
            nativeReportShareResult(2);
            Log.i("DTDEVICE", "report_share=chooser_opened uri=" + uri + " grant_read=1");
        } catch (IOException | RuntimeException failed) {
            nativeReportShareResult(5);
            Log.i("DTDEVICE", "report_share=failed reason=" + failed.getClass().getSimpleName());
        }
    }

    private void hideSystemUi() {
        View decorView = getWindow().getDecorView();
        decorView.setSystemUiVisibility(
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                        | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                        | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                        | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_FULLSCREEN
        );
    }
}
