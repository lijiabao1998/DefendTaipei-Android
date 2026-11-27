//! Android entry point. `#[bevy_main]` generates the `android_main` symbol that
//! GameActivity calls after `System.loadLibrary("defend_taipei_android")`.

use bevy::prelude::*;

#[bevy_main]
fn main() {
    defend_taipei::run();
}

/// These short callbacks run on Java-owned threads. No JNI references escape
/// the call, and a bad diagnostic never unwinds through the Android VM.
#[cfg(target_os = "android")]
mod device_bridge {
    use jni::{
        JNIEnv,
        objects::{JClass, JString},
        sys::{JNI_FALSE, JNI_TRUE, jboolean, jint},
    };

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_tw_lijiabao_defendtaipei_MainActivity_nativePublishDeviceSnapshot(
        mut env: JNIEnv<'_>,
        _class: JClass<'_>,
        value: JString<'_>,
    ) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Bound before materializing UTF-8 as well as in the shared parser.
            let len = env
                .call_method(&value, "length", "()I", &[])
                .and_then(|value| value.i());
            if !len.is_ok_and(|len| {
                len >= 0 && len as usize <= defend_taipei::device::DEVICE_JSON_LIMIT
            }) {
                return;
            }
            if let Ok(text) = env.get_string(&value) {
                let text: String = text.into();
                if let Err(reason) = defend_taipei::device::publish_android_snapshot(&text) {
                    eprintln!("DTDEVICE rejected reason={reason}");
                }
            }
        }));
        // A diagnostics allocation/read error must not crash the activity.
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_tw_lijiabao_defendtaipei_MainActivity_nativeRequestedFrameRate(
        _env: JNIEnv<'_>,
        _class: JClass<'_>,
    ) -> jint {
        defend_taipei::device::requested_frame_rate_hint() as jint
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_tw_lijiabao_defendtaipei_MainActivity_nativeTakeHapticRequest(
        _env: JNIEnv<'_>,
        _class: JClass<'_>,
    ) -> jint {
        defend_taipei::device::take_haptic_request() as jint
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_tw_lijiabao_defendtaipei_MainActivity_nativeClearHapticRequest(
        _env: JNIEnv<'_>,
        _class: JClass<'_>,
    ) {
        defend_taipei::device::clear_haptic_request();
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_tw_lijiabao_defendtaipei_MainActivity_nativeTakeReportShareRequest(
        _env: JNIEnv<'_>,
        _class: JClass<'_>,
    ) -> jboolean {
        if defend_taipei::device::take_report_share_request() {
            JNI_TRUE
        } else {
            JNI_FALSE
        }
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_tw_lijiabao_defendtaipei_MainActivity_nativeReportShareResult(
        _env: JNIEnv<'_>,
        _class: JClass<'_>,
        status: jint,
    ) {
        defend_taipei::device::publish_report_share_result(status.try_into().unwrap_or(5));
    }
}
