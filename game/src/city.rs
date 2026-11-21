//! Placeholder Wanhua street grid: flat asphalt, sidewalks and box buildings.

use bevy::prelude::*;

/// Half-size of the playable square, in world units (metres).
pub const MAP_HALF: f32 = 66.0;
const BLOCK: f32 = 18.0;
const STREET: f32 = 7.0;

/// Axis-aligned building footprints used for collision and camera occlusion.
#[derive(Resource, Default)]
pub struct Obstacles(pub Vec<Aabb2>);

#[derive(Clone, Copy, Debug)]
pub struct Aabb2 {
    pub min: Vec2,
    pub max: Vec2,
    pub height: f32,
}

impl Aabb2 {
    pub fn contains(&self, p: Vec2, pad: f32) -> bool {
        p.x > self.min.x - pad
            && p.x < self.max.x + pad
            && p.y > self.min.y - pad
            && p.y < self.max.y + pad
    }
}

/// A building that can fade out when it hides the player.
#[derive(Component)]
pub struct Building {
    pub index: usize,
    pub opaque: Handle<StandardMaterial>,
    pub faded: Handle<StandardMaterial>,
}

pub struct CityPlugin;

impl Plugin for CityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Obstacles>()
            .add_systems(Startup, spawn_city);
    }
}

/// Small deterministic RNG so the layout is the same on every device.
pub struct Lcg(pub u64);
impl Lcg {
    pub fn next_f32(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / ((1u64 << 24) as f32)
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }
}

/// Block origins along one axis: blocks are `BLOCK` wide, separated by `STREET`.
pub fn block_starts() -> Vec<f32> {
    let pitch = BLOCK + STREET;
    let mut v = Vec::new();
    let mut x = -MAP_HALF + STREET;
    while x + BLOCK <= MAP_HALF - STREET + 0.01 {
        v.push(x);
        x += pitch;
    }
    v
}

fn spawn_city(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut obstacles: ResMut<Obstacles>,
) {
    // Asphalt
    commands.spawn((
        Mesh3d(
            meshes.add(
                Plane3d::default()
                    .mesh()
                    .size(MAP_HALF * 2.0, MAP_HALF * 2.0),
            ),
        ),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.27, 0.28, 0.30),
            perceptual_roughness: 0.95,
            ..default()
        })),
    ));

    let sidewalk = materials.add(StandardMaterial {
        base_color: Color::srgb(0.62, 0.58, 0.52),
        perceptual_roughness: 0.9,
        ..default()
    });
    let lane = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.80, 0.55),
        unlit: true,
        ..default()
    });
    let palette = [
        Color::srgb(0.78, 0.74, 0.66), // weathered concrete
        Color::srgb(0.66, 0.63, 0.60),
        Color::srgb(0.72, 0.60, 0.50), // tile facade
        Color::srgb(0.58, 0.62, 0.60),
        Color::srgb(0.80, 0.78, 0.74),
        Color::srgb(0.55, 0.50, 0.46),
    ];
    let signs = [
        Color::srgb(0.85, 0.15, 0.12),
        Color::srgb(0.95, 0.75, 0.10),
        Color::srgb(0.10, 0.55, 0.75),
        Color::srgb(0.20, 0.65, 0.30),
    ];

    let starts = block_starts();
    let mut rng = Lcg(0x5eed_1a2b);
    let slab = meshes.add(Cuboid::new(1.0, 1.0, 1.0));

    // Dashed centre lines on the streets
    let pitch = BLOCK + STREET;
    for (i, _) in starts.iter().enumerate().skip(1) {
        let c = starts[0] - STREET * 0.5 + pitch * i as f32;
        let mut t = -MAP_HALF + 2.0;
        while t < MAP_HALF - 2.0 {
            for (x, z, sx, sz) in [(c, t, 0.18, 1.6), (t, c, 1.6, 0.18)] {
                commands.spawn((
                    Mesh3d(slab.clone()),
                    MeshMaterial3d(lane.clone()),
                    Transform::from_xyz(x, 0.01, z).with_scale(Vec3::new(sx, 0.02, sz)),
                ));
            }
            t += 4.0;
        }
    }

    for &bx in &starts {
        for &bz in &starts {
            // Sidewalk slab under the block
            commands.spawn((
                Mesh3d(slab.clone()),
                MeshMaterial3d(sidewalk.clone()),
                Transform::from_xyz(bx + BLOCK * 0.5, 0.06, bz + BLOCK * 0.5)
                    .with_scale(Vec3::new(BLOCK, 0.12, BLOCK)),
            ));
            // 2×2 walk-ups per block with a narrow alley between them (Wanhua-style lanes)
            let inset = 1.2;
            let alley = 2.0;
            let cell = (BLOCK - inset * 2.0 - alley) * 0.5;
            for ix in 0..2 {
                for iz in 0..2 {
                    if rng.next_f32() < 0.1 {
                        continue; // empty lot / small square
                    }
                    let w = cell * rng.range(0.75, 1.0);
                    let d = cell * rng.range(0.75, 1.0);
                    let h = rng.range(4.0, 13.0);
                    let x0 = bx + inset + ix as f32 * (cell + alley);
                    let z0 = bz + inset + iz as f32 * (cell + alley);
                    let min = Vec2::new(x0, z0);
                    let max = Vec2::new(x0 + w, z0 + d);
                    let color =
                        palette[(rng.next_f32() * palette.len() as f32) as usize % palette.len()];
                    let opaque = materials.add(StandardMaterial {
                        base_color: color,
                        perceptual_roughness: 0.85,
                        ..default()
                    });
                    let faded = materials.add(StandardMaterial {
                        base_color: color.with_alpha(0.22),
                        alpha_mode: AlphaMode::Blend,
                        perceptual_roughness: 0.85,
                        ..default()
                    });
                    let index = obstacles.0.len();
                    obstacles.0.push(Aabb2 {
                        min,
                        max,
                        height: h,
                    });
                    let centre = (min + max) * 0.5;
                    commands
                        .spawn((
                            Building {
                                index,
                                opaque: opaque.clone(),
                                faded,
                            },
                            Mesh3d(slab.clone()),
                            MeshMaterial3d(opaque),
                            Transform::from_xyz(centre.x, h * 0.5, centre.y)
                                .with_scale(Vec3::new(w, h, d)),
                        ))
                        .with_children(|b| {
                            // Shop sign on the street-facing side, in unit-cube local space
                            let sc =
                                signs[(rng.next_f32() * signs.len() as f32) as usize % signs.len()];
                            let sign_y = (2.6 / h) - 0.5;
                            b.spawn((
                                Mesh3d(slab.clone()),
                                MeshMaterial3d(materials.add(StandardMaterial {
                                    base_color: sc,
                                    emissive: LinearRgba::from(sc) * 0.4,
                                    ..default()
                                })),
                                Transform::from_xyz(0.0, sign_y, 0.52).with_scale(Vec3::new(
                                    0.7,
                                    0.9 / h,
                                    0.04,
                                )),
                            ));
                        });
                }
            }
        }
    }

    // Sun. Shadows are off on Android: some GPUs crash with Bevy shadow maps
    // (bevyengine/bevy#8214), and it saves a lot of fill rate on phones.
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.91, 0.75),
            illuminance: 9_000.0,
            shadow_maps_enabled: !cfg!(target_os = "android"),
            ..default()
        },
        Transform::from_xyz(30.0, 60.0, 15.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
