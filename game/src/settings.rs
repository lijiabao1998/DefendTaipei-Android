//! Device preferences are independent of the permadeath game slot.
//! Web reference: `src/ui/settings.ts`; day duration applies only to a new run.

use crate::{
    audio::AudioSettings, i18n::Language, input::ControlPreferences,
    performance::PerformancePreferences,
};
use bevy::{
    prelude::*,
    window::{AppLifecycle, WindowFocused},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GamePreferences {
    pub day_hours: u8,
}
impl Default for GamePreferences {
    fn default() -> Self {
        Self { day_hours: 1 }
    }
}

#[derive(Resource, Default, Clone, Debug)]
pub struct SettingsStatus {
    pub error: Option<String>,
    pub saved: bool,
}

#[derive(Message, Default)]
pub struct FlushSettings;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
struct StoredSettings {
    version: u32,
    audio: AudioSettings,
    day_hours: u8,
    language: String,
    performance: PerformancePreferences,
    controls: ControlPreferences,
}
impl Default for StoredSettings {
    fn default() -> Self {
        Self {
            version: 1,
            audio: AudioSettings::default(),
            day_hours: 1,
            language: "zh-TW".into(),
            performance: PerformancePreferences::default(),
            controls: ControlPreferences::default(),
        }
    }
}
impl StoredSettings {
    fn current(
        audio: &AudioSettings,
        preferences: &GamePreferences,
        language: Language,
        performance: &PerformancePreferences,
        controls: &ControlPreferences,
    ) -> Self {
        Self {
            audio: (*audio).sanitize(),
            performance: (*performance).sanitize(),
            controls: controls.clone().sanitize(),
            day_hours: if matches!(preferences.day_hours, 1 | 2 | 4) {
                preferences.day_hours
            } else {
                1
            },
            language: if language == Language::En {
                "en"
            } else {
                "zh-TW"
            }
            .into(),
            ..default()
        }
    }
    fn normalize(mut self) -> Self {
        self.audio = self.audio.sanitize();
        self.performance = self.performance.sanitize();
        self.controls = self.controls.sanitize();
        if !matches!(self.day_hours, 1 | 2 | 4) {
            self.day_hours = 1;
        }
        if self.language != "en" {
            self.language = "zh-TW".into();
        }
        self
    }
}

#[derive(Resource)]
struct SettingsLocation(Option<PathBuf>);
#[derive(Resource, Default)]
struct SaveState {
    observed: Option<StoredSettings>,
    written: Option<StoredSettings>,
    settled: f32,
}

pub struct SettingsPlugin;
impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioSettings>()
            .init_resource::<GamePreferences>()
            .init_resource::<PerformancePreferences>()
            .init_resource::<ControlPreferences>()
            .init_resource::<Language>()
            .init_resource::<SettingsStatus>()
            .init_resource::<SaveState>()
            .insert_resource(SettingsLocation(settings_path()))
            .add_message::<FlushSettings>()
            .add_message::<WindowFocused>()
            .add_message::<AppLifecycle>()
            .add_systems(Startup, load_settings)
            .add_systems(Last, persist_settings);
    }
}

fn load_settings(
    location: Res<SettingsLocation>,
    mut audio: ResMut<AudioSettings>,
    mut preferences: ResMut<GamePreferences>,
    mut performance: ResMut<PerformancePreferences>,
    mut controls: ResMut<ControlPreferences>,
    mut language: ResMut<Language>,
    mut status: ResMut<SettingsStatus>,
    mut state: ResMut<SaveState>,
) {
    match location.0.as_ref().map(|path| read_settings(path)) {
        Some(Ok(Some(value))) => {
            *audio = value.audio;
            preferences.day_hours = value.day_hours;
            *performance = value.performance;
            *controls = value.controls;
            *language = if value.language == "en" {
                Language::En
            } else {
                Language::ZhTw
            };
        }
        Some(Ok(None)) => {}
        Some(Err(error)) => status.error = Some(format!("Settings could not be read: {error}")),
        None => status.error = Some("Settings directory is unavailable".into()),
    }
    let value = StoredSettings::current(&audio, &preferences, *language, &performance, &controls);
    state.observed = Some(value.clone());
    state.written = Some(value);
}

fn persist_settings(
    time: Res<Time<Real>>,
    location: Res<SettingsLocation>,
    audio: Res<AudioSettings>,
    preferences: Res<GamePreferences>,
    performance: Res<PerformancePreferences>,
    controls: Res<ControlPreferences>,
    language: Res<Language>,
    mut status: ResMut<SettingsStatus>,
    mut state: ResMut<SaveState>,
    mut flush: MessageReader<FlushSettings>,
    mut focus: MessageReader<WindowFocused>,
    mut exits: MessageReader<AppExit>,
    mut lifecycle: MessageReader<AppLifecycle>,
) {
    let forced = !flush.is_empty()
        || focus.read().any(|event| !event.focused)
        || !exits.is_empty()
        || lifecycle
            .read()
            .any(|event| matches!(event, AppLifecycle::WillSuspend | AppLifecycle::Suspended));
    flush.clear();
    focus.clear();
    exits.clear();
    lifecycle.clear();
    let current = StoredSettings::current(&audio, &preferences, *language, &performance, &controls);
    if state.observed.as_ref() != Some(&current) {
        state.observed = Some(current.clone());
        state.settled = 0.0;
        status.saved = false;
    } else {
        state.settled += time.delta_secs();
    }
    if state.written.as_ref() == Some(&current) || (!forced && state.settled < 0.3) {
        return;
    }
    let Some(path) = &location.0 else {
        return;
    };
    match write_settings(path, &current) {
        Ok(()) => {
            state.written = Some(current);
            status.error = None;
            status.saved = true;
            println!("DTSET saved=true");
        }
        Err(error) => {
            status.error = Some(format!("Settings could not be saved: {error}"));
            state.settled = -2.0;
        }
    }
}

fn read_settings(path: &Path) -> io::Result<Option<StoredSettings>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if bytes.len() > 65_536 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "preferences exceed size limit",
        ));
    }
    let value: StoredSettings = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if value.version != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported preferences version",
        ));
    }
    Ok(Some(value.normalize()))
}

fn write_settings(path: &Path, value: &StoredSettings) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing preferences directory"))?;
    fs::create_dir_all(parent)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let temporary = parent.join(format!(".settings-{}-{stamp}.tmp", std::process::id()));
    let mut created = false;
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        created = true;
        let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        if read_settings(&temporary)?.as_ref() != Some(value) {
            return Err(io::Error::other("preferences read-back mismatch"));
        }
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(target_os = "android")]
pub(crate) fn settings_path() -> Option<PathBuf> {
    bevy::android::ANDROID_APP
        .get()
        .and_then(|app| app.internal_data_path())
        .map(|path| path.join("settings.json"))
}
#[cfg(not(target_os = "android"))]
pub(crate) fn settings_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("DEFEND_TAIPEI_SETTINGS_DIR")
        .or_else(|| std::env::var_os("DEFEND_TAIPEI_SAVE_DIR"))
        .filter(|path| !path.is_empty())
    {
        return Some(PathBuf::from(path).join("settings.json"));
    }
    if std::env::args().any(|arg| arg == "--qa") {
        return Some(
            std::env::var_os("DEFEND_TAIPEI_QA_OUT")
                .map_or_else(|| PathBuf::from("qa-artifacts"), PathBuf::from)
                .join("save/settings.json"),
        );
    }
    #[cfg(target_os = "windows")]
    let directory = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let directory = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let directory = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    directory.map(|path| path.join("DefendTaipei/settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                    "dt-settings-{}-{}",
                    std::process::id(),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                )))
        }
        fn path(&self) -> PathBuf {
            self.0.join("settings.json")
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn background_flush_survives_fresh_start_without_a_game_slot() {
        let dir = Temp::new();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(SettingsPlugin)
            .insert_resource(SettingsLocation(Some(dir.path())));
        app.update();
        app.world_mut().resource_mut::<AudioSettings>().master = 0.23;
        app.world_mut().resource_mut::<AudioSettings>().muted = true;
        app.world_mut().resource_mut::<GamePreferences>().day_hours = 4;
        *app.world_mut().resource_mut::<Language>() = Language::En;
        *app.world_mut().resource_mut::<PerformancePreferences>() = PerformancePreferences {
            profile: crate::performance::PerformanceProfile::Quality,
            frame_cap: 90,
            render_scale: 85,
            overlay: true,
        };
        app.world_mut()
            .resource_mut::<ControlPreferences>()
            .vibration = false;
        app.world_mut()
            .resource_mut::<ControlPreferences>()
            .button_percent = 110;
        app.world_mut()
            .resource_mut::<ControlPreferences>()
            .rebind(crate::input::ControlAction::Interact, KeyCode::KeyR)
            .unwrap();
        app.world_mut().write_message(AppLifecycle::WillSuspend);
        app.update();
        let stored = read_settings(&dir.path()).unwrap().unwrap();
        assert_eq!(stored.audio.master, 0.23);
        assert_eq!(stored.day_hours, 4);
        assert_eq!(stored.performance.frame_cap, 90);
        assert_eq!(stored.performance.render_scale, 85);
        assert!(stored.performance.overlay);
        assert!(!stored.controls.vibration);
        assert_eq!(stored.controls.button_percent, 110);
        assert_eq!(
            stored.controls.key(crate::input::ControlAction::Interact),
            KeyCode::KeyR
        );
        drop(app);
        let mut restored = App::new();
        restored
            .add_plugins(MinimalPlugins)
            .add_plugins(SettingsPlugin)
            .insert_resource(SettingsLocation(Some(dir.path())));
        restored.update();
        assert_eq!(restored.world().resource::<AudioSettings>().master, 0.23);
        assert!(restored.world().resource::<AudioSettings>().muted);
        assert_eq!(restored.world().resource::<GamePreferences>().day_hours, 4);
        assert_eq!(*restored.world().resource::<Language>(), Language::En);
        assert_eq!(
            *restored.world().resource::<ControlPreferences>(),
            stored.controls
        );
        assert_eq!(
            *restored.world().resource::<PerformancePreferences>(),
            stored.performance
        );
    }
    #[test]
    fn old_install_defaults_and_bad_preferences_are_bounded() {
        let value: StoredSettings = serde_json::from_str(r#"{"day_hours":7,"language":"bad","audio":{"master":7,"effects":-4,"ambience":0.2,"radio":0.4,"muted":true}}"#).unwrap();
        let value = value.normalize();
        assert_eq!(value.day_hours, 1);
        assert_eq!(value.language, "zh-TW");
        assert_eq!(value.audio.master, 1.0);
        assert_eq!(value.audio.effects, 0.0);
        assert!(value.audio.muted);
        assert_eq!(
            serde_json::from_str::<StoredSettings>("{}").unwrap(),
            StoredSettings::default()
        );
    }
    #[test]
    fn preferences_round_trip_separately_from_run_save() {
        let dir = Temp::new();
        fs::create_dir_all(&dir.0).unwrap();
        let game = dir.0.join("save.json");
        fs::write(&game, b"existing run untouched").unwrap();
        let mut value = StoredSettings {
            day_hours: 4,
            language: "en".into(),
            ..default()
        };
        value.audio.master = 0.35;
        value.audio.muted = true;
        write_settings(&dir.path(), &value).unwrap();
        assert_eq!(read_settings(&dir.path()).unwrap(), Some(value));
        assert_eq!(fs::read(game).unwrap(), b"existing run untouched");
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 2);
    }
    #[test]
    fn version_one_without_performance_loads_defaults_without_rewriting_other_preferences() {
        let dir = Temp::new();
        fs::create_dir_all(&dir.0).unwrap();
        let legacy = r#"{"version":1,"audio":{"master":0.8,"effects":0.65,"ambience":0.4,"radio":0.5,"muted":true},"day_hours":4,"language":"en"}"#;
        fs::write(dir.path(), legacy).unwrap();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(SettingsPlugin)
            .insert_resource(SettingsLocation(Some(dir.path())));
        app.update();
        assert_eq!(
            *app.world().resource::<PerformancePreferences>(),
            PerformancePreferences::default()
        );
        app.world_mut().write_message(FlushSettings);
        app.update();
        assert_eq!(fs::read_to_string(dir.path()).unwrap(), legacy);
        app.world_mut()
            .resource_mut::<PerformancePreferences>()
            .frame_cap = 30;
        app.world_mut().write_message(FlushSettings);
        app.update();
        let stored = read_settings(&dir.path()).unwrap().unwrap();
        assert_eq!(stored.performance.frame_cap, 30);
        assert_eq!(stored.audio.effects, 0.65);
        assert_eq!(stored.audio.ambience, 0.4);
        assert_eq!(stored.audio.radio, 0.5);
        assert!(stored.audio.muted);
        assert_eq!(stored.day_hours, 4);
        assert_eq!(stored.language, "en");
        assert_eq!(stored.version, 1);
    }
    #[test]
    fn invalid_performance_values_are_normalized_before_persisting() {
        let dir = Temp::new();
        let raw = PerformancePreferences {
            frame_cap: 144,
            render_scale: 255,
            ..default()
        };
        let value = StoredSettings::current(
            &AudioSettings::default(),
            &GamePreferences::default(),
            Language::ZhTw,
            &raw,
            &ControlPreferences::default(),
        );
        write_settings(&dir.path(), &value).unwrap();
        let restored = read_settings(&dir.path()).unwrap().unwrap();
        assert_eq!(restored.performance, raw.sanitize());
        assert!(matches!(restored.performance.frame_cap, 30 | 60 | 90 | 120));
        assert!(matches!(
            restored.performance.render_scale,
            0 | 70 | 85 | 100
        ));
    }
    #[test]
    fn corrupt_and_future_preferences_do_not_silently_load() {
        let dir = Temp::new();
        fs::create_dir_all(&dir.0).unwrap();
        for bytes in ["broken", r#"{"version":99}"#] {
            fs::write(dir.path(), bytes).unwrap();
            assert!(read_settings(&dir.path()).is_err());
            assert_eq!(fs::read_to_string(dir.path()).unwrap(), bytes);
        }
    }
}
