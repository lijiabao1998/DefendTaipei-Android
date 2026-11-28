//! Responsive preferences overlay. Ordered raw pointer messages preserve short
//! taps, and the overlay never resumes an underlying paused game when closing.
use super::PauseOpen;
use crate::{
    audio::{AudioCategory, AudioSettings, SfxEvent},
    game_flow::{Campaign, GameFlow},
    i18n::{Language, UiFont, tr},
    input::{CONTROL_ACTIONS, ControlAction, ControlPreferences, ScreenRect, safe_insets},
    performance::{
        PerformanceAction, PerformancePreferences, PerformanceProfile, PerformanceStatus,
    },
    settings::{FlushSettings, GamePreferences, SettingsStatus},
};
use bevy::{
    input::{
        ButtonState,
        mouse::MouseButtonInput,
        touch::{TouchInput, TouchPhase},
    },
    prelude::*,
    window::{AppLifecycle, PrimaryWindow, WindowFocused},
};

#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SettingsSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SettingsPage {
    #[default]
    Audio,
    Game,
    Performance,
    Controls,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PerformanceSection {
    #[default]
    Display,
    Diagnostics,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ControlsSection {
    #[default]
    Touch,
    Keyboard,
}
#[derive(Resource, Default)]
pub(crate) struct SettingsUi {
    pub open: bool,
    pub captured: bool,
    pub page: SettingsPage,
    pub performance_section: PerformanceSection,
    pub controls_section: ControlsSection,
    pub bindings_page: usize,
    pub binding_capture: Option<ControlAction>,
    binding_notice: String,
    drag: Option<(Option<u64>, usize)>,
    viewport: Vec2,
}
#[derive(Message, Clone, Copy, Debug)]
pub(crate) enum SettingsAction {
    Open,
    Close,
    Page(SettingsPage),
    Volume(usize, f32),
    AdjustVolume(usize, f32),
    Mute,
    Preview(AudioCategory),
    Language(Language),
    DayHours(u8),
    ControlsSection(ControlsSection),
    Vibration,
    PreviewHaptic,
    ButtonSize(u8),
    BindingsPage(bool),
    CaptureBinding(usize),
    Bind(ControlAction, KeyCode),
    CancelBinding,
    PerformanceSection(PerformanceSection),
    PerformanceProfile(PerformanceProfile),
    FrameCap(u16),
    RenderScale(u8),
    PerformanceOverlay,
    CaptureStart,
    CaptureCancel,
    ExportReport,
    Reset,
}
#[derive(Component)]
pub(crate) struct SettingsRoot;
#[derive(Component, Clone, Copy)]
pub(super) enum PerformanceLiveText {
    Summary,
    Report,
}
#[derive(Clone, PartialEq)]
struct RenderKey {
    viewport: Vec2,
    page: SettingsPage,
    performance_section: PerformanceSection,
    performance: PerformancePreferences,
    controls: ControlPreferences,
    controls_section: ControlsSection,
    bindings_page: usize,
    binding_capture: Option<ControlAction>,
    binding_notice: String,
    capture_active: bool,
    report_available: bool,
    audio: AudioSettings,
    day_hours: u8,
    active_day_hours: Option<u8>,
    language: Language,
    error: Option<String>,
    saved: bool,
}
#[derive(Resource, Default)]
pub(super) struct SettingsCache(Option<RenderKey>);

#[derive(Clone)]
pub(crate) struct SettingsLayout {
    pub frame: ScreenRect,
    pub tabs: [ScreenRect; 3],
    pub performance_sections: [ScreenRect; 2],
    pub profiles: [ScreenRect; 4],
    pub frame_caps: [ScreenRect; 4],
    pub render_scales: [ScreenRect; 5],
    pub performance_overlay: ScreenRect,
    pub capture: ScreenRect,
    pub export: ScreenRect,
    performance_summary: ScreenRect,
    performance_help: ScreenRect,
    performance_labels: [ScreenRect; 3],
    pub sliders: [ScreenRect; 4],
    minus: [ScreenRect; 4],
    plus: [ScreenRect; 4],
    labels: [ScreenRect; 4],
    pub audio_actions: [ScreenRect; 4],
    pub languages: [ScreenRect; 2],
    pub durations: [ScreenRect; 3],
    pub controls_open: ScreenRect,
    pub reset: ScreenRect,
    pub close: ScreenRect,
    status: ScreenRect,
    detail: ScreenRect,
}
pub(crate) fn settings_layout(viewport: Vec2) -> SettingsLayout {
    let safe = safe_insets(viewport);
    let short = viewport.y < 500.0;
    let width = (viewport.x - safe.left - safe.right).min(900.0);
    let height = (viewport.y - safe.top - safe.bottom).min(800.0);
    let frame = ScreenRect::new(
        (viewport.x - width) * 0.5,
        safe.top + (viewport.y - safe.top - safe.bottom - height) * 0.5,
        width,
        height,
    );
    let x = frame.min.x + 12.0;
    let w = width - 24.0;
    let tab_y = frame.min.y + if short { 28.0 } else { 44.0 };
    let tab_h = if short { 30.0 } else { 40.0 };
    let tab_w = (w - 16.0) / 3.0;
    let tabs =
        std::array::from_fn(|i| ScreenRect::new(x + i as f32 * (tab_w + 8.0), tab_y, tab_w, tab_h));
    let footer_h = if short { 36.0 } else { 44.0 };
    let footer_y = frame.max.y - footer_h - 10.0;
    let reset = ScreenRect::new(x, footer_y, (w - 10.0) * 0.5, footer_h);
    let close = ScreenRect::new(x + (w + 10.0) * 0.5, footer_y, (w - 10.0) * 0.5, footer_h);
    let status = ScreenRect::new(x, footer_y - 26.0, w, 22.0);
    let top = tab_y + tab_h + 10.0;
    let columns = if viewport.x > viewport.y { 2 } else { 1 };
    let rows = 4 / columns;
    let cell_w = (w - 16.0 * (columns - 1) as f32) / columns as f32;
    let audition_height = if short { 36.0 } else { 40.0 };
    let stride = ((status.min.y - top - audition_height - 8.0) / rows as f32).clamp(56.0, 88.0);
    let labels = std::array::from_fn(|i| {
        ScreenRect::new(
            x + (i % columns) as f32 * (cell_w + 16.0),
            top + (i / columns) as f32 * stride,
            cell_w,
            18.0,
        )
    });
    let minus = std::array::from_fn(|i| {
        ScreenRect::new(labels[i].min.x, labels[i].min.y + 20.0, 36.0, 36.0)
    });
    let plus = std::array::from_fn(|i| {
        ScreenRect::new(labels[i].max.x - 36.0, labels[i].min.y + 20.0, 36.0, 36.0)
    });
    let sliders = std::array::from_fn(|i| {
        ScreenRect::new(minus[i].max.x + 8.0, minus[i].min.y, cell_w - 88.0, 36.0)
    });
    let actions_y = top + rows as f32 * stride;
    let action_w = (w - 18.0) / 4.0;
    let audio_actions = std::array::from_fn(|i| {
        ScreenRect::new(
            x + i as f32 * (action_w + 6.0),
            actions_y,
            action_w,
            audition_height,
        )
    });
    let languages = [
        ScreenRect::new(x, top + 22.0, (w - 10.0) * 0.5, 40.0),
        ScreenRect::new(x + (w + 10.0) * 0.5, top + 22.0, (w - 10.0) * 0.5, 40.0),
    ];
    let dy = top + if short { 88.0 } else { 110.0 };
    let duration_w = (w - 16.0) / 3.0;
    let durations = std::array::from_fn(|i| {
        ScreenRect::new(x + i as f32 * (duration_w + 8.0), dy, duration_w, 40.0)
    });
    let detail = ScreenRect::new(x, dy + 46.0, w, (status.min.y - dy - 50.0).max(16.0));
    let section_h = if short { 28.0 } else { 36.0 };
    let performance_sections = std::array::from_fn(|i| {
        ScreenRect::new(
            x + i as f32 * (w + 8.0) * 0.5,
            top,
            (w - 8.0) * 0.5,
            section_h,
        )
    });
    let performance_top = top + section_h + if short { 6.0 } else { 12.0 };
    let content_h = (status.min.y - performance_top - 6.0).max(96.0);
    // Portrait keeps the options above the explanation. A short landscape
    // viewport puts the three groups beside the explanation, preserving touch
    // targets without overflowing the footer.
    let option_w = if short { w * 0.62 } else { w };
    let row_h = if short {
        content_h / 3.0
    } else {
        (content_h / 4.1).clamp(52.0, 76.0)
    };
    let label_h = if short { 12.0 } else { 18.0 };
    let button_h = if short {
        (row_h - label_h - 4.0).max(24.0)
    } else {
        36.0
    };
    let performance_labels = std::array::from_fn(|i| {
        ScreenRect::new(x, performance_top + i as f32 * row_h, option_w, label_h)
    });
    let option_group = |index: usize| {
        std::array::from_fn(|i| {
            let option = performance_labels[index];
            let width = (option.width() - 18.0) * 0.25;
            ScreenRect::new(
                option.min.x + i as f32 * (width + 6.0),
                option.min.y + label_h + 2.0,
                width,
                button_h,
            )
        })
    };
    let profiles = option_group(0);
    let frame_caps = option_group(1);
    let render_scales: [ScreenRect; 5] = std::array::from_fn(|i| {
        let option = performance_labels[2];
        let width = (option.width() - 24.0) / 5.0;
        ScreenRect::new(
            option.min.x + i as f32 * (width + 6.0),
            option.min.y + label_h + 2.0,
            width,
            button_h,
        )
    });
    let performance_help = if short {
        ScreenRect::new(
            x + option_w + 14.0,
            performance_top,
            w - option_w - 14.0,
            content_h,
        )
    } else {
        let y = render_scales[0].max.y + 12.0;
        ScreenRect::new(x, y, w, (status.min.y - y - 6.0).max(20.0))
    };
    let diagnostics_w = if short { w * 0.52 } else { w };
    let action_x = if short { x + diagnostics_w + 14.0 } else { x };
    let action_w = if short { w - diagnostics_w - 14.0 } else { w };
    let action_h = if short { 28.0 } else { 36.0 };
    let summary_h = if short {
        content_h
    } else {
        (content_h * 0.52).clamp(72.0, 206.0)
    };
    let performance_summary = ScreenRect::new(x, performance_top, diagnostics_w, summary_h);
    let action_y = if short {
        performance_top
    } else {
        performance_top + summary_h + 8.0
    };
    let performance_overlay = ScreenRect::new(action_x, action_y, action_w, action_h);
    let capture = ScreenRect::new(
        action_x,
        action_y + action_h + 6.0,
        (action_w - 8.0) * 0.5,
        action_h,
    );
    let export = ScreenRect::new(
        capture.max.x + 8.0,
        capture.min.y,
        capture.width(),
        action_h,
    );
    let controls_open = ScreenRect::new(
        detail.min.x,
        detail.min.y,
        detail.width(),
        if short { 28.0 } else { 38.0 },
    );
    SettingsLayout {
        frame,
        tabs,
        performance_sections,
        profiles,
        frame_caps,
        render_scales,
        performance_overlay,
        capture,
        export,
        performance_summary,
        performance_help,
        performance_labels,
        sliders,
        minus,
        plus,
        labels,
        audio_actions,
        languages,
        durations,
        controls_open,
        reset,
        close,
        status,
        detail,
    }
}
const BINDING_ROWS: usize = 4;

pub(crate) struct ControlsLayout {
    pub sections: [ScreenRect; 2],
    pub vibration: ScreenRect,
    pub preview: ScreenRect,
    pub sizes: [ScreenRect; 3],
    pub bindings: [ScreenRect; BINDING_ROWS],
    binding_labels: [ScreenRect; BINDING_ROWS],
    pub previous: ScreenRect,
    pub next: ScreenRect,
    page_label: ScreenRect,
    touch_help: ScreenRect,
    key_help: ScreenRect,
}

pub(crate) fn controls_layout(layout: &SettingsLayout, viewport: Vec2) -> ControlsLayout {
    let short = viewport.y < 500.0;
    let sections = layout.performance_sections;
    let x = sections[0].min.x;
    let w = layout.frame.width() - 24.0;
    let top = sections[0].max.y + 8.0;
    let bottom = layout.status.min.y - 6.0;
    let height = if short { 30.0 } else { 42.0 };
    let vibration = ScreenRect::new(x, top, (w - 8.0) * 0.65, height);
    let preview = ScreenRect::new(vibration.max.x + 8.0, top, (w - 8.0) * 0.35, height);
    let sizes_top = top + height + if short { 22.0 } else { 34.0 };
    let size_w = (w - 16.0) / 3.0;
    let sizes = std::array::from_fn(|i| {
        ScreenRect::new(x + i as f32 * (size_w + 8.0), sizes_top, size_w, height)
    });
    let touch_help = ScreenRect::new(
        x,
        sizes[0].max.y + 8.0,
        w,
        (bottom - sizes[0].max.y - 8.0).max(0.0),
    );
    let previous = ScreenRect::new(x, top, w * 0.25, height);
    let next = ScreenRect::new(x + w * 0.75, top, w * 0.25, height);
    let page_label = ScreenRect::new(
        previous.max.x + 8.0,
        top + 5.0,
        w * 0.5 - 16.0,
        height - 5.0,
    );
    let rows_top = top + height + 6.0;
    let row_h = ((bottom - rows_top - 26.0) / BINDING_ROWS as f32 - 4.0).clamp(22.0, 46.0);
    let binding_labels = std::array::from_fn(|i| {
        ScreenRect::new(
            x,
            rows_top + i as f32 * (row_h + 4.0),
            w * 0.60 - 8.0,
            row_h,
        )
    });
    let bindings = std::array::from_fn(|i| {
        ScreenRect::new(x + w * 0.60, binding_labels[i].min.y, w * 0.40, row_h)
    });
    let key_help = ScreenRect::new(
        x,
        bindings[BINDING_ROWS - 1].max.y + 6.0,
        w,
        (bottom - bindings[BINDING_ROWS - 1].max.y - 6.0).max(0.0),
    );
    ControlsLayout {
        sections,
        vibration,
        preview,
        sizes,
        bindings,
        binding_labels,
        previous,
        next,
        page_label,
        touch_help,
        key_help,
    }
}

fn hit_controls(
    layout: &SettingsLayout,
    viewport: Vec2,
    state: &SettingsUi,
    point: Vec2,
) -> Option<SettingsAction> {
    let controls = controls_layout(layout, viewport);
    for (index, rect) in controls.sections.iter().enumerate() {
        if rect.contains(point) {
            return Some(SettingsAction::ControlsSection(if index == 0 {
                ControlsSection::Touch
            } else {
                ControlsSection::Keyboard
            }));
        }
    }
    if state.controls_section == ControlsSection::Touch {
        if controls.vibration.contains(point) {
            return Some(SettingsAction::Vibration);
        }
        if controls.preview.contains(point) {
            return Some(SettingsAction::PreviewHaptic);
        }
        for (index, rect) in controls.sizes.iter().enumerate() {
            if rect.contains(point) {
                return Some(SettingsAction::ButtonSize([85, 100, 110][index]));
            }
        }
    } else {
        if controls.previous.contains(point) {
            return Some(SettingsAction::BindingsPage(false));
        }
        if controls.next.contains(point) {
            return Some(SettingsAction::BindingsPage(true));
        }
        for (row, rect) in controls.bindings.iter().enumerate() {
            if rect.contains(point) {
                return Some(SettingsAction::CaptureBinding(row));
            }
        }
    }
    None
}

fn render_controls(
    root: &mut ChildSpawnerCommands,
    layout: &SettingsLayout,
    viewport: Vec2,
    state: &SettingsUi,
    prefs: &ControlPreferences,
    language: Language,
    font: &UiFont,
) {
    let controls = controls_layout(layout, viewport);
    let rect = |r: ScreenRect| [r.min.x, r.min.y, r.max.x, r.max.y];
    println!(
        "DTCONTROLS layout={}",
        serde_json::json!({"viewport":[viewport.x,viewport.y],"sections":controls.sections.map(rect),"vibration":rect(controls.vibration),"preview":rect(controls.preview),"sizes":controls.sizes.map(rect),"bindings":controls.bindings.map(rect),"previous":rect(controls.previous),"next":rect(controls.next),"page":state.bindings_page})
    );
    let short = viewport.y < 500.0;
    for (index, title) in [
        tr(language, "觸控", "Touch"),
        tr(language, "鍵盤", "Keyboard"),
    ]
    .into_iter()
    .enumerate()
    {
        button(
            root,
            controls.sections[index],
            title,
            if (index == 0) == (state.controls_section == ControlsSection::Touch) {
                GOLD
            } else {
                MUTED
            },
            font,
        );
    }
    if state.controls_section == ControlsSection::Touch {
        button(
            root,
            controls.vibration,
            if prefs.vibration {
                tr(language, "震動回饋：開啟", "Haptics: on")
            } else {
                tr(language, "震動回饋：關閉", "Haptics: off")
            },
            if prefs.vibration { GOLD } else { MUTED },
            font,
        );
        button(
            root,
            controls.preview,
            tr(language, "測試震動", "Test haptic"),
            MUTED,
            font,
        );
        label(
            root,
            ScreenRect::new(
                controls.sizes[0].min.x,
                controls.sizes[0].min.y - 20.0,
                layout.frame.width() - 24.0,
                18.0,
            ),
            tr(language, "觸控按鈕大小", "Touch button size"),
            12.0,
            PAPER,
            font,
        );
        for (index, title) in [
            tr(language, "小 85%", "Small 85%"),
            tr(language, "標準 100%", "Standard"),
            tr(language, "大 110%", "Large 110%"),
        ]
        .into_iter()
        .enumerate()
        {
            button(
                root,
                controls.sizes[index],
                title,
                if prefs.button_percent == [85, 100, 110][index] {
                    GOLD
                } else {
                    MUTED
                },
                font,
            );
        }
        bounded_label(
            root,
            controls.touch_help,
            tr(
                language,
                "點按攻擊或蹲低按鈕會產生短震動；裝置不支援時略過。\n載具：搖桿決定方向與倒車，按住煞車停止；點互動下車。裝甲載具可使用射擊、裝填與切換武器按鈕。",
                "Attack and crouch taps produce short pulses when the device supports vibration.\nVehicles: steer or reverse with the pad; hold Brake to stop and tap Use to exit. Armored vehicles also have Fire, Reload and Switch buttons.",
            ),
            if short { 10.0 } else { 13.0 },
            MUTED,
            font,
        );
    } else {
        button(
            root,
            controls.previous,
            tr(language, "上一頁", "Previous"),
            MUTED,
            font,
        );
        button(
            root,
            controls.next,
            tr(language, "下一頁", "Next"),
            MUTED,
            font,
        );
        label(
            root,
            controls.page_label,
            &format!(
                "{} / {}",
                state.bindings_page + 1,
                CONTROL_ACTIONS.len().div_ceil(BINDING_ROWS)
            ),
            13.0,
            PAPER,
            font,
        );
        for (row, action) in CONTROL_ACTIONS
            .iter()
            .skip(state.bindings_page * BINDING_ROWS)
            .take(BINDING_ROWS)
            .enumerate()
        {
            bounded_label(
                root,
                controls.binding_labels[row],
                action.label(language),
                if short { 11.0 } else { 13.0 },
                PAPER,
                font,
            );
            let waiting = state.binding_capture == Some(*action);
            let label = if waiting {
                tr(language, "按下新按鍵…", "Press a key…").into()
            } else {
                prefs.key_label(*action)
            };
            button_sized(
                root,
                controls.bindings[row],
                &label,
                if waiting { GOLD } else { MUTED },
                if short { 11.0 } else { 13.0 },
                font,
            );
        }
        let help = if !state.binding_notice.is_empty() {
            state.binding_notice.as_str()
        } else {
            tr(
                language,
                "點選後按新鍵；Esc 取消，重複按鍵互換。",
                "Click a binding, then press a key. Esc cancels; duplicates swap.",
            )
        };
        bounded_label(
            root,
            controls.key_help,
            help,
            if short { 10.0 } else { 12.0 },
            MUTED,
            font,
        );
    }
}

fn volume(audio: &AudioSettings, index: usize) -> f32 {
    match index {
        0 => audio.master,
        1 => audio.effects,
        2 => audio.ambience,
        _ => audio.radio,
    }
}
fn volume_at(layout: &SettingsLayout, index: usize, x: f32) -> f32 {
    ((x - layout.sliders[index].min.x) / layout.sliders[index].width()).clamp(0.0, 1.0)
}
fn hit(
    layout: &SettingsLayout,
    page: SettingsPage,
    point: Vec2,
    _audio: &AudioSettings,
    performance_section: PerformanceSection,
    capture_active: bool,
) -> Option<SettingsAction> {
    if layout.close.contains(point) {
        return Some(SettingsAction::Close);
    }
    if layout.reset.contains(point) {
        return Some(SettingsAction::Reset);
    }
    for (i, rect) in layout.tabs.iter().enumerate() {
        if rect.contains(point) {
            return Some(SettingsAction::Page(
                [
                    SettingsPage::Audio,
                    SettingsPage::Game,
                    SettingsPage::Performance,
                ][i],
            ));
        }
    }
    if page == SettingsPage::Audio {
        for i in 0..4 {
            if layout.minus[i].contains(point) {
                return Some(SettingsAction::AdjustVolume(i, -0.05));
            }
            if layout.plus[i].contains(point) {
                return Some(SettingsAction::AdjustVolume(i, 0.05));
            }
            if layout.sliders[i].contains(point) {
                return Some(SettingsAction::Volume(i, volume_at(layout, i, point.x)));
            }
        }
        for (i, rect) in layout.audio_actions.iter().enumerate() {
            if rect.contains(point) {
                return Some(match i {
                    0 => SettingsAction::Mute,
                    1 => SettingsAction::Preview(AudioCategory::Effects),
                    2 => SettingsAction::Preview(AudioCategory::Ambience),
                    _ => SettingsAction::Preview(AudioCategory::Radio),
                });
            }
        }
    } else if page == SettingsPage::Game {
        if layout.controls_open.contains(point) {
            return Some(SettingsAction::Page(SettingsPage::Controls));
        }
        for (i, rect) in layout.languages.iter().enumerate() {
            if rect.contains(point) {
                return Some(SettingsAction::Language(if i == 0 {
                    Language::ZhTw
                } else {
                    Language::En
                }));
            }
        }
        for (i, rect) in layout.durations.iter().enumerate() {
            if rect.contains(point) {
                return Some(SettingsAction::DayHours([1, 2, 4][i]));
            }
        }
    } else if page == SettingsPage::Performance {
        for (i, rect) in layout.performance_sections.iter().enumerate() {
            if rect.contains(point) {
                return Some(SettingsAction::PerformanceSection(if i == 0 {
                    PerformanceSection::Display
                } else {
                    PerformanceSection::Diagnostics
                }));
            }
        }
        if performance_section == PerformanceSection::Display {
            for (i, rect) in layout.profiles.iter().enumerate() {
                if rect.contains(point) {
                    return Some(SettingsAction::PerformanceProfile(
                        [
                            PerformanceProfile::Auto,
                            PerformanceProfile::Battery,
                            PerformanceProfile::Balanced,
                            PerformanceProfile::Quality,
                        ][i],
                    ));
                }
            }
            for (i, rect) in layout.frame_caps.iter().enumerate() {
                if rect.contains(point) {
                    return Some(SettingsAction::FrameCap([30, 60, 90, 120][i]));
                }
            }
            for (i, rect) in layout.render_scales.iter().enumerate() {
                if rect.contains(point) {
                    return Some(SettingsAction::RenderScale([0, 60, 70, 85, 100][i]));
                }
            }
        } else {
            if layout.performance_overlay.contains(point) {
                return Some(SettingsAction::PerformanceOverlay);
            }
            if layout.capture.contains(point) {
                return Some(if capture_active {
                    SettingsAction::CaptureCancel
                } else {
                    SettingsAction::CaptureStart
                });
            }
            if layout.export.contains(point) {
                return Some(SettingsAction::ExportReport);
            }
        }
    }
    None
}

pub(super) fn pointer_actions(
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    mut state: ResMut<SettingsUi>,
    audio: Res<AudioSettings>,
    performance_status: Option<Res<PerformanceStatus>>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    mut mouse_events: MessageReader<MouseButtonInput>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut touch_events: MessageReader<TouchInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut actions: MessageWriter<SettingsAction>,
    mut focus: MessageReader<WindowFocused>,
    mut lifecycle: MessageReader<AppLifecycle>,
    menu: Option<Res<super::TitleMenu>>,
) {
    let mouse_events: Vec<_> = mouse_events.read().copied().collect();
    let touch_events: Vec<_> = touch_events.read().copied().collect();
    let unfocused = focus.read().any(|event| !event.focused);
    let suspending = lifecycle
        .read()
        .any(|event| matches!(event, AppLifecycle::WillSuspend | AppLifecycle::Suspended));
    state.captured = state.open;
    if unfocused || suspending {
        state.binding_capture = None;
        state.drag = None;
        return;
    }
    let Some((window_id, window)) = windows.iter().next() else {
        return;
    };
    let viewport = Vec2::new(window.width(), window.height());
    if !state.open {
        // The entry button also needs ordered events: a whole short tap can
        // finish between slow Android frames before Touches is sampled.
        let entry = if flow.get() == &GameFlow::Title
            && menu
                .as_ref()
                .is_some_and(|menu| menu.page == super::TitlePage::Home)
        {
            Some(super::layout::home_layout(viewport).settings)
        } else if flow.get() == &GameFlow::Playing && pause.0 {
            Some(super::layout::pause_layout(viewport).settings)
        } else {
            None
        };
        if let Some(entry) = entry {
            let touch = touch_events.iter().any(|event| {
                event.window == window_id
                    && event.phase == TouchPhase::Started
                    && entry.contains(event.position)
            });
            let mouse = mouse_events.iter().any(|event| {
                event.window == window_id
                    && event.button == MouseButton::Left
                    && event.state == ButtonState::Pressed
            }) && window
                .cursor_position()
                .is_some_and(|point| entry.contains(point));
            if touch || mouse {
                actions.write(SettingsAction::Open);
            }
        }
        return;
    }
    if flow.get() != &GameFlow::Title && !(flow.get() == &GameFlow::Playing && pause.0) {
        state.open = false;
        state.drag = None;
        return;
    }
    let resized = state.viewport != Vec2::ZERO && state.viewport != viewport;
    state.viewport = viewport;
    if resized {
        state.binding_capture = None;
        state.drag = None;
        return;
    }
    if let Some(action) = state.binding_capture {
        if keys.just_pressed(KeyCode::Escape) {
            keys.clear_just_pressed(KeyCode::Escape);
            actions.write(SettingsAction::CancelBinding);
        } else {
            let key = keys.get_just_pressed().next().copied();
            if let Some(key) = key {
                keys.clear_just_pressed(key);
                actions.write(SettingsAction::Bind(action, key));
            }
        }
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        actions.write(SettingsAction::Close);
        return;
    }
    let layout = settings_layout(viewport);
    let start = |id: Option<u64>,
                 point: Vec2,
                 state: &mut SettingsUi,
                 actions: &mut MessageWriter<SettingsAction>| {
        if state.drag.is_some() {
            return;
        }
        if state.page == SettingsPage::Audio
            && let Some(index) = layout.sliders.iter().position(|r| r.contains(point))
        {
            state.drag = Some((id, index));
        }
        if state.page == SettingsPage::Controls
            && let Some(action) = hit_controls(&layout, viewport, state, point)
        {
            actions.write(action);
            return;
        }
        if let Some(action) = hit(
            &layout,
            state.page,
            point,
            &audio,
            state.performance_section,
            performance_status
                .as_ref()
                .is_some_and(|status| status.capture_active),
        ) {
            actions.write(action);
        }
    };
    for event in mouse_events {
        if event.window != window_id || event.button != MouseButton::Left {
            continue;
        }
        if event.state == ButtonState::Pressed {
            if let Some(point) = window.cursor_position() {
                start(None, point, &mut state, &mut actions);
            }
        } else if state.drag.is_some_and(|(id, _)| id.is_none()) {
            state.drag = None;
        }
    }
    if mouse.pressed(MouseButton::Left)
        && let Some((None, index)) = state.drag
        && let Some(point) = window.cursor_position()
    {
        actions.write(SettingsAction::Volume(
            index,
            volume_at(&layout, index, point.x),
        ));
    }
    for event in touch_events {
        if event.window != window_id {
            continue;
        }
        match event.phase {
            TouchPhase::Started => start(Some(event.id), event.position, &mut state, &mut actions),
            TouchPhase::Moved | TouchPhase::Ended => {
                if let Some((Some(id), index)) = state.drag
                    && id == event.id
                {
                    actions.write(SettingsAction::Volume(
                        index,
                        volume_at(&layout, index, event.position.x),
                    ));
                    if event.phase == TouchPhase::Ended {
                        state.drag = None;
                    }
                }
            }
            TouchPhase::Canceled => {
                if state.drag.is_some_and(|(id, _)| id == Some(event.id)) {
                    state.drag = None;
                }
            }
        }
    }
}

pub(super) fn apply_actions(
    mut events: MessageReader<SettingsAction>,
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    mut state: ResMut<SettingsUi>,
    mut audio: ResMut<AudioSettings>,
    mut preferences: ResMut<GamePreferences>,
    mut performance: ResMut<PerformancePreferences>,
    mut controls: ResMut<ControlPreferences>,
    mut performance_actions: MessageWriter<PerformanceAction>,
    mut language: ResMut<Language>,
    mut sfx: MessageWriter<SfxEvent>,
    mut flush: MessageWriter<FlushSettings>,
) {
    for event in events.read() {
        if matches!(event, SettingsAction::Open)
            && (flow.get() == &GameFlow::Title || flow.get() == &GameFlow::Playing && pause.0)
        {
            state.open = true;
            state.captured = true;
            state.drag = None;
            println!("DTUI action=SettingsOpen");
            continue;
        }
        if !state.open {
            continue;
        }
        match *event {
            SettingsAction::Close => {
                state.open = false;
                state.captured = true;
                state.drag = None;
                state.binding_capture = None;
                flush.write(FlushSettings);
                println!("DTUI action=SettingsClose");
            }
            SettingsAction::Page(page) => {
                state.page = page;
                state.drag = None;
                state.binding_capture = None;
                println!("DTSETTINGS action=page value={page:?}");
            }
            SettingsAction::Volume(index, value) | SettingsAction::AdjustVolume(index, value) => {
                let value = if matches!(event, SettingsAction::AdjustVolume(_, _)) {
                    volume(&audio, index) + value
                } else {
                    value
                };
                let value = if value.is_finite() {
                    value.clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let value = (value * 100.0).round() / 100.0;
                match index {
                    0 => audio.master = value,
                    1 => audio.effects = value,
                    2 => audio.ambience = value,
                    3 => audio.radio = value,
                    _ => {}
                }
            }
            SettingsAction::Mute => {
                audio.muted = !audio.muted;
                println!("DTSETTINGS action=mute value={}", audio.muted);
            }
            SettingsAction::Preview(category) => {
                sfx.write(SfxEvent::Preview { category });
                println!("DTSETTINGS action=preview category={category:?}");
            }
            SettingsAction::Language(value) => *language = value,
            SettingsAction::DayHours(value) if matches!(value, 1 | 2 | 4) => {
                preferences.day_hours = value;
                println!("DTSETTINGS action=day_hours value={value}");
            }
            SettingsAction::ControlsSection(section) => {
                state.controls_section = section;
                state.binding_capture = None;
                println!("DTSETTINGS action=controls_section value={section:?}");
            }
            SettingsAction::Vibration => {
                controls.vibration = !controls.vibration;
                if !controls.vibration {
                    crate::device::clear_haptic_request();
                }
                println!("DTSETTINGS action=vibration value={}", controls.vibration);
            }
            SettingsAction::PreviewHaptic => {
                let queued = controls.haptic(15);
                println!(
                    "DTSETTINGS action=haptic_preview queued={queued} enabled={}",
                    controls.vibration
                );
            }
            SettingsAction::ButtonSize(value) if matches!(value, 85 | 100 | 110) => {
                controls.button_percent = value;
                println!("DTSETTINGS action=button_size value={value}");
            }
            SettingsAction::BindingsPage(next) => {
                let pages = CONTROL_ACTIONS.len().div_ceil(BINDING_ROWS);
                state.bindings_page = if next {
                    (state.bindings_page + 1) % pages
                } else {
                    (state.bindings_page + pages - 1) % pages
                };
                state.binding_capture = None;
                println!(
                    "DTSETTINGS action=bindings_page value={}",
                    state.bindings_page
                );
            }
            SettingsAction::CaptureBinding(row) => {
                state.binding_capture = CONTROL_ACTIONS
                    .get(state.bindings_page * BINDING_ROWS + row)
                    .copied();
                state.binding_notice.clear();
                println!(
                    "DTSETTINGS action=binding_capture value={:?}",
                    state.binding_capture
                );
            }
            SettingsAction::Bind(action, key) => {
                state.binding_notice = match controls.rebind(action, key) {
                    Ok(Some(other)) => {
                        format!("{} ↔ {}", action.label(*language), other.label(*language))
                    }
                    Ok(None) => format!(
                        "{} = {}",
                        action.label(*language),
                        controls.key_label(action)
                    ),
                    Err(_) => tr(
                        *language,
                        "此按鍵保留，請選擇其他按鍵。",
                        "This key is reserved. Choose another key.",
                    )
                    .into(),
                };
                state.binding_capture = None;
                println!("DTSETTINGS action=rebind action_id={action:?} key={key:?}");
            }
            SettingsAction::CancelBinding => {
                state.binding_capture = None;
                println!("DTSETTINGS action=binding_cancel");
            }
            SettingsAction::PerformanceSection(section) => {
                state.performance_section = section;
                state.drag = None;
                println!("DTSETTINGS action=performance_section value={section:?}");
            }
            SettingsAction::PerformanceProfile(profile) => {
                performance.profile = profile;
                println!("DTSETTINGS action=performance_profile value={profile:?}");
            }
            SettingsAction::FrameCap(value) if matches!(value, 30 | 60 | 90 | 120) => {
                performance.frame_cap = value;
                println!("DTSETTINGS action=frame_cap value={value}");
            }
            SettingsAction::RenderScale(value) if matches!(value, 0 | 60 | 70 | 85 | 100) => {
                performance.render_scale = value;
                println!("DTSETTINGS action=render_scale value={value}");
            }
            SettingsAction::PerformanceOverlay => {
                performance.overlay = !performance.overlay;
                println!(
                    "DTSETTINGS action=performance_overlay value={}",
                    performance.overlay
                );
            }
            SettingsAction::CaptureStart => {
                performance_actions.write(PerformanceAction::StartCapture);
                println!("DTSETTINGS action=capture_start");
            }
            SettingsAction::CaptureCancel => {
                performance_actions.write(PerformanceAction::CancelCapture);
                println!("DTSETTINGS action=capture_cancel");
            }
            SettingsAction::ExportReport => {
                performance_actions.write(PerformanceAction::ExportReport);
                println!("DTSETTINGS action=export_report");
            }
            SettingsAction::Reset => match state.page {
                SettingsPage::Audio => *audio = AudioSettings::default(),
                SettingsPage::Game => {
                    *preferences = GamePreferences::default();
                    *language = Language::ZhTw;
                }
                SettingsPage::Performance => *performance = PerformancePreferences::default(),
                SettingsPage::Controls => {
                    *controls = ControlPreferences::default();
                    state.binding_capture = None;
                    state.binding_notice.clear();
                }
            },
            _ => {}
        }
    }
}

const PANEL: Color = Color::srgb(0.06, 0.075, 0.07);
const GOLD: Color = Color::srgb(0.82, 0.68, 0.4);
const PAPER: Color = Color::srgb(0.94, 0.91, 0.82);
const MUTED: Color = Color::srgb(0.64, 0.68, 0.62);
pub(super) fn render(
    mut commands: Commands,
    state: Res<SettingsUi>,
    audio: Res<AudioSettings>,
    preferences: Res<GamePreferences>,
    performance: Res<PerformancePreferences>,
    controls: Res<ControlPreferences>,
    performance_status: Option<Res<PerformanceStatus>>,
    campaign: Option<Res<Campaign>>,
    flow: Res<State<GameFlow>>,
    language: Res<Language>,
    status: Res<SettingsStatus>,
    font: Res<UiFont>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cache: ResMut<SettingsCache>,
    roots: Query<Entity, With<SettingsRoot>>,
    mut live_texts: Query<(&PerformanceLiveText, &mut Text)>,
) {
    if !state.open {
        for entity in &roots {
            commands.entity(entity).despawn();
        }
        cache.0 = None;
        return;
    }
    let viewport = windows.iter().next().map_or(Vec2::new(1280.0, 720.0), |w| {
        Vec2::new(w.width(), w.height())
    });
    let key = RenderKey {
        viewport,
        page: state.page,
        performance_section: state.performance_section,
        performance: *performance,
        controls: (*controls).clone(),
        controls_section: state.controls_section,
        bindings_page: state.bindings_page,
        binding_capture: state.binding_capture,
        binding_notice: state.binding_notice.clone(),
        capture_active: state.page == SettingsPage::Performance
            && state.performance_section == PerformanceSection::Diagnostics
            && performance_status
                .as_ref()
                .is_some_and(|status| status.capture_active),
        report_available: state.page == SettingsPage::Performance
            && state.performance_section == PerformanceSection::Diagnostics
            && performance_status
                .as_ref()
                .is_some_and(|status| status.report_available),
        audio: *audio,
        day_hours: preferences.day_hours,
        active_day_hours: if flow.get() == &GameFlow::Playing {
            campaign.map(|c| c.day_hours)
        } else {
            None
        },
        language: *language,
        error: status.error.clone(),
        saved: status.saved,
    };
    // Telemetry can update every render interval on a slow GPU. Keep the
    // visible subtree stable: replacing it for each revision can leave newly
    // spawned nodes without a renderable layout/visibility for that frame.
    if state.page == SettingsPage::Performance
        && state.performance_section == PerformanceSection::Diagnostics
    {
        for (kind, mut text) in &mut live_texts {
            let next = match kind {
                PerformanceLiveText::Summary => {
                    performance_summary(performance_status.as_deref(), *language).to_owned()
                }
                PerformanceLiveText::Report => performance_report_text(
                    performance_status.as_deref(),
                    flow.get() == &GameFlow::Playing,
                    *language,
                ),
            };
            if text.0 != next {
                text.0 = next;
            }
        }
    }
    if cache.0.as_ref() == Some(&key) && !roots.is_empty() {
        return;
    }
    for entity in &roots {
        commands.entity(entity).despawn();
    }
    let l = settings_layout(viewport);
    let lang = *language;
    let short = viewport.y < 500.0;
    if cache.0.as_ref().is_none_or(|old| old.viewport != viewport) {
        let rect = |r: ScreenRect| [r.min.x, r.min.y, r.max.x, r.max.y];
        println!(
            "DTSETTINGS layout={}",
            serde_json::json!({"viewport":[viewport.x,viewport.y],"tabs":l.tabs.map(rect),"sliders":l.sliders.map(rect),"minus":l.minus.map(rect),"plus":l.plus.map(rect),"audio_actions":l.audio_actions.map(rect),"languages":l.languages.map(rect),"durations":l.durations.map(rect),"controls_open":rect(l.controls_open),"reset":rect(l.reset),"close":rect(l.close),"performance_sections":l.performance_sections.map(rect),"profiles":l.profiles.map(rect),"frame_caps":l.frame_caps.map(rect),"render_scales":l.render_scales.map(rect),"performance_overlay":rect(l.performance_overlay),"capture":rect(l.capture),"export":rect(l.export)})
        );
    }
    commands.spawn((SettingsRoot,Node{position_type:PositionType::Absolute,width:percent(100.0),height:percent(100.0),..default()},BackgroundColor(Color::srgba(0.01,0.015,0.012,0.96)),GlobalZIndex(90))).with_children(|root|{
        root.spawn((node(l.frame),BackgroundColor(PANEL),BorderColor::all(GOLD)));
        label(root,ScreenRect::new(l.frame.min.x+12.0,l.frame.min.y+4.0,l.frame.width()-24.0,28.0),tr(lang,"設定","Settings"),if short{20.0}else{24.0},PAPER,&font);
        for(i,title)in [tr(lang,"聲音","Audio"),tr(lang,"遊戲","Game"),tr(lang,"效能","Performance")].iter().enumerate(){button(root,l.tabs[i],title,if [SettingsPage::Audio,SettingsPage::Game,SettingsPage::Performance][i]==state.page || (i==1 && state.page==SettingsPage::Controls){GOLD}else{MUTED},&font);}
        if state.page==SettingsPage::Audio{
            let names=[tr(lang,"總音量","Master"),tr(lang,"音效","Effects"),tr(lang,"音樂／環境","Music / ambience"),tr(lang,"廣播音效","Radio effects")];
            for(i,name)in names.iter().enumerate(){let value=volume(&audio,i);label(root,l.labels[i],&format!("{name}    {:.0}%",value*100.0),13.0,PAPER,&font);button(root,l.minus[i],"−",MUTED,&font);button(root,l.plus[i],"+",MUTED,&font);let bar=l.sliders[i];root.spawn((node(ScreenRect::new(bar.min.x,bar.min.y+15.0,bar.width(),6.0)),BackgroundColor(Color::srgb(0.19,0.23,0.2))));root.spawn((node(ScreenRect::new(bar.min.x,bar.min.y+15.0,bar.width()*value,6.0)),BackgroundColor(GOLD)));root.spawn((node(ScreenRect::new((bar.min.x+bar.width()*value-7.0).clamp(bar.min.x,bar.max.x-14.0),bar.min.y+5.0,14.0,26.0)),BackgroundColor(PAPER)));}
            let labels=[if audio.muted{tr(lang,"已靜音","Muted")}else{tr(lang,"靜音","Mute")},tr(lang,"音效 ▶","FX ▶"),tr(lang,"環境 ▶","Amb ▶"),tr(lang,"廣播 ▶","Radio ▶")];
            for(i,title)in labels.iter().enumerate(){button(root,l.audio_actions[i],title,if i==0&&audio.muted{GOLD}else{MUTED},&font);}
        }else if state.page==SettingsPage::Game{
            label(root,ScreenRect::new(l.languages[0].min.x,l.languages[0].min.y-21.0,l.frame.width()-24.0,18.0),tr(lang,"介面語言","Interface language"),13.0,PAPER,&font);
            button(root,l.languages[0],"繁體中文",if lang==Language::ZhTw{GOLD}else{MUTED},&font);button(root,l.languages[1],"English",if lang==Language::En{GOLD}else{MUTED},&font);
            label(root,ScreenRect::new(l.durations[0].min.x,l.durations[0].min.y-22.0,l.frame.width()-24.0,20.0),tr(lang,"新遊戲：一天的現實時間（1×）","New run: real hours per day (1×)"),13.0,PAPER,&font);
            for(i,hours)in [1,2,4].iter().enumerate(){button(root,l.durations[i],&format!("{} {}",hours,tr(lang,"小時",if *hours==1{"hour"}else{"hours"})),if preferences.day_hours==*hours{GOLD}else{MUTED},&font);}
            let text=if let Some(hours)=key.active_day_hours {format!("{} {} {}。{}",tr(lang,"本局維持","This run keeps"),hours,tr(lang,"小時",if hours==1{"hour"}else{"hours"}),tr(lang,"下次出發時套用新設定。","The new value applies on your next departure."))}else{tr(lang,"日長只在開始新局時套用，繼續遊戲保留原設定。","Day length applies when starting a new run. Continue keeps the saved value.").to_owned()};
            button(root,l.controls_open,tr(lang,"操作設定","Controls"),GOLD,&font);
            let detail=ScreenRect::new(l.detail.min.x,l.controls_open.max.y+4.0,l.detail.width(),(l.detail.max.y-l.controls_open.max.y-4.0).max(0.0));
            bounded_label(root,detail,&text,if short{10.0}else{13.0},MUTED,&font);
        }
        if state.page==SettingsPage::Controls { render_controls(root,&l,viewport,&state,&controls,lang,&font); }
        if state.page==SettingsPage::Performance {
            render_performance(root, &l, state.performance_section, &performance, performance_status.as_deref(), flow.get()==&GameFlow::Playing, lang, short, &font);
        }
        label(root,l.status,if status.error.is_some(){tr(lang,"設定未能儲存，請稍後重試。","Preferences could not be saved. Please retry.")}else if status.saved{tr(lang,"設定已儲存。","Preferences saved.")}else{tr(lang,"設定即時套用，自動儲存。","Changes apply now and save automatically.")},if short{10.0}else{11.0},if status.error.is_some(){Color::srgb(1.0,0.55,0.42)}else{MUTED},&font);
        button(root,l.reset,tr(lang,"還原本頁預設","Reset this page"),MUTED,&font);button(root,l.close,tr(lang,"完成／返回","Done / back"),GOLD,&font);
    });
    cache.0 = Some(key);
}
fn render_performance(
    root: &mut ChildSpawnerCommands,
    layout: &SettingsLayout,
    section: PerformanceSection,
    preferences: &PerformancePreferences,
    status: Option<&PerformanceStatus>,
    playing: bool,
    lang: Language,
    short: bool,
    font: &UiFont,
) {
    for (i, title) in [
        tr(lang, "畫面設定", "Display"),
        tr(lang, "診斷與記錄", "Diagnostics"),
    ]
    .iter()
    .enumerate()
    {
        button_sized(
            root,
            layout.performance_sections[i],
            title,
            if (i == 0) == (section == PerformanceSection::Display) {
                GOLD
            } else {
                MUTED
            },
            if short { 11.0 } else { 13.0 },
            font,
        );
    }
    let size = if short { 10.0 } else { 13.0 };
    if section == PerformanceSection::Display {
        for (i, text) in [
            tr(lang, "效能模式", "Performance profile"),
            tr(lang, "目標幀率（FPS）", "Requested frame cap (FPS)"),
            tr(lang, "3D 畫面解析度", "3D render resolution"),
        ]
        .iter()
        .enumerate()
        {
            bounded_label(
                root,
                layout.performance_labels[i],
                text,
                if short { 10.0 } else { 12.0 },
                PAPER,
                font,
            );
        }
        for (i, (profile, title)) in [
            (PerformanceProfile::Auto, tr(lang, "自動", "Auto")),
            (PerformanceProfile::Battery, tr(lang, "省電", "Battery")),
            (PerformanceProfile::Balanced, tr(lang, "均衡", "Balanced")),
            (PerformanceProfile::Quality, tr(lang, "畫質", "Quality")),
        ]
        .iter()
        .enumerate()
        {
            button_sized(
                root,
                layout.profiles[i],
                title,
                if preferences.profile == *profile {
                    GOLD
                } else {
                    MUTED
                },
                size,
                font,
            );
        }
        for (i, cap) in [30, 60, 90, 120].iter().enumerate() {
            button_sized(
                root,
                layout.frame_caps[i],
                &cap.to_string(),
                if preferences.frame_cap == *cap {
                    GOLD
                } else {
                    MUTED
                },
                size,
                font,
            );
        }
        for (i, scale) in [0, 60, 70, 85, 100].iter().enumerate() {
            let text = if *scale == 0 {
                tr(lang, "自動", "Auto").to_owned()
            } else {
                format!("{scale}%")
            };
            button_sized(
                root,
                layout.render_scales[i],
                &text,
                if preferences.render_scale == *scale {
                    GOLD
                } else {
                    MUTED
                },
                size,
                font,
            );
        }
        bounded_label(
            root,
            layout.performance_help,
            tr(
                lang,
                "自動依裝置與幀時間調整。過熱或省電模式會限制目標。\n90／120 FPS 依螢幕支援套用，並非保證幀率。\n解析度只調整 3D 世界，介面保持清晰。",
                "Auto adapts to device and frame times. Heat and power saving can limit targets.\n90/120 FPS require display support; actual FPS can be lower.\nResolution scales the 3D world; UI stays sharp.",
            ),
            if short { 10.0 } else { 12.0 },
            MUTED,
            font,
        );
        return;
    }
    bounded_live_label(
        root,
        layout.performance_summary,
        performance_summary(status, lang),
        if short { 10.0 } else { 12.0 },
        PAPER,
        font,
        PerformanceLiveText::Summary,
    );
    button_sized(
        root,
        layout.performance_overlay,
        if preferences.overlay {
            tr(lang, "效能浮層：開", "Performance overlay: on")
        } else {
            tr(lang, "效能浮層：關", "Performance overlay: off")
        },
        if preferences.overlay { GOLD } else { MUTED },
        if short { 11.0 } else { 13.0 },
        font,
    );
    let active = status.is_some_and(|status| status.capture_active);
    let available = status.is_some_and(|status| status.report_available);
    button_sized(
        root,
        layout.capture,
        if active {
            tr(lang, "取消記錄", "Cancel capture")
        } else {
            tr(lang, "記錄 60 秒", "Record 60 s")
        },
        if playing || active { GOLD } else { MUTED },
        if short { 10.0 } else { 12.0 },
        font,
    );
    button_sized(
        root,
        layout.export,
        tr(lang, "匯出報告", "Export report"),
        if available { GOLD } else { MUTED },
        if short { 10.0 } else { 12.0 },
        font,
    );
    let bottom = ScreenRect::new(
        layout.performance_overlay.min.x,
        layout.capture.max.y + 6.0,
        layout.performance_overlay.width(),
        (layout.status.min.y - layout.capture.max.y - 10.0).max(14.0),
    );
    bounded_live_label(
        root,
        bottom,
        &performance_report_text(status, playing, lang),
        if short { 9.0 } else { 11.0 },
        MUTED,
        font,
        PerformanceLiveText::Report,
    );
}

fn performance_summary(status: Option<&PerformanceStatus>, lang: Language) -> &str {
    status
        .map(|status| status.summary.as_str())
        .filter(|text| !text.is_empty())
        .unwrap_or(tr(
            lang,
            "正在讀取裝置與執行狀態…",
            "Reading device and runtime status…",
        ))
}

fn performance_report_text(
    status: Option<&PerformanceStatus>,
    playing: bool,
    lang: Language,
) -> String {
    if let Some(status) = status.filter(|status| !status.report_status.is_empty()) {
        return status.report_status.clone();
    }
    if status.is_some_and(|status| status.capture_active) {
        let seconds = status.map_or(0.0, |status| status.capture_seconds).max(0.0);
        return format!(
            "{} {:.0}/60 s. {}",
            tr(lang, "已採樣", "Captured"),
            seconds,
            tr(
                lang,
                "關閉設定並繼續遊戲。",
                "Close settings and resume play."
            )
        );
    }
    if !playing {
        tr(lang, "進入遊戲後可記錄。暖機 5 秒，再採樣 60 秒；暫停、設定及背景不計時。", "Enter a game to record. Warm-up: 5 s, capture: 60 s. Pauses, settings and background time are excluded.").to_owned()
    } else {
        tr(lang, "暖機 5 秒，再記錄 60 秒實際遊玩；暫停及背景不計時。報告留在裝置內。", "5 s warm-up, then 60 s of actual play; pauses and background time are excluded. Reports stay on this device.").to_owned()
    }
}

fn node(r: ScreenRect) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(r.min.x),
        top: px(r.min.y),
        width: px(r.width()),
        height: px(r.height()),
        ..default()
    }
}
fn label(
    root: &mut ChildSpawnerCommands,
    r: ScreenRect,
    text: &str,
    size: f32,
    color: Color,
    font: &UiFont,
) {
    root.spawn((
        node(r),
        Text::new(text),
        TextFont::from_font_size(FontSize::Px(size)).with_font(font.0.clone()),
        TextColor(color),
    ));
}
fn bounded_label(
    root: &mut ChildSpawnerCommands,
    r: ScreenRect,
    text: &str,
    size: f32,
    color: Color,
    font: &UiFont,
) {
    bounded_paragraph(root, r, text, size, color, font, None);
}
fn bounded_live_label(
    root: &mut ChildSpawnerCommands,
    r: ScreenRect,
    text: &str,
    size: f32,
    color: Color,
    font: &UiFont,
    kind: PerformanceLiveText,
) {
    bounded_paragraph(root, r, text, size, color, font, Some(kind));
}
fn bounded_paragraph(
    root: &mut ChildSpawnerCommands,
    r: ScreenRect,
    text: &str,
    size: f32,
    color: Color,
    font: &UiFont,
    live: Option<PerformanceLiveText>,
) {
    root.spawn((Node {
        overflow: Overflow::clip(),
        ..node(r)
    },))
        .with_children(|parent| {
            let mut text = parent.spawn((
                Node {
                    width: percent(100.0),
                    ..default()
                },
                Text::new(text),
                TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
                TextFont::from_font_size(FontSize::Px(size)).with_font(font.0.clone()),
                TextColor(color),
            ));
            if let Some(kind) = live {
                text.insert(kind);
            }
        });
}
fn button(root: &mut ChildSpawnerCommands, r: ScreenRect, text: &str, color: Color, font: &UiFont) {
    button_sized(root, r, text, color, 13.0, font);
}
fn button_sized(
    root: &mut ChildSpawnerCommands,
    r: ScreenRect,
    text: &str,
    color: Color,
    size: f32,
    font: &UiFont,
) {
    root.spawn((
        Node {
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(5.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..node(r)
        },
        BackgroundColor(Color::srgb(0.12, 0.15, 0.13)),
        BorderColor::all(color),
    ))
    .with_children(|p| {
        p.spawn((
            Text::new(text),
            TextFont::from_font_size(FontSize::Px(size)).with_font(font.0.clone()),
            TextColor(color),
            TextLayout::justify(Justify::Center),
        ));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input_app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(State::new(GameFlow::Title))
            .init_resource::<PauseOpen>()
            .init_resource::<SettingsUi>()
            .init_resource::<AudioSettings>()
            .init_resource::<GamePreferences>()
            .init_resource::<PerformancePreferences>()
            .init_resource::<ControlPreferences>()
            .init_resource::<PerformanceStatus>()
            .add_message::<PerformanceAction>()
            .init_resource::<Language>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<MouseButtonInput>()
            .add_message::<TouchInput>()
            .add_message::<WindowFocused>()
            .add_message::<AppLifecycle>()
            .add_message::<SettingsAction>()
            .add_message::<SfxEvent>()
            .add_message::<FlushSettings>()
            .add_systems(Update, (pointer_actions, apply_actions).chain());
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: (390, 844).into(),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.world_mut().write_message(SettingsAction::Open);
        app.update();
        (app, window)
    }
    fn touch(app: &mut App, window: Entity, id: u64, phase: TouchPhase, position: Vec2) {
        app.world_mut().write_message(TouchInput {
            window,
            id,
            phase,
            position,
            force: None,
        });
    }

    #[test]
    fn controls_accept_short_taps_and_capture_or_cancel_exactly_one_binding() {
        let (mut app, window) = input_app();
        let layout = settings_layout(Vec2::new(390.0, 844.0));
        app.world_mut()
            .write_message(SettingsAction::Page(SettingsPage::Game));
        app.update();
        touch(
            &mut app,
            window,
            0,
            TouchPhase::Started,
            layout.controls_open.center(),
        );
        touch(
            &mut app,
            window,
            0,
            TouchPhase::Ended,
            layout.controls_open.center(),
        );
        app.update();
        assert_eq!(
            app.world().resource::<SettingsUi>().page,
            SettingsPage::Controls
        );
        let controls = controls_layout(&layout, Vec2::new(390.0, 844.0));
        touch(
            &mut app,
            window,
            0,
            TouchPhase::Started,
            controls.vibration.center(),
        );
        touch(
            &mut app,
            window,
            0,
            TouchPhase::Ended,
            controls.vibration.center(),
        );
        touch(
            &mut app,
            window,
            0,
            TouchPhase::Started,
            controls.sizes[2].center(),
        );
        touch(
            &mut app,
            window,
            0,
            TouchPhase::Ended,
            controls.sizes[2].center(),
        );
        app.update();
        assert!(!app.world().resource::<ControlPreferences>().vibration);
        assert_eq!(
            app.world().resource::<ControlPreferences>().button_percent,
            110
        );
        app.world_mut()
            .write_message(SettingsAction::ControlsSection(ControlsSection::Keyboard));
        app.world_mut()
            .write_message(SettingsAction::CaptureBinding(0));
        app.update();
        assert_eq!(
            app.world().resource::<SettingsUi>().binding_capture,
            Some(ControlAction::Up)
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyE);
        app.update();
        let prefs = app.world().resource::<ControlPreferences>();
        assert_eq!(prefs.key(ControlAction::Up), KeyCode::KeyE);
        assert_eq!(prefs.key(ControlAction::Interact), KeyCode::KeyW);
        assert!(
            app.world()
                .resource::<SettingsUi>()
                .binding_capture
                .is_none()
        );
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::KeyE)
        );
        app.world_mut()
            .write_message(SettingsAction::CaptureBinding(1));
        app.update();
        let before = app.world().resource::<ControlPreferences>().clone();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert_eq!(*app.world().resource::<ControlPreferences>(), before);
        assert!(app.world().resource::<SettingsUi>().open);
        assert!(
            app.world()
                .resource::<SettingsUi>()
                .binding_capture
                .is_none()
        );
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::Escape)
        );
    }

    #[test]
    fn controls_pages_fit_both_orientations_without_overlapping_actions() {
        for viewport in [
            Vec2::new(390.0, 844.0),
            Vec2::new(844.0, 390.0),
            Vec2::new(360.0, 640.0),
            Vec2::new(640.0, 360.0),
        ] {
            let page = settings_layout(viewport);
            let layout = controls_layout(&page, viewport);
            for group in [
                vec![
                    layout.sections[0],
                    layout.sections[1],
                    layout.vibration,
                    layout.preview,
                    layout.sizes[0],
                    layout.sizes[1],
                    layout.sizes[2],
                    page.close,
                    page.reset,
                ],
                vec![
                    layout.sections[0],
                    layout.sections[1],
                    layout.previous,
                    layout.next,
                    layout.bindings[0],
                    layout.bindings[1],
                    layout.bindings[2],
                    layout.bindings[3],
                    page.close,
                    page.reset,
                ],
            ] {
                for (index, rect) in group.iter().enumerate() {
                    assert!(
                        rect.min.x >= 0.0
                            && rect.min.y >= 0.0
                            && rect.max.x <= viewport.x
                            && rect.max.y <= viewport.y,
                        "{viewport:?}: {rect:?}"
                    );
                    for other in group.iter().skip(index + 1) {
                        assert!(
                            !rect.intersects(*other),
                            "{viewport:?}: {rect:?} vs {other:?}"
                        );
                    }
                }
            }
            assert!(layout.bindings[3].max.y <= page.status.min.y);
        }
    }
    #[test]
    fn settings_entry_accepts_short_taps_without_repeating_held_input() {
        #[derive(Resource, Default)]
        struct OpenCount(usize);
        fn count(mut events: MessageReader<SettingsAction>, mut count: ResMut<OpenCount>) {
            count.0 += events
                .read()
                .filter(|event| matches!(event, SettingsAction::Open))
                .count();
        }
        for paused in [false, true] {
            for held in [false, true] {
                let (mut app, window) = input_app();
                app.init_resource::<super::super::TitleMenu>()
                    .init_resource::<crate::roles::SelectedRole>()
                    .init_resource::<crate::save::SaveStatus>()
                    .init_resource::<super::super::ConsumedPresses>()
                    .init_resource::<UiFont>()
                    .init_resource::<OpenCount>()
                    .add_message::<super::super::TitleAction>()
                    .add_message::<crate::game_flow::StartGame>()
                    .add_message::<crate::save::ContinueGame>()
                    .add_message::<crate::save::SaveAndMenu>()
                    .add_message::<super::super::TogglePause>()
                    .add_systems(
                        Update,
                        (
                            super::super::title::pointer_actions,
                            super::super::title::apply_actions,
                            super::super::pause::drive_pause,
                            count,
                        )
                            .chain()
                            .after(apply_actions),
                    );
                app.world_mut()
                    .resource_mut::<Messages<SettingsAction>>()
                    .clear();
                app.world_mut().resource_mut::<SettingsUi>().open = false;
                app.world_mut().resource_mut::<SettingsUi>().captured = false;
                let entry = if paused {
                    app.insert_resource(State::new(GameFlow::Playing));
                    app.world_mut().resource_mut::<PauseOpen>().0 = true;
                    super::super::layout::pause_layout(Vec2::new(390.0, 844.0)).settings
                } else {
                    super::super::layout::home_layout(Vec2::new(390.0, 844.0)).settings
                };
                if held {
                    app.world_mut()
                        .resource_mut::<super::super::ConsumedPresses>()
                        .points
                        .push(entry.center());
                }
                touch(&mut app, window, 0, TouchPhase::Started, entry.center());
                if !held {
                    touch(&mut app, window, 0, TouchPhase::Ended, entry.center());
                }
                app.update();
                assert!(app.world().resource::<SettingsUi>().open);
                assert_eq!(app.world().resource::<OpenCount>().0, 1);
                assert!(
                    app.world()
                        .resource::<Messages<super::super::TitleAction>>()
                        .is_empty()
                );
                touch(&mut app, window, 0, TouchPhase::Moved, entry.center());
                app.update();
                touch(&mut app, window, 0, TouchPhase::Ended, entry.center());
                app.update();
                assert_eq!(app.world().resource::<OpenCount>().0, 1);
                app.world_mut().write_message(SettingsAction::Close);
                app.update();
                assert!(!app.world().resource::<SettingsUi>().open);
                assert_eq!(app.world().resource::<OpenCount>().0, 1);
                assert_eq!(app.world().resource::<PauseOpen>().0, paused);
            }
        }
    }
    #[test]
    fn two_short_taps_with_reused_android_pointer_increment_twice() {
        let (mut app, window) = input_app();
        let l = settings_layout(Vec2::new(390.0, 844.0));
        for _ in 0..2 {
            touch(&mut app, window, 0, TouchPhase::Started, l.plus[0].center());
            touch(&mut app, window, 0, TouchPhase::Ended, l.plus[0].center());
        }
        app.update();
        assert!((app.world().resource::<AudioSettings>().master - 0.9).abs() < 0.001);
    }
    #[test]
    fn cancelled_and_rotated_slider_drag_does_not_later_jump() {
        let (mut app, window) = input_app();
        let l = settings_layout(Vec2::new(390.0, 844.0));
        let start = Vec2::new(
            l.sliders[0].min.x + l.sliders[0].width() * 0.3,
            l.sliders[0].center().y,
        );
        touch(&mut app, window, 0, TouchPhase::Started, start);
        app.update();
        touch(&mut app, window, 0, TouchPhase::Canceled, l.sliders[0].max);
        touch(&mut app, window, 0, TouchPhase::Moved, l.sliders[0].max);
        app.update();
        assert!((app.world().resource::<AudioSettings>().master - 0.3).abs() < 0.001);
        touch(&mut app, window, 1, TouchPhase::Started, start);
        app.update();
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set(800.0, 360.0);
        touch(
            &mut app,
            window,
            1,
            TouchPhase::Moved,
            Vec2::new(700.0, 180.0),
        );
        app.update();
        touch(
            &mut app,
            window,
            1,
            TouchPhase::Ended,
            Vec2::new(700.0, 180.0),
        );
        app.update();
        assert!((app.world().resource::<AudioSettings>().master - 0.3).abs() < 0.001);
        assert!(app.world().resource::<SettingsUi>().drag.is_none());
    }
    #[test]
    fn app_background_cancels_slider_before_pointer_id_is_reused() {
        let (mut app, window) = input_app();
        let l = settings_layout(Vec2::new(390.0, 844.0));
        let point = Vec2::new(
            l.sliders[0].min.x + l.sliders[0].width() * 0.25,
            l.sliders[0].center().y,
        );
        touch(&mut app, window, 0, TouchPhase::Started, point);
        app.update();
        app.world_mut().write_message(AppLifecycle::Suspended);
        app.update();
        touch(&mut app, window, 0, TouchPhase::Moved, l.sliders[0].max);
        app.update();
        assert_eq!(app.world().resource::<AudioSettings>().master, 0.25);
        assert!(app.world().resource::<SettingsUi>().drag.is_none());
    }
    #[test]
    fn closing_settings_keeps_pause_and_current_run_day_length() {
        let (mut app, _) = input_app();
        app.insert_resource(State::new(GameFlow::Playing));
        app.world_mut().resource_mut::<PauseOpen>().0 = true;
        app.insert_resource(Campaign {
            day_hours: 2,
            ..default()
        });
        app.world_mut().write_message(SettingsAction::DayHours(4));
        app.world_mut().write_message(SettingsAction::Close);
        app.update();
        assert!(app.world().resource::<PauseOpen>().0);
        assert!(!app.world().resource::<SettingsUi>().open);
        assert!(app.world().resource::<SettingsUi>().captured);
        assert_eq!(app.world().resource::<GamePreferences>().day_hours, 4);
        assert_eq!(app.world().resource::<Campaign>().day_hours, 2);
        assert!(!app.world().resource::<Messages<FlushSettings>>().is_empty());
    }
    #[test]
    fn performance_short_taps_apply_options_and_only_reset_performance() {
        let (mut app, window) = input_app();
        let layout = settings_layout(Vec2::new(390.0, 844.0));
        app.world_mut().resource_mut::<AudioSettings>().effects = 0.65;
        app.world_mut().resource_mut::<GamePreferences>().day_hours = 4;
        for point in [
            layout.tabs[2].center(),
            layout.profiles[3].center(),
            layout.frame_caps[2].center(),
            layout.render_scales[3].center(),
        ] {
            touch(&mut app, window, 0, TouchPhase::Started, point);
            touch(&mut app, window, 0, TouchPhase::Ended, point);
            app.update();
        }
        let performance = *app.world().resource::<PerformancePreferences>();
        assert_eq!(performance.profile, PerformanceProfile::Quality);
        assert_eq!(performance.frame_cap, 90);
        assert_eq!(performance.render_scale, 85);
        for point in [
            layout.performance_sections[1].center(),
            layout.performance_overlay.center(),
        ] {
            touch(&mut app, window, 0, TouchPhase::Started, point);
            touch(&mut app, window, 0, TouchPhase::Ended, point);
            app.update();
        }
        assert!(app.world().resource::<PerformancePreferences>().overlay);
        app.world_mut().write_message(SettingsAction::Reset);
        app.update();
        assert_eq!(
            *app.world().resource::<PerformancePreferences>(),
            PerformancePreferences::default()
        );
        assert_eq!(app.world().resource::<AudioSettings>().effects, 0.65);
        assert_eq!(app.world().resource::<GamePreferences>().day_hours, 4);
    }
    #[test]
    fn diagnostics_actions_do_not_resume_paused_run() {
        let (mut app, window) = input_app();
        app.insert_resource(State::new(GameFlow::Playing));
        app.world_mut().resource_mut::<PauseOpen>().0 = true;
        let layout = settings_layout(Vec2::new(390.0, 844.0));
        app.world_mut().resource_mut::<SettingsUi>().page = SettingsPage::Performance;
        app.world_mut()
            .resource_mut::<SettingsUi>()
            .performance_section = PerformanceSection::Diagnostics;
        for (active, expected) in [(false, "start"), (true, "cancel")] {
            app.world_mut()
                .resource_mut::<PerformanceStatus>()
                .capture_active = active;
            touch(
                &mut app,
                window,
                0,
                TouchPhase::Started,
                layout.capture.center(),
            );
            touch(
                &mut app,
                window,
                0,
                TouchPhase::Ended,
                layout.capture.center(),
            );
            app.update();
            let events: Vec<_> = app
                .world_mut()
                .resource_mut::<Messages<PerformanceAction>>()
                .drain()
                .collect();
            assert_eq!(events.len(), 1);
            assert!(match expected {
                "start" => matches!(events[0], PerformanceAction::StartCapture),
                _ => matches!(events[0], PerformanceAction::CancelCapture),
            });
            assert!(app.world().resource::<PauseOpen>().0);
            assert!(app.world().resource::<SettingsUi>().open);
        }
        touch(
            &mut app,
            window,
            0,
            TouchPhase::Started,
            layout.export.center(),
        );
        touch(
            &mut app,
            window,
            0,
            TouchPhase::Ended,
            layout.export.center(),
        );
        app.update();
        assert!(matches!(
            app.world_mut()
                .resource_mut::<Messages<PerformanceAction>>()
                .drain()
                .next(),
            Some(PerformanceAction::ExportReport)
        ));
        assert!(app.world().resource::<PauseOpen>().0);
    }
    #[test]
    fn telemetry_refresh_keeps_performance_root_and_children_alive() {
        let (mut app, window) = input_app();
        app.init_resource::<UiFont>()
            .init_resource::<SettingsStatus>()
            .init_resource::<SettingsCache>()
            .add_systems(PostUpdate, render);
        for (section, size) in [
            (PerformanceSection::Display, [800.0, 360.0]),
            (PerformanceSection::Diagnostics, [390.0, 844.0]),
        ] {
            app.world_mut()
                .get_mut::<Window>(window)
                .unwrap()
                .resolution
                .set(size[0], size[1]);
            app.world_mut().resource_mut::<SettingsUi>().page = SettingsPage::Performance;
            app.world_mut()
                .resource_mut::<SettingsUi>()
                .performance_section = section;
            app.update();
            let root = app
                .world_mut()
                .query_filtered::<Entity, With<SettingsRoot>>()
                .single(app.world())
                .unwrap();
            let child_count = app.world().get::<Children>(root).unwrap().len();
            let entity_count = app.world().entities().len();
            for revision in 1..=10 {
                {
                    let mut status = app.world_mut().resource_mut::<PerformanceStatus>();
                    status.revision = revision;
                    status.summary = format!("live summary {revision}");
                    status.report_status = format!("live report {revision}");
                    status.capture_seconds = revision as f32;
                }
                app.update();
                assert!(
                    app.world().get::<SettingsRoot>(root).is_some(),
                    "{section:?}: telemetry replaced visible settings root"
                );
                assert_eq!(
                    app.world().get::<Children>(root).unwrap().len(),
                    child_count
                );
                assert_eq!(app.world().entities().len(), entity_count);
            }
            if section == PerformanceSection::Diagnostics {
                let mut query = app.world_mut().query::<(&PerformanceLiveText, &Text)>();
                let values: Vec<_> = query
                    .iter(app.world())
                    .map(|(_, text)| text.0.clone())
                    .collect();
                assert!(values.iter().any(|text| text == "live summary 10"));
                assert!(values.iter().any(|text| text == "live report 10"));
            }
        }
    }
    #[test]
    fn controls_fit_both_orientations() {
        for viewport in [
            Vec2::new(320.0, 568.0),
            Vec2::new(390.0, 844.0),
            Vec2::new(800.0, 360.0),
            Vec2::new(568.0, 320.0),
            Vec2::new(844.0, 390.0),
            Vec2::new(1280.0, 720.0),
        ] {
            let l = settings_layout(viewport);
            for r in l
                .tabs
                .iter()
                .chain(l.sliders.iter())
                .chain(l.audio_actions.iter())
                .chain(l.languages.iter())
                .chain(l.durations.iter())
                .chain(l.performance_sections.iter())
                .chain(l.profiles.iter())
                .chain(l.frame_caps.iter())
                .chain(l.render_scales.iter())
                .chain([
                    &l.close,
                    &l.reset,
                    &l.capture,
                    &l.export,
                    &l.performance_overlay,
                ])
            {
                assert!(
                    r.min.x >= 0.0
                        && r.max.x <= viewport.x
                        && r.min.y >= 0.0
                        && r.max.y <= viewport.y,
                    "{viewport:?} {r:?}"
                );
            }
            assert!(
                l.audio_actions[0].max.y <= l.status.min.y,
                "audio/status overlap {viewport:?}"
            );
            let display_controls: Vec<_> = l
                .performance_sections
                .iter()
                .chain(l.profiles.iter())
                .chain(l.frame_caps.iter())
                .chain(l.render_scales.iter())
                .chain([&l.close, &l.reset])
                .collect();
            let diagnostic_controls: Vec<_> = l
                .performance_sections
                .iter()
                .chain([
                    &l.performance_overlay,
                    &l.capture,
                    &l.export,
                    &l.close,
                    &l.reset,
                ])
                .collect();
            for controls in [&display_controls, &diagnostic_controls] {
                for (i, a) in controls.iter().enumerate() {
                    for b in controls.iter().skip(i + 1) {
                        assert!(
                            !a.intersects(**b),
                            "performance controls overlap {viewport:?}: {a:?} {b:?}"
                        );
                    }
                }
            }
            assert!(
                l.render_scales[0].max.y <= l.status.min.y,
                "performance options/status overlap {viewport:?}"
            );
            assert!(
                l.capture.max.y <= l.status.min.y,
                "capture/status overlap {viewport:?}"
            );
            assert!(
                l.durations[0].max.y <= l.status.min.y,
                "duration/status overlap {viewport:?}"
            );
        }
    }
    #[test]
    fn slider_and_buttons_share_geometry() {
        let l = settings_layout(Vec2::new(390.0, 844.0));
        let a = AudioSettings::default();
        assert!(matches!(
            hit(
                &l,
                SettingsPage::Audio,
                l.audio_actions[0].center(),
                &a,
                PerformanceSection::Display,
                false
            ),
            Some(SettingsAction::Mute)
        ));
        assert!(matches!(
            hit(
                &l,
                SettingsPage::Game,
                l.durations[2].center(),
                &a,
                PerformanceSection::Display,
                false
            ),
            Some(SettingsAction::DayHours(4))
        ));
        assert_eq!(volume_at(&l, 0, l.sliders[0].min.x - 100.0), 0.0);
        assert_eq!(volume_at(&l, 0, l.sliders[0].max.x + 100.0), 1.0);
    }
}
