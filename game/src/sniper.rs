//! v22.5 狙擊手 — enemy snipers over the main junctions (Web `src/systems/sniper.ts`
//! and `setupSnipers` in `src/core/main.ts`).
//!
//! Each post is the roof corner, nearest the junction, of the tallest building
//! 12–50 m from a crossing of two named main roads (西園路, 和平西路, 康定路,
//! 中華路, 環河南路, 桂林路); at most five. By day (06:00–18:30) a player in the
//! open within 60 m of the junction and in the sniper's sight is locked: a glint
//! and a red laser for 1.6 s, then a 38–47 damage shot (half the time it starts
//! bleeding), then 3.5 s to work the bolt. Arcades and indoors, a vehicle within
//! 1.8 m or a wall between break the lock.
//!
//! The player can shoot back — aimed gunfire within 10° of him and in range
//! hits (a scoped rifle is almost sure and kills outright) — or climb to the roof
//! from the building's door and overpower him. A cleared post is safe by day,
//! counts as a kill and leaves 7.62 rounds and a bandage at the door.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat::tuning::GunFam;
use crate::combat::{CombatRng, GameState, Gunshot, HitCause, PlayerHit, WeaponInput};
use crate::dialogue::{NpcKey, NpcSpot, NpcSpots, TalkTo};
use crate::game_flow::{Campaign, GameFlow};
use crate::human::anim::AnimState;
use crate::human::rig::RigOpts;
use crate::human::{Driver, Humanoid, RigAnim};
use crate::i18n::{Language, tr};
use crate::map::wreckage::WreckKind;
use crate::player::Player;
use crate::ui::PauseOpen;
use crate::worldmap::{MapProgress, NoteKind};
use crate::worldplay::{ScavengeState, WorldNotice, WorldSet};

pub const AIM_T: f32 = 1.6;
const ZONE_R: f32 = 60.0;
const WANT: [&str; 6] = [
    "西園路",
    "和平西路",
    "康定路",
    "中華路",
    "環河南路",
    "桂林路",
];

/// A sniper post (`Post`).
#[derive(Clone, Debug, PartialEq)]
pub struct Post {
    pub name: String,
    /// The junction he watches.
    pub j: Vec2,
    pub building: u32,
    /// Where he lies on the roof.
    pub p: Vec2,
    /// Roof height.
    pub y: f32,
}

impl Post {
    /// `snpEye`: 1.4 m out past the parapet, so his own roof does not hide the street.
    pub fn eye(&self) -> Vec2 {
        let c = crate::map::world().blds[self.building as usize].c;
        let d = (self.p - c).normalize_or_zero();
        self.p + d * 1.4
    }
}

fn seg_inter(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<Vec2> {
    let r = b - a;
    let s = d - c;
    let den = r.x * s.y - r.y * s.x;
    if den.abs() < 1e-9 {
        return None;
    }
    let t = ((c.x - a.x) * s.y - (c.y - a.y) * s.x) / den;
    let u = ((c.x - a.x) * r.y - (c.y - a.y) * r.x) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(a + r * t)
}

/// `findPosts`: junctions of the named main roads (sorted, deterministic), then
/// the tallest building 12–50 m from each.
pub fn find_posts(max: usize) -> Vec<Post> {
    let w = crate::map::world();
    let named: Vec<_> = w
        .roads
        .iter()
        .filter(|r| {
            WANT.iter().any(|n| r.name.contains(n))
                && !r.name.contains('巷')
                && !r.name.contains('弄')
        })
        .collect();
    let mut x: Vec<(Vec2, String)> = Vec::new();
    for (i, a) in named.iter().enumerate() {
        for b in &named[i + 1..] {
            if a.name == b.name {
                continue;
            }
            for pa in a.p.windows(2) {
                for pb in b.p.windows(2) {
                    if let Some(q) = seg_inter(pa[0], pa[1], pb[0], pb[1])
                        && !x.iter().any(|(j, _)| j.distance(q) < 60.0)
                    {
                        let mut names = [a.name.clone(), b.name.clone()];
                        names.sort();
                        x.push((q, names.join("／")));
                    }
                }
            }
        }
    }
    let rank = |name: &str| WANT.iter().position(|w| name.contains(w)).unwrap_or(99);
    x.sort_by(|a, b| rank(&a.1).cmp(&rank(&b.1)).then(a.0.x.total_cmp(&b.0.x)));
    let mut out = Vec::new();
    let mut used = Vec::new();
    for (j, name) in x {
        if out.len() >= max {
            break;
        }
        let Some(b) = w
            .blds
            .iter()
            .filter(|b| b.h >= 9.0 && !used.contains(&b.id))
            .filter(|b| (12.0..50.0).contains(&b.c.distance(j)))
            .min_by(|p, q| {
                q.h.total_cmp(&p.h)
                    .then(p.c.distance(j).total_cmp(&q.c.distance(j)))
            })
        else {
            continue;
        };
        used.push(b.id);
        // the roof corner nearest the junction, pulled 0.6 m inside the parapet
        let best = *b
            .pts
            .iter()
            .min_by(|p, q| p.distance(j).total_cmp(&q.distance(j)))
            .unwrap();
        let k = 0.6 / b.c.distance(best).max(1e-3);
        out.push(Post {
            name,
            j,
            building: b.id,
            p: best + (b.c - best) * k,
            y: b.h,
        });
    }
    out
}

/// Saved: the posts already cleared (`surv.snpDead`).
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SniperState {
    pub dead: Vec<u8>,
    /// Posts already marked on the map.
    pub known: Vec<u8>,
}

impl SniperState {
    pub fn is_valid(&self) -> bool {
        self.dead.len() <= 8 && self.known.len() <= 8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sight {
    Idle,
    Aim,
    Cool,
}

/// One live sniper.
#[derive(Component)]
struct SniperMan {
    i: usize,
    hp: f32,
    st: Sight,
    t: f32,
}

#[derive(Component)]
struct Laser(usize);

#[derive(Component)]
struct Glint(usize);

/// Is any sniper aiming at the player now (for the HUD and QA)?
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct SniperLock(pub Option<usize>);

#[derive(Resource)]
pub struct Posts(pub Vec<Post>);

pub fn day(game_min: f32) -> bool {
    let h = (game_min / 60.0) % 24.0;
    (6.0..18.5).contains(&h)
}

pub struct SniperPlugin;

impl Plugin for SniperPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SniperState>()
            .init_resource::<SniperLock>()
            .insert_resource(Posts(find_posts(5)))
            .add_systems(Startup, spawn)
            .add_systems(OnEnter(GameFlow::Prologue), |mut s: ResMut<SniperState>| {
                *s = SniperState::default();
            })
            .add_systems(
                Update,
                (aim, return_fire, publish_doors, climb)
                    .chain()
                    .in_set(WorldSet)
                    .run_if(in_state(GameFlow::Playing)),
            );
    }
}

fn spawn(
    mut commands: Commands,
    posts: Res<Posts>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let laser = meshes.add(Cuboid::new(0.04, 0.04, 1.0));
    let laser_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.13, 0.13, 0.85),
        emissive: LinearRgba::rgb(6.0, 0.3, 0.3),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let glint = meshes.add(Sphere::new(0.18).mesh().ico(1).unwrap());
    let glint_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::rgb(12.0, 11.0, 9.0),
        unlit: true,
        ..default()
    });
    for (i, p) in posts.0.iter().enumerate() {
        let face = (p.j.x - p.p.x).atan2(p.j.y - p.p.y);
        commands.spawn((
            SniperMan {
                i,
                hp: 100.0,
                st: Sight::Idle,
                t: 0.0,
            },
            Humanoid {
                opts: RigOpts::soldier("qbu88"),
                driver: Driver::Ally,
            },
            Transform::from_xyz(p.p.x, p.y, p.p.y).with_rotation(Quat::from_rotation_y(face)),
            Visibility::default(),
        ));
        commands.spawn((
            Laser(i),
            Mesh3d(laser.clone()),
            MeshMaterial3d(laser_mat.clone()),
            Transform::default(),
            Visibility::Hidden,
            bevy::light::NotShadowCaster,
        ));
        commands.spawn((
            Glint(i),
            Mesh3d(glint.clone()),
            MeshMaterial3d(glint_mat.clone()),
            Transform::from_xyz(p.p.x, p.y + 1.1, p.p.y),
            Visibility::Hidden,
            bevy::light::NotShadowCaster,
        ));
    }
}

/// `sees`: in the open, not beside a vehicle, and in his line of sight.
pub(crate) fn sees(
    post: &Post,
    p: Vec2,
    upstairs: bool,
    obstacles: &crate::map::Obstacles,
) -> bool {
    let w = crate::map::world();
    if upstairs || w.bld_at(p).is_some() {
        return false;
    }
    if w.wrecks
        .iter()
        .any(|x| matches!(x.kind, WreckKind::Car | WreckKind::Bus) && x.p.distance(p) < 1.8)
    {
        return false;
    }
    obstacles.los(post.eye(), p)
}

#[allow(clippy::too_many_arguments)]
fn aim(
    time: Res<Time>,
    pause: Res<PauseOpen>,
    lang: Res<Language>,
    posts: Res<Posts>,
    obstacles: Res<crate::map::Obstacles>,
    upstairs_vehicles: (
        Option<Res<crate::upstairs::Upstairs>>,
        Option<Res<crate::vehicles::VehicleFleet>>,
    ),
    (mut state, mut lock): (ResMut<SniperState>, ResMut<SniperLock>),
    (mut campaign, mut game): (ResMut<Campaign>, ResMut<GameState>),
    mut notice: ResMut<WorldNotice>,
    mut map: ResMut<MapProgress>,
    mut rng: ResMut<CombatRng>,
    player: Query<&Transform, (With<Player>, Without<SniperMan>)>,
    mut men: Query<
        (
            &mut SniperMan,
            &mut Transform,
            &mut Visibility,
            Option<&mut RigAnim>,
        ),
        Without<Player>,
    >,
    mut lasers: Query<
        (&Laser, &mut Transform, &mut Visibility),
        (Without<SniperMan>, Without<Player>, Without<Glint>),
    >,
    mut glints: Query<
        (&Glint, &mut Transform, &mut Visibility),
        (Without<SniperMan>, Without<Player>, Without<Laser>),
    >,
    mut out: (
        MessageWriter<crate::audio::SfxEvent>,
        MessageWriter<PlayerHit>,
    ),
) {
    if pause.0 {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let t = time.elapsed_secs();
    let (upstairs, vehicles) = upstairs_vehicles;
    let Ok(pt) = player.single() else { return };
    let pp = pt.translation.xz();
    let up = upstairs.as_deref().is_some_and(|u| u.active());
    let is_day = day(campaign.game_min);
    let mut locked = None;
    for (mut s, mut tf, mut vis, anim) in &mut men {
        let post = &posts.0[s.i];
        let dead = state.dead.contains(&(s.i as u8));
        vis.set_if_neq(if dead {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
        if dead {
            continue;
        }
        if let Some(mut r) = anim {
            r.state = AnimState::Aim;
            r.speed = 0.0;
            if s.st != Sight::Idle {
                r.yaw = (pp.x - post.p.x).atan2(pp.y - post.p.y);
            }
        }
        let in_range = pp.distance(post.p) < ZONE_R + 25.0 && pp.distance(post.j) < ZONE_R;
        let see = is_day && in_range && sees(post, pp, up, &obstacles);
        s.t -= dt;
        match s.st {
            Sight::Cool => {
                if s.t <= 0.0 {
                    s.st = Sight::Idle;
                }
            }
            Sight::Idle => {
                if see {
                    s.st = Sight::Aim;
                    s.t = AIM_T;
                    crate::device::request_haptic(18);
                    out.0.write(crate::audio::SfxEvent::Clink {
                        dist: pp.distance(post.p),
                    });
                    if !state.known.contains(&(s.i as u8)) {
                        state.known.push(s.i as u8);
                        map.add_note(post.j, NoteKind::Danger);
                    }
                }
            }
            Sight::Aim => {
                if !see {
                    s.st = Sight::Idle;
                } else if s.t <= 0.0 {
                    out.0.write(crate::audio::SfxEvent::Gunshot {
                        kind: crate::audio::GunKind::Sniper,
                        dist: pp.distance(post.p),
                        indoor: false,
                    });
                    let raw_dmg = 38.0 + ((s.i as u32 * 7 + 3) % 10) as f32;
                    let (dmg, bleed) = crate::combat::armor::shield_damage(
                        vehicles.as_deref(),
                        raw_dmg,
                        !game.bleed && rng.0.next_f32() < 0.5,
                    );
                    if !game.over {
                        crate::device::request_haptic(45);
                        game.hp -= dmg;
                        game.bleed |= bleed;
                        out.1.write(PlayerHit {
                            dmg,
                            cause: HitCause::Rifle,
                            bleed,
                        });
                        campaign.sleeping = false;
                        notice.text = tr(
                            *lang,
                            "砰！你被狙擊手擊中！快找掩蔽！",
                            "Bang! A sniper hit you! Get to cover!",
                        )
                        .into();
                    }
                    s.st = Sight::Cool;
                    s.t = 3.5;
                }
            }
        }
        let aiming = s.st == Sight::Aim;
        if aiming {
            locked = Some(s.i);
        }
        for (l, mut ltf, mut lv) in &mut lasers {
            if l.0 != s.i {
                continue;
            }
            lv.set_if_neq(if aiming {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
            if aiming {
                let a = Vec3::new(post.p.x, post.y + 1.2, post.p.y);
                let b = Vec3::new(pp.x, pt.translation.y + 1.3, pp.y);
                let len = a.distance(b).max(0.1);
                *ltf = Transform::from_translation((a + b) * 0.5)
                    .looking_at(b, Vec3::Y)
                    .with_scale(Vec3::new(1.0, 1.0, len));
            }
        }
        for (g, mut gtf, mut gv) in &mut glints {
            if g.0 != s.i {
                continue;
            }
            let flash = aiming || (is_day && in_range && (t / 0.3 + post.p.x).sin() > 0.6);
            gv.set_if_neq(if flash {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
            let k = if aiming {
                1.2 + (t / 0.06).sin() * 0.5
            } else {
                0.7
            };
            gtf.scale = Vec3::splat(k);
        }
        let _ = &mut tf;
    }
    if locked.is_some() && lock.0.is_none() {
        notice.text = tr(
            *lang,
            "你被狙擊手鎖定！躲進騎樓、車後或巷子",
            "A sniper has you in their sights! Hide under an arcade, behind a car or in an alley.",
        )
        .into();
        // slowDown
        campaign.speed = 1;
        campaign.sleeping = false;
    }
    lock.set_if_neq(SniperLock(locked));
}

/// Clears post `i` (`cleared`).
#[allow(clippy::too_many_arguments)]
fn clear_post(
    i: usize,
    posts: &Posts,
    state: &mut SniperState,
    game: &mut GameState,
    progress: &mut ScavengeState,
    map: &mut MapProgress,
    notice: &mut WorldNotice,
    lang: Language,
) {
    if state.dead.contains(&(i as u8)) {
        return;
    }
    state.dead.push(i as u8);
    game.kills += 1;
    let post = &posts.0[i];
    notice.text = match lang {
        Language::ZhTw => format!("狙擊手已清除：{} 一帶白天安全了。", post.name),
        Language::En => format!(
            "Sniper eliminated: {} is now safe during the day.",
            post.name
        ),
    };
    map.remove_near(post.j, 3.0);
    map.add_note(post.j, NoteKind::Safe);
    let id = format!("snpbox{i}");
    if !progress.drops.iter().any(|(d, _)| *d == id) {
        let at = crate::map::world().blds[post.building as usize]
            .door
            .map_or(post.p, |d| d.p + d.n * 1.2);
        let mut stash = crate::survival::Stash::default();
        stash.add("a762n", 5);
        stash.add("bandage", 1);
        progress.loot.push((id.clone(), stash));
        progress.drops.push((id.clone(), at.to_array()));
        progress.night_seen.push(id);
    }
}

/// Aimed gunfire hits a sniper within 10° and in range.
#[allow(clippy::too_many_arguments)]
fn return_fire(
    mut shots: MessageReader<Gunshot>,
    lang: Res<Language>,
    posts: Res<Posts>,
    input: Res<WeaponInput>,
    obstacles: Res<crate::map::Obstacles>,
    (mut state, mut game, mut rng): (ResMut<SniperState>, ResMut<GameState>, ResMut<CombatRng>),
    (mut progress, mut map, mut notice): (
        ResMut<ScavengeState>,
        ResMut<MapProgress>,
        ResMut<WorldNotice>,
    ),
    player: Query<&Transform, (With<Player>, Without<SniperMan>)>,
    mut men: Query<&mut SniperMan>,
) {
    for shot in shots.read().filter(|s| s.by_player) {
        let Ok(pt) = player.single() else { continue };
        let pp = pt.translation.xz();
        let f = pt.forward();
        let dir = input
            .aim
            .try_normalize()
            .unwrap_or(Vec2::new(f.x, f.z).normalize_or_zero());
        let scoped = matches!(shot.fam, GunFam::Sniper | GunFam::Dmr);
        let range = if scoped { 160.0 } else { 70.0 };
        for mut s in &mut men {
            if state.dead.contains(&(s.i as u8)) {
                continue;
            }
            let post = &posts.0[s.i];
            let to = post.p - pp;
            let d = to.length();
            if d > range || d < 1.0 || to.normalize().dot(dir) < 10f32.to_radians().cos() {
                continue;
            }
            if !obstacles.los(pp, post.eye()) {
                continue;
            }
            if rng.0.next_f32() >= if scoped { 0.8 } else { 0.3 } {
                continue;
            }
            // a scoped rifle's hit is in the head
            s.hp -= if scoped { 999.0 } else { 40.0 };
            s.st = Sight::Cool;
            s.t = 3.0;
            if s.hp <= 0.0 {
                clear_post(
                    s.i,
                    &posts,
                    &mut state,
                    &mut game,
                    &mut progress,
                    &mut map,
                    &mut notice,
                    *lang,
                );
            }
        }
    }
}

fn publish_doors(
    lang: Res<Language>,
    posts: Res<Posts>,
    state: Res<SniperState>,
    vehicles: Option<Res<crate::vehicles::VehicleFleet>>,
    mut spots: ResMut<NpcSpots>,
) {
    if vehicles.as_deref().is_some_and(|v| v.current.is_some()) {
        spots.publish(|k| matches!(k, NpcKey::Sniper(_)), []);
        return;
    }
    let w = crate::map::world();
    let list = posts
        .0
        .iter()
        .enumerate()
        .filter(|(i, _)| !state.dead.contains(&(*i as u8)))
        .filter_map(|(i, post)| {
            let door = w.blds[post.building as usize].door?;
            Some(NpcSpot {
                key: NpcKey::Sniper(i as u8),
                pos: door.p + door.n * 0.4,
                reach: 2.0,
                label: tr(*lang, "爬頂樓", "Climb to the roof").into(),
                detail: match *lang {
                    Language::ZhTw => format!("爬上頂樓制伏狙擊手（{}）", post.name),
                    Language::En => format!("Climb up and overpower the sniper ({})", post.name),
                },
                through: true,
            })
        })
        .collect::<Vec<_>>();
    spots.publish(|k| matches!(k, NpcKey::Sniper(_)), list);
}

#[allow(clippy::too_many_arguments)]
fn climb(
    mut talks: MessageReader<TalkTo>,
    lang: Res<Language>,
    posts: Res<Posts>,
    vehicles: Option<Res<crate::vehicles::VehicleFleet>>,
    (mut state, mut game): (ResMut<SniperState>, ResMut<GameState>),
    (mut progress, mut map, mut notice): (
        ResMut<ScavengeState>,
        ResMut<MapProgress>,
        ResMut<WorldNotice>,
    ),
    mut sfx: MessageWriter<crate::audio::SfxEvent>,
) {
    if vehicles.as_deref().is_some_and(|v| v.current.is_some()) {
        return;
    }
    for TalkTo(key) in talks.read() {
        let NpcKey::Sniper(i) = *key else { continue };
        let i = i as usize;
        if i >= posts.0.len() || state.dead.contains(&(i as u8)) {
            continue;
        }
        // up the stairwell to the roof, then a struggle in the dark of the parapet
        sfx.write(crate::audio::SfxEvent::MeleeHit {
            dist: 0.0,
            blunt: true,
        });
        clear_post(
            i,
            &posts,
            &mut state,
            &mut game,
            &mut progress,
            &mut map,
            &mut notice,
            *lang,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posts_watch_main_junctions_from_tall_roofs() {
        let posts = find_posts(5);
        assert!(!posts.is_empty() && posts.len() <= 5, "{}", posts.len());
        let w = crate::map::world();
        for p in &posts {
            let b = &w.blds[p.building as usize];
            assert!(b.h >= 9.0 && (12.0..50.0).contains(&b.c.distance(p.j)));
            assert!(crate::map::data::pip(p.p, &b.pts));
            assert!(p.name.contains('／'));
            assert!(w.bld_at(p.eye()).is_none_or(|x| x.id != b.id));
        }
        assert_eq!(find_posts(5), posts, "deterministic");
    }

    #[test]
    fn snipers_work_by_day_only() {
        assert!(day(7.0 * 60.0));
        assert!(day(18.4 * 60.0));
        assert!(!day(18.5 * 60.0));
        assert!(!day(3.0 * 60.0 + 1440.0));
    }
}
