//! Evacuation sites, placed as in the original:
//! - helipad: `buildEvacZone` at the westernmost road point within ±250 m (z) of the start
//!   with no building at it or 6 m either side (main.ts, after `setupInformant`);
//! - boat: the informant's three rendezvous spots `RV` (淡水河岸 / 碼頭 / 萬華河堤缺口), each the
//!   westernmost free road point in a z band relative to the start, nudged by `freeNear`.
//!
//! Other systems read [`EvacSites`] for positions, flip `heli_open` to light the pad, and set
//! `boat_spot` / `boat_visible` once the contact has named a spot and it has been scouted.

use std::f32::consts::FRAC_PI_2;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::canvas::{Canvas, css};
use super::data::WorldData;
use crate::geom::{MeshBuf, at, capsule, cuboid, cylinder, hex, place, plane, sphere, torus, trs};

/// One rendezvous spot for the boat (`RV[i]`).
#[derive(Clone, Debug)]
pub struct BoatSpot {
    pub name_zh: &'static str,
    pub name_en: &'static str,
    pub pos: Vec2,
}

/// World positions (x, z on the ground plane) of the evacuation sites.
#[derive(Resource, Clone, Debug)]
pub struct EvacSites {
    /// Centre of the green H. The original evacuates when the player is within 5 m while open.
    pub heli: Vec2,
    /// The three boat rendezvous spots; the original picks one at random per run.
    pub boat_spots: [BoatSpot; 3],
    /// Which spot the boat waits at (default 0).
    pub boat_spot: usize,
    /// The original only shows the boat after the spot is scouted; set true to show it.
    pub boat_visible: bool,
    /// Evacuation window open: pulse the ring and light the red flare.
    pub heli_open: bool,
}

impl EvacSites {
    pub const HELI_RADIUS: f32 = 5.0;
    pub fn boat(&self) -> Vec2 {
        self.boat_spots[self.boat_spot.min(2)].pos
    }
    /// Where the inflatable boat sits (`r.p[0] - 3`).
    pub fn boat_hull(&self) -> Vec2 {
        self.boat() - Vec2::new(3.0, 0.0)
    }
}

/// Which site a marker entity belongs to.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvacMarker {
    Heli,
    Boat,
}

#[derive(Component)]
pub(super) struct EvacRing;
#[derive(Component)]
pub(super) struct EvacFlare;
#[derive(Component)]
pub(super) struct EvacFlag;

impl WorldData {
    fn free_near(&self, p: Vec2, r: f32) -> Vec2 {
        for k in 0..300 {
            let a = k as f32 * 2.4;
            let d = (k as f32 * 0.15) % r + 0.5;
            let q = p + Vec2::from_angle(a) * d;
            if self.bld_at(q).is_none()
                && self.bld_at(q + Vec2::X * 0.8).is_none()
                && self.bld_at(q + Vec2::Y * 0.8).is_none()
            {
                return q;
            }
        }
        p
    }

    fn inside(&self, q: Vec2) -> bool {
        q.x > self.min.x + 8.0 && q.x < self.max.x && q.y > self.min.y && q.y < self.max.y
    }

    pub fn plan_evac(&self) -> EvacSites {
        let z0 = self.spawn.y;
        let pts = || self.roads.iter().flat_map(|r| r.p.iter().copied());
        let heli = pts()
            .filter(|&q| {
                (q.y - z0).abs() < 250.0
                    && self.inside(q)
                    && self.bld_at(q).is_none()
                    && self.bld_at(q + Vec2::X * 6.0).is_none()
                    && self.bld_at(q - Vec2::X * 6.0).is_none()
            })
            .min_by(|a, b| a.x.total_cmp(&b.x))
            .unwrap_or(Vec2::new(self.min.x + 20.0, z0));
        let band = |zh, en, a: f32, b: f32| {
            let best = pts()
                .filter(|&q| {
                    q.y - z0 > a && q.y - z0 < b && self.inside(q) && self.bld_at(q).is_none()
                })
                .min_by(|p, q| p.x.total_cmp(&q.x));
            BoatSpot {
                name_zh: zh,
                name_en: en,
                pos: self.free_near(best.unwrap_or(Vec2::new(self.min.x + 20.0, z0)), 3.0),
            }
        };
        EvacSites {
            heli,
            boat_spots: [
                band("淡水河岸", "Tamsui River bank", -220.0, -70.0),
                band("碼頭（大稻埕式）", "Wharf (Dadaocheng style)", -70.0, 70.0),
                band("萬華河堤缺口", "Gap in the Wanhua levee", 70.0, 220.0),
            ],
            boat_spot: 0,
            boat_visible: true,
            heli_open: false,
        }
    }
}

/// The green H plate (label('H', '#1d6b34')): white frame and letter on green.
fn h_texture() -> Image {
    let mut c = Canvas::new(128, 128);
    c.fill_rect(0.0, 0.0, 128.0, 128.0, css("#1d6b34"));
    c.stroke_rect(6.0, 6.0, 116.0, 116.0, 8.0, css("#ffffff"));
    let w = css("#ffffff");
    c.fill_rect(38.0, 32.0, 12.0, 64.0, w);
    c.fill_rect(78.0, 32.0, 12.0, 64.0, w);
    c.fill_rect(38.0, 58.0, 52.0, 12.0, w);
    c.to_image(true, false)
}

/// EVAC sign: red plate with a white frame and bars standing in for the lettering.
fn sign_texture() -> Image {
    let mut c = Canvas::new(256, 64);
    c.fill_rect(0.0, 0.0, 256.0, 64.0, css("#b02020"));
    c.stroke_rect(3.0, 3.0, 250.0, 58.0, 4.0, css("#ffffff"));
    for i in 0..4 {
        c.fill_rect(40.0 + i as f32 * 48.0, 22.0, 32.0, 20.0, css("#ffffff"));
    }
    c.to_image(true, false)
}

pub(super) fn spawn_evac(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let sites = super::world().plan_evac();
    info!(
        "evac sites: heli {:?}, boat {:?}",
        sites.heli,
        sites.boat_spots.iter().map(|s| s.pos).collect::<Vec<_>>()
    );
    let lit = mats.add(StandardMaterial {
        perceptual_roughness: 0.8,
        ..default()
    });
    let tex = |img: Image, images: &mut Assets<Image>, mats: &mut Assets<StandardMaterial>| {
        mats.add(StandardMaterial {
            base_color_texture: Some(images.add(img)),
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        })
    };
    let h_mat = tex(h_texture(), &mut images, &mut mats);
    let sign_mat = tex(sign_texture(), &mut images, &mut mats);

    // Helipad: H plate, pulsing ring, flag pole, sign post, red flare.
    let mut props = MeshBuf::default();
    props.add(&cuboid(0.12, 5.0, 0.12), at(6.5, 2.5, 0.0), hex(0xcccccc));
    props.add(&cuboid(0.1, 2.2, 0.1), at(-6.5, 1.1, 0.05), hex(0x666666));
    let mut flag = MeshBuf::default();
    flag.add(&cuboid(1.4, 0.8, 0.04), at(0.7, 0.0, 0.0), hex(0xff7a00));
    let ring = torus(5.6, 0.4, 2, 40, std::f32::consts::TAU);
    let mut ring_flat = MeshBuf::default();
    ring_flat.add_white(
        &ring,
        trs([0.0, 0.06, 0.0], [FRAC_PI_2, 0.0, 0.0], [1.0, 1.0, 0.05]),
    );
    let h = sites.heli;
    commands
        .spawn((
            EvacMarker::Heli,
            Transform::from_xyz(h.x, 0.0, h.y),
            Visibility::Visible,
        ))
        .with_children(|c| {
            c.spawn((
                Mesh3d(meshes.add(plane(4.0, 4.0).into_mesh())),
                MeshMaterial3d(h_mat),
                Transform::from_xyz(0.0, 0.05, 0.0)
                    .with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
                NotShadowCaster,
            ));
            c.spawn((
                Mesh3d(meshes.add(props.into_mesh())),
                MeshMaterial3d(lit.clone()),
                Transform::default(),
            ));
            c.spawn((
                EvacFlag,
                Mesh3d(meshes.add(flag.into_mesh())),
                MeshMaterial3d(mats.add(StandardMaterial {
                    emissive: LinearRgba::rgb(0.3, 0.14, 0.0),
                    ..default()
                })),
                Transform::from_xyz(6.5, 4.5, 0.0),
            ));
            c.spawn((
                Mesh3d(meshes.add(plane(3.4, 0.85).into_mesh())),
                MeshMaterial3d(sign_mat),
                Transform::from_xyz(-6.5, 2.2, 0.0),
            ));
            c.spawn((
                EvacRing,
                Mesh3d(meshes.add(ring_flat.into_mesh())),
                MeshMaterial3d(mats.add(StandardMaterial {
                    base_color: Color::srgba(0.25, 1.0, 0.44, 0.7),
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                })),
                Transform::default(),
                Visibility::Hidden,
                NotShadowCaster,
            ));
            c.spawn((
                EvacFlare,
                PointLight {
                    color: Color::srgb(1.0, 0.19, 0.13),
                    intensity: 0.0,
                    range: 30.0,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Mesh3d(meshes.add(sphere(0.25, 8, 6).into_mesh())),
                MeshMaterial3d(mats.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.25, 0.13),
                    unlit: true,
                    ..default()
                })),
                Transform::from_xyz(0.0, 1.2, 3.0),
                Visibility::Hidden,
            ));
        });

    // Boat: the dark inflatable lying across the bank (CapsuleGeometry(0.6, 2.4) rotZ π/2),
    // plus a short plank jetty so the spot reads as a dock.
    let mut boat = MeshBuf::default();
    boat.add(
        &capsule(0.6, 2.4, 3, 8),
        trs([0.0, 0.4, 0.0], [0.0, 0.0, FRAC_PI_2], [1.0; 3]),
        hex(0x2a2a2a),
    );
    boat.add(&cuboid(0.9, 0.3, 0.9), at(1.2, 0.9, 0.0), hex(0x3a3a3a));
    for i in 0..5 {
        boat.add(
            &cuboid(0.35, 0.08, 2.4),
            place(-2.2 - i as f32 * 0.42, 0.25, 0.0, 0.0),
            hex(0x6b4a2e),
        );
    }
    for (x, z) in [(-2.0, 1.1), (-2.0, -1.1), (-4.0, 1.1), (-4.0, -1.1)] {
        boat.add(&cylinder(0.08, 0.08, 1.0, 6), at(x, 0.0, z), hex(0x4a3420));
    }
    let b = sites.boat_hull();
    commands.spawn((
        EvacMarker::Boat,
        Mesh3d(meshes.add(boat.into_mesh())),
        MeshMaterial3d(lit),
        Transform::from_xyz(b.x, 0.0, b.y),
        if sites.boat_visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        },
    ));
    commands.insert_resource(sites);
}

/// buildEvacZone.update: ring and flare only while open; ring pulses, flare flickers, flag sways.
#[allow(clippy::type_complexity)]
pub(super) fn update_evac(
    time: Res<Time>,
    sites: Option<Res<EvacSites>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut ring: Query<
        (&mut Visibility, &MeshMaterial3d<StandardMaterial>),
        (With<EvacRing>, Without<EvacFlare>),
    >,
    mut flare: Query<(&mut Visibility, &mut PointLight), (With<EvacFlare>, Without<EvacRing>)>,
    mut flag: Query<&mut Transform, With<EvacFlag>>,
    mut boat: Query<
        (&EvacMarker, &mut Transform, &mut Visibility),
        (Without<EvacFlag>, Without<EvacRing>, Without<EvacFlare>),
    >,
) {
    let Some(sites) = sites else { return };
    let t = time.elapsed_secs();
    let open = sites.heli_open;
    let vis = if open {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for (mut v, m) in &mut ring {
        v.set_if_neq(vis);
        if open && let Some(mut m) = mats.get_mut(&m.0) {
            m.base_color.set_alpha(0.45 + 0.3 * (t * 3.0).sin());
        }
    }
    for (mut v, mut l) in &mut flare {
        v.set_if_neq(vis);
        // three: 40 + sin(t·23)·15 + rand·10 candela-ish; Bevy wants lumens.
        let k = (t * 23.0).sin() * 15.0 + (t * 71.3).sin().abs() * 10.0;
        l.intensity = if open { (40.0 + k) * 4_000.0 } else { 0.0 };
    }
    for mut f in &mut flag {
        f.rotation = Quat::from_rotation_y((t * 2.0).sin() * 0.2);
    }
    if sites.is_changed() {
        let b = sites.boat_hull();
        for (m, mut tf, mut v) in &mut boat {
            if *m == EvacMarker::Boat {
                tf.translation = Vec3::new(b.x, 0.0, b.y);
                v.set_if_neq(if sites.boat_visible {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn evac_sites_are_west_and_clear() {
        let w = super::super::world();
        let s = w.plan_evac();
        println!("heli {:?}", s.heli);
        for b in &s.boat_spots {
            println!("boat {} {:?}", b.name_en, b.pos);
        }
        assert!(w.bld_at(s.heli).is_none());
        assert!(s.heli.x < w.spawn.x);
        for b in &s.boat_spots {
            assert!(w.bld_at(b.pos).is_none());
        }
    }
}
