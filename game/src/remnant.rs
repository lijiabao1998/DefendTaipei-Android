//! v24.3 國軍殘部 — the ROC Army squads that did not withdraw (Web
//! `src/npc/remnant.ts` and its wiring in `src/core/main.ts`).
//!
//! Four sandbagged street corners 110–420 m from the start ([`crate::map::fortify`]),
//! each with a flag and four soldiers. They fire on 解放軍 soldiers that come into
//! their sights (42 m, over their own sandbags, walls block) and take fire back.
//! Men fall one by one; the rest do not withdraw. When the last one is down the
//! radio carries their last words and the position leaves its gear in a crate.
//!
//! The player can talk to the squad leader: once a day the squad shares
//! ammunition, a ration and a bandage; with a bandage or first-aid kit the
//! player can treat a wounded man; and the squad marks the 解放軍 checkpoints on
//! the survivor map. Each morning a squad still holding reports on the radio.
//! Every unit designation and name is fictional, as in the Web build.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat::{
    self, Body, CombatEnv, CombatRng, CombatWorldRes, Enemy, EnemyDied, Gunshot, SolState,
    tuning::GunFam,
};
use crate::dialogue::{
    Dialogue, DialogueCard, DialogueChosen, DialogueKey, DialogueOption, NpcKey, NpcSpot, NpcSpots,
    TalkTo,
};
use crate::game_flow::{Campaign, GameFlow};
use crate::human::anim::AnimState;
use crate::human::rig::RigOpts;
use crate::human::{Driver, Humanoid, RigAnim};
use crate::i18n::{Language, tr};
use crate::inventory::Inventory;
use crate::map::fortify::{BAG, RemnantSite};
use crate::player::Player;
use crate::survival::Shelter;
use crate::ui::PauseOpen;
use crate::worldmap::{MapProgress, NoteKind};
use crate::worldplay::{ScavengeState, WorldNotice, WorldSet};

/// Saved state of one position (Web `RemSave`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RemSave {
    pub hp: [f32; 4],
    pub fallen: bool,
    /// Day the supplies were last handed out.
    pub supply: Option<u32>,
    pub met: bool,
    /// Lines already spoken (rotates the leader's words).
    pub last: u32,
}

impl Default for RemSave {
    fn default() -> Self {
        Self {
            hp: [100.0; 4],
            fallen: false,
            supply: None,
            met: false,
            last: 0,
        }
    }
}

impl RemSave {
    pub fn alive(&self) -> usize {
        self.hp.iter().filter(|h| **h > 0.0).count()
    }
}

/// Every position's state, persisted in the save (`surv.rem`).
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RemnantState {
    pub squads: Vec<RemSave>,
}

impl RemnantState {
    pub fn is_valid(&self) -> bool {
        self.squads.len() <= ROSTER.len()
            && self.squads.iter().all(|s| {
                s.hp.iter()
                    .all(|h| h.is_finite() && (0.0..=100.0).contains(h))
                    && s.last < 1_000_000
                    && s.supply.is_none_or(|d| d <= 1_000_000)
            })
    }

    /// The state of squad `i`, created on first use (`sv(id)`).
    pub fn squad(&mut self, i: usize) -> &mut RemSave {
        if self.squads.len() <= i {
            self.squads.resize(i + 1, RemSave::default());
        }
        &mut self.squads[i]
    }

    pub fn get(&self, i: usize) -> RemSave {
        self.squads.get(i).cloned().unwrap_or_default()
    }
}

struct Unit {
    unit: (&'static str, &'static str),
    leader: (&'static str, &'static str),
    lines: [(&'static str, &'static str); 3],
    fallen: (&'static str, &'static str),
}

const ROSTER: [Unit; 4] = [
    Unit {
        unit: (
            "陸軍守備第 307 營・二連殘部",
            "2nd Company remnant, 307th Garrison Battalion",
        ),
        leader: ("張上士", "Sergeant First Class Chang"),
        lines: [
            (
                "「三天了，增援沒有來，我們也沒有走。只要這面國旗還插在這裡，這條街就還是中華民國的。」",
                "“Three days. No reinforcements came, and we did not leave. As long as this flag stands here, this street is still the Republic of China.”",
            ),
            (
                "「子彈省著用，一發一個。我們不是來拚命的，是來守住的——守住了，大家才回得了家。」",
                "“Make every round count. We are not here to throw our lives away; we are here to hold. Hold, and everyone gets to go home.”",
            ),
            (
                "「你看到那些老百姓沒有？他們還在。我們就不能先倒。」",
                "“See those people still in their homes? They are still here. So we cannot be the first to fall.”",
            ),
        ],
        fallen: (
            "（雜訊）……這裡是二連……敵人上來了，我們守住巷口……弟兄們，跟我上——（無線電沒有了聲音）",
            "(static) …This is Second Company… they are coming up, we are holding the alley mouth… On me, brothers— (the radio goes silent)",
        ),
    },
    Unit {
        unit: (
            "後備動員第 8813 大隊・第一排",
            "1st Platoon, 8813th Reserve Mobilization Group",
        ),
        leader: ("林中尉", "Lieutenant Lin"),
        lines: [
            (
                "「我們是後備的，上個月還在上班。現在剩七個人，子彈不夠，骨氣夠。」",
                "“We are reservists. Last month we were at our desks. Seven of us left — not enough ammunition, more than enough spine.”",
            ),
            (
                "「老百姓先走，我們最後走。這不是命令，是我們自己決定的。」",
                "“Civilians go first, we go last. That is not an order; we decided it ourselves.”",
            ),
            (
                "「我女兒在新店。我守在這裡，就是在守她回家的那條路。」",
                "“My daughter is in Xindian. Holding here is holding the road she will come home on.”",
            ),
        ],
        fallen: (
            "（雜訊）……一排報告，彈藥用完了，上刺刀……告訴我女兒，爸爸沒有退——（訊號中斷）",
            "(static) …First Platoon, out of ammunition, fixing bayonets… Tell my daughter her father did not fall back— (signal lost)",
        ),
    },
    Unit {
        unit: (
            "憲兵第 4207 連・殘部",
            "Remnant of the 4207th Military Police Company",
        ),
        leader: ("陳士官長", "Sergeant Major Chen"),
        lines: [
            (
                "「我當兵二十六年，沒想過會在自己長大的巷子裡打仗。也好，這裡每一條巷子我都熟。」",
                "“Twenty-six years in uniform. Never thought I would fight in the alleys I grew up in. Good — I know every one of them.”",
            ),
            (
                "「怕？當然怕。可是怕跟退是兩回事。」",
                "“Afraid? Of course. But being afraid and falling back are two different things.”",
            ),
            (
                "「他們有坦克、有無人機，我們有這條街。街是我們的。」",
                "“They have tanks and drones. We have this street. The street is ours.”",
            ),
        ],
        fallen: (
            "（雜訊）……士官長在此，最後一個……他們不會那麼容易過去的——（槍聲，然後安靜）",
            "(static) …Sergeant Major here, the last one… they will not get through easily— (gunfire, then silence)",
        ),
    },
    Unit {
        unit: (
            "陸軍特戰「艋舺」分隊・殘部",
            "Remnant of Army Special Forces Team “Monga”",
        ),
        leader: ("吳下士", "Corporal Wu"),
        lines: [
            (
                "「隊長倒在第一天。現在我帶隊。我二十三歲，可是我知道該怎麼做。」",
                "“The team leader fell on day one. I lead now. I am twenty-three, and I know what to do.”",
            ),
            (
                "「我們晚上出去，把他們的哨點一個一個標起來。你拿去，比放在我們身上有用。」",
                "“We go out at night and mark their checkpoints one by one. Take it — it is more use to you than to us.”",
            ),
            (
                "「如果明天你沒看到我們，別難過。看看那面旗，它還在就好。」",
                "“If you do not see us tomorrow, do not grieve. Look at the flag. As long as it is still there, that is enough.”",
            ),
        ],
        fallen: (
            "（雜訊）……艋舺分隊……旗還在……旗還在——（無線電只剩雜訊）",
            "(static) …Team Monga… the flag is still up… the flag is still up— (only static remains)",
        ),
    },
];

const WEAPONS: [&str; 4] = ["t91", "t91", "t75saw", "t65k2"];
/// The squad answers foes within this range of its position.
const ENGAGE_R: f32 = 42.0;
/// Positions further than this from the player are hidden and do not fight.
const ACTIVE_R: f32 = 150.0;
const TALK_R: f32 = 3.2;
const GEAR: [(&str, u32); 5] = [
    ("t91", 1),
    ("a556n", 40),
    ("ration", 2),
    ("medkit", 1),
    ("grenade", 2),
];

pub fn unit_name(i: usize, lang: Language) -> &'static str {
    let u = &ROSTER[i % ROSTER.len()];
    tr(lang, u.unit.0, u.unit.1)
}

fn leader(i: usize, lang: Language) -> &'static str {
    let u = &ROSTER[i % ROSTER.len()];
    tr(lang, u.leader.0, u.leader.1)
}

/// Id of the crate a fallen position leaves.
pub fn gear_id(i: usize) -> String {
    format!("remgear{i}")
}

/// One soldier at a position.
#[derive(Component)]
pub struct RemnantMan {
    pub squad: usize,
    pub k: usize,
    pub pos: Vec2,
    cd: f32,
    /// Went down in front of the player (the fall plays); others lie where they fell.
    fell: bool,
}

#[derive(Component)]
struct RemnantSiteRoot(usize);

#[derive(Resource, Default)]
struct RemnantClock {
    t: f32,
    /// Day of the last dawn report.
    reported: Option<u32>,
}

/// Counters for tests and QA (Web `stats`).
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct RemnantStats {
    pub shots: u32,
    pub hits: u32,
    pub wounded: u32,
}

pub struct RemnantPlugin;

impl Plugin for RemnantPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RemnantState>()
            .init_resource::<RemnantClock>()
            .init_resource::<RemnantStats>()
            .add_systems(Startup, spawn_positions)
            .add_systems(OnEnter(GameFlow::Prologue), reset_run)
            .add_systems(
                Update,
                (fight, publish_spots, talk, choose, dawn_report)
                    .chain()
                    .in_set(WorldSet)
                    .run_if(in_state(GameFlow::Playing)),
            );
    }
}

fn reset_run(mut state: ResMut<RemnantState>, mut clock: ResMut<RemnantClock>) {
    *state = RemnantState::default();
    *clock = RemnantClock::default();
}

/// The ROC flag (`roc.ts` `flagTex`): red field, blue canton, twelve-rayed white sun.
pub fn flag_image() -> Image {
    const W: usize = 96;
    const H: usize = 64;
    let mut px = vec![0u8; W * H * 4];
    let red = [0xfe, 0x00, 0x00];
    let blue = [0x00, 0x00, 0x95];
    let white = [0xff, 0xff, 0xff];
    let sun = Vec2::new(24.0, 16.0);
    for y in 0..H {
        for x in 0..W {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let mut c = if x < 48 && y < 32 { blue } else { red };
            if x < 48 && y < 32 {
                let d = p - sun;
                let r = d.length();
                // twelve triangular rays from radius 6 to 13
                // rays centred every 30° from the +x axis (the canvas rotates by π/6)
                let a = (d.y.atan2(d.x) + std::f32::consts::TAU / 24.0)
                    .rem_euclid(std::f32::consts::TAU / 12.0)
                    - std::f32::consts::TAU / 24.0;
                let half = 3.0 * (13.0 - r) / 7.0;
                let ray = (6.0..13.0).contains(&r) && (a.sin() * r).abs() < half;
                if ray || r < 6.5 {
                    c = white;
                }
                if (4.6..5.5).contains(&r) {
                    c = blue;
                }
            }
            let i = (y * W + x) * 4;
            px[i..i + 3].copy_from_slice(&c);
            px[i + 3] = 255;
        }
    }
    Image::new(
        bevy::render::render_resource::Extent3d {
            width: W as u32,
            height: H as u32,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        px,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    )
}

/// `flagPole`: a 3.2 m steel pole and a 1.2 × 0.8 m double-sided flag.
pub fn spawn_flag(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    at: Vec2,
    parent: Entity,
) {
    let pole = commands
        .spawn((
            Mesh3d(meshes.add(Cylinder::new(0.035, 3.2))),
            MeshMaterial3d(materials.add(Color::srgb(0.17, 0.19, 0.13))),
            Transform::from_xyz(at.x, 1.6, at.y),
        ))
        .id();
    let cloth = commands
        .spawn((
            Mesh3d(meshes.add(Rectangle::new(1.2, 0.8))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(images.add(flag_image())),
                double_sided: true,
                cull_mode: None,
                perceptual_roughness: 0.8,
                ..default()
            })),
            Transform::from_xyz(at.x + 0.62, 2.75, at.y).with_rotation(Quat::from_rotation_y(0.35)),
        ))
        .id();
    commands.entity(parent).add_children(&[pole, cloth]);
}

fn roc_rig(k: usize) -> RigOpts {
    RigOpts {
        female: k == 3,
        top: if k == 2 { 0x55603c } else { 0x68714a },
        bottom: 0x4a5234,
        shoes: 0x1a1a14,
        soldier: true,
        jacket: None,
        pack: None,
        outfit: "",
        gun: WEAPONS[k],
    }
}

fn spawn_positions(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    use crate::geom::{MeshBuf, cuboid, hex, place, sphere, trs};
    let w = crate::map::world();
    // vertex-coloured, one merged mesh per position (one draw call)
    let sand = materials.add(StandardMaterial {
        perceptual_roughness: 1.0,
        ..default()
    });
    let bag = cuboid(BAG[0], BAG[1], BAG[2]);
    for (i, site) in w.remnants.iter().enumerate().take(ROSTER.len()) {
        let root = commands
            .spawn((RemnantSiteRoot(i), Transform::default(), Visibility::Hidden))
            .id();
        let mut bags = MeshBuf::default();
        for wall in site.walls() {
            for (p, yaw) in wall.bags() {
                bags.add(&bag, place(p[0], p[1], p[2], yaw), hex(0x8a7a58));
            }
        }
        let b = commands
            .spawn((
                Mesh3d(meshes.add(bags.into_mesh())),
                MeshMaterial3d(sand.clone()),
                Transform::default(),
            ))
            .id();
        commands.entity(root).add_child(b);
        spawn_flag(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            site.flag(),
            root,
        );
        for k in 0..4 {
            let p = site.man(k);
            commands.spawn((
                RemnantMan {
                    squad: i,
                    k,
                    pos: p,
                    cd: 0.35 + k as f32 * 0.2,
                    fell: false,
                },
                Humanoid {
                    opts: roc_rig(k),
                    driver: Driver::Ally,
                },
                Transform::from_xyz(p.x, 0.0, p.y),
                Visibility::Hidden,
            ));
        }
    }
    // decay.ts checkpoint sandbags: squashed bags in three earth tones
    let lump = sphere(0.28, 8, 5);
    for cp in &w.checkpoints {
        let mut buf = MeshBuf::default();
        let mut seed = crate::worldplay::id_hash(&format!("cp{:.0},{:.0}", cp.p.x, cp.p.y));
        let mut rnd = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        for row in &cp.rows {
            for (p, yaw) in row.bags() {
                let tone = [0x8a7a58, 0x7a6c4c, 0x9a8a68][(rnd() * 3.0) as usize % 3];
                let jitter = (rnd() - 0.5) * 0.2;
                buf.add(
                    &lump,
                    trs(p, [0.0, yaw + jitter, 0.0], [1.0, 0.42, 0.6]),
                    hex(tone),
                );
            }
        }
        commands.spawn((
            Mesh3d(meshes.add(buf.into_mesh())),
            MeshMaterial3d(sand.clone()),
            Transform::default(),
            Visibility::default(),
        ));
    }
}

fn sites() -> &'static [RemnantSite] {
    let w = crate::map::world();
    &w.remnants[..w.remnants.len().min(ROSTER.len())]
}

/// Line of sight over the 1 m sandbags (theirs within 2.6 m are fired over).
fn sight(world: &CombatWorldRes, p: Vec2, q: Vec2) -> bool {
    world.0.shot_ray(p, q, 1.5, 1.4, 2.6) > 0.98
}

#[allow(clippy::too_many_arguments)]
fn fight(
    mut commands: Commands,
    time: Res<Time>,
    pause: Res<PauseOpen>,
    env: Res<CombatEnv>,
    world: Res<CombatWorldRes>,
    lang: Res<Language>,
    (mut clock, mut stats): (ResMut<RemnantClock>, ResMut<RemnantStats>),
    mut state: ResMut<RemnantState>,
    mut rng: ResMut<CombatRng>,
    (mut progress, mut notice, mut campaign): (
        ResMut<ScavengeState>,
        ResMut<WorldNotice>,
        ResMut<Campaign>,
    ),
    player: Query<&Transform, (With<Player>, Without<RemnantMan>, Without<Enemy>)>,
    mut roots: Query<(&RemnantSiteRoot, &mut Visibility), Without<RemnantMan>>,
    mut men: Query<(&mut RemnantMan, &mut Visibility, Option<&mut RigAnim>), Without<Enemy>>,
    mut enemies: Query<(Entity, &mut Enemy, &Transform), Without<RemnantMan>>,
    mut out: (
        MessageWriter<Gunshot>,
        MessageWriter<EnemyDied>,
        MessageWriter<crate::audio::SfxEvent>,
    ),
) {
    if pause.0 {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    clock.t += dt;
    let Ok(pt) = player.single() else { return };
    let pp = pt.translation.xz();
    let sites = sites();
    for (root, mut vis) in &mut roots {
        let far = sites[root.0].p.distance(pp) > ACTIVE_R;
        vis.set_if_neq(if far {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
    for (i, site) in sites.iter().enumerate() {
        let far = site.p.distance(pp) > ACTIVE_R;
        let sv = state.squad(i).clone();
        // men: show / hide; the dead lie where they fell
        for (mut man, mut vis, anim) in &mut men {
            if man.squad != i {
                continue;
            }
            vis.set_if_neq(if far {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            });
            if sv.hp[man.k] > 0.0 {
                man.fell = false;
            } else if let Some(mut r) = anim {
                // one already down when the run was loaded lies where he fell
                if !man.fell {
                    r.anim.death_v = (man.k % 3) as u8;
                    r.anim.dead_t = 1.0;
                }
                r.state = AnimState::Death;
                r.speed = 0.0;
            }
        }
        if far || sv.fallen {
            continue;
        }
        // foes in range of the position
        let foes: Vec<(Entity, Vec2, SolState)> = enemies
            .iter()
            .filter(|(_, e, _)| {
                e.0.alive() && e.0.active(env.night) && e.0.pos.distance(site.p) < ENGAGE_R
            })
            .map(|(ent, e, _)| (ent, e.0.pos, e.0.state))
            .collect();
        for (mut man, _, anim) in &mut men {
            if man.squad != i || sv.hp[man.k] <= 0.0 {
                continue;
            }
            let Some(mut r) = anim else { continue };
            let mut targets: Vec<&(Entity, Vec2, SolState)> = foes.iter().collect();
            targets.sort_by(|a, b| {
                a.1.distance_squared(man.pos)
                    .total_cmp(&b.1.distance_squared(man.pos))
            });
            let Some(&&(ent, fp, _)) = targets
                .iter()
                .find(|(_, fp, _)| sight(&world, man.pos, *fp))
            else {
                r.state = AnimState::Idle;
                r.speed = 0.0;
                r.yaw = std::f32::consts::FRAC_PI_2 - site.ang;
                continue;
            };
            let dist = fp.distance(man.pos);
            r.yaw = (fp.x - man.pos.x).atan2(fp.y - man.pos.y);
            r.state = AnimState::Aim;
            r.speed = 0.0;
            man.cd -= dt;
            if man.cd > 0.0 {
                continue;
            }
            man.cd = 0.9 + ((man.k as i64 * 7 + (clock.t * 10.0) as i64) % 9) as f32 * 0.1;
            r.anim.fire();
            let from = Vec3::new(man.pos.x, 1.4, man.pos.y);
            out.0.write(Gunshot {
                pos: from,
                by_player: false,
                fam: if man.k == 2 {
                    GunFam::Lmg
                } else {
                    GunFam::Rifle
                },
            });
            combat::tracer(&mut commands, from, Vec3::new(fp.x, 1.2, fp.y));
            stats.shots += 1;
            // their gunfire draws patrols
            let noise_at = man.pos;
            for (_, mut e, _) in &mut enemies {
                combat::soldier::noise(
                    std::iter::once(&mut e.0),
                    noise_at,
                    45.0,
                    env.rain,
                    env.night,
                    &mut rng.0,
                );
            }
            if rng.0.next_f32() < (0.45 - dist / 120.0).max(0.18)
                && let Ok((ent, mut e, tf)) = enemies.get_mut(ent)
            {
                stats.hits += 1;
                // (v24.41 one they hit looks for them at their position; the kill is theirs)
                if combat::soldier::damage(&mut e.0, 40.0, man.pos) {
                    commands.entity(ent).insert(Body::new(e.0.kind));
                    out.1.write(EnemyDied {
                        id: e.0.id,
                        kind: e.0.kind,
                        pos: tf.translation,
                    });
                }
            }
        }
        // return fire: each soldier fighting with sight of the position may wound a defender
        let mut sv = sv;
        for (_, fp, st) in &foes {
            if !matches!(st, SolState::Combat | SolState::Investigate)
                || !sight(&world, *fp, site.p)
                || rng.0.next_f32() >= dt * 0.05
            {
                continue;
            }
            let alive: Vec<usize> = (0..4).filter(|&k| sv.hp[k] > 0.0).collect();
            if alive.is_empty() {
                break;
            }
            let k = alive[(rng.0.next_f32() * alive.len() as f32) as usize % alive.len()];
            sv.hp[k] -= 30.0;
            stats.wounded += 1;
            combat::tracer(
                &mut commands,
                Vec3::new(fp.x, 1.4, fp.y),
                Vec3::new(site.man(k).x, 1.2, site.man(k).y),
            );
            for (mut man, _, anim) in &mut men {
                if man.squad == i && man.k == k {
                    if let Some(mut r) = anim {
                        r.anim.hurt();
                    }
                    if sv.hp[k] <= 0.0 {
                        man.fell = true;
                    }
                }
            }
            if sv.hp[k] <= 0.0 {
                sv.hp[k] = 0.0;
                notice.text = match *lang {
                    Language::ZhTw => {
                        format!("{}又倒下一名弟兄。其餘的人沒有退。", unit_name(i, *lang))
                    }
                    Language::En => format!(
                        "Another soldier of the {} has fallen. The rest do not fall back.",
                        unit_name(i, *lang)
                    ),
                };
            }
        }
        if sv.alive() == 0 && !sv.fallen {
            sv.fallen = true;
            let u = &ROSTER[i];
            campaign.notice = tr(*lang, u.fallen.0, u.fallen.1).to_owned();
            campaign.notice_t = 9.0;
            out.2.write(crate::audio::SfxEvent::Radio { dist: 0.0 });
            notice.text = match *lang {
                Language::ZhTw => format!(
                    "{}全員戰至最後一刻。旗子還插著；他們的裝備留在陣地。",
                    unit_name(i, *lang)
                ),
                Language::En => format!(
                    "The {} fought to the very last. Their flag still stands; their gear was left at the position.",
                    unit_name(i, *lang)
                ),
            };
            leave_gear(&mut progress, i, site.gear());
        }
        *state.squad(i) = sv;
    }
}

/// `leave`: the fallen position's crate, once.
fn leave_gear(progress: &mut ScavengeState, i: usize, at: Vec2) {
    let id = gear_id(i);
    if progress.drops.iter().any(|(d, _)| *d == id) {
        return;
    }
    let mut stash = crate::survival::Stash::default();
    for (key, n) in GEAR {
        stash.add(key, n);
    }
    progress.loot.push((id.clone(), stash));
    progress.drops.push((id.clone(), at.to_array()));
    progress.night_seen.push(id);
}

fn publish_spots(state: Res<RemnantState>, lang: Res<Language>, mut spots: ResMut<NpcSpots>) {
    let list: Vec<NpcSpot> = sites()
        .iter()
        .enumerate()
        .map(|(i, site)| {
            let sv = state.get(i);
            let (label, detail) = if sv.fallen {
                (
                    tr(*lang, "陣地", "Position").to_owned(),
                    match *lang {
                        Language::ZhTw => format!("國軍殘部最後的陣地（{}）", unit_name(i, *lang)),
                        Language::En => {
                            format!("Last position of the ROC remnant ({})", unit_name(i, *lang))
                        }
                    },
                )
            } else {
                (
                    tr(*lang, "交談", "Talk").to_owned(),
                    match *lang {
                        Language::ZhTw => {
                            format!("和{}交談（{}）", leader(i, *lang), unit_name(i, *lang))
                        }
                        Language::En => {
                            format!("Talk to {} ({})", leader(i, *lang), unit_name(i, *lang))
                        }
                    },
                )
            };
            NpcSpot {
                key: NpcKey::Remnant(i as u8),
                pos: site.p,
                reach: TALK_R,
                label,
                detail,
                through: false,
            }
        })
        .collect();
    spots.publish(|k| matches!(k, NpcKey::Remnant(_)), list);
}

/// [E] near a squad: the leader's card (`talk()`).
fn talk(
    mut talks: MessageReader<TalkTo>,
    lang: Res<Language>,
    shelter: Res<Shelter>,
    inventory: Res<Inventory>,
    mut state: ResMut<RemnantState>,
    mut dialogue: ResMut<Dialogue>,
) {
    for TalkTo(key) in talks.read() {
        let NpcKey::Remnant(i) = *key else { continue };
        let i = i as usize;
        if i >= sites().len() {
            continue;
        }
        let lang = *lang;
        let unit = unit_name(i, lang);
        let sv = state.squad(i);
        if sv.fallen {
            dialogue.0 = Some(DialogueCard {
                title: tr(
                    lang,
                    "國軍殘部最後的陣地",
                    "Last position of the ROC remnant",
                )
                .into(),
                body: match lang {
                    Language::ZhTw => format!(
                        "{unit}在這個路口戰至最後一人。旗子還插著。他們剩下的裝備留在沙包旁的箱子裡。"
                    ),
                    Language::En => format!(
                        "{unit} held this corner to the last man. The flag is still up. Their remaining gear was left in the crate beside the sandbags."
                    ),
                },
                options: Vec::new(),
            });
            continue;
        }
        let day = shelter.day;
        let first = !sv.met;
        sv.met = true;
        let u = &ROSTER[i];
        let (zh, en) = u.lines[((day + sv.last) as usize) % u.lines.len()];
        sv.last += 1;
        let wounded = sv.hp.iter().any(|h| *h > 0.0 && *h < 100.0);
        let alive = sv.alive();
        let mut options = Vec::new();
        if sv.supply != Some(day) {
            options.push(DialogueOption {
                label: tr(lang, "領補給", "Supplies").into(),
                detail: tr(
                    lang,
                    "領今天的補給：5.56 子彈 20 發、口糧、繃帶。",
                    "Take today’s supplies: 20 rounds of 5.56, a ration and a bandage.",
                )
                .into(),
                key: DialogueKey::RemnantSupply(i as u8),
                enabled: true,
            });
        }
        if wounded {
            let kit = inventory.count("bandage") > 0 || inventory.count("medkit") > 0;
            options.push(DialogueOption {
                label: tr(lang, "包紮", "Bandage").into(),
                detail: if kit {
                    tr(
                        lang,
                        "幫傷兵包紮（用掉一份繃帶或急救包）。",
                        "Bandage the wounded (uses a bandage or first-aid kit).",
                    )
                } else {
                    tr(
                        lang,
                        "有傷兵，但你身上沒有繃帶或急救包。",
                        "There are wounded, but you have no bandage or first-aid kit.",
                    )
                }
                .into(),
                key: DialogueKey::RemnantBandage(i as u8),
                enabled: kit,
            });
        }
        options.push(DialogueOption {
            label: tr(lang, "問敵情", "Intel").into(),
            detail: tr(
                lang,
                "問敵情：他們把解放軍哨點標上你的地圖。",
                "Ask about the enemy: they mark the PLA checkpoints on your map.",
            )
            .into(),
            key: DialogueKey::RemnantIntel(i as u8),
            enabled: true,
        });
        let intro = if first {
            match lang {
                Language::ZhTw => {
                    format!("{unit}。沙包後面還有 {alive} 名弟兄，背後插著一面國旗。\n")
                }
                Language::En => format!(
                    "{unit}. {alive} soldiers behind the sandbags, a flag at their backs.\n"
                ),
            }
        } else {
            String::new()
        };
        dialogue.0 = Some(DialogueCard {
            title: leader(i, lang).into(),
            body: format!("{intro}{}", tr(lang, zh, en)),
            options,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn choose(
    mut chosen: MessageReader<DialogueChosen>,
    lang: Res<Language>,
    shelter: Res<Shelter>,
    mut state: ResMut<RemnantState>,
    mut inventory: ResMut<Inventory>,
    mut notice: ResMut<WorldNotice>,
    mut map: ResMut<MapProgress>,
    mut sfx: MessageWriter<crate::audio::SfxEvent>,
    (mut japan, liaison): (
        ResMut<crate::japan::JapanState>,
        Res<crate::japan::LiaisonSpot>,
    ),
) {
    let lang = *lang;
    for DialogueChosen(key) in chosen.read() {
        match *key {
            DialogueKey::RemnantSupply(i) => {
                let i = i as usize;
                let day = shelter.day;
                let sv = state.squad(i);
                if sv.fallen || sv.supply == Some(day) {
                    continue;
                }
                sv.supply = Some(day);
                inventory.add("a556n", 20);
                inventory.add("ration", 1);
                inventory.add("bandage", 1);
                sfx.write(crate::audio::SfxEvent::Pickup);
                notice.text = match lang {
                    Language::ZhTw => format!(
                        "{}把一盒 5.56 子彈、一包口糧和繃帶塞給你：「我們還有，你拿去。」",
                        leader(i, lang)
                    ),
                    Language::En => format!(
                        "{} presses a box of 5.56 rounds, a ration and a bandage into your hands: “We have more. Take it.”",
                        leader(i, lang)
                    ),
                };
            }
            DialogueKey::RemnantBandage(i) => {
                let sv = state.squad(i as usize);
                // (not one who has died since the talk began: the game goes on while it is open)
                let Some(k) = (0..4)
                    .filter(|&k| sv.hp[k] > 0.0 && sv.hp[k] < 100.0)
                    .min_by(|&a, &b| sv.hp[a].total_cmp(&sv.hp[b]))
                else {
                    continue;
                };
                let (kit, heal) = if inventory.count("medkit") > 0 {
                    ("medkit", 70.0)
                } else if inventory.count("bandage") > 0 {
                    ("bandage", 40.0)
                } else {
                    continue;
                };
                inventory.take(kit, 1);
                sv.hp[k] = (sv.hp[k] + heal).min(100.0);
                sfx.write(crate::audio::SfxEvent::Heal);
                notice.text = tr(
                    lang,
                    "你幫一名受傷的弟兄止了血。他咬著牙說：「謝了，我還能打。」",
                    "You stop a wounded soldier’s bleeding. He grits his teeth: “Thanks. I can still fight.”",
                )
                .into();
            }
            DialogueKey::RemnantIntel(i) => {
                for cp in &crate::map::world().checkpoints {
                    map.reveal(cp.p, 20.0);
                    map.add_note(cp.p, NoteKind::Danger);
                }
                notice.text = match lang {
                    Language::ZhTw => format!(
                        "{}攤開一張手畫的地圖：「他們的哨點都在這裡，我們一個一個標的。」（已標上地圖）",
                        leader(i as usize, lang)
                    ),
                    Language::En => format!(
                        "{} unfolds a hand-drawn map: “Their checkpoints, every one. We marked them one by one.” (added to your map)",
                        leader(i as usize, lang)
                    ),
                };
                // v24.4 they also know where the Japanese liaison is hiding
                if crate::japan::reveal(&mut japan, &mut map, &liaison) {
                    notice.text.push_str(tr(
                        lang,
                        "「日本的連絡官躲在附近一棟樓裡，他有一台能叫飛機的無線電。」",
                        " “There is a Japanese liaison officer hiding in a building nearby. He has a radio that can call aircraft.”",
                    ));
                }
            }
            _ => {}
        }
    }
}

/// Morning radio from a squad still holding (`dawn()`).
fn dawn_report(
    lang: Res<Language>,
    shelter: Res<Shelter>,
    state: Res<RemnantState>,
    mut clock: ResMut<RemnantClock>,
    mut campaign: ResMut<Campaign>,
) {
    let day = shelter.day;
    let Some(last) = clock.reported else {
        // the first frame of a run or a Continue is a baseline, not a new morning
        clock.reported = Some(day);
        return;
    };
    if day <= last {
        return;
    }
    clock.reported = Some(day);
    let n = sites().len();
    let holding: Vec<usize> = (0..n).filter(|&i| !state.get(i).fallen).collect();
    if holding.is_empty() {
        return;
    }
    let i = holding[day as usize % holding.len()];
    campaign.notice = match *lang {
        Language::ZhTw => format!(
            "國軍殘部無線電：「這裡是{}，我們還在萬華，我們沒有撤退。大家撐住。」",
            unit_name(i, *lang)
        ),
        Language::En => format!(
            "ROC remnant radio: “This is {}. We are still in Wanhua. We have not withdrawn. Hold on, everyone.”",
            unit_name(i, *lang)
        ),
    };
    campaign.notice_t = 8.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_matches_the_four_positions() {
        assert_eq!(sites().len(), 4);
        for (i, unit) in ROSTER.iter().enumerate() {
            assert!(!unit_name(i, Language::ZhTw).is_empty());
            assert_eq!(unit.lines.len(), 3);
        }
    }

    #[test]
    fn saved_state_defaults_and_validates() {
        let mut st = RemnantState::default();
        assert!(st.is_valid());
        assert_eq!(st.squad(2).alive(), 4);
        assert_eq!(st.squads.len(), 3);
        st.squad(0).hp[1] = f32::NAN;
        assert!(!st.is_valid());
        let json = serde_json::to_string(&RemnantState::default()).unwrap();
        let back: RemnantState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, RemnantState::default());
    }

    #[test]
    fn gear_crate_is_left_once() {
        let mut p = ScavengeState::default();
        leave_gear(&mut p, 1, Vec2::new(1.0, 2.0));
        leave_gear(&mut p, 1, Vec2::new(1.0, 2.0));
        assert_eq!(p.drops.len(), 1);
        let stash = &p.loot[0].1;
        assert_eq!(stash.count("t91"), 1);
        assert_eq!(stash.count("a556n"), 40);
        for (key, _) in GEAR {
            assert!(crate::inventory::item(key).is_some(), "{key}");
        }
    }

    #[test]
    fn flag_has_a_blue_canton_and_a_white_sun() {
        let img = flag_image();
        let data = img.data.as_ref().unwrap();
        let at = |x: usize, y: usize| {
            let i = (y * 96 + x) * 4;
            [data[i], data[i + 1], data[i + 2]]
        };
        assert_eq!(at(24, 16), [0xff, 0xff, 0xff], "sun centre");
        assert_eq!(at(2, 2), [0x00, 0x00, 0x95], "canton");
        assert_eq!(at(80, 50), [0xfe, 0x00, 0x00], "field");
    }
}
