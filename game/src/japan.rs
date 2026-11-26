//! v24.4 日本支援 — a JSDF liaison officer and his field radio (Web
//! `src/systems/japan.ts` and the liaison wiring in `src/core/main.ts`).
//!
//! Captain Takahashi hides just inside the door of a building 140–280 m from
//! the start. The ROC remnants' intelligence (or the Japanese survivor's own
//! knowledge) marks him on the map. Finding him gives the JSDF field radio and
//! two batteries; afterwards he hands out one battery a day when the player has
//! none. Each radio call takes one battery, is heard by nearby patrols and has a
//! cooldown:
//! - C-2 airdrop (6 min): a transport passes, a crate comes down on a parachute
//!   near the player and puts out red smoke — Japanese rifles, 5.56 NATO,
//!   field rations, medicine, water and a battery.
//! - F-2 strike (5 min): a pair of fighters bombs the densest group of 解放軍
//!   soldiers 18–150 m from the player (three bombs along the track).
//! - AH-64DJ cover (4 min): a gunship circles the player for 45 s, firing its
//!   30 mm gun at soldiers within 55 m.
//!
//! Aircraft are low-poly props with 日の丸 roundels. Callsigns, units and people
//! are fictional, as in the Web build.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat::{
    self, Body, CombatEnv, CombatRng, CombatWorldRes, Enemy, EnemyDied, GameState, HitCause,
    PlayerHit, standin::CombatFxAssets,
};
use crate::dialogue::{
    Dialogue, DialogueCard, DialogueChosen, DialogueKey, DialogueOption, NpcKey, NpcSpot, NpcSpots,
    TalkTo,
};
use crate::game_flow::{Campaign, GameFlow};
use crate::geom::{MeshBuf, cuboid, hex, place, plane, shape, sphere_ex};
use crate::human::anim::AnimState;
use crate::human::rig::RigOpts;
use crate::human::{Driver, Humanoid, RigAnim};
use crate::i18n::{Language, tr};
use crate::inventory::Inventory;
use crate::player::Player;
use crate::roles::SelectedRole;
use crate::survival::{Shelter, Stash};
use crate::ui::PauseOpen;
use crate::worldmap::{MapProgress, NoteKind};
use crate::worldplay::{ScavengeState, WorldNotice, WorldSet};

/// What the radio can call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JpKind {
    Drop,
    Strike,
    Heli,
}

impl JpKind {
    pub const ALL: [JpKind; 3] = [JpKind::Drop, JpKind::Strike, JpKind::Heli];

    fn index(self) -> usize {
        self as usize
    }

    /// `JP_COOLDOWN` in seconds.
    pub fn cooldown(self) -> f32 {
        match self {
            JpKind::Drop => 360.0,
            JpKind::Strike => 300.0,
            JpKind::Heli => 240.0,
        }
    }

    fn label(self, lang: Language) -> &'static str {
        match self {
            JpKind::Drop => tr(
                lang,
                "請求空投（C-2 運輸機）",
                "Request an airdrop (C-2 transport)",
            ),
            JpKind::Strike => tr(lang, "呼叫戰機轟炸（F-2）", "Call an F-2 air strike"),
            JpKind::Heli => tr(
                lang,
                "呼叫直升機掩護（AH-64DJ）",
                "Call AH-64DJ gunship cover",
            ),
        }
    }

    fn short(self, lang: Language) -> &'static str {
        match self {
            JpKind::Drop => tr(lang, "空投", "Airdrop"),
            JpKind::Strike => tr(lang, "轟炸", "Strike"),
            JpKind::Heli => tr(lang, "掩護", "Gunship"),
        }
    }

    fn key(self) -> DialogueKey {
        match self {
            JpKind::Drop => DialogueKey::CallAirdrop,
            JpKind::Strike => DialogueKey::CallStrike,
            JpKind::Heli => DialogueKey::CallApache,
        }
    }
}

/// Saved state of the JSDF support (`surv.jp*` and the cooldowns).
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JapanState {
    /// The liaison is marked on the map.
    pub known: bool,
    pub met: bool,
    pub talk: u32,
    /// Day a spare battery was last handed out.
    pub battery_day: Option<u32>,
    /// Seconds left before each kind of sortie can launch again.
    pub cd: [f32; 3],
    pub drops: u32,
}

impl Default for JapanState {
    fn default() -> Self {
        Self {
            known: false,
            met: false,
            talk: 0,
            battery_day: None,
            cd: [0.0; 3],
            drops: 0,
        }
    }
}

impl JapanState {
    pub fn is_valid(&self) -> bool {
        self.cd
            .iter()
            .zip(JpKind::ALL)
            .all(|(c, k)| c.is_finite() && (0.0..=k.cooldown()).contains(c))
            && self.drops < 100_000
            && self.talk < 1_000_000
    }

    pub fn ready(&self, k: JpKind) -> bool {
        self.cd[k.index()] <= 0.0
    }
}

/// Where the liaison stands (`liaisonSpot`), if the map has such a building.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct LiaisonSpot(pub Option<Vec2>);

/// `liaisonSpot`: 1.6 m inside the door of a core building 140–280 m from `c`,
/// the lowest building id first.
pub fn liaison_spot(w: &std::sync::Arc<crate::map::WorldData>, c: Vec2) -> Option<Vec2> {
    let obstacles = crate::map::Obstacles::new(w.clone());
    let mut cand: Vec<_> = w
        .blds
        .iter()
        .filter(|b| b.door.is_some() && !b.fill)
        .filter(|b| (140.0..280.0).contains(&b.c.distance(c)))
        .collect();
    cand.sort_by_key(|b| b.id);
    cand.into_iter().find_map(|b| {
        let door = b.door?;
        let q = door.p - door.n * 1.6;
        // indoors, clear of walls and furniture (`walkable`)
        (crate::map::data::pip(q, &b.pts) && obstacles.collide(q, 0.35).distance(q) < 1e-3)
            .then_some(q)
    })
}

#[derive(Component)]
struct LiaisonMan;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CraftKind {
    Drop,
    Strike,
    Wingman,
    Heli,
}

/// An aircraft on its way (Web `Craft`).
#[derive(Component)]
struct Craft {
    kind: CraftKind,
    t: f32,
    a: Vec3,
    b: Vec3,
    v: f32,
    dropped: bool,
    heard: bool,
    rotor_t: f32,
    gun_t: f32,
    target: Option<Vec2>,
    land: Option<Vec2>,
}

#[derive(Component)]
struct Rotor;

/// A crate coming down on its parachute.
#[derive(Component)]
struct Chute {
    land: Vec2,
}

/// A bomb that goes off after its delay.
#[derive(Component)]
struct Bomb {
    at: Vec2,
    delay: f32,
}

/// Red smoke over a landed crate (90 s).
#[derive(Component)]
struct Smoke {
    t: f32,
    seed: f32,
}

#[derive(Resource)]
struct JapanArt {
    f2: Handle<Mesh>,
    c2: Handle<Mesh>,
    ah64: Handle<Mesh>,
    rotor: Handle<Mesh>,
    chute: Handle<Mesh>,
    crate_mesh: Handle<Mesh>,
    roundel: Handle<Mesh>,
    smoke: Handle<Mesh>,
    painted: Handle<StandardMaterial>,
    roundel_mat: Handle<StandardMaterial>,
    smoke_mat: Handle<StandardMaterial>,
    tracer: Handle<StandardMaterial>,
}

/// Counters for tests and QA (Web `stats`).
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct JapanStats {
    pub drops: u32,
    pub landed: u32,
    pub strikes: u32,
    pub bombs: u32,
    pub heli_shots: u32,
    pub heli_hits: u32,
}

/// Ask the JSDF for a sortie (the radio card writes this).
#[derive(Message, Clone, Copy, Debug)]
pub struct CallSupport(pub JpKind);

pub struct JapanPlugin;

impl Plugin for JapanPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<JapanState>()
            .init_resource::<JapanStats>()
            .add_message::<CallSupport>()
            .add_systems(Startup, (setup_art, spawn_liaison))
            .add_systems(OnEnter(GameFlow::Prologue), reset_run)
            .add_systems(
                Update,
                (
                    know_from_role,
                    publish_spot,
                    talk,
                    choose,
                    call_support,
                    fly,
                    bombs,
                    chutes,
                    smoke,
                )
                    .chain()
                    .in_set(WorldSet)
                    .run_if(in_state(GameFlow::Playing)),
            )
            .add_systems(OnExit(GameFlow::Playing), clear_sky);
    }
}

fn reset_run(mut state: ResMut<JapanState>) {
    *state = JapanState::default();
}

fn clear_sky(
    mut commands: Commands,
    q: Query<Entity, Or<(With<Craft>, With<Chute>, With<Bomb>, With<Smoke>)>>,
) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// 日の丸: a red disc inside a thin white ring, on a transparent square.
fn roundel_image() -> Image {
    const N: usize = 64;
    let mut px = vec![0u8; N * N * 4];
    for y in 0..N {
        for x in 0..N {
            let d = Vec2::new(x as f32 + 0.5 - 32.0, y as f32 + 0.5 - 32.0).length();
            let c: [u8; 4] = if d < 25.0 {
                [0xc8, 0x10, 0x2e, 255]
            } else if d < 30.0 {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 0]
            };
            px[(y * N + x) * 4..(y * N + x) * 4 + 4].copy_from_slice(&c);
        }
    }
    Image::new(
        bevy::render::render_resource::Extent3d {
            width: N as u32,
            height: N as u32,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        px,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    )
}

fn boxm(m: &mut MeshBuf, col: u32, w: f32, h: f32, d: f32, x: f32, y: f32, z: f32) {
    m.add(&cuboid(w, h, d), place(x, y, z, 0.0), hex(col));
}

/// Roundel decal positions per aircraft (x, y, z, rx, ry, size).
type Decal = (f32, f32, f32, f32, f32, f32);

/// F-2 (blue sea camouflage), nose along +Z, ~15 m.
fn f2_mesh() -> (MeshBuf, Vec<Decal>) {
    let (m, dk) = (0x3f5f86, 0x2a3e5a);
    let mut g = MeshBuf::default();
    boxm(&mut g, m, 1.3, 1.2, 13.0, 0.0, 0.0, 0.0);
    boxm(&mut g, dk, 0.8, 0.6, 2.2, 0.0, 0.6, 3.2);
    let wing = shape(&[
        Vec2::new(-5.2, -3.6),
        Vec2::new(0.0, 2.4),
        Vec2::new(5.2, -3.6),
    ]);
    // three.js ShapeGeometry lies in XY; rotateX(−π/2) puts it in XZ (y → −z).
    // The triangle is symmetric in x, so a copy turned about Y first is its back face.
    for back in [0.0, std::f32::consts::PI] {
        g.add(
            &wing,
            Mat4::from_translation(Vec3::new(0.0, 0.1, 0.0))
                * Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2)
                * Mat4::from_rotation_y(back),
            hex(m),
        );
    }
    boxm(&mut g, m, 0.15, 2.4, 2.2, 0.0, 1.4, -5.2);
    boxm(&mut g, dk, 1.0, 1.0, 0.4, 0.0, 0.0, -6.6);
    let fl = -std::f32::consts::FRAC_PI_2;
    (
        g,
        vec![
            (-3.0, 0.16, -1.8, fl, 0.0, 1.4),
            (3.0, 0.16, -1.8, fl, 0.0, 1.4),
        ],
    )
}

/// C-2 transport (grey), high wing, T-tail, ~44 m.
fn c2_mesh() -> (MeshBuf, Vec<Decal>) {
    let (m, dk) = (0x8a9096, 0x5e646a);
    let mut g = MeshBuf::default();
    boxm(&mut g, m, 4.4, 4.4, 38.0, 0.0, 0.0, 0.0);
    boxm(&mut g, m, 44.0, 0.5, 5.4, 0.0, 2.2, 1.5);
    boxm(&mut g, dk, 1.6, 1.8, 4.2, -7.0, 1.2, 2.2);
    boxm(&mut g, dk, 1.6, 1.8, 4.2, 7.0, 1.2, 2.2);
    boxm(&mut g, m, 0.4, 7.0, 5.0, 0.0, 5.0, -17.0);
    boxm(&mut g, m, 15.0, 0.4, 3.6, 0.0, 8.4, -18.0);
    let fl = -std::f32::consts::FRAC_PI_2;
    (
        g,
        vec![
            (-14.0, 2.5, 1.5, fl, 0.0, 2.6),
            (14.0, 2.5, 1.5, fl, 0.0, 2.6),
        ],
    )
}

/// AH-64DJ (olive drab), ~17 m; the rotor is a separate spinning part.
fn ah64_mesh() -> (MeshBuf, Vec<Decal>) {
    let (m, dk) = (0x4a5236, 0x2a2e22);
    let mut g = MeshBuf::default();
    boxm(&mut g, m, 1.4, 2.0, 6.5, 0.0, 0.0, 1.2);
    boxm(&mut g, dk, 1.0, 0.9, 1.8, 0.0, 0.8, 3.2);
    boxm(&mut g, m, 0.5, 0.6, 7.5, 0.0, 0.3, -5.5);
    boxm(&mut g, m, 0.15, 2.0, 1.4, 0.0, 1.0, -9.0);
    boxm(&mut g, m, 4.4, 0.25, 1.0, 0.0, -0.1, 0.6);
    boxm(&mut g, dk, 0.35, 0.35, 1.5, 0.0, -1.1, 3.8);
    (
        g,
        vec![(0.3, 0.3, -6.0, 0.0, std::f32::consts::FRAC_PI_2, 0.9)],
    )
}

fn setup_art(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let painted = materials.add(StandardMaterial {
        perceptual_roughness: 0.6,
        metallic: 0.2,
        ..default()
    });
    let mut rotor = MeshBuf::default();
    boxm(&mut rotor, 0x2a2e22, 14.6, 0.06, 0.5, 0.0, 0.0, 0.0);
    boxm(&mut rotor, 0x2a2e22, 0.5, 0.06, 14.6, 0.0, 0.0, 0.0);
    let mut chute = MeshBuf::default();
    chute.add(
        &sphere_ex(
            2.6,
            12,
            6,
            0.0,
            std::f32::consts::TAU,
            0.0,
            std::f32::consts::FRAC_PI_2,
        ),
        place(0.0, 5.0, 0.0, 0.0),
        hex(0xd8d2c0),
    );
    boxm(&mut chute, 0x55603a, 1.3, 1.1, 1.3, 0.0, 0.55, 0.0);
    let (f2, _) = f2_mesh();
    let (c2, _) = c2_mesh();
    let (ah, _) = ah64_mesh();
    let mut crate_mesh = MeshBuf::default();
    boxm(&mut crate_mesh, 0x55603a, 1.2, 1.3, 0.8, 0.0, 0.0, 0.0);
    commands.insert_resource(JapanArt {
        f2: meshes.add(f2.into_mesh()),
        c2: meshes.add(c2.into_mesh()),
        ah64: meshes.add(ah.into_mesh()),
        rotor: meshes.add(rotor.into_mesh()),
        chute: meshes.add(chute.into_mesh()),
        crate_mesh: meshes.add(crate_mesh.into_mesh()),
        roundel: meshes.add(plane(1.0, 1.0).into_mesh()),
        smoke: meshes.add(Sphere::new(0.6).mesh().ico(1).unwrap()),
        painted: painted.clone(),
        roundel_mat: materials.add(StandardMaterial {
            base_color_texture: Some(images.add(roundel_image())),
            alpha_mode: AlphaMode::Blend,
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
        smoke_mat: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.23, 0.16, 0.5),
            emissive: LinearRgba::rgb(0.4, 0.05, 0.02),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
        tracer: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.82, 0.44),
            unlit: true,
            ..default()
        }),
    });
}

fn spawn_liaison(mut commands: Commands) {
    let w = crate::map::world();
    let spot = liaison_spot(w, w.spawn);
    commands.insert_resource(LiaisonSpot(spot));
    let Some(p) = spot else { return };
    // JGSDF type III camouflage greens and browns
    commands.spawn((
        LiaisonMan,
        Humanoid {
            opts: RigOpts {
                female: false,
                top: 0x5f5d3e,
                bottom: 0x4f4a33,
                shoes: 0x1a1a14,
                soldier: true,
                jacket: None,
                pack: None,
                outfit: "",
                gun: "type20",
            },
            driver: Driver::Ally,
        },
        Transform::from_xyz(p.x, 0.0, p.y),
        Visibility::default(),
    ));
}

/// The Japanese survivor knows where the liaison is from the start.
fn know_from_role(
    selected: Res<SelectedRole>,
    spot: Res<LiaisonSpot>,
    mut state: ResMut<JapanState>,
    mut map: ResMut<MapProgress>,
) {
    if selected.id == "jp" && !state.known {
        reveal(&mut state, &mut map, &spot);
    }
}

/// `revealLiaison`: mark him on the survivor map (once).
pub fn reveal(state: &mut JapanState, map: &mut MapProgress, spot: &LiaisonSpot) -> bool {
    let Some(p) = spot.0 else { return false };
    let first = !state.known;
    state.known = true;
    if first {
        map.reveal(p, 14.0);
        map.add_note(p, NoteKind::Supplies);
    }
    first
}

fn publish_spot(
    spot: Res<LiaisonSpot>,
    lang: Res<Language>,
    time: Res<Time>,
    mut spots: ResMut<NpcSpots>,
    mut man: Query<(&Transform, Option<&mut RigAnim>), With<LiaisonMan>>,
    player: Query<&Transform, (With<Player>, Without<LiaisonMan>)>,
) {
    let Some(p) = spot.0 else { return };
    // he keeps watch on the door, and looks at the player when they come in
    if let Ok((_, Some(mut r))) = man.single_mut() {
        r.state = AnimState::Idle;
        r.speed = 0.0;
        if let Ok(pt) = player.single() {
            let d = pt.translation.xz() - p;
            if d.length() < 8.0 {
                let want = d.x.atan2(d.y);
                r.yaw += (want - r.yaw).sin().atan2((want - r.yaw).cos())
                    * (time.delta_secs() * 4.0).min(1.0);
            }
        }
    }
    spots.publish(
        |k| k == NpcKey::Liaison,
        [NpcSpot {
            key: NpcKey::Liaison,
            pos: p,
            reach: 2.4,
            label: tr(*lang, "交談", "Talk").into(),
            detail: tr(
                *lang,
                "自衛隊連絡官 高橋一尉",
                "JSDF liaison, Captain Takahashi",
            )
            .into(),
            through: false,
        }],
    );
}

const LINES: [(&str, &str); 4] = [
    (
        "「大丈夫、必ず助けが来ます。」——別擔心，救援一定會來。",
        "“Daijōbu. Help will come — I promise you that.”",
    ),
    (
        "「你們守住的每一條巷子，我們都看得到。」",
        "“Every alley you hold, we can see it from up there.”",
    ),
    (
        "「空投箱落地會冒紅煙。拿了就走，別在那裡待太久。」",
        "“The drop crate puts out red smoke when it lands. Grab it and go — do not linger.”",
    ),
    (
        "「F-2 只打離你 18 公尺以外的目標。再近，連你也會一起炸到。」",
        "“The F-2s only hit targets 18 m or more from you. Any closer and you would be under the bombs too.”",
    ),
];

#[allow(clippy::too_many_arguments)]
fn talk(
    mut talks: MessageReader<TalkTo>,
    lang: Res<Language>,
    shelter: Res<Shelter>,
    spot: Res<LiaisonSpot>,
    mut state: ResMut<JapanState>,
    mut map: ResMut<MapProgress>,
    mut inventory: ResMut<Inventory>,
    mut dialogue: ResMut<Dialogue>,
    mut sfx: MessageWriter<crate::audio::SfxEvent>,
) {
    let lang = *lang;
    for TalkTo(key) in talks.read() {
        match key {
            NpcKey::Liaison => {
                let first = !state.met;
                state.met = true;
                reveal(&mut state, &mut map, &spot);
                if first {
                    inventory.add("jpradio", 1);
                    inventory.add("battery", 2);
                    sfx.write(crate::audio::SfxEvent::Pickup);
                    dialogue.0 = Some(DialogueCard {
                        title: tr(lang, "高橋一尉（陸上自衛隊 連絡幹部）", "Captain Takahashi (JGSDF liaison officer)").into(),
                        body: tr(
                            lang,
                            "「陸上自衛隊、連絡幹部の高橋一尉です。」——「我是自衛隊的連絡官高橋。日本不會袖手旁觀。這台無線電給你：需要的時候，天空會有人回應。電池省著用，無線電一響，附近的巡邏也會聽到。」（獲得：自衛隊野戰無線電、電池 ×2）",
                            "“I am Captain Takahashi, JGSDF liaison officer. Japan will not stand by and watch. Take this radio: when you need it, someone in the sky will answer. Go easy on the batteries — every call is heard by the patrols nearby.” (Received: JSDF field radio, 2 batteries)",
                        )
                        .into(),
                        options: Vec::new(),
                    });
                    continue;
                }
                state.talk += 1;
                let (zh, en) = LINES[((shelter.day + state.talk) as usize) % LINES.len()];
                let mut options = Vec::new();
                if inventory.count("battery") < 1 {
                    let given = state.battery_day == Some(shelter.day);
                    options.push(DialogueOption {
                        label: tr(lang, "電池", "Battery").into(),
                        detail: if given {
                            tr(
                                lang,
                                "「今天的已經給你了。」",
                                "“I already gave you today’s.”",
                            )
                        } else {
                            tr(lang, "「電池沒了……」", "“I am out of batteries…”")
                        }
                        .into(),
                        key: DialogueKey::LiaisonBattery,
                        enabled: !given,
                    });
                }
                dialogue.0 = Some(DialogueCard {
                    title: tr(lang, "高橋一尉", "Captain Takahashi").into(),
                    body: tr(lang, zh, en).into(),
                    options,
                });
            }
            NpcKey::Radio => {
                if inventory.count("jpradio") == 0 {
                    continue;
                }
                let batteries = inventory.count("battery");
                let options = JpKind::ALL
                    .into_iter()
                    .map(|k| {
                        let cd = state.cd[k.index()].ceil() as u32;
                        DialogueOption {
                            label: k.short(lang).into(),
                            detail: if cd > 0 {
                                match lang {
                                    Language::ZhTw => format!("{}（{cd} 秒）", k.label(lang)),
                                    Language::En => format!("{} ({cd} s)", k.label(lang)),
                                }
                            } else {
                                k.label(lang).into()
                            },
                            key: k.key(),
                            enabled: true,
                        }
                    })
                    .collect();
                dialogue.0 = Some(DialogueCard {
                    title: tr(lang, "自衛隊野戰無線電", "JSDF field radio").into(),
                    body: match lang {
                        Language::ZhTw => {
                            format!("電池：{batteries} 顆。每次呼叫用一顆，附近的巡邏會聽到。")
                        }
                        Language::En => format!(
                            "Batteries: {batteries}. Every call uses one and is heard by nearby patrols."
                        ),
                    },
                    options,
                });
            }
            _ => {}
        }
    }
}

fn choose(
    mut chosen: MessageReader<DialogueChosen>,
    lang: Res<Language>,
    shelter: Res<Shelter>,
    mut state: ResMut<JapanState>,
    mut inventory: ResMut<Inventory>,
    mut notice: ResMut<WorldNotice>,
    mut calls: MessageWriter<CallSupport>,
) {
    for DialogueChosen(key) in chosen.read() {
        match key {
            DialogueKey::LiaisonBattery => {
                if state.battery_day == Some(shelter.day) {
                    notice.text = tr(
                        *lang,
                        "「今天的已經給你了。」",
                        "“I already gave you today’s.”",
                    )
                    .into();
                    continue;
                }
                state.battery_day = Some(shelter.day);
                inventory.add("battery", 1);
                notice.text = tr(
                    *lang,
                    "高橋一尉從背包裡掏出一顆電池給你。",
                    "Captain Takahashi digs a battery out of his pack for you.",
                )
                .into();
            }
            DialogueKey::CallAirdrop => {
                calls.write(CallSupport(JpKind::Drop));
            }
            DialogueKey::CallStrike => {
                calls.write(CallSupport(JpKind::Strike));
            }
            DialogueKey::CallApache => {
                calls.write(CallSupport(JpKind::Heli));
            }
            _ => {}
        }
    }
}

/// The densest 解放軍 group 18–150 m from `p` (most soldiers within 12 m of one).
pub fn strike_target(p: Vec2, foes: &[Vec2]) -> Option<Vec2> {
    let live: Vec<Vec2> = foes
        .iter()
        .copied()
        .filter(|f| (18.0..150.0).contains(&f.distance(p)) && f.distance(p) > 18.0)
        .collect();
    let mut best: Option<(Vec2, usize)> = None;
    for f in &live {
        let n = live.iter().filter(|o| o.distance(*f) < 12.0).count();
        if best.is_none_or(|(_, bn)| n > bn) {
            best = Some((*f, n));
        }
    }
    best.map(|(f, _)| f)
}

/// The track every sortie flies along.
fn track() -> Vec3 {
    Vec3::new(1.0, 0.0, 0.35).normalize()
}

#[allow(clippy::too_many_arguments)]
fn call_support(
    mut commands: Commands,
    mut calls: MessageReader<CallSupport>,
    lang: Res<Language>,
    art: Res<JapanArt>,
    env: Res<CombatEnv>,
    obstacles: Res<crate::map::Obstacles>,
    (mut state, mut stats): (ResMut<JapanState>, ResMut<JapanStats>),
    mut inventory: ResMut<Inventory>,
    (mut notice, mut campaign): (ResMut<WorldNotice>, ResMut<Campaign>),
    mut rng: ResMut<CombatRng>,
    player: Query<&Transform, With<Player>>,
    mut enemies: Query<&mut Enemy>,
    mut sfx: MessageWriter<crate::audio::SfxEvent>,
) {
    let lang = *lang;
    let Ok(pt) = player.single() else { return };
    let pp = pt.translation.xz();
    for CallSupport(k) in calls.read() {
        let k = *k;
        if inventory.count("battery") < 1 {
            notice.text = tr(
                lang,
                "無線電沒電了——找電池（住家、五金行），或回去找高橋一尉",
                "The radio is dead — find batteries (homes, hardware stores), or go back to Captain Takahashi",
            )
            .into();
            continue;
        }
        if !state.ready(k) {
            let cd = state.cd[k.index()].ceil() as u32;
            notice.text = match lang {
                Language::ZhTw => format!("「了解，下一架次 {cd} 秒後才能出動。」"),
                Language::En => {
                    format!("“Understood. The next sortie can launch in {cd} seconds.”")
                }
            };
            continue;
        }
        let dir = track();
        let msg = match k {
            JpKind::Strike => {
                let foes: Vec<Vec2> = enemies
                    .iter()
                    .filter(|e| e.0.alive() && e.0.active(env.night))
                    .map(|e| e.0.pos)
                    .collect();
                let Some(tg) = strike_target(pp, &foes) else {
                    notice.text = tr(
                        lang,
                        "「目標未確認——附近 18～150 公尺內沒有發現解放軍。」",
                        "“No target confirmed — no PLA soldiers 18–150 m from you.”",
                    )
                    .into();
                    continue;
                };
                let c = Vec3::new(tg.x, 42.0, tg.y);
                spawn_craft(
                    &mut commands,
                    &art,
                    CraftKind::Strike,
                    c - dir * 380.0,
                    c + dir * 380.0,
                    75.0,
                    Some(tg),
                    None,
                );
                let off = Vec3::new(-dir.z, 0.0, dir.x) * 14.0;
                spawn_craft(
                    &mut commands,
                    &art,
                    CraftKind::Wingman,
                    c - dir * 420.0 + off,
                    c + dir * 340.0 + off,
                    75.0,
                    None,
                    None,
                );
                stats.strikes += 1;
                tr(
                    lang,
                    "「こちらスピア・ワン、目標確認。投下まで六秒。」（槍隊一號：目標確認，六秒後投彈——離開那裡！）",
                    "“Spear One, target confirmed. Release in six seconds.” — get clear!",
                )
            }
            JpKind::Heli => {
                let c = Vec3::new(pp.x, 18.0, pp.y);
                spawn_craft(
                    &mut commands,
                    &art,
                    CraftKind::Heli,
                    c - dir * 220.0,
                    c,
                    32.0,
                    None,
                    None,
                );
                tr(
                    lang,
                    "「アパッチ、現着まで七秒。頭を下げて。」（阿帕契七秒後到場，壓低身子！）",
                    "“Apache on station in seven seconds. Keep your head down.”",
                )
            }
            JpKind::Drop => {
                let c = Vec3::new(pp.x, 70.0, pp.y);
                let land = free_near(&obstacles, pp + Vec2::new(dir.x, dir.z) * 22.0);
                spawn_craft(
                    &mut commands,
                    &art,
                    CraftKind::Drop,
                    c - dir * 500.0,
                    c + dir * 500.0,
                    48.0,
                    None,
                    Some(land),
                );
                stats.drops += 1;
                tr(
                    lang,
                    "「C-2、投下準備よし。」（C-2 運輸機十秒後通過，空投箱帶紅色煙霧落地。）",
                    "“C-2, ready to drop.” The transport passes over in about ten seconds; the crate comes down on a parachute.",
                )
            }
        };
        state.cd[k.index()] = k.cooldown();
        inventory.take("battery", 1);
        campaign.notice = msg.into();
        campaign.notice_t = 7.0;
        // slowDown: a call ends accelerated time
        campaign.speed = 1;
        campaign.sleeping = false;
        sfx.write(crate::audio::SfxEvent::Radio { dist: 0.0 });
        if k != JpKind::Drop {
            for mut e in &mut enemies {
                combat::soldier::noise(
                    std::iter::once(&mut e.0),
                    pp,
                    60.0,
                    env.rain,
                    env.night,
                    &mut rng.0,
                );
            }
        }
    }
}

/// `freeNear`: an open spot near `p` for the crate.
fn free_near(obstacles: &crate::map::Obstacles, p: Vec2) -> Vec2 {
    for r in 0..12 {
        for k in 0..8 {
            let q = p + Vec2::from_angle(k as f32 * std::f32::consts::FRAC_PI_4) * r as f32 * 2.0;
            if !obstacles.blocked(q, 1.0) && crate::map::world().bld_at(q).is_none() {
                return q;
            }
        }
    }
    p
}

#[allow(clippy::too_many_arguments)]
fn spawn_craft(
    commands: &mut Commands,
    art: &JapanArt,
    kind: CraftKind,
    a: Vec3,
    b: Vec3,
    v: f32,
    target: Option<Vec2>,
    land: Option<Vec2>,
) {
    let (mesh, decals) = match kind {
        CraftKind::Strike | CraftKind::Wingman => (art.f2.clone(), f2_mesh().1),
        CraftKind::Drop => (art.c2.clone(), c2_mesh().1),
        CraftKind::Heli => (art.ah64.clone(), ah64_mesh().1),
    };
    let facing = Transform::from_translation(a).looking_at(Vec3::new(b.x, a.y, b.z), Vec3::Y);
    // the models' nose is +Z; looking_at points −Z at the target
    let tf = facing.with_rotation(facing.rotation * Quat::from_rotation_y(std::f32::consts::PI));
    commands
        .spawn((
            Craft {
                kind,
                t: 0.0,
                a,
                b,
                v,
                dropped: false,
                heard: false,
                rotor_t: 0.0,
                gun_t: 0.0,
                target,
                land,
            },
            Mesh3d(mesh),
            MeshMaterial3d(art.painted.clone()),
            tf,
            Visibility::default(),
        ))
        .with_children(|c| {
            for (x, y, z, rx, ry, s) in decals {
                c.spawn((
                    Mesh3d(art.roundel.clone()),
                    MeshMaterial3d(art.roundel_mat.clone()),
                    Transform::from_xyz(x, y, z)
                        .with_rotation(Quat::from_euler(EulerRot::XYZ, rx, ry, 0.0))
                        .with_scale(Vec3::splat(s)),
                    bevy::light::NotShadowCaster,
                ));
            }
            if kind == CraftKind::Heli {
                c.spawn((
                    Rotor,
                    Mesh3d(art.rotor.clone()),
                    MeshMaterial3d(art.painted.clone()),
                    Transform::from_xyz(0.0, 1.6, 0.0),
                ));
            }
        });
}

fn face(tf: &mut Transform, toward: Vec3) {
    let flat = Vec3::new(toward.x, tf.translation.y, toward.z);
    if flat.distance(tf.translation) > 0.01 {
        tf.look_at(flat, Vec3::Y);
        tf.rotate_y(std::f32::consts::PI);
    }
}

#[allow(clippy::too_many_arguments)]
fn fly(
    mut commands: Commands,
    time: Res<Time>,
    pause: Res<PauseOpen>,
    art: Res<JapanArt>,
    world: Res<CombatWorldRes>,
    lang: Res<Language>,
    (mut state, mut stats): (ResMut<JapanState>, ResMut<JapanStats>),
    mut rng: ResMut<CombatRng>,
    mut notice: ResMut<WorldNotice>,
    player: Query<&Transform, (With<Player>, Without<Craft>)>,
    mut crafts: Query<(Entity, &mut Craft, &mut Transform), Without<Rotor>>,
    mut rotors: Query<&mut Transform, (With<Rotor>, Without<Craft>, Without<Player>)>,
    mut enemies: Query<
        (Entity, &mut Enemy, &Transform),
        (Without<Craft>, Without<Player>, Without<Rotor>),
    >,
    mut out: (
        MessageWriter<crate::audio::SfxEvent>,
        MessageWriter<EnemyDied>,
    ),
) {
    if pause.0 {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    for c in &mut state.cd {
        *c = (*c - dt).max(0.0);
    }
    let Ok(pt) = player.single() else { return };
    let pp = pt.translation.xz();
    let indoor = world.0.building_at(pp).is_some();
    for mut r in &mut rotors {
        r.rotate_y(dt * 28.0);
    }
    for (ent, mut c, mut tf) in &mut crafts {
        c.t += dt;
        if c.kind == CraftKind::Heli {
            // fly in, orbit the player for 45 s firing on soldiers within 55 m, then leave
            c.rotor_t -= dt;
            if c.rotor_t <= 0.0 {
                c.rotor_t = 1.1;
                out.0.write(crate::audio::SfxEvent::Rotor {
                    dist: tf.translation.xz().distance(pp),
                });
            }
            let len = c.a.distance(c.b);
            if c.t * c.v < len {
                tf.translation = c.a.lerp(c.b, c.t * c.v / len);
                continue;
            }
            if c.t > 52.0 {
                tf.translation.y += dt * 6.0;
                tf.translation.x += dt * 40.0;
                if c.t > 60.0 {
                    commands.entity(ent).despawn();
                }
                continue;
            }
            let ang = (c.t - 7.0) * 0.35;
            let at = pp + Vec2::from_angle(ang) * 22.0;
            tf.translation = Vec3::new(at.x, 18.0, at.y);
            let ahead = pp + Vec2::from_angle(ang + 0.3) * 22.0;
            face(&mut tf, Vec3::new(ahead.x, 18.0, ahead.y));
            c.gun_t -= dt;
            if c.gun_t > 0.0 {
                continue;
            }
            c.gun_t = 0.9;
            let Some((foe, fpos)) = enemies
                .iter()
                .filter(|(_, e, _)| e.0.alive() && e.0.pos.distance(at) < 55.0)
                .min_by(|a, b| {
                    a.1.0
                        .pos
                        .distance_squared(at)
                        .total_cmp(&b.1.0.pos.distance_squared(at))
                })
                .map(|(e, s, _)| (e, s.0.pos))
            else {
                continue;
            };
            stats.heli_shots += 1;
            crate::combat::sfx::play_gunshot_sfx(
                &mut out.0,
                crate::combat::tuning::GunFam::Lmg,
                at.distance(pp),
                indoor,
            );
            spawn_tracer(
                &mut commands,
                &art,
                tf.translation,
                Vec3::new(fpos.x, 0.8, fpos.y),
            );
            if rng.0.next_f32() < 0.6
                && let Ok((fe, mut e, etf)) = enemies.get_mut(foe)
            {
                stats.heli_hits += 1;
                if combat::soldier::damage(&mut e.0, 45.0, fpos) {
                    commands.entity(fe).insert(Body::new(e.0.kind));
                    out.1.write(EnemyDied {
                        id: e.0.id,
                        kind: e.0.kind,
                        pos: etf.translation,
                    });
                }
            }
            continue;
        }
        let len = c.a.distance(c.b);
        let s = (c.t * c.v / len).min(1.0);
        tf.translation = c.a.lerp(c.b, s);
        // the roar reaches you a moment before the plane is overhead
        if !c.heard && s >= 0.42 {
            c.heard = true;
            out.0.write(crate::audio::SfxEvent::JetPass {
                dist: tf.translation.xz().distance(pp),
            });
        }
        if c.kind == CraftKind::Strike && !c.dropped && s >= 0.5 {
            c.dropped = true;
            if let Some(tg) = c.target {
                let d = (c.b - c.a).with_y(0.0).normalize();
                for (i, o) in [-6.0_f32, 0.0, 6.0].into_iter().enumerate() {
                    commands.spawn(Bomb {
                        at: tg + Vec2::new(d.x, d.z) * o,
                        delay: 0.25 + i as f32 * 0.22,
                    });
                }
                notice.text = tr(*lang, "F-2 投彈命中！", "F-2 bombs on target!").into();
            }
        }
        if c.kind == CraftKind::Drop
            && !c.dropped
            && s >= 0.5
            && let Some(land) = c.land
        {
            c.dropped = true;
            commands.spawn((
                Chute { land },
                Mesh3d(art.chute.clone()),
                MeshMaterial3d(art.painted.clone()),
                Transform::from_xyz(land.x, 60.0, land.y),
                Visibility::default(),
            ));
        }
        if s >= 1.0 {
            commands.entity(ent).despawn();
        }
    }
}

#[derive(Component)]
struct JpTracer(f32);

fn spawn_tracer(commands: &mut Commands, art: &JapanArt, a: Vec3, b: Vec3) {
    let len = a.distance(b);
    if len < 0.1 {
        return;
    }
    let mid = (a + b) * 0.5;
    let tf = Transform::from_translation(mid)
        .looking_at(b, Vec3::Y)
        .with_scale(Vec3::new(0.06, 0.06, len));
    commands.spawn((
        JpTracer(0.12),
        Mesh3d(art.crate_mesh.clone()),
        MeshMaterial3d(art.tracer.clone()),
        tf,
        bevy::light::NotShadowCaster,
    ));
}

#[allow(clippy::too_many_arguments)]
fn bombs(
    mut commands: Commands,
    time: Res<Time>,
    pause: Res<PauseOpen>,
    env: Res<CombatEnv>,
    world: Res<CombatWorldRes>,
    fx: Option<Res<CombatFxAssets>>,
    (mut stats, mut game, mut rng): (ResMut<JapanStats>, ResMut<GameState>, ResMut<CombatRng>),
    player: Query<&Transform, With<Player>>,
    mut q: Query<(Entity, &mut Bomb)>,
    mut tracers: Query<(Entity, &mut JpTracer)>,
    mut enemies: Query<(Entity, &mut Enemy, &Transform), Without<Player>>,
    mut out: (
        MessageWriter<crate::audio::SfxEvent>,
        MessageWriter<EnemyDied>,
        MessageWriter<PlayerHit>,
    ),
) {
    if pause.0 {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    for (e, mut t) in &mut tracers {
        t.0 -= dt;
        if t.0 <= 0.0 {
            commands.entity(e).despawn();
        }
    }
    let pp = player.single().map_or(Vec2::ZERO, |t| t.translation.xz());
    for (be, mut b) in &mut q {
        b.delay -= dt;
        if b.delay > 0.0 {
            continue;
        }
        commands.entity(be).despawn();
        let p = b.at;
        stats.bombs += 1;
        if let Some(fx) = fx.as_deref() {
            combat::standin::spawn_blast(&mut commands, fx, p);
        }
        out.0.write(crate::audio::SfxEvent::ShellBoom {
            dist: pp.distance(p),
            indoor: world.0.building_at(pp).is_some(),
        });
        // blast(): 220 to soldiers within 6 m; the wounded look where it fell
        for (ent, mut e, tf) in &mut enemies {
            if e.0.alive()
                && e.0.pos.distance(p) < 6.0
                && combat::soldier::damage(&mut e.0, 220.0, p)
            {
                commands.entity(ent).insert(Body::new(e.0.kind));
                out.1.write(EnemyDied {
                    id: e.0.id,
                    kind: e.0.kind,
                    pos: tf.translation,
                });
            }
            combat::soldier::noise(
                std::iter::once(&mut e.0),
                p,
                45.0,
                env.rain,
                env.night,
                &mut rng.0,
            );
        }
        let dp = pp.distance(p);
        if dp < 6.0 && !game.over {
            let dmg = 45.0 * (1.0 - dp / 6.0);
            game.hp -= dmg;
            out.2.write(PlayerHit {
                dmg,
                cause: HitCause::Grenade,
                bleed: false,
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn chutes(
    mut commands: Commands,
    time: Res<Time>,
    pause: Res<PauseOpen>,
    art: Res<JapanArt>,
    env: Res<CombatEnv>,
    lang: Res<Language>,
    (mut state, mut stats): (ResMut<JapanState>, ResMut<JapanStats>),
    mut progress: ResMut<ScavengeState>,
    mut notice: ResMut<WorldNotice>,
    mut map: ResMut<MapProgress>,
    mut rng: ResMut<CombatRng>,
    mut q: Query<(Entity, &Chute, &mut Transform)>,
    mut enemies: Query<&mut Enemy>,
) {
    if pause.0 {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    for (e, chute, mut tf) in &mut q {
        tf.translation.y = (tf.translation.y - dt * 7.0).max(0.0);
        tf.rotate_y(dt * 0.4);
        if tf.translation.y > 0.0 {
            continue;
        }
        commands.entity(e).despawn();
        stats.landed += 1;
        let id = format!("jpdrop{}", state.drops);
        state.drops += 1;
        progress
            .loot
            .push((id.clone(), airdrop_items(&mut || rng.0.next_f32())));
        progress.drops.push((id.clone(), chute.land.to_array()));
        progress.night_seen.push(id);
        map.add_note(chute.land, NoteKind::Supplies);
        notice.text = tr(
            *lang,
            "日本空投箱落地了（紅色煙霧處）：自衛隊的武器、彈藥和戰鬥糧食。",
            "The Japanese supply crate is down (red smoke): JSDF weapons, ammunition and field rations.",
        )
        .into();
        for i in 0..10 {
            commands.spawn((
                Smoke {
                    t: 0.0,
                    seed: i as f32 * 0.37,
                },
                Mesh3d(art.smoke.clone()),
                MeshMaterial3d(art.smoke_mat.clone()),
                Transform::from_xyz(chute.land.x, 0.5 + i as f32 * 1.1, chute.land.y),
                bevy::light::NotShadowCaster,
            ));
        }
        // the patrols saw the parachute too
        for mut s in &mut enemies {
            combat::soldier::noise(
                std::iter::once(&mut s.0),
                chute.land,
                50.0,
                env.rain,
                env.night,
                &mut rng.0,
            );
        }
    }
}

/// `jpLanded`: what the crate holds.
pub fn airdrop_items(r: &mut impl FnMut() -> f32) -> Stash {
    let mut s = Stash::default();
    s.add(if r() < 0.5 { "type20" } else { "type89" }, 1);
    s.add("a556n", 60);
    s.add("jsdfration", 3);
    s.add("medkit", 1);
    s.add("water", 2);
    s.add("battery", 1);
    if r() < 0.35 {
        s.add("p220", 1);
        s.add("a9", 18);
    }
    if r() < 0.12 {
        s.add("minimi", 1);
    }
    if r() < 0.12 {
        s.add("lmat", 1);
        s.add("lmatm", 1);
    }
    s
}

fn smoke(
    mut commands: Commands,
    time: Res<Time>,
    pause: Res<PauseOpen>,
    mut q: Query<(Entity, &mut Smoke, &mut Transform)>,
) {
    if pause.0 {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    for (e, mut s, mut tf) in &mut q {
        s.t += dt;
        if s.t > 90.0 {
            commands.entity(e).despawn();
            continue;
        }
        // a slow drifting column, renewed from the ground
        tf.translation.y += dt * 1.2;
        tf.translation.x += (s.t * 0.7 + s.seed * 9.0).sin() * dt * 0.3;
        if tf.translation.y > 12.0 {
            tf.translation.y = 0.4;
        }
        let k = 0.6 + tf.translation.y / 12.0 * 1.4;
        tf.scale = Vec3::splat(k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liaison_stands_inside_a_core_building_140_to_280_m_out() {
        let w = crate::map::world();
        let p = liaison_spot(w, w.spawn).expect("a liaison building");
        let b = w.bld_at(p).expect("inside");
        assert!((140.0..280.0).contains(&b.c.distance(w.spawn)));
        assert!(b.door.is_some() && !b.fill);
    }

    #[test]
    fn strike_picks_the_densest_group_out_of_danger_close() {
        let p = Vec2::ZERO;
        let foes = [
            Vec2::new(10.0, 0.0), // too close
            Vec2::new(60.0, 0.0),
            Vec2::new(64.0, 3.0),
            Vec2::new(66.0, -2.0),
            Vec2::new(-90.0, 0.0),
            Vec2::new(200.0, 0.0), // too far
        ];
        let t = strike_target(p, &foes).unwrap();
        assert!(t.x > 55.0 && t.x < 70.0);
        assert!(strike_target(p, &foes[..1]).is_none());
    }

    #[test]
    fn airdrop_always_has_the_core_supplies() {
        let mut seq = [0.9_f32, 0.9, 0.9, 0.9].into_iter().cycle();
        let s = airdrop_items(&mut || seq.next().unwrap());
        assert_eq!(s.count("type89"), 1);
        assert_eq!(s.count("a556n"), 60);
        assert_eq!(s.count("jsdfration"), 3);
        assert_eq!(s.count("battery"), 1);
        let mut low = [0.0_f32].into_iter().cycle();
        let rare = airdrop_items(&mut || low.next().unwrap());
        assert_eq!(rare.count("type20"), 1);
        assert_eq!(rare.count("minimi"), 1);
        assert_eq!(rare.count("lmatm"), 1);
        for (key, _) in rare.stacks() {
            assert!(crate::inventory::item(key).is_some(), "{key}");
        }
    }

    #[test]
    fn saved_state_validates_cooldowns() {
        let mut s = JapanState::default();
        assert!(s.is_valid() && s.ready(JpKind::Drop));
        s.cd[1] = 301.0;
        assert!(!s.is_valid());
        s.cd[1] = 299.0;
        assert!(s.is_valid() && !s.ready(JpKind::Strike));
    }
}
