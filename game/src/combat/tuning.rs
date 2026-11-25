//! Combat numbers, ported 1:1 from the original defendtaipei (TypeScript):
//! - guns: `src/systems/items.ts` (kind: 'gun') + `src/npc/firearms.ts` (GUN_FAM, RECOIL)
//! - squad AI: `src/npc/squadai.ts` (AI)
//! - soldier loop / player damage: `src/core/main.ts` (update → soldiers, playerAttack, damageSol, plaLook)
//!
//! Keep values identical to the original unless a comment says otherwise.

// ── Guns (items.ts) ─────────────────────────────────────────────────────

/// Model family (firearms.ts `GunFam`): sets recoil and the muzzle flash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GunFam {
    Pistol,
    Rifle,
    Bullpup,
    Dmr,
    Sniper,
    Lmg,
    Smg,
    Shotgun,
}

/// One firearm from items.ts. `dmg` is per hit (×0.8–1.2 roll), `range` in metres,
/// `cd` seconds between shots, `noise` the radius soldiers hear it in, `cap` magazine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gun {
    pub id: &'static str,
    pub name: &'static str,
    pub fam: GunFam,
    pub dmg: f32,
    pub range: f32,
    pub cd: f32,
    pub noise: f32,
    pub cap: u32,
}

macro_rules! gun {
    ($id:literal, $name:literal, $fam:ident, $dmg:expr, $range:expr, $cd:expr, $noise:expr, $cap:expr) => {
        Gun {
            id: $id,
            name: $name,
            fam: GunFam::$fam,
            dmg: $dmg,
            range: $range,
            cd: $cd,
            noise: $noise,
            cap: $cap,
        }
    };
}

/// Small arms ported so far (items.ts). Launchers / AT (`at: true`) are not ported yet.
pub const GUNS: &[Gun] = &[
    gun!("pistol", "手槍", Pistol, 48.0, 28.0, 0.4, 40.0, 15),
    gun!("rifle", "突擊步槍", Rifle, 62.0, 45.0, 0.18, 55.0, 30),
    gun!(
        "qbz191",
        "QBZ-191 自動步槍",
        Rifle,
        64.0,
        46.0,
        0.15,
        55.0,
        30
    ),
    gun!(
        "qbz95",
        "QBZ-95-1 自動步槍",
        Bullpup,
        60.0,
        44.0,
        0.17,
        55.0,
        30
    ),
    gun!(
        "qbu191",
        "QBU-191 精確射手步槍",
        Dmr,
        82.0,
        60.0,
        0.45,
        58.0,
        30
    ),
    gun!(
        "qbu88",
        "QBU-88 狙擊步槍",
        Sniper,
        90.0,
        64.0,
        0.7,
        60.0,
        10
    ),
    gun!("qsz92", "QSZ-92 手槍", Pistol, 46.0, 26.0, 0.38, 40.0, 15),
    gun!("qjy88", "QJY-88 通用機槍", Lmg, 66.0, 55.0, 0.09, 70.0, 200),
    gun!(
        "qcw05",
        "QCW-05 微聲衝鋒槍",
        Smg,
        34.0,
        24.0,
        0.08,
        12.0,
        50
    ),
    gun!("qbs09", "QBS-09 霰彈槍", Shotgun, 110.0, 14.0, 0.6, 65.0, 5),
];

pub fn gun(id: &str) -> Option<&'static Gun> {
    GUNS.iter().find(|g| g.id == id)
}

/// The player's gun in this slice (inventory / pickup is not ported yet).
pub const PLAYER_START_GUN: &str = "rifle";
/// Rounds the player starts with for it (provisional until the inventory port: the original
/// player starts with whatever the role's pack holds).
pub const PLAYER_START_ROUNDS: u32 = 120;

/// firearms.ts RECOIL: kick (rad), climb, back (m), body, jitter (rad), heat per shot, screen shake.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Recoil {
    pub kick: f32,
    pub climb: f32,
    pub back: f32,
    pub body: f32,
    pub jitter: f32,
    pub heat: f32,
    pub shake: f32,
}

pub fn recoil(f: GunFam) -> Recoil {
    let r = |kick, climb, back, body, jitter, heat, shake| Recoil {
        kick,
        climb,
        back,
        body,
        jitter,
        heat,
        shake,
    };
    match f {
        GunFam::Pistol => r(0.42, 0.12, 0.03, 0.8, 0.05, 0.3, 0.025),
        GunFam::Rifle => r(0.2, 0.14, 0.05, 1.0, 0.07, 0.22, 0.03),
        GunFam::Bullpup => r(0.2, 0.13, 0.05, 1.0, 0.07, 0.22, 0.03),
        GunFam::Smg => r(0.13, 0.12, 0.035, 0.75, 0.09, 0.16, 0.02),
        GunFam::Lmg => r(0.16, 0.15, 0.06, 1.35, 0.1, 0.18, 0.035),
        GunFam::Dmr => r(0.32, 0.15, 0.06, 1.15, 0.05, 0.35, 0.05),
        GunFam::Sniper => r(0.5, 0.1, 0.08, 1.5, 0.04, 0.6, 0.08),
        GunFam::Shotgun => r(0.55, 0.12, 0.08, 1.6, 0.06, 0.6, 0.08),
    }
}

// ── Player shot (playerAttack) ──────────────────────────────────────────
/// A target this close to the shot line is hit (m).
pub const SHOT_HIT_RADIUS: f32 = 0.55;
/// Shooter eye / target chest heights for the wall ray.
pub const SHOT_FROM_H: f32 = 1.35;
pub const SHOT_TO_H: f32 = 1.25;
pub const SHOT_TO_H_CROUCH: f32 = 0.75;
/// Mobile fire button: aim at the nearest soldier in sight within this (main.ts `nearestFoe(25)`).
pub const AUTO_AIM_RANGE: f32 = 25.0;
/// Tracer lifetime and height (main.ts `tracer`).
pub const TRACER_SECS: f32 = 0.08;
pub const TRACER_H: f32 = 1.2;

// ── Player health ───────────────────────────────────────────────────────
pub const PLAYER_MAX_HP: f32 = 100.0;
/// HP lost per second while bleeding.
pub const BLEED_PER_SEC: f32 = 0.6;

// ── Soldiers (main.ts spawnSoldiers / update) ───────────────────────────
pub const SOLDIER_HP: f32 = 100.0;
pub const SOLDIER_RADIUS: f32 = 0.35;
/// Further than this from the player a soldier is dormant.
pub const DORMANT_DIST: f32 = 120.0;
pub const VISION_DAY: f32 = 20.0;
pub const VISION_NIGHT: f32 = 11.0;
/// Half-angle of the vision cone (π/5 → a 72° wedge).
pub const VISION_HALF_ANGLE: f32 = std::f32::consts::PI / 5.0;
/// Anyone this close is noticed even outside the cone.
pub const VISION_NEAR: f32 = 1.6;
pub const SPEED_PATROL_DAY: f32 = 2.0;
pub const SPEED_PATROL_NIGHT: f32 = 1.6;
pub const SPEED_INVESTIGATE: f32 = 2.6;
pub const SPEED_TO_COVER: f32 = 3.2;
pub const SPEED_FLANK: f32 = 3.4;
pub const SPEED_RETREAT: f32 = 3.6;
/// Soldiers fight (aim and fire) within this distance of what they see.
pub const ENGAGE_DIST: f32 = 24.0;
/// …and close in on it while further than this.
pub const CLOSE_IN_DIST: f32 = 8.0;
/// Soldier's rifle: base hit chance falls with distance, never below the floor.
pub const SOL_HIT_BASE: f32 = 0.55;
pub const SOL_HIT_FALLOFF: f32 = 60.0;
pub const SOL_HIT_FLOOR: f32 = 0.08;
pub const SOL_DMG_MIN: f32 = 10.0;
pub const SOL_DMG_RAND: f32 = 12.0;
pub const SOL_BLEED_CHANCE: f32 = 0.45;
pub const SOL_FIRE_CD_MIN: f32 = 1.1;
pub const SOL_FIRE_CD_RAND: f32 = 0.6;
/// Rifle-butt melee (v22.10).
pub const MELEE_START_DIST: f32 = 1.6;
pub const MELEE_REACH: f32 = 1.9;
pub const MELEE_WINDUP: f32 = 0.62 * 0.44;
pub const MELEE_DMG_MIN: f32 = 12.0;
pub const MELEE_DMG_RAND: f32 = 6.0;
pub const MELEE_KNOCKBACK: f32 = 0.6;
/// Noise radii (main.ts `noise` calls).
pub const NOISE_SPOTTED: f32 = 25.0;
pub const NOISE_SOL_SHOT: f32 = 40.0;
pub const NOISE_GRENADE: f32 = 40.0;
/// Vision-cone draw radius / opacity (scale 20 by day, 11 at night).
pub const CONE_OPACITY_DAY: f32 = 0.12;
pub const CONE_OPACITY_NIGHT: f32 = 0.22;

// ── Squad tactics (squadai.ts AI) ───────────────────────────────────────
pub const SQUAD_R: f32 = 25.0;
pub const FLANK_DIST: f32 = 7.0;
pub const FLANK_T: f32 = 7.0;
pub const RETREAT_HP: f32 = 35.0;
pub const RETREAT_T: f32 = 5.0;
pub const RADIO_R: f32 = 45.0;
pub const RADIO_CD: f32 = 6.0;
pub const GREN_CAMP: f32 = 7.0;
pub const GREN_CD: f32 = 25.0;
pub const GREN_MIN: f32 = 5.0;
pub const GREN_MAX: f32 = 22.0;
pub const GREN_FLIGHT: f32 = 1.0;
pub const GREN_FUSE: f32 = 1.6;
pub const GREN_CORE: f32 = 2.5;
pub const GREN_WOUND: f32 = 6.0;
pub const GREN_CORE_DMG: f32 = 70.0;
pub const GREN_WOUND_DMG: f32 = 40.0;

// ── Mobile budget (not in the original; Bevy-side caps) ─────────────────
pub const MAX_SOLDIERS: usize = 40;
pub const MAX_TRACERS: usize = 32;
/// Bodies stay this long before they are cleared.
pub const BODY_SECS: f32 = 20.0;
