//! A bounded, cached paper-map image plus crisp native text and controls.
use std::collections::HashSet;

use bevy::{prelude::*, window::PrimaryWindow};

use crate::game_flow::GameFlow;
use crate::i18n::{Language, UiFont, tr};
use crate::input::ScreenRect;
use crate::map::EvacSites;
use crate::map::canvas::{Canvas, css};
use crate::map::data::{Bld, WorldData};
use crate::player::Player;
use crate::survival::{Shelter, TRADE_SPOTS};
use crate::ui::{PauseOpen, UiModal};

use super::{
    Button, CELL, GRID_H, GRID_W, MAX_NOTES, MapOpen, MapProgress, MapTool, MapView, NoteKind,
    ORIGIN, map_layout, toggle_rect,
};

const PAPER: Color = Color::srgb(0.91, 0.87, 0.77);
const GOLD: Color = Color::srgb(0.82, 0.68, 0.40);
const MUTED: Color = Color::srgb(0.68, 0.67, 0.60);
const INK: Color = Color::srgb(0.08, 0.09, 0.085);

#[derive(Clone, PartialEq)]
struct RenderKey {
    open: bool,
    viewport: Vec2,
    language: Language,
    revision: u64,
    player: IVec2,
    heading: i32,
    progress_tick: u32,
    home: Vec2,
    evac: Option<Vec2>,
}

#[derive(Resource, Default)]
pub(super) struct MapCache {
    key: Option<RenderKey>,
    root: Option<Entity>,
    image: Option<Handle<Image>>,
    next_draw: f64,
}

struct Label {
    rect: ScreenRect,
    copy: String,
    color: Color,
    size: f32,
    backing: bool,
}

const PLAYER_RING_RADIUS: f32 = 8.5;
const PLAYER_DOT_RADIUS: f32 = 5.5;

/// One logical-screen shape drives both the rasterized pointer and label exclusion.
/// Its directional bounds keep adjacent names visible when the arrow points away.
struct PlayerPointer {
    center: Vec2,
    arrow: [Vec2; 3],
}

impl PlayerPointer {
    fn new(center: Vec2, heading: f32) -> Self {
        let direction = Vec2::new(heading.sin(), heading.cos());
        let tangent = Vec2::new(direction.y, -direction.x);
        Self {
            center,
            arrow: [
                center + direction * 15.0,
                center + direction * 4.0 + tangent * 5.0,
                center + direction * 4.0 - tangent * 5.0,
            ],
        }
    }

    fn bounds(&self) -> ScreenRect {
        let mut min = self.center - PLAYER_RING_RADIUS;
        let mut max = self.center + PLAYER_RING_RADIUS;
        for point in self.arrow {
            min = min.min(point);
            max = max.max(point);
        }
        ScreenRect { min, max }
    }
}

pub(super) fn render(
    mut commands: Commands,
    flow: Res<State<GameFlow>>,
    pause: Res<PauseOpen>,
    modal: Res<UiModal>,
    windows: Query<&Window, With<PrimaryWindow>>,
    player: Query<&Transform, With<Player>>,
    open: Res<MapOpen>,
    view: Res<MapView>,
    progress: Res<MapProgress>,
    lang: Res<Language>,
    font: Res<UiFont>,
    shelter: Res<Shelter>,
    evac: Res<EvacSites>,
    time: Res<Time<Real>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<MapCache>,
) {
    let visible = flow.get() == &GameFlow::Playing && !pause.0 && (open.0 || !modal.open);
    if !visible {
        if let Some(root) = cache.root.take() {
            commands.entity(root).despawn();
        }
        cache.key = None;
        return;
    }
    let Some(window) = windows.iter().next() else {
        return;
    };
    let viewport = Vec2::new(window.width(), window.height());
    let Some(transform) = player.iter().next() else {
        return;
    };
    let p = Vec2::new(transform.translation.x, transform.translation.z);
    let forward = transform.rotation * Vec3::NEG_Z;
    let heading = forward.x.atan2(forward.z);
    let key = RenderKey {
        open: open.0,
        viewport,
        language: *lang,
        revision: if open.0 { view.revision } else { 0 },
        player: if open.0 {
            (p * 0.5).as_ivec2()
        } else {
            IVec2::ZERO
        },
        heading: if open.0 { (heading * 10.0) as i32 } else { 0 },
        progress_tick: if open.0 {
            progress.last_changed().get()
        } else {
            0
        },
        home: shelter
            .home
            .map(Vec2::from_array)
            .unwrap_or(crate::map::world().spawn),
        evac: (open.0 && evac.heli_open).then_some(evac.heli),
    };
    if cache.key.as_ref() == Some(&key) {
        return;
    }
    let now = time.elapsed_secs_f64();
    // Keep gesture redraw bounded. Opening, closing and rotation are immediate.
    let same_frame = cache
        .key
        .as_ref()
        .is_some_and(|old| old.open == key.open && old.viewport == key.viewport);
    if open.0 && same_frame && now < cache.next_draw {
        return;
    }
    cache.next_draw = now + 0.10;
    if let Some(root) = cache.root.take() {
        commands.entity(root).despawn();
    }
    if !open.0 {
        let root = commands
            .spawn((
                GlobalZIndex(19),
                Node {
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|root| {
                button(
                    root,
                    toggle_rect(viewport),
                    tr(*lang, "地圖 M", "Map M"),
                    false,
                    &font,
                )
            })
            .id();
        cache.root = Some(root);
        cache.key = Some(key);
        return;
    }
    let layout = map_layout(viewport);
    let (canvas, labels) = draw_paper(
        crate::map::world(),
        &view,
        &progress,
        layout.content,
        p,
        heading,
        key.home,
        key.evac,
        *lang,
    );
    let image = canvas.to_image(true, false);
    let handle = if let Some(handle) = cache.image.clone() {
        if let Some(mut previous) = images.get_mut(&handle) {
            *previous = image;
            handle
        } else {
            images.add(image)
        }
    } else {
        images.add(image)
    };
    cache.image = Some(handle.clone());
    let root = commands
        .spawn((
            GlobalZIndex(60),
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
            BackgroundColor(Color::srgb(0.065, 0.075, 0.07)),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            text(
                root,
                layout.header,
                tr(*lang, "萬華 · 生存者地圖", "Wanhua · Survivor map"),
                if viewport.x < 350.0 { 18.0 } else { 20.0 },
                PAPER,
                &font,
                false,
            );
            root.spawn((
                node(layout.content),
                ImageNode::new(handle).with_mode(NodeImageMode::Stretch),
                BorderColor::all(Color::srgb(0.42, 0.39, 0.30)),
            ));
            for label in labels {
                if label.backing {
                    root.spawn((
                        node(label.rect),
                        BackgroundColor(Color::srgba(0.94, 0.91, 0.83, 0.90)),
                    ));
                }
                text(
                    root,
                    label.rect,
                    &label.copy,
                    label.size,
                    label.color,
                    &font,
                    true,
                );
            }
            for &(rect, action) in &layout.buttons {
                let label = match action {
                    Button::Close => tr(*lang, "關閉", "Close"),
                    Button::ZoomIn => "+",
                    Button::ZoomOut => "−",
                    Button::Center => tr(*lang, "我的位置", "Center"),
                    Button::Fit => tr(*lang, "全區", "Whole core"),
                    Button::Tool(MapTool::Pan) => tr(*lang, "拖曳", "Pan"),
                    Button::Tool(MapTool::Erase) => tr(*lang, "刪除", "Erase"),
                    Button::Tool(MapTool::Mark(kind)) => note_label(kind, *lang),
                };
                let selected = matches!(action, Button::Tool(tool) if tool == view.tool);
                button(root, rect, label, selected, &font);
            }
            let status = match view.tool {
                MapTool::Pan => tr(
                    *lang,
                    "拖曳平移 · 雙指／滾輪縮放",
                    "Drag to pan · Pinch / scroll to zoom",
                ),
                MapTool::Mark(_) if progress.notes.len() >= MAX_NOTES => tr(
                    *lang,
                    "註記已滿，請先刪除舊標記",
                    "Note limit reached; erase an old mark",
                ),
                MapTool::Mark(_) => tr(*lang, "輕點地圖放置註記", "Tap the map to place a mark"),
                MapTool::Erase => tr(
                    *lang,
                    "輕點自己的註記以刪除",
                    "Tap one of your marks to erase it",
                ),
            };
            let small = viewport.y < 500.0;
            text(
                root,
                ScreenRect::new(
                    layout.footer.min.x,
                    layout.footer.min.y + 44.0,
                    layout.footer.width(),
                    18.0,
                ),
                status,
                12.0,
                GOLD,
                &font,
                false,
            );
            let stats = format!(
                "{} {:.1}% · {} {} · {}",
                tr(*lang, "探索", "Explored"),
                progress.revealed.len() as f32 / (GRID_W * GRID_H) as f32 * 100.0,
                tr(*lang, "註記", "Notes"),
                progress.notes.len(),
                tr(*lang, "開圖不暫停", "World stays live")
            );
            if !small {
                text(
                    root,
                    ScreenRect::new(
                        layout.footer.min.x,
                        layout.footer.min.y + 64.0,
                        layout.footer.width(),
                        18.0,
                    ),
                    &stats,
                    11.0,
                    MUTED,
                    &font,
                    false,
                );
            }
            text(
                root,
                ScreenRect::new(
                    layout.footer.min.x,
                    layout.footer.min.y + if small { 64.0 } else { 84.0 },
                    layout.footer.width(),
                    16.0,
                ),
                "© OpenStreetMap contributors · ODbL",
                10.0,
                MUTED,
                &font,
                false,
            );
        })
        .id();
    cache.root = Some(root);
    cache.key = Some(key);
}

pub(super) fn note_label(kind: NoteKind, lang: Language) -> &'static str {
    match kind {
        NoteKind::Supplies => tr(lang, "補給", "Supply"),
        NoteKind::Danger => tr(lang, "危險", "Danger"),
        NoteKind::Safe => tr(lang, "安全", "Safe"),
        NoteKind::Evacuation => tr(lang, "撤離", "Evac"),
    }
}

fn note_color(kind: NoteKind) -> [f32; 4] {
    css(match kind {
        NoteKind::Supplies => "#3b733a",
        NoteKind::Danger => "#a23d2c",
        NoteKind::Safe => "#a47420",
        NoteKind::Evacuation => "#365f8a",
    })
}

fn zone(building: &Bld) -> [f32; 4] {
    if building.name.chars().any(|c| "寺宮廟殿祠".contains(c)) {
        css("#c9a35a")
    } else {
        css(match building.kind {
            "store" | "food" => "#d7b98a",
            "pharmacy" => "#d8a0a3",
            "police" => "#9cacc3",
            _ => "#bdb6a2",
        })
    }
}

fn draw_paper(
    data: &WorldData,
    view: &MapView,
    progress: &MapProgress,
    rect: ScreenRect,
    player: Vec2,
    heading: f32,
    home: Vec2,
    evac: Option<Vec2>,
    lang: Language,
) -> (Canvas, Vec<Label>) {
    let ratio = (1000.0 / rect.width()).min(800.0 / rect.height()).min(1.0);
    let width = (rect.width() * ratio).ceil().max(1.0) as usize;
    let height = (rect.height() * ratio).ceil().max(1.0) as usize;
    let mut c = Canvas::new(width, height);
    c.fill_rect(0.0, 0.0, width as f32, height as f32, css("#e8dfc8"));
    let px = |p: Vec2| (view.screen(p, rect) - rect.min) * ratio;
    let world_min = view.world(rect.min, rect);
    let world_max = view.world(rect.max, rect);
    // Subtle paper grid. Its metre spacing stays attached to the world.
    for axis in 0..2 {
        let lo = (world_min[axis] / 50.0).floor() as i32;
        let hi = (world_max[axis] / 50.0).ceil() as i32;
        for v in lo..=hi {
            let mut a = world_min;
            let mut b = world_max;
            a[axis] = v as f32 * 50.0;
            b[axis] = a[axis];
            let a = px(a);
            let b = px(b);
            c.line(a.x, a.y, b.x, b.y, 0.7, css("#ded4bc"));
        }
    }
    for b in &data.blds {
        if b.max.x < world_min.x
            || b.min.x > world_max.x
            || b.max.y < world_min.y
            || b.min.y > world_max.y
        {
            continue;
        }
        let points: Vec<_> = b.pts.iter().map(|p| px(*p)).collect();
        let clipped = clip_poly(points, Vec2::new(width as f32, height as f32));
        let polygon: Vec<_> = clipped.iter().map(|p| (p.x, p.y)).collect();
        c.fill_poly(&polygon, zone(b));
        if view.scale > 0.7 {
            for i in 0..b.pts.len() {
                let a = px(b.pts[i]);
                let b = px(b.pts[(i + 1) % b.pts.len()]);
                if let Some((a, b)) = clip_line(a, b, width as f32, height as f32) {
                    c.line(a.x, a.y, b.x, b.y, 0.6, css("#a49c88"));
                }
            }
        }
    }
    for road in &data.roads {
        for pair in road.p.windows(2) {
            let a = px(pair[0]);
            let b = px(pair[1]);
            if let Some((a, b)) = clip_line(a, b, width as f32, height as f32) {
                let thickness = (road.w * view.scale * ratio).max(1.1);
                c.line(a.x, a.y, b.x, b.y, thickness + 1.6, css("#b8ae96"));
                c.line(
                    a.x,
                    a.y,
                    b.x,
                    b.y,
                    thickness,
                    css(if road.w >= 9.0 { "#faf4df" } else { "#f0e9d5" }),
                );
            }
        }
    }
    // Opaque, irregularly toned paper fog prevents unknown streets and labels
    // from becoming an accidental live intelligence map.
    let lo = ((world_min - ORIGIN) / CELL)
        .floor()
        .max(Vec2::ZERO)
        .as_uvec2();
    let hi = ((world_max - ORIGIN) / CELL)
        .ceil()
        .min(Vec2::new(GRID_W as f32, GRID_H as f32))
        .max(Vec2::ZERO)
        .as_uvec2();
    for y in lo.y as usize..hi.y as usize {
        for x in lo.x as usize..hi.x as usize {
            if progress.revealed.contains(&((y * GRID_W + x) as u16)) {
                continue;
            }
            let p = px(ORIGIN + Vec2::new(x as f32, y as f32) * CELL);
            let shade = 0.19 + ((x * 17 + y * 31) % 5) as f32 * 0.004;
            c.fill_rect(
                p.x,
                p.y,
                CELL * view.scale * ratio + 0.3,
                CELL * view.scale * ratio + 0.3,
                [shade, shade + 0.015, shade, 1.0],
            );
        }
    }
    let mut labels = Vec::new();
    let mut used_names = HashSet::new();
    if view.scale >= 0.45 {
        for road in &data.roads {
            if road.name.is_empty() || used_names.contains(&road.name) || labels.len() >= 12 {
                continue;
            }
            for pair in road.p.windows(2) {
                let midpoint = (pair[0] + pair[1]) * 0.5;
                let p = view.screen(midpoint, rect);
                if !progress.explored(midpoint) || !rect.contains(p) {
                    continue;
                }
                let label_w = (road.name.chars().count() as f32 * 12.0 + 8.0).min(150.0);
                let r = ScreenRect::new(p.x - label_w * 0.5, p.y - 8.0, label_w, 17.0);
                if !rect.contains(r.min) || r.max.x > rect.max.x || r.max.y > rect.max.y {
                    continue;
                }
                if labels.iter().any(|old: &Label| old.rect.intersects(r)) {
                    continue;
                }
                labels.push(Label {
                    rect: r,
                    copy: road.name.clone(),
                    color: INK,
                    size: 11.0,
                    backing: true,
                });
                used_names.insert(road.name.clone());
                break;
            }
        }
    }
    for note in &progress.notes {
        let p = Vec2::from_array(note.position);
        marker(&mut c, &px, p, note_color(note.kind), ratio, false);
        add_marker_label(&mut labels, view, rect, p, note_label(note.kind, lang), INK);
    }
    marker(&mut c, &px, home, css("#c39842"), ratio, true);
    add_marker_label(
        &mut labels,
        view,
        rect,
        home,
        tr(lang, "避難所", "Shelter"),
        INK,
    );
    for spot in TRADE_SPOTS {
        let position = Vec2::from_array(spot.pos);
        if progress.explored(position) {
            marker(&mut c, &px, position, css("#568468"), ratio, true);
            add_marker_label(
                &mut labels,
                view,
                rect,
                position,
                tr(lang, "交易點", "Trade"),
                INK,
            );
        }
    }
    if let Some(evac) = evac {
        marker(&mut c, &px, evac, css("#568aca"), ratio, true);
        add_marker_label(
            &mut labels,
            view,
            rect,
            evac,
            tr(lang, "撤離開放", "Extraction open"),
            INK,
        );
    }
    let pointer = PlayerPointer::new(view.screen(player, rect), heading);
    let p = (pointer.center - rect.min) * ratio;
    c.fill_ellipse(
        p.x,
        p.y,
        PLAYER_RING_RADIUS * ratio,
        PLAYER_RING_RADIUS * ratio,
        css("#f5eed9"),
    );
    c.fill_ellipse(
        p.x,
        p.y,
        PLAYER_DOT_RADIUS * ratio,
        PLAYER_DOT_RADIUS * ratio,
        css("#ae3e2c"),
    );
    let arrow = pointer.arrow.map(|p| (p - rect.min) * ratio);
    c.fill_poly(&arrow.map(|p| (p.x, p.y)), css("#ae3e2c"));
    let scale_metres = if view.scale > 2.0 {
        25.0
    } else if view.scale > 1.0 {
        50.0
    } else {
        100.0
    };
    let len = scale_metres * view.scale * ratio;
    c.fill_rect(
        12.0 * ratio,
        height as f32 - 18.0 * ratio,
        len,
        2.0 * ratio,
        css("#f4edd9"),
    );
    labels.push(Label {
        rect: ScreenRect::new(rect.min.x + 12.0, rect.max.y - 42.0, 52.0, 19.0),
        copy: format!("{scale_metres:.0} m"),
        color: INK,
        size: 11.0,
        backing: true,
    });
    labels.push(Label {
        rect: ScreenRect::new(rect.max.x - 32.0, rect.min.y + 10.0, 24.0, 40.0),
        copy: "N\n↑".into(),
        color: INK,
        size: 13.0,
        backing: true,
    });
    // Labels are native UI layered above the canvas. Drawing the player last in the
    // image alone cannot protect it from a road-name backing at the same location.
    let player_bounds = pointer.bounds();
    labels.retain(|label| !label.rect.intersects(player_bounds));
    (c, labels)
}

fn marker(
    c: &mut Canvas,
    px: &impl Fn(Vec2) -> Vec2,
    world: Vec2,
    color: [f32; 4],
    ratio: f32,
    square: bool,
) {
    let p = px(world);
    if p.x < -15.0 || p.y < -15.0 || p.x > c.w as f32 + 15.0 || p.y > c.h as f32 + 15.0 {
        return;
    }
    if square {
        c.fill_rect(
            p.x - 6.0 * ratio,
            p.y - 6.0 * ratio,
            12.0 * ratio,
            12.0 * ratio,
            css("#f5eed9"),
        );
        c.fill_rect(
            p.x - 4.0 * ratio,
            p.y - 4.0 * ratio,
            8.0 * ratio,
            8.0 * ratio,
            color,
        );
    } else {
        c.fill_ellipse(p.x, p.y, 6.5 * ratio, 6.5 * ratio, css("#f5eed9"));
        c.fill_ellipse(p.x, p.y, 4.5 * ratio, 4.5 * ratio, color);
    }
}

fn add_marker_label(
    labels: &mut Vec<Label>,
    view: &MapView,
    rect: ScreenRect,
    p: Vec2,
    copy: &str,
    color: Color,
) {
    let p = view.screen(p, rect);
    let width = (copy.chars().count() as f32 * 12.0).max(32.0);
    let r = ScreenRect::new(p.x - width * 0.5, p.y + 10.0, width, 18.0);
    if rect.contains(r.min) && r.max.x < rect.max.x && r.max.y < rect.max.y {
        // POI/note names take precedence over street names in a crowded block.
        // The pins remain visible even if a neighbouring label is displaced.
        labels.retain(|existing| !existing.rect.intersects(r));
        labels.push(Label {
            rect: r,
            copy: copy.into(),
            color,
            size: 11.0,
            backing: true,
        });
    }
}

fn clip_poly(mut points: Vec<Vec2>, max: Vec2) -> Vec<Vec2> {
    for axis in 0..2 {
        for (edge, greater) in [(0.0, true), (max[axis], false)] {
            let old = std::mem::take(&mut points);
            if old.is_empty() {
                return points;
            }
            let mut a = *old.last().unwrap();
            for b in old {
                let ain = if greater {
                    a[axis] >= edge
                } else {
                    a[axis] <= edge
                };
                let bin = if greater {
                    b[axis] >= edge
                } else {
                    b[axis] <= edge
                };
                if ain != bin {
                    points.push(a + (b - a) * ((edge - a[axis]) / (b[axis] - a[axis])));
                }
                if bin {
                    points.push(b);
                }
                a = b;
            }
        }
    }
    points
}

fn clip_line(a: Vec2, b: Vec2, width: f32, height: f32) -> Option<(Vec2, Vec2)> {
    let d = b - a;
    let mut lo = 0.0f32;
    let mut hi = 1.0f32;
    for (p, q) in [
        (-d.x, a.x),
        (d.x, width - a.x),
        (-d.y, a.y),
        (d.y, height - a.y),
    ] {
        if p.abs() < 1e-6 {
            if q < 0.0 {
                return None;
            }
        } else if p < 0.0 {
            lo = lo.max(q / p);
        } else {
            hi = hi.min(q / p);
        }
        if lo > hi {
            return None;
        }
    }
    Some((a + d * lo, a + d * hi))
}

fn node(rect: ScreenRect) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(rect.min.x),
        top: px(rect.min.y),
        width: px(rect.width()),
        height: px(rect.height()),
        ..default()
    }
}

fn text(
    root: &mut ChildSpawnerCommands,
    rect: ScreenRect,
    value: &str,
    size: f32,
    color: Color,
    font: &UiFont,
    center: bool,
) {
    root.spawn((
        node(rect),
        Text::new(value),
        TextFont::from_font_size(FontSize::Px(size)).with_font(font.0.clone()),
        TextColor(color),
        TextLayout::new(
            if center {
                Justify::Center
            } else {
                Justify::Left
            },
            LineBreak::WordOrCharacter,
        ),
    ));
}

fn button(
    root: &mut ChildSpawnerCommands,
    rect: ScreenRect,
    value: &str,
    selected: bool,
    font: &UiFont,
) {
    root.spawn((
        Node {
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(4.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..node(rect)
        },
        BackgroundColor(if selected {
            GOLD
        } else {
            Color::srgba(0.12, 0.13, 0.12, 0.96)
        }),
        BorderColor::all(if selected {
            GOLD
        } else {
            Color::srgb(0.37, 0.34, 0.27)
        }),
    ))
    .with_child((
        Text::new(value),
        TextFont::from_font_size(FontSize::Px(if rect.width() < 50.0 { 11.0 } else { 13.0 }))
            .with_font(font.0.clone()),
        TextColor(if selected { INK } else { PAPER }),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_map_labels_leave_the_actual_player_pointer_visible() {
        let data = crate::map::world();
        // Actual start-home/player position from the Android save/continue smoke.
        // The nearby 昆明街160巷 label previously hid all but six arrow-tip pixels.
        let home = Vec2::new(146.527_24, -132.237_72);
        assert!(data.bld_at(home).is_some());
        let rect = map_layout(Vec2::new(390.0, 844.0)).content;
        let mut view = MapView::default();
        view.center_on(home);
        let mut progress = MapProgress::default();
        progress.reveal(home, super::super::REVEAL_RADIUS);
        let forward = Transform::default().rotation * Vec3::NEG_Z;
        let default_heading = forward.x.atan2(forward.z);
        for heading in [default_heading, 0.0, std::f32::consts::FRAC_PI_2] {
            let (canvas, labels) = draw_paper(
                data,
                &view,
                &progress,
                rect,
                home,
                heading,
                home,
                None,
                Language::ZhTw,
            );
            let pointer = PlayerPointer::new(view.screen(home, rect), heading);
            assert!(
                labels
                    .iter()
                    .all(|label| !label.rect.intersects(pointer.bounds()))
            );
            let shelter_visible = labels.iter().any(|label| label.copy == "避難所");
            assert_eq!(
                shelter_visible,
                heading != 0.0,
                "keep shelter unless the arrow crosses it"
            );
            // The actual raster still contains a substantial red circle and arrow,
            // and the UI bounds above guarantee that no later label covers them.
            let center = pointer.center - rect.min;
            let red_pixels = canvas
                .px
                .iter()
                .enumerate()
                .filter(|(i, color)| {
                    let p = Vec2::new((i % canvas.w) as f32, (i / canvas.w) as f32);
                    p.distance(center) < 17.0 && color[0] > 0.6 && color[1] < 0.3 && color[2] < 0.25
                })
                .count();
            assert!(
                red_pixels > 100,
                "player raster lost its circle/arrow: {red_pixels}"
            );
        }
    }

    #[test]
    fn clipping_a_long_road_keeps_it_in_view_and_discards_parallel_outside_lines() {
        let (a, b) = clip_line(
            Vec2::new(-800.0, 50.0),
            Vec2::new(800.0, 50.0),
            390.0,
            600.0,
        )
        .unwrap();
        assert_eq!(a, Vec2::new(0.0, 50.0));
        assert!((b.x - 390.0).abs() < 0.001);
        assert!(clip_line(Vec2::new(-8.0, 0.0), Vec2::new(-8.0, 1000.0), 390.0, 600.0).is_none());
    }

    #[test]
    fn clipping_a_footprint_never_rasterizes_unbounded_rows() {
        let poly = clip_poly(
            vec![
                Vec2::splat(-5000.0),
                Vec2::new(5000.0, -5000.0),
                Vec2::splat(5000.0),
                Vec2::new(-5000.0, 5000.0),
            ],
            Vec2::new(390.0, 600.0),
        );
        assert_eq!(poly.len(), 4);
        assert!(
            poly.iter()
                .all(|p| p.cmpge(Vec2::ZERO).all() && p.cmple(Vec2::new(390.0, 600.0)).all())
        );
    }
}
