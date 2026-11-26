//! Bounded, local Android diagnostics. No device identifiers or network I/O.
//!
//! Java samples platform APIs off the UI thread and publishes JSON through JNI.
//! The render thread only copies a newer validated snapshot; missing readings
//! remain `None`, rather than being mistaken for a cool or powerful device.

use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
};
use std::time::Instant;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const DEVICE_JSON_LIMIT: usize = 16 * 1024;
pub const PERF_REPORT_FILENAME: &str = "perf-report.json";
const MAX_TEXT_CHARS: usize = 96;
const MAX_REFRESH_RATES: usize = 64;
const MAX_MEMORY_BYTES: u64 = 16 * 1024 * 1024 * 1024 * 1024;

/// Platform observations, not benchmark results or a phone-model rating.
/// PSS is in KiB; system memory is bytes. Headroom is dimensionless and 1.0
/// means the platform's severe-throttling threshold, **not** a temperature.
#[derive(Resource, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct DeviceSnapshot {
    pub schema_version: u32,
    pub platform: String,
    pub manufacturer: String,
    pub model: String,
    pub device: String,
    pub sdk_int: Option<u32>,
    pub total_memory_bytes: Option<u64>,
    pub available_memory_bytes: Option<u64>,
    pub low_ram: Option<bool>,
    pub low_memory: Option<bool>,
    pub process_pss_kb: Option<u64>,
    pub process_pss_age_ms: Option<u64>,
    pub current_refresh_hz: Option<f32>,
    /// Only modes with the current display's physical width and height.
    pub supported_refresh_hz: Vec<f32>,
    /// Android PowerManager constants 0..=6; unknown is `None`.
    pub thermal_status: Option<u8>,
    pub thermal_headroom: Option<f32>,
    pub thermal_headroom_age_ms: Option<u64>,
    pub power_save: Option<bool>,
    pub foreground: Option<bool>,
    pub media_volume: Option<u32>,
    pub media_max_volume: Option<u32>,
    pub media_muted: Option<bool>,
    pub sampled_at_elapsed_ms: Option<u64>,
    pub requested_frame_rate: u32,
    pub applied_refresh_hint_hz: Option<f32>,
    #[serde(skip_deserializing)]
    pub reported: bool,
    #[serde(skip_deserializing)]
    pub revision: u64,
    #[serde(skip_deserializing)]
    pub sample_age_secs: f32,
}

impl Default for DeviceSnapshot {
    fn default() -> Self {
        Self {
            schema_version: 1,
            platform: std::env::consts::OS.into(),
            manufacturer: String::new(),
            model: String::new(),
            device: String::new(),
            sdk_int: None,
            total_memory_bytes: None,
            available_memory_bytes: None,
            low_ram: None,
            low_memory: None,
            process_pss_kb: None,
            process_pss_age_ms: None,
            current_refresh_hz: None,
            supported_refresh_hz: Vec::new(),
            thermal_status: None,
            thermal_headroom: None,
            thermal_headroom_age_ms: None,
            power_save: None,
            foreground: None,
            media_volume: None,
            media_max_volume: None,
            media_muted: None,
            sampled_at_elapsed_ms: None,
            requested_frame_rate: 0,
            applied_refresh_hint_hz: None,
            reported: false,
            revision: 0,
            sample_age_secs: 0.0,
        }
    }
}

impl DeviceSnapshot {
    /// A bridge result older than ten seconds is not a current observation.
    /// A paused activity stops polling and sends one final foreground=false.
    pub fn is_fresh(&self) -> bool {
        self.reported && self.sample_age_secs.is_finite() && self.sample_age_secs <= 10.0
    }

    pub fn thermal_label(&self) -> &'static str {
        match self.thermal_status {
            Some(0) => "none",
            Some(1) => "light",
            Some(2) => "moderate",
            Some(3) => "severe",
            Some(4) => "critical",
            Some(5) => "emergency",
            Some(6) => "shutdown",
            _ => "unknown",
        }
    }
}

struct PublishedSnapshot {
    snapshot: DeviceSnapshot,
    received: Instant,
}

static SNAPSHOT: OnceLock<Mutex<Option<PublishedSnapshot>>> = OnceLock::new();
static REVISION: AtomicU64 = AtomicU64::new(0);
static REQUESTED_FRAME_RATE: AtomicU32 = AtomicU32::new(0);
static SHARE_REQUESTED: AtomicBool = AtomicBool::new(false);
static SHARE_STATUS: AtomicU8 = AtomicU8::new(0);

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeviceSet;

pub struct DevicePlugin;

impl Plugin for DevicePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DeviceSnapshot>()
            .add_systems(PreUpdate, receive_snapshot.in_set(DeviceSet));
    }
}

fn receive_snapshot(mut device: ResMut<DeviceSnapshot>) {
    let Some(store) = SNAPSHOT.get() else {
        return;
    };
    // Never block a frame on the Android diagnostics worker.
    let Ok(published) = store.try_lock() else {
        return;
    };
    let Some(published) = published.as_ref() else {
        return;
    };
    let age = published.received.elapsed().as_secs_f32();
    if device.revision != published.snapshot.revision {
        *device = published.snapshot.clone();
        device.sample_age_secs = age;
    } else if (device.sample_age_secs - age).abs() >= 0.5 {
        // No per-frame Resource change merely for a diagnostic clock.
        device.sample_age_secs = age;
    }
}

/// Called only by the Android JNI bridge. A rejected payload leaves the last
/// valid observation intact. Parsing occurs on the Android worker, never in a
/// Bevy update system.
pub fn publish_android_snapshot(json: &str) -> Result<(), &'static str> {
    let mut snapshot = parse_android_snapshot(json)?;
    let store = SNAPSHOT.get_or_init(|| Mutex::new(None));
    let Ok(mut published) = store.lock() else {
        return Err("device bridge lock unavailable");
    };
    snapshot.reported = true;
    snapshot.revision = REVISION.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    *published = Some(PublishedSnapshot {
        snapshot,
        received: Instant::now(),
    });
    Ok(())
}

fn parse_android_snapshot(json: &str) -> Result<DeviceSnapshot, &'static str> {
    if json.len() > DEVICE_JSON_LIMIT {
        return Err("device payload too large");
    }
    let mut snapshot: DeviceSnapshot =
        serde_json::from_str(json).map_err(|_| "invalid device JSON")?;
    if snapshot.schema_version != 1 || snapshot.platform != "android" {
        return Err("unsupported device schema or platform");
    }
    for value in [&snapshot.manufacturer, &snapshot.model, &snapshot.device] {
        if value.chars().count() > MAX_TEXT_CHARS || value.chars().any(char::is_control) {
            return Err("invalid device label");
        }
    }
    if snapshot.supported_refresh_hz.len() > MAX_REFRESH_RATES {
        return Err("too many display modes");
    }
    snapshot.sdk_int = snapshot.sdk_int.filter(|v| (1..=1000).contains(v));
    snapshot.total_memory_bytes = snapshot
        .total_memory_bytes
        .filter(|v| (1..=MAX_MEMORY_BYTES).contains(v));
    snapshot.available_memory_bytes = snapshot
        .available_memory_bytes
        .filter(|v| *v <= MAX_MEMORY_BYTES);
    if let (Some(total), Some(available)) =
        (snapshot.total_memory_bytes, snapshot.available_memory_bytes)
        && available > total
    {
        snapshot.available_memory_bytes = None;
    }
    snapshot.process_pss_kb = snapshot
        .process_pss_kb
        .filter(|v| (1..=MAX_MEMORY_BYTES / 1024).contains(v));
    snapshot.current_refresh_hz = snapshot.current_refresh_hz.filter(valid_refresh);
    snapshot.applied_refresh_hint_hz = snapshot.applied_refresh_hint_hz.filter(valid_refresh);
    snapshot.supported_refresh_hz.retain(valid_refresh);
    snapshot.supported_refresh_hz.sort_by(f32::total_cmp);
    snapshot
        .supported_refresh_hz
        .dedup_by(|a, b| (*a - *b).abs() < 0.1);
    snapshot.thermal_status = snapshot.thermal_status.filter(|v| *v <= 6);
    snapshot.thermal_headroom = snapshot
        .thermal_headroom
        .filter(|v| v.is_finite() && (0.0..=1000.0).contains(v));
    snapshot.media_max_volume = snapshot.media_max_volume.filter(|v| *v <= 1000);
    snapshot.media_volume = snapshot.media_volume.filter(|v| {
        *v <= 1000
            && snapshot
                .media_max_volume
                .is_none_or(|maximum| *v <= maximum)
    });
    snapshot.requested_frame_rate = normalize_frame_rate_hint(snapshot.requested_frame_rate);
    Ok(snapshot)
}

fn valid_refresh(value: &f32) -> bool {
    value.is_finite() && (1.0..=1000.0).contains(value)
}

fn normalize_frame_rate_hint(fps: u32) -> u32 {
    if fps == 0 { 0 } else { fps.clamp(30, 240) }
}

/// A best-effort Android display preference. It neither paces application
/// frames nor guarantees the OS will select this refresh rate. `0` releases it.
pub fn request_frame_rate_hint(fps: u32) {
    REQUESTED_FRAME_RATE.store(normalize_frame_rate_hint(fps), Ordering::Relaxed);
}

pub fn requested_frame_rate_hint() -> u32 {
    REQUESTED_FRAME_RATE.load(Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportShareStatus {
    Idle,
    Pending,
    ChooserOpened,
    FileMissing,
    Unavailable,
    Failed,
}

/// Ask the Android foreground activity to open its system share chooser for
/// app-private `perf-report.json`. This never chooses a recipient or sends data.
pub fn request_report_share() -> bool {
    let ready = SNAPSHOT
        .get()
        .and_then(|store| store.lock().ok())
        .and_then(|published| {
            published.as_ref().map(|value| {
                value.snapshot.foreground == Some(true) && value.received.elapsed().as_secs() <= 10
            })
        })
        .unwrap_or(false);
    if !ready {
        SHARE_STATUS.store(4, Ordering::Relaxed);
        return false;
    }
    SHARE_STATUS.store(1, Ordering::Release);
    SHARE_REQUESTED.store(true, Ordering::Release);
    true
}

/// JNI main-thread polling consumes a request once; it is never re-sent on
/// lifecycle changes or on cancellation of the Android chooser.
pub fn take_report_share_request() -> bool {
    SHARE_REQUESTED.swap(false, Ordering::AcqRel)
}

/// `2` chooser opened, `3` missing file, `4` unavailable, `5` other failure.
pub fn publish_report_share_result(status: u8) {
    SHARE_STATUS.store(
        if (2..=5).contains(&status) { status } else { 5 },
        Ordering::Release,
    );
}

pub fn report_share_status() -> ReportShareStatus {
    match SHARE_STATUS.load(Ordering::Acquire) {
        0 => ReportShareStatus::Idle,
        1 => ReportShareStatus::Pending,
        2 => ReportShareStatus::ChooserOpened,
        3 => ReportShareStatus::FileMissing,
        4 => ReportShareStatus::Unavailable,
        _ => ReportShareStatus::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_readings_are_unknown_not_cool_or_low_memory() {
        let value = parse_android_snapshot(r#"{"platform":"android"}"#).unwrap();
        assert_eq!(value.thermal_status, None);
        assert_eq!(value.thermal_label(), "unknown");
        assert_eq!(value.power_save, None);
        assert_eq!(value.low_ram, None);
        assert!(!value.reported);
        assert!(!value.is_fresh());
    }

    #[test]
    fn malformed_payloads_cannot_replace_platform_observations() {
        assert!(parse_android_snapshot("{").is_err());
        assert!(parse_android_snapshot(r#"{"platform":"desktop"}"#).is_err());
        assert!(parse_android_snapshot(r#"{"platform":"android","schema_version":2}"#).is_err());
        assert!(parse_android_snapshot(&" ".repeat(DEVICE_JSON_LIMIT + 1)).is_err());
        let long = serde_json::json!({"platform":"android","model":"x".repeat(97)});
        assert!(parse_android_snapshot(&long.to_string()).is_err());
        assert!(parse_android_snapshot(r#"{"platform":"android","model":"bad\nlog"}"#).is_err());
    }

    #[test]
    fn bogus_optional_values_become_unknown_and_rates_are_bounded() {
        let value = parse_android_snapshot(
            r#"{"platform":"android","thermal_status":7,"thermal_headroom":-1,
                "current_refresh_hz":0,"supported_refresh_hz":[120,60,60.01,-1,9000],
                "total_memory_bytes":1024,"available_memory_bytes":2048,
                "media_volume":16,"media_max_volume":15}"#,
        )
        .unwrap();
        assert_eq!(value.thermal_status, None);
        assert_eq!(value.thermal_headroom, None);
        assert_eq!(value.current_refresh_hz, None);
        assert_eq!(value.supported_refresh_hz, vec![60.0, 120.0]);
        assert_eq!(value.available_memory_bytes, None);
        assert_eq!(value.media_volume, None);
        assert_eq!(value.media_max_volume, Some(15));
    }

    #[test]
    fn headroom_above_severe_and_zero_media_volume_are_valid_observations() {
        let value = parse_android_snapshot(
            r#"{"platform":"android","thermal_status":4,"thermal_headroom":1.3,
                "media_volume":0,"media_max_volume":15,"media_muted":true}"#,
        )
        .unwrap();
        assert_eq!(value.thermal_label(), "critical");
        assert_eq!(value.thermal_headroom, Some(1.3));
        assert_eq!(value.media_volume, Some(0));
        assert_eq!(value.media_muted, Some(true));
    }

    #[test]
    fn stale_snapshots_are_not_current_thermal_evidence() {
        let mut value = DeviceSnapshot {
            reported: true,
            sample_age_secs: 9.0,
            ..default()
        };
        assert!(value.is_fresh());
        value.sample_age_secs = 10.1;
        assert!(!value.is_fresh());
        value.sample_age_secs = f32::NAN;
        assert!(!value.is_fresh());
    }

    #[test]
    fn refresh_preferences_support_release_and_bounded_phone_targets() {
        assert_eq!(normalize_frame_rate_hint(0), 0);
        assert_eq!(normalize_frame_rate_hint(1), 30);
        assert_eq!(normalize_frame_rate_hint(60), 60);
        assert_eq!(normalize_frame_rate_hint(120), 120);
        assert_eq!(normalize_frame_rate_hint(u32::MAX), 240);
    }
}
