//! Combat → audio bridge (`crate::audio::SfxEvent` from main, PR #2).
//!
//! Combat systems write their own messages (`Gunshot`, `PlayerHit`, `EnemyDied`,
//! `PlayerDied`, `Callout`); this one system turns them into `SfxEvent`s with the
//! distance from the player, as the original's `AU.*` calls did.

use bevy::prelude::*;

use super::tuning::GunFam;
use super::{Callout, EnemyDied, Gunshot, HitCause, PlayerDied, PlayerHit};
use crate::audio::{GunKind, SfxEvent};
use crate::player::Player;

/// audio.ts `gunKind` for the ported guns: pistols; dmg ≥ 85 → sniper; cd < 0.12 → mg
/// (QJY-88, and QCW-05 at 0.08 s); 12-gauge → shotgun; else rifle.
pub fn gun_kind(fam: GunFam) -> GunKind {
    match fam {
        GunFam::Pistol => GunKind::Pistol,
        GunFam::Sniper => GunKind::Sniper,
        GunFam::Lmg | GunFam::Smg => GunKind::Mg,
        GunFam::Shotgun => GunKind::Shotgun,
        GunFam::Rifle | GunFam::Bullpup | GunFam::Dmr => GunKind::Rifle,
    }
}

/// Original `callout` / `radio` only play within this distance of the player.
const RADIO_HEAR: f32 = 40.0;

pub(super) fn combat_sfx(
    player: Query<&Transform, With<Player>>,
    mut shots: MessageReader<Gunshot>,
    mut hits: MessageReader<PlayerHit>,
    mut kills: MessageReader<EnemyDied>,
    mut deaths: MessageReader<PlayerDied>,
    mut calls: MessageReader<Callout>,
    mut sfx: MessageWriter<SfxEvent>,
) {
    let pp = player.single().map_or(Vec3::ZERO, |t| t.translation);
    let dist = |p: Vec3| p.xz().distance(pp.xz());
    for s in shots.read() {
        play_gunshot_sfx(&mut sfx, s.fam, if s.by_player { 0.0 } else { dist(s.pos) });
    }
    for h in hits.read() {
        sfx.write(match h.cause {
            HitCause::RifleButt => SfxEvent::MeleeHit {
                dist: 0.0,
                blunt: true,
            },
            HitCause::Rifle | HitCause::Grenade => SfxEvent::Hurt { dist: 0.0 },
        });
    }
    for k in kills.read() {
        sfx.write(SfxEvent::Death { dist: dist(k.pos) });
    }
    for _ in deaths.read() {
        sfx.write(SfxEvent::Death { dist: 0.0 });
    }
    for c in calls.read() {
        let d = dist(c.pos);
        if d < RADIO_HEAR {
            sfx.write(SfxEvent::Radio { dist: d });
        }
    }
}

/// One gunshot (`AU.gunshot`). `indoor` is false until the map can answer it.
pub fn play_gunshot_sfx(sfx: &mut MessageWriter<SfxEvent>, fam: GunFam, dist: f32) {
    sfx.write(SfxEvent::Gunshot {
        kind: gun_kind(fam),
        dist,
        indoor: false,
    });
}
