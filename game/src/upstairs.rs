//! v23.4 / v24.1 上樓 in play (Web `src/core/main.ts` upstairs state and
//! `src/map/upstairs.ts`).
//!
//! At a stairwell landing Interact climbs one floor; on a floor the landing goes
//! down and the flight beside it goes up, to the top storey. Each floor is the
//! building's footprint at its real height with its own furniture and
//! containers. The building's walls rise to the floor and stop in a low ring,
//! the roof is cut away, so the floor reads as upstairs over the real street.
//!
//! Upstairs counts as indoors (sleep works); occupation soldiers cannot follow
//! and lose the player, but they may still be near the door when the player
//! comes down. A save made upstairs records the ground-floor landing; Continue
//! starts there.

use bevy::prelude::*;

use crate::combat::PlayerStance;
use crate::dialogue::{NpcKey, NpcSpot, NpcSpots, TalkTo};
use crate::game_flow::GameFlow;
use crate::geom::{MeshBuf, cuboid, hex, place, shape};
use crate::i18n::{Language, tr};
use crate::map::data::{Seg, seg_dist};
use crate::map::interiors::{Fixture, body_mesh};
use crate::map::upstairs::{Stair, flight_boxes, plan_floor, stair_of};
use crate::player::{PLAYER_RADIUS, Player};
use crate::save::LoadCompleted;
use crate::worldplay::{WorldNotice, WorldSet};

/// Where the player is: the street, or floor `floor` of building `building`.
#[derive(Resource, Default, Clone, Debug)]
pub struct Upstairs {
    at: Option<(Stair, u8)>,
    fixtures: Vec<Fixture>,
    walls: Vec<Seg>,
    pts: Vec<Vec2>,
}

impl Upstairs {
    pub fn active(&self) -> bool {
        self.at.is_some()
    }

    /// (building, floor) while upstairs.
    pub fn floor(&self) -> Option<(u32, u8)> {
        self.at.map(|(s, f)| (s.building, f))
    }

    /// The ground-floor landing a save made upstairs records.
    pub fn landing(&self) -> Option<Vec2> {
        self.at.map(|(s, _)| s.landing)
    }

    /// Height of the floor the player stands on.
    pub fn height(&self) -> f32 {
        self.at.map_or(0.0, |(s, f)| s.floor_y(f))
    }

    /// This floor's furniture (its containers are searchable).
    pub fn fixtures(&self) -> &[Fixture] {
        &self.fixtures
    }

    fn enter(&mut self, stair: Stair, f: u8) {
        let w = crate::map::world();
        let b = &w.blds[stair.building as usize];
        self.fixtures = plan_floor(w, b, &stair, f);
        self.pts = b.pts.clone();
        self.walls = (0..b.pts.len())
            .map(|i| Seg {
                a: b.pts[i],
                b: b.pts[(i + 1) % b.pts.len()],
                h: None,
                bld: Some(b.id),
            })
            .chain(
                self.fixtures
                    .iter()
                    .flat_map(|f| f.segments().collect::<Vec<_>>()),
            )
            .collect();
        self.at = Some((stair, f));
    }

    fn leave(&mut self) {
        *self = Self::default();
    }

    /// Movement on this floor: walls and furniture push the player back, and
    /// the player never leaves the footprint.
    pub fn move_circle(&self, from: Vec2, to: Vec2, r: f32) -> Vec2 {
        let delta = to - from;
        let steps = (delta.length() / (r.max(0.1) * 0.5)).ceil().max(1.0) as u32;
        let step = delta / steps as f32;
        let mut pos = from;
        for _ in 0..steps {
            let mut p = pos + step;
            for _ in 0..2 {
                for s in &self.walls {
                    let d = s.b - s.a;
                    let t = ((p - s.a).dot(d) / d.length_squared().max(1e-12)).clamp(0.0, 1.0);
                    let c = s.a + d * t;
                    let o = p - c;
                    let l = o.length();
                    if l < r && l > 1e-6 {
                        p = c + o / l * r;
                    }
                }
            }
            if !crate::map::data::pip(p, &self.pts) {
                p = pos;
            }
            pos = p;
        }
        pos
    }

    /// Can a fixture of this floor be reached from `p`?
    pub fn reaches(&self, p: Vec2, fixture: &Fixture) -> bool {
        p.distance(fixture.approach) < 1.2 || p.distance(fixture.pos) < 2.2
    }

    fn clear_of_furniture(&self, p: Vec2) -> bool {
        self.walls
            .iter()
            .all(|s| seg_dist(p, s.a, s.b) >= PLAYER_RADIUS)
    }
}

/// Put the player back on the street at once (QA fixtures and tooling).
pub fn force_ground(world: &mut World) {
    world.resource_mut::<Upstairs>().leave();
    world.resource_mut::<PlayerStance>().upstairs = false;
    let views: Vec<Entity> = world
        .query_filtered::<Entity, With<FloorView>>()
        .iter(world)
        .collect();
    for e in views {
        world.despawn(e);
    }
}

/// The current floor's meshes.
#[derive(Component)]
struct FloorView;

/// Ground-floor flights of the stairwells (merged per 80 m cell).
#[derive(Component)]
struct GroundFlights;

#[derive(Resource)]
struct FloorArt {
    furniture: Handle<StandardMaterial>,
    slab: Handle<StandardMaterial>,
}

pub struct UpstairsPlugin;

impl Plugin for UpstairsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Upstairs>()
            .add_systems(Startup, (setup_art, spawn_ground_flights))
            .add_systems(OnEnter(GameFlow::Prologue), go_down_silently)
            .add_systems(OnExit(GameFlow::Playing), go_down_silently)
            .add_systems(PreUpdate, reset_on_load)
            .add_systems(
                Update,
                (publish_stairs, climb)
                    .chain()
                    .in_set(WorldSet)
                    .run_if(in_state(GameFlow::Playing)),
            )
            .add_systems(
                PostUpdate,
                hold_height.before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

fn setup_art(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(FloorArt {
        furniture: materials.add(StandardMaterial {
            perceptual_roughness: 0.88,
            ..default()
        }),
        slab: materials.add(StandardMaterial {
            perceptual_roughness: 0.92,
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
    });
}

fn spawn_ground_flights(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    art: Res<FloorArt>,
) {
    let w = crate::map::world();
    let step = cuboid(1.0, 1.0, 1.0);
    let mut cells: std::collections::BTreeMap<(i32, i32), MeshBuf> = Default::default();
    for s in &w.stairs {
        let key = (
            (s.landing.x / 80.0).floor() as i32,
            (s.landing.y / 80.0).floor() as i32,
        );
        let buf = cells.entry(key).or_default();
        for (c, size, yaw) in flight_boxes(s.landing, s.dir, 0.0, 1.6, 6) {
            buf.add(
                &step,
                place(c.x, c.y, c.z, yaw) * Mat4::from_scale(size),
                hex(0x8a8276),
            );
        }
    }
    for (_, buf) in cells {
        commands.spawn((
            GroundFlights,
            Mesh3d(meshes.add(buf.into_mesh())),
            MeshMaterial3d(art.furniture.clone()),
            Transform::default(),
            Visibility::default(),
        ));
    }
}

fn floor_label(f: u8, lang: Language) -> String {
    match (lang, f) {
        (Language::ZhTw, 1) => "一樓".into(),
        (Language::ZhTw, f) => format!("{f} 樓"),
        (Language::En, 1) => "the ground floor".into(),
        (Language::En, f) => format!("floor {f}"),
    }
}

fn publish_stairs(
    lang: Res<Language>,
    upstairs: Res<Upstairs>,
    player: Query<&Transform, With<Player>>,
    mut spots: ResMut<NpcSpots>,
) {
    let Ok(pt) = player.single() else { return };
    let p = pt.translation.xz();
    let lang = *lang;
    let mut list = Vec::new();
    let spot = |key, pos, label: &str, to: u8| NpcSpot {
        key,
        pos,
        reach: 1.3,
        label: label.to_owned(),
        detail: match lang {
            Language::ZhTw => format!("樓梯（往{}）", floor_label(to, lang)),
            Language::En => format!("Stairs (to {})", floor_label(to, lang)),
        },
        through: true,
    };
    match upstairs.at {
        Some((s, f)) => {
            list.push(spot(
                NpcKey::StairsDown,
                s.landing,
                tr(lang, "下樓", "Go down"),
                f - 1,
            ));
            if f < s.floors {
                list.push(spot(
                    NpcKey::StairsUp,
                    s.up(),
                    tr(lang, "上樓", "Go upstairs"),
                    f + 1,
                ));
            }
        }
        None => {
            let w = crate::map::world();
            if let Some(b) = w.bld_at(p)
                && let Some(s) = stair_of(w, b.id)
            {
                list.push(spot(
                    NpcKey::StairsUp,
                    s.landing,
                    tr(lang, "上樓", "Go upstairs"),
                    2,
                ));
            }
        }
    }
    spots.publish(|k| matches!(k, NpcKey::StairsUp | NpcKey::StairsDown), list);
}

#[allow(clippy::too_many_arguments)]
fn climb(
    mut commands: Commands,
    mut talks: MessageReader<TalkTo>,
    lang: Res<Language>,
    art: Res<FloorArt>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut upstairs: ResMut<Upstairs>,
    mut stance: ResMut<PlayerStance>,
    mut notice: ResMut<WorldNotice>,
    mut player: Query<&mut Transform, With<Player>>,
    views: Query<Entity, With<FloorView>>,
    mut sfx: MessageWriter<crate::audio::SfxEvent>,
) {
    for TalkTo(key) in talks.read() {
        let Ok(mut tf) = player.single_mut() else {
            return;
        };
        let p = tf.translation.xz();
        let (stair, to, arrive) = match (*key, upstairs.at) {
            (NpcKey::StairsUp, None) => {
                let w = crate::map::world();
                let Some(s) = w.bld_at(p).and_then(|b| stair_of(w, b.id)).copied() else {
                    continue;
                };
                (s, 2, s.landing)
            }
            (NpcKey::StairsUp, Some((s, f))) if f < s.floors => (s, f + 1, s.landing),
            (NpcKey::StairsDown, Some((s, f))) => {
                // from above one arrives at the flight up of the floor below
                let arrive = if f > 2 { s.up() } else { s.landing };
                (s, f - 1, arrive)
            }
            _ => continue,
        };
        for e in &views {
            commands.entity(e).despawn();
        }
        if to <= 1 {
            upstairs.leave();
            tf.translation = Vec3::new(arrive.x, 0.0, arrive.y);
        } else {
            upstairs.enter(stair, to);
            // a free spot by the landing if furniture sits on it
            let mut q = arrive;
            if !upstairs.clear_of_furniture(q) {
                q = stair.landing - stair.dir * 0.8;
            }
            tf.translation = Vec3::new(q.x, stair.floor_y(to), q.y);
            spawn_floor(&mut commands, &mut meshes, &art, &upstairs);
        }
        stance.upstairs = upstairs.active();
        sfx.write(crate::audio::SfxEvent::Footstep {
            surface: crate::audio::Surface::Wood,
            dist: 0.0,
            heavy: true,
            run: false,
        });
        notice.text = match *lang {
            Language::ZhTw => format!("你走到了{}。", floor_label(to, *lang)),
            Language::En => format!("You climb to {}.", floor_label(to, *lang)),
        };
        println!(
            "DTUPSTAIRS building={} floor={} fixtures={}",
            stair.building,
            to,
            upstairs.fixtures.len()
        );
    }
}

/// The floor slab, a low parapet ring, the furniture, the flight up and the
/// stairwell opening.
fn spawn_floor(commands: &mut Commands, meshes: &mut Assets<Mesh>, art: &FloorArt, up: &Upstairs) {
    let Some((s, f)) = up.at else { return };
    let y = s.floor_y(f);
    let mut slab = MeshBuf::default();
    let floor = shape(&up.pts).transformed(
        Mat4::from_translation(Vec3::new(0.0, y + 0.02, 0.0))
            * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2),
    );
    // three ShapeGeometry lies in XY: turned flat, (x, y) → (x, z)
    slab.add(&floor, Mat4::IDENTITY, hex(0xb59a76));
    let mut body = MeshBuf::default();
    for i in 0..up.pts.len() {
        let (a, b) = (up.pts[i], up.pts[(i + 1) % up.pts.len()]);
        let l = a.distance(b);
        if l < 0.05 {
            continue;
        }
        let m = (a + b) * 0.5;
        let yaw = (b - a).y.atan2((b - a).x);
        body.add(
            &cuboid(l + 0.2, 1.0, 0.2),
            place(m.x, y + 0.5, m.y, -yaw),
            hex(0xd8d2c4),
        );
    }
    for fx in &up.fixtures {
        body.add(
            &body_mesh(fx.kind),
            place(fx.pos.x, y + 0.04, fx.pos.y, fx.front.x.atan2(fx.front.y)),
            [1.0; 4],
        );
    }
    let step = cuboid(1.0, 1.0, 1.0);
    if f < s.floors {
        for (c, size, yaw) in flight_boxes(s.up(), s.dir, y, s.storey * 0.5, 6) {
            body.add(
                &step,
                place(c.x, c.y, c.z, yaw) * Mat4::from_scale(size),
                hex(0x8a8276),
            );
        }
    }
    // the opening down: a dark well with a rail on three sides
    let yaw = s.dir.x.atan2(s.dir.y);
    body.add(
        &cuboid(1.1, 0.02, 1.6),
        place(s.landing.x, y + 0.03, s.landing.y, yaw)
            * Mat4::from_translation(Vec3::new(0.0, 0.0, 0.5)),
        hex(0x2a2622),
    );
    for (dx, dz, w, d) in [
        (-0.6, 0.5, 0.05, 1.6),
        (0.6, 0.5, 0.05, 1.6),
        (0.0, 1.3, 1.2, 0.05),
    ] {
        body.add(
            &cuboid(w, 0.9, d),
            place(s.landing.x, y + 0.45, s.landing.y, yaw)
                * Mat4::from_translation(Vec3::new(dx, 0.0, dz)),
            hex(0x5a5a58),
        );
    }
    commands.spawn((
        FloorView,
        Mesh3d(meshes.add(slab.into_mesh())),
        MeshMaterial3d(art.slab.clone()),
        Transform::default(),
        Visibility::default(),
    ));
    commands.spawn((
        FloorView,
        Mesh3d(meshes.add(body.into_mesh())),
        MeshMaterial3d(art.furniture.clone()),
        Transform::default(),
        Visibility::default(),
    ));
}

/// Keep the player at the floor's height (other systems only move x and z).
fn hold_height(upstairs: Res<Upstairs>, mut player: Query<&mut Transform, With<Player>>) {
    let y = upstairs.height();
    for mut tf in &mut player {
        if tf.translation.y != y {
            tf.translation.y = y;
        }
    }
}

fn go_down_silently(
    mut commands: Commands,
    mut upstairs: ResMut<Upstairs>,
    mut stance: ResMut<PlayerStance>,
    views: Query<Entity, With<FloorView>>,
) {
    upstairs.leave();
    stance.upstairs = false;
    for e in &views {
        commands.entity(e).despawn();
    }
}

fn reset_on_load(
    mut commands: Commands,
    mut loaded: MessageReader<LoadCompleted>,
    mut upstairs: ResMut<Upstairs>,
    mut stance: ResMut<PlayerStance>,
    views: Query<Entity, With<FloorView>>,
) {
    if loaded.read().count() > 0 {
        upstairs.leave();
        stance.upstairs = false;
        for e in &views {
            commands.entity(e).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_floor_keeps_the_player_inside_and_out_of_the_furniture() {
        let w = crate::map::world();
        let s = *w.stairs.iter().find(|s| s.floors >= 3).unwrap();
        let mut up = Upstairs::default();
        up.enter(s, 2);
        assert!(up.active() && up.floor() == Some((s.building, 2)));
        assert_eq!(up.landing(), Some(s.landing));
        assert!((up.height() - s.storey).abs() < 1e-4);
        // walking far out of the building stops at its walls
        let out = up.move_circle(s.landing, s.landing - s.dir * 60.0, PLAYER_RADIUS);
        assert!(crate::map::data::pip(out, &w.blds[s.building as usize].pts));
        // furniture is solid
        if let Some(fx) = up.fixtures().first() {
            let p = up.move_circle(fx.approach, fx.pos, PLAYER_RADIUS);
            assert!(!fx.contains(p, -0.05));
        }
        up.leave();
        assert!(!up.active() && up.height() == 0.0);
    }
}
