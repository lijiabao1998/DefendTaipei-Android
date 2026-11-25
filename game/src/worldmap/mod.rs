//! The survivor's paper map, using the same Wanhua geometry as the world.
//!
//! Web reference: `src/map/map.ts` at v24.42.0. Discovery and the four kinds
//! of player annotations persist with the run. The native scope is the playable
//! core; it does not pretend that the unported districts or intel systems exist.

#[cfg(test)]
mod input_tests;
mod render;

use std::collections::{BTreeMap, BTreeSet};

use bevy::{
    input::mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    prelude::*,
    window::PrimaryWindow,
};
use serde::{Deserialize, Serialize};

use crate::game_flow::{GameFlow, StartGame};
use crate::input::{HudLayout, ScreenRect, safe_insets};
use crate::player::Player;
use crate::save::SaveGame;
use crate::ui::{ConsumedPresses, PauseOpen, UiModal};

// Fixed world-space grid, independent of screen size, pan or zoom. Keeping the
// origin/cell size stable makes old exploration meaningful after an app update.
pub const GRID_W: usize = 112;
pub const GRID_H: usize = 100;
pub const CELL: f32 = 10.0;
pub const ORIGIN: Vec2 = Vec2::new(-560.0, -500.0);
pub const MAX_NOTES: usize = 128;
const REVEAL_RADIUS: f32 = 38.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    Supplies,
    Danger,
    Safe,
    Evacuation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapNote {
    pub position: [f32; 2],
    pub kind: NoteKind,
}

#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MapProgress {
    pub revealed: BTreeSet<u16>,
    pub notes: Vec<MapNote>,
}

impl MapProgress {
    pub fn validate(&self) -> bool {
        self.revealed
            .iter()
            .all(|i| (*i as usize) < GRID_W * GRID_H)
            && self.notes.len() <= MAX_NOTES
            && self
                .notes
                .iter()
                .all(|note| cell_of(Vec2::from_array(note.position)).is_some())
    }

    pub fn explored(&self, p: Vec2) -> bool {
        cell_of(p).is_some_and(|(x, y)| self.revealed.contains(&((y * GRID_W + x) as u16)))
    }

    /// Always reveal the cell containing the player, including at cell edges.
    pub fn reveal(&mut self, p: Vec2, radius: f32) -> bool {
        let Some((cx, cy)) = cell_of(p) else {
            return false;
        };
        if !radius.is_finite() || radius < 0.0 {
            return false;
        }
        let before = self.revealed.len();
        let span = (radius.min(2000.0) / CELL).ceil() as usize + 1;
        for y in cy.saturating_sub(span)..=(cy + span).min(GRID_H - 1) {
            for x in cx.saturating_sub(span)..=(cx + span).min(GRID_W - 1) {
                let center = ORIGIN + Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * CELL;
                if (x == cx && y == cy) || center.distance_squared(p) <= radius * radius {
                    self.revealed.insert((y * GRID_W + x) as u16);
                }
            }
        }
        before != self.revealed.len()
    }

    pub fn add_note(&mut self, p: Vec2, kind: NoteKind) -> bool {
        if cell_of(p).is_none() || self.notes.len() >= MAX_NOTES {
            return false;
        }
        // A double tap or a repeated release must not duplicate one mark.
        if self
            .notes
            .iter()
            .any(|n| n.kind == kind && Vec2::from_array(n.position).distance(p) < 3.0)
        {
            return false;
        }
        self.notes.push(MapNote {
            position: p.to_array(),
            kind,
        });
        true
    }

    pub fn remove_near(&mut self, p: Vec2, radius: f32) -> bool {
        let closest = self
            .notes
            .iter()
            .enumerate()
            .map(|(i, n)| (i, Vec2::from_array(n.position).distance_squared(p)))
            .filter(|(_, d)| *d <= radius * radius)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = closest {
            self.notes.remove(i);
            true
        } else {
            false
        }
    }
}

fn cell_of(p: Vec2) -> Option<(usize, usize)> {
    if !p.is_finite() {
        return None;
    }
    let q = (p - ORIGIN) / CELL;
    if q.x < 0.0 || q.y < 0.0 || q.x >= GRID_W as f32 || q.y >= GRID_H as f32 {
        return None;
    }
    Some((q.x.floor() as usize, q.y.floor() as usize))
}

#[derive(Resource, Default, Debug)]
pub(crate) struct MapOpen(pub bool);

#[derive(Message, Clone, Copy, Debug)]
pub(crate) struct ToggleMap;

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MapInputSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MapTool {
    #[default]
    Pan,
    Mark(NoteKind),
    Erase,
}

#[derive(Resource, Debug)]
pub(crate) struct MapView {
    pub center: Vec2,
    /// Logical screen pixels per world metre.
    pub scale: f32,
    pub tool: MapTool,
    pub revision: u64,
}

impl Default for MapView {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            scale: 1.8,
            tool: MapTool::Pan,
            revision: 0,
        }
    }
}

impl MapView {
    pub fn screen(&self, p: Vec2, rect: ScreenRect) -> Vec2 {
        rect.center() + (p - self.center) * self.scale
    }
    pub fn world(&self, p: Vec2, rect: ScreenRect) -> Vec2 {
        self.center + (p - rect.center()) / self.scale
    }
    fn constrain(&mut self) {
        self.scale = self.scale.clamp(0.08, 5.0);
        self.center = self.center.clamp(
            ORIGIN,
            ORIGIN + Vec2::new(GRID_W as f32, GRID_H as f32) * CELL,
        );
    }
    fn zoom_at(&mut self, factor: f32, anchor: Vec2, rect: ScreenRect) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let before = self.world(anchor, rect);
        self.scale = (self.scale * factor).clamp(0.08, 5.0);
        self.center = before - (anchor - rect.center()) / self.scale;
        self.constrain();
        self.revision += 1;
    }
    pub fn center_on(&mut self, p: Vec2) {
        self.center = p;
        self.scale = self.scale.max(1.8);
        self.constrain();
        self.revision += 1;
    }
    fn fit(&mut self, rect: ScreenRect) {
        self.center = ORIGIN + Vec2::new(GRID_W as f32, GRID_H as f32) * CELL * 0.5;
        self.scale = (rect.width() / (GRID_W as f32 * CELL))
            .min(rect.height() / (GRID_H as f32 * CELL))
            * 0.94;
        self.constrain();
        self.revision += 1;
    }
}

#[derive(Clone, Copy)]
enum Button {
    Close,
    ZoomIn,
    ZoomOut,
    Center,
    Fit,
    Tool(MapTool),
}

pub(crate) struct MapLayout {
    pub content: ScreenRect,
    pub header: ScreenRect,
    pub footer: ScreenRect,
    buttons: Vec<(ScreenRect, Button)>,
}

pub(crate) fn map_layout(viewport: Vec2) -> MapLayout {
    let safe = safe_insets(viewport);
    let x = safe.left;
    let width = (viewport.x - safe.left - safe.right).max(1.0);
    let top = safe.top + 4.0;
    let footer_height = if viewport.y < 500.0 { 82.0 } else { 108.0 };
    let bottom = viewport.y - safe.bottom - footer_height;
    let content = ScreenRect::new(x, top + 82.0, width, (bottom - top - 90.0).max(20.0));
    let footer = ScreenRect::new(x, content.max.y + 8.0, width, footer_height);
    let mut buttons = vec![
        (
            ScreenRect::new(x + width - 64.0, top, 64.0, 34.0),
            Button::Close,
        ),
        (ScreenRect::new(x, top + 40.0, 42.0, 34.0), Button::ZoomOut),
        (
            ScreenRect::new(x + 48.0, top + 40.0, 42.0, 34.0),
            Button::ZoomIn,
        ),
        (
            ScreenRect::new(x + 96.0, top + 40.0, (width - 108.0) * 0.5, 34.0),
            Button::Center,
        ),
        (
            ScreenRect::new(
                x + 102.0 + (width - 108.0) * 0.5,
                top + 40.0,
                (width - 108.0) * 0.5,
                34.0,
            ),
            Button::Fit,
        ),
    ];
    let tools = [
        MapTool::Pan,
        MapTool::Mark(NoteKind::Supplies),
        MapTool::Mark(NoteKind::Danger),
        MapTool::Mark(NoteKind::Safe),
        MapTool::Mark(NoteKind::Evacuation),
        MapTool::Erase,
    ];
    let bw = (width - 5.0 * 4.0) / 6.0;
    buttons.extend(tools.into_iter().enumerate().map(|(i, tool)| {
        (
            ScreenRect::new(x + i as f32 * (bw + 4.0), footer.min.y, bw, 38.0),
            Button::Tool(tool),
        )
    }));
    MapLayout {
        content,
        header: ScreenRect::new(x, top, width - 72.0, 34.0),
        footer,
        buttons,
    }
}

pub(crate) fn toggle_rect(viewport: Vec2) -> ScreenRect {
    let layout = HudLayout::from_viewport(viewport);
    if let Some(crouch) = layout.sec {
        ScreenRect::new(
            viewport.x - layout.insets.right - 72.0,
            crouch.min.y - 54.0,
            64.0,
            44.0,
        )
    } else {
        ScreenRect::new(
            viewport.x - layout.insets.right - 84.0,
            viewport.y - layout.insets.bottom - 110.0,
            76.0,
            44.0,
        )
    }
}

pub(crate) fn captures_pointer(viewport: Vec2, p: Vec2, open: bool) -> bool {
    open || toggle_rect(viewport).contains(p)
}

#[derive(Clone)]
struct PointerTrack {
    last: Vec2,
    origin: Vec2,
    moved: bool,
}

#[derive(Resource, Default)]
struct MapGesture {
    pointers: BTreeMap<u64, PointerTrack>,
    ignored: BTreeSet<u64>,
    pinched: bool,
}

pub struct WorldMapPlugin;
impl Plugin for WorldMapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MapProgress>()
            .init_resource::<MapOpen>()
            .init_resource::<MapView>()
            .init_resource::<MapGesture>()
            .init_resource::<render::MapCache>()
            .add_message::<ToggleMap>()
            .configure_sets(Update, MapInputSet.before(crate::input::InputSet))
            .add_systems(
                Update,
                (close_for_panels, input).chain().in_set(MapInputSet),
            )
            .add_systems(PostUpdate, discover.before(render::render))
            .add_systems(
                PostUpdate,
                render::render.before(bevy::ui::UiSystems::Prepare),
            );
    }
}

fn close_for_panels(
    mut panels: MessageReader<crate::ui::OpenPanel>,
    mut open: ResMut<MapOpen>,
    mut gesture: ResMut<MapGesture>,
) {
    if panels.read().count() > 0 && open.0 {
        open.0 = false;
        *gesture = MapGesture::default();
        println!("DTMAP open=false");
    }
}

fn discover(
    flow: Res<State<GameFlow>>,
    mut starts: MessageReader<StartGame>,
    player: Query<&Transform, With<Player>>,
    mut progress: ResMut<MapProgress>,
    mut last: Local<Option<Vec2>>,
    mut view: ResMut<MapView>,
) {
    if starts.read().count() > 0 {
        *progress = MapProgress::default();
        *last = None;
        view.tool = MapTool::Pan;
    }
    if !matches!(flow.get(), GameFlow::Prologue | GameFlow::Playing) {
        return;
    }
    let Some(t) = player.iter().next() else {
        return;
    };
    let p = Vec2::new(t.translation.x, t.translation.z);
    if last.is_none_or(|old| old.distance_squared(p) >= 9.0) || !progress.explored(p) {
        progress.reveal(p, REVEAL_RADIUS);
        *last = Some(p);
    }
}

fn input(
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    windows: Query<&Window, With<PrimaryWindow>>,
    player: Query<&Transform, With<Player>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    scroll: Res<AccumulatedMouseScroll>,
    consumed: Res<ConsumedPresses>,
    mut toggles: MessageReader<ToggleMap>,
    mut open: ResMut<MapOpen>,
    mut view: ResMut<MapView>,
    mut gesture: ResMut<MapGesture>,
    mut progress: ResMut<MapProgress>,
    mut modal: ResMut<UiModal>,
    mut saves: MessageWriter<SaveGame>,
) {
    let toggled = toggles.read().count() % 2 == 1;
    if flow.get() != &GameFlow::Playing || pause.0 {
        open.0 = false;
        *gesture = MapGesture::default();
        return;
    }
    let Some(window) = windows.iter().next() else {
        return;
    };
    let viewport = Vec2::new(window.width(), window.height());
    let layout = map_layout(viewport);
    let pos = player
        .iter()
        .next()
        .map_or(Vec2::ZERO, |t| Vec2::new(t.translation.x, t.translation.z));
    let map_key = keys.just_pressed(KeyCode::KeyM);
    let close_key = open.0 && keys.just_pressed(KeyCode::Escape);
    if map_key {
        keys.clear_just_pressed(KeyCode::KeyM);
    }
    if close_key {
        keys.clear_just_pressed(KeyCode::Escape);
    }
    let clicked_open = !open.0
        && !modal.open
        && consumed
            .points
            .iter()
            .any(|p| toggle_rect(viewport).contains(*p));
    if close_key || ((toggled || map_key || clicked_open) && (open.0 || !modal.open)) {
        open.0 = !open.0;
        modal.open = open.0;
        *gesture = MapGesture::default();
        if open.0 {
            view.center_on(pos);
            gesture
                .ignored
                .extend(touches.iter().map(|touch| touch.id()));
            if mouse.pressed(MouseButton::Left) {
                gesture.ignored.insert(u64::MAX);
            }
        }
        println!("DTMAP open={}", open.0);
        return;
    }
    if !open.0 {
        return;
    }
    modal.open = true;
    for point in &consumed.points {
        if let Some((_, action)) = layout
            .buttons
            .iter()
            .find(|(rect, _)| rect.contains(*point))
        {
            match action {
                Button::Close => {
                    open.0 = false;
                    modal.open = false;
                    println!("DTMAP open=false");
                }
                Button::ZoomIn => view.zoom_at(1.35, layout.content.center(), layout.content),
                Button::ZoomOut => {
                    view.zoom_at(1.0 / 1.35, layout.content.center(), layout.content)
                }
                Button::Center => view.center_on(pos),
                Button::Fit => view.fit(layout.content),
                Button::Tool(tool) => {
                    view.tool = *tool;
                    view.revision += 1;
                }
            }
            *gesture = MapGesture::default();
        }
    }
    if !open.0 {
        return;
    }
    let dy = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    };
    if dy != 0.0 {
        let anchor = window
            .cursor_position()
            .filter(|p| layout.content.contains(*p))
            .unwrap_or(layout.content.center());
        view.zoom_at(1.15f32.powf(dy), anchor, layout.content);
    }
    let mut active: BTreeMap<u64, Vec2> = touches.iter().map(|t| (t.id(), t.position())).collect();
    if active.is_empty()
        && mouse.pressed(MouseButton::Left)
        && let Some(p) = window.cursor_position()
    {
        active.insert(u64::MAX, p);
    }
    let released: Vec<_> = gesture
        .pointers
        .keys()
        .filter(|id| !active.contains_key(id))
        .copied()
        .collect();
    for id in released {
        if let Some(track) = gesture.pointers.remove(&id)
            && !track.moved
            && !gesture.pinched
            && layout.content.contains(track.last)
        {
            let p = view.world(track.last, layout.content);
            let changed = match view.tool {
                MapTool::Pan => false,
                MapTool::Mark(kind) => progress.add_note(p, kind),
                MapTool::Erase => progress.remove_near(p, 18.0 / view.scale),
            };
            if changed {
                view.revision += 1;
                saves.write(SaveGame);
                println!("DTMAP notes={}", progress.notes.len());
            }
        }
    }
    gesture.ignored.retain(|id| active.contains_key(id));
    if active.is_empty() {
        gesture.pinched = false;
    }
    // Only a gesture that began on the paper can pan it. A held toolbar button
    // cannot become a drag when the finger crosses into the map.
    for (&id, &p) in &active {
        if !layout.content.contains(p) && !gesture.pointers.contains_key(&id) {
            gesture.ignored.insert(id);
        }
        if layout.content.contains(p)
            && !gesture.pointers.contains_key(&id)
            && !gesture.ignored.contains(&id)
        {
            gesture.pointers.insert(
                id,
                PointerTrack {
                    last: p,
                    origin: p,
                    moved: false,
                },
            );
        }
    }
    let tracked: Vec<_> = active
        .iter()
        .filter(|(id, _)| gesture.pointers.contains_key(id))
        .map(|(id, p)| (*id, *p))
        .collect();
    if tracked.len() >= 2 {
        let (a, pa) = tracked[0];
        let (b, pb) = tracked[1];
        let oa = gesture.pointers[&a].last;
        let ob = gesture.pointers[&b].last;
        let old_distance = oa.distance(ob);
        if old_distance > 2.0 {
            let old_center = (oa + ob) * 0.5;
            let center = (pa + pb) * 0.5;
            let scale = view.scale;
            view.center -= (center - old_center) / scale;
            view.zoom_at(pa.distance(pb) / old_distance, center, layout.content);
        }
        gesture.pinched = true;
        for track in gesture.pointers.values_mut() {
            track.moved = true;
        }
    } else if let Some(&(id, p)) = tracked.first() {
        let old = &gesture.pointers[&id];
        let delta = p - old.last;
        if delta.length_squared() > 0.0 {
            let scale = view.scale;
            view.center -= delta / scale;
            view.constrain();
            view.revision += 1;
        }
    }
    for (id, p) in tracked {
        let track = gesture.pointers.get_mut(&id).expect("tracked pointer");
        track.moved |= p.distance(track.origin) > 6.0;
        track.last = p;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_covers_the_containing_cell_and_is_monotonic() {
        let mut p = MapProgress::default();
        let corner = ORIGIN + Vec2::splat(0.01);
        assert!(p.reveal(corner, 0.0));
        assert!(p.explored(corner));
        assert!(!p.reveal(corner, 0.0));
        assert!(p.reveal(corner + Vec2::splat(30.0), 38.0));
        assert!(p.explored(corner));
        assert!(p.validate());
        assert!(!p.reveal(Vec2::splat(f32::NAN), 38.0));
    }

    #[test]
    fn notes_are_bounded_deduplicated_and_survive_json_roundtrip() {
        let mut p = MapProgress::default();
        assert!(p.add_note(Vec2::ZERO, NoteKind::Supplies));
        assert!(!p.add_note(Vec2::ONE, NoteKind::Supplies));
        assert!(!p.add_note(Vec2::splat(f32::INFINITY), NoteKind::Danger));
        p.reveal(Vec2::ZERO, 38.0);
        let restored: MapProgress =
            serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
        assert_eq!(p, restored);
        assert!(p.remove_near(Vec2::ONE, 3.0));
        assert!(!p.remove_near(Vec2::ONE, 3.0));
        for i in 0..MAX_NOTES {
            assert!(p.add_note(
                Vec2::new((i % 32) as f32 * 6.0, (i / 32) as f32 * 6.0),
                NoteKind::Safe
            ));
        }
        assert!(!p.add_note(Vec2::new(0.0, 100.0), NoteKind::Danger));
    }

    #[test]
    fn corrupt_map_data_cannot_enter_a_validated_save() {
        let mut p = MapProgress::default();
        p.revealed.insert(u16::MAX);
        assert!(!p.validate());
        p.revealed.clear();
        p.notes.push(MapNote {
            position: [9999.0, 0.0],
            kind: NoteKind::Danger,
        });
        assert!(!p.validate());
    }

    #[test]
    fn map_coordinates_roundtrip_and_zoom_preserves_the_anchor() {
        let rect = map_layout(Vec2::new(390.0, 844.0)).content;
        let mut view = MapView {
            center: Vec2::new(110.0, -230.0),
            ..default()
        };
        let point = rect.center() + Vec2::new(40.0, -30.0);
        let world = view.world(point, rect);
        assert!(view.screen(world, rect).distance(point) < 0.001);
        view.zoom_at(1.35, point, rect);
        assert!(view.world(point, rect).distance(world) < 0.001);
    }

    #[test]
    fn map_controls_stay_inside_phone_insets_and_clear_gameplay_pads() {
        for size in [
            Vec2::new(320.0, 568.0),
            Vec2::new(390.0, 844.0),
            Vec2::new(800.0, 360.0),
            Vec2::new(1280.0, 720.0),
        ] {
            let layout = map_layout(size);
            assert!(layout.content.height() >= 100.0);
            for (i, (a, _)) in layout.buttons.iter().enumerate() {
                assert!(a.min.cmpge(Vec2::ZERO).all() && a.max.cmple(size).all());
                assert!(a.width() >= 40.0 && a.height() >= 34.0);
                for (b, _) in layout.buttons.iter().skip(i + 1) {
                    assert!(!a.intersects(*b));
                }
            }
            let button = toggle_rect(size);
            let pads = HudLayout::from_viewport(size);
            assert!(!button.intersects(pads.joy));
            assert!(pads.act.is_none_or(|r| !button.intersects(r)));
            assert!(pads.sec.is_none_or(|r| !button.intersects(r)));
        }
    }
}
