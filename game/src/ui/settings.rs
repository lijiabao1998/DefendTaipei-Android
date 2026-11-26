//! Responsive preferences overlay. Ordered raw pointer messages preserve short
//! taps, and the overlay never resumes an underlying paused game when closing.
use super::PauseOpen;
use crate::{
    audio::{AudioCategory, AudioSettings, SfxEvent},
    game_flow::{Campaign, GameFlow},
    i18n::{Language, UiFont, tr},
    input::{ScreenRect, safe_insets},
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
}
#[derive(Resource, Default)]
pub(crate) struct SettingsUi {
    pub open: bool,
    pub captured: bool,
    pub page: SettingsPage,
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
    Reset,
}
#[derive(Component)]
pub(super) struct SettingsRoot;
#[derive(Clone, PartialEq)]
struct RenderKey {
    viewport: Vec2,
    page: SettingsPage,
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
    pub tabs: [ScreenRect; 2],
    pub sliders: [ScreenRect; 4],
    minus: [ScreenRect; 4],
    plus: [ScreenRect; 4],
    labels: [ScreenRect; 4],
    pub audio_actions: [ScreenRect; 4],
    pub languages: [ScreenRect; 2],
    pub durations: [ScreenRect; 3],
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
    let tabs = [
        ScreenRect::new(x, tab_y, (w - 8.0) * 0.5, tab_h),
        ScreenRect::new(x + (w + 8.0) * 0.5, tab_y, (w - 8.0) * 0.5, tab_h),
    ];
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
    SettingsLayout {
        frame,
        tabs,
        sliders,
        minus,
        plus,
        labels,
        audio_actions,
        languages,
        durations,
        reset,
        close,
        status,
        detail,
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
) -> Option<SettingsAction> {
    if layout.close.contains(point) {
        return Some(SettingsAction::Close);
    }
    if layout.reset.contains(point) {
        return Some(SettingsAction::Reset);
    }
    for (i, rect) in layout.tabs.iter().enumerate() {
        if rect.contains(point) {
            return Some(SettingsAction::Page(if i == 0 {
                SettingsPage::Audio
            } else {
                SettingsPage::Game
            }));
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
    } else {
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
    }
    None
}

pub(super) fn pointer_actions(
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    mut state: ResMut<SettingsUi>,
    audio: Res<AudioSettings>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    mut mouse_events: MessageReader<MouseButtonInput>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut touch_events: MessageReader<TouchInput>,
    keys: Res<ButtonInput<KeyCode>>,
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
        state.drag = None;
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
        if let Some(action) = hit(&layout, state.page, point, &audio) {
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
                flush.write(FlushSettings);
                println!("DTUI action=SettingsClose");
            }
            SettingsAction::Page(page) => {
                state.page = page;
                state.drag = None;
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
            SettingsAction::Reset => {
                if state.page == SettingsPage::Audio {
                    *audio = AudioSettings::default()
                } else {
                    *preferences = GamePreferences::default();
                    *language = Language::ZhTw;
                }
            }
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
    campaign: Option<Res<Campaign>>,
    flow: Res<State<GameFlow>>,
    language: Res<Language>,
    status: Res<SettingsStatus>,
    font: Res<UiFont>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cache: ResMut<SettingsCache>,
    roots: Query<Entity, With<SettingsRoot>>,
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
            serde_json::json!({"viewport":[viewport.x,viewport.y],"tabs":l.tabs.map(rect),"sliders":l.sliders.map(rect),"minus":l.minus.map(rect),"plus":l.plus.map(rect),"audio_actions":l.audio_actions.map(rect),"languages":l.languages.map(rect),"durations":l.durations.map(rect),"reset":rect(l.reset),"close":rect(l.close)})
        );
    }
    commands.spawn((SettingsRoot,Node{position_type:PositionType::Absolute,width:percent(100.0),height:percent(100.0),..default()},BackgroundColor(Color::srgba(0.01,0.015,0.012,0.96)),GlobalZIndex(90))).with_children(|root|{
        root.spawn((node(l.frame),BackgroundColor(PANEL),BorderColor::all(GOLD)));
        label(root,ScreenRect::new(l.frame.min.x+12.0,l.frame.min.y+4.0,l.frame.width()-24.0,28.0),tr(lang,"設定","Settings"),if short{20.0}else{24.0},PAPER,&font);
        for(i,title)in [tr(lang,"聲音","Audio"),tr(lang,"遊戲與語言","Game & language")].iter().enumerate(){button(root,l.tabs[i],title,if (i==0)==(state.page==SettingsPage::Audio){GOLD}else{MUTED},&font);}
        if state.page==SettingsPage::Audio{
            let names=[tr(lang,"總音量","Master"),tr(lang,"音效","Effects"),tr(lang,"音樂／環境","Music / ambience"),tr(lang,"廣播音效","Radio effects")];
            for(i,name)in names.iter().enumerate(){let value=volume(&audio,i);label(root,l.labels[i],&format!("{name}    {:.0}%",value*100.0),13.0,PAPER,&font);button(root,l.minus[i],"−",MUTED,&font);button(root,l.plus[i],"+",MUTED,&font);let bar=l.sliders[i];root.spawn((node(ScreenRect::new(bar.min.x,bar.min.y+15.0,bar.width(),6.0)),BackgroundColor(Color::srgb(0.19,0.23,0.2))));root.spawn((node(ScreenRect::new(bar.min.x,bar.min.y+15.0,bar.width()*value,6.0)),BackgroundColor(GOLD)));root.spawn((node(ScreenRect::new((bar.min.x+bar.width()*value-7.0).clamp(bar.min.x,bar.max.x-14.0),bar.min.y+5.0,14.0,26.0)),BackgroundColor(PAPER)));}
            let labels=[if audio.muted{tr(lang,"已靜音","Muted")}else{tr(lang,"靜音","Mute")},tr(lang,"音效 ▶","FX ▶"),tr(lang,"環境 ▶","Amb ▶"),tr(lang,"廣播 ▶","Radio ▶")];
            for(i,title)in labels.iter().enumerate(){button(root,l.audio_actions[i],title,if i==0&&audio.muted{GOLD}else{MUTED},&font);}
        }else{
            label(root,ScreenRect::new(l.languages[0].min.x,l.languages[0].min.y-21.0,l.frame.width()-24.0,18.0),tr(lang,"介面語言","Interface language"),13.0,PAPER,&font);
            button(root,l.languages[0],"繁體中文",if lang==Language::ZhTw{GOLD}else{MUTED},&font);button(root,l.languages[1],"English",if lang==Language::En{GOLD}else{MUTED},&font);
            label(root,ScreenRect::new(l.durations[0].min.x,l.durations[0].min.y-22.0,l.frame.width()-24.0,20.0),tr(lang,"新遊戲：一天的現實時間（1×）","New run: real hours per day (1×)"),13.0,PAPER,&font);
            for(i,hours)in [1,2,4].iter().enumerate(){button(root,l.durations[i],&format!("{} {}",hours,tr(lang,"小時",if *hours==1{"hour"}else{"hours"})),if preferences.day_hours==*hours{GOLD}else{MUTED},&font);}
            let text=if let Some(hours)=key.active_day_hours {format!("{} {} {}。{}",tr(lang,"本局維持","This run keeps"),hours,tr(lang,"小時",if hours==1{"hour"}else{"hours"}),tr(lang,"下次出發時套用新設定。","The new value applies on your next departure."))}else{tr(lang,"日長只在開始新局時套用，繼續遊戲保留原設定。","Day length applies when starting a new run. Continue keeps the saved value.").to_owned()};
            label(root,l.detail,&text,if short{10.0}else{13.0},MUTED,&font);
        }
        label(root,l.status,if status.error.is_some(){tr(lang,"設定未能儲存，請稍後重試。","Preferences could not be saved. Please retry.")}else if status.saved{tr(lang,"設定已儲存。","Preferences saved.")}else{tr(lang,"音量即時套用，設定自動儲存。","Audio updates now. Changes save automatically.")},if short{10.0}else{11.0},if status.error.is_some(){Color::srgb(1.0,0.55,0.42)}else{MUTED},&font);
        button(root,l.reset,tr(lang,"還原本頁預設","Reset this page"),MUTED,&font);button(root,l.close,tr(lang,"完成／返回","Done / back"),GOLD,&font);
    });
    cache.0 = Some(key);
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
fn button(root: &mut ChildSpawnerCommands, r: ScreenRect, text: &str, color: Color, font: &UiFont) {
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
            TextFont::from_font_size(FontSize::Px(13.0)).with_font(font.0.clone()),
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
                .chain([&l.close, &l.reset])
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
            hit(&l, SettingsPage::Audio, l.audio_actions[0].center(), &a),
            Some(SettingsAction::Mute)
        ));
        assert!(matches!(
            hit(&l, SettingsPage::Game, l.durations[2].center(), &a),
            Some(SettingsAction::DayHours(4))
        ));
        assert_eq!(volume_at(&l, 0, l.sliders[0].min.x - 100.0), 0.0);
        assert_eq!(volume_at(&l, 0, l.sliders[0].max.x + 100.0), 1.0);
    }
}
