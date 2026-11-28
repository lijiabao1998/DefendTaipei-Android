//! Desktop-only end-to-end visual QA through the real Bevy renderer.
//!
//! `--qa` uses an offscreen texture, with no OS window or display-server socket.
//! All game plugins still run. The virtual primary window provides the same
//! viewport inputs as a real window. Screenshots come from GPU readback, and a
//! save/menu/continue cycle must preserve the live run before QA can succeed.

use std::any::TypeId;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::{
    app::AppExit,
    camera::RenderTarget,
    prelude::*,
    render::{
        render_resource::{TextureFormat, TextureUsages},
        renderer::RenderAdapterInfo,
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    time::TimeUpdateStrategy,
    window::PrimaryWindow,
};
use serde::Serialize;

mod wave5;
mod wave5_exterior;

use crate::camera::{CameraZoom, GameCamera};
use crate::game_flow::{Campaign, GameFlow, SkipPrologue};
use crate::i18n::UiFont;
use crate::inventory::Inventory;
use crate::player::{PLAYER_RADIUS, Player};
use crate::roles::SelectedRole;
use crate::save::{SaveAndMenu, SaveStatus};
use crate::survival::{Shelter, Stash, TRADE_SPOTS};
use crate::ui::{OpenPanel, PanelKind, TitleAction, TitleMenu, TitlePage};

const SETTLE_FRAMES: u32 = 60;
const MAX_STAGE_FRAMES: u32 = 1800;

#[derive(Clone, Copy)]
enum Action {
    None,
    SettingsAudio,
    SettingsGame,
    SettingsClose,
    SettingsPause,
    SettingsPerformance,
    SettingsPerformanceLandscape,
    SettingsDiagnostics,
    PerformanceStreet,
    Exterior(ExteriorScene),
    Commercial(CommercialScene),
    Streets(StreetScene),
    Lighting(LightingScene),
    Wave5(wave5::Scene),
    StreetDetail(wave5_exterior::Scene),
    ChooseSurvivor,
    ReviewDeparture,
    Depart,
    Skip,
    Street,
    Entrance,
    DoorOpen,
    DoorClose,
    Barricade,
    BarricadeBlocked,
    Unbarricade,
    BarricadeInventory,
    ShopInterior,
    Scavenge,
    ScavengeOne,
    ScavengeAll,
    ShopDepleted,
    HomeInterior,
    Map,
    Rain,
    Night,
    Blackout,
    Journal,
    Pause,
    Warnings,
    Shelter,
    Inventory,
    Trade,
    SaveMenu,
    Continue,
    /// v24.3 a ROC remnant position in the street.
    Remnant,
    /// The squad leader's conversation card.
    RemnantTalk,
    /// v24.14 an area taken back by actual kills.
    Reclaim,
    /// v24.4 a C-2 airdrop called on the JSDF radio, after it lands.
    Airdrop,
    /// v23.4 the second floor of the nearest walk-up, reached by its stairs.
    Upstairs,
    /// v24.8 the day's call for help answered on its floor.
    Rescue,
    /// v24.20 / v24.21 a thrown grenade, a mortar barrage and an incoming rocket.
    War,
    /// v22.5 a rooftop sniper locking on in the open.
    Sniper,
    /// v24.24 PLA Z-10 patrol helicopter over Wanhua.
    Heli,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExteriorScene {
    Townhouse,
    Walkup,
    Shophouse,
    ServiceStair,
    Rooftop,
    Cutaway,
}

/// Commercial scenes share the normal fixed isometric camera. The final two
/// deliberately repeat the exact Food lot, position and zoom with power on/off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommercialScene {
    Arcade,
    Food,
    Essentials,
    Night,
    Blackout,
}

/// These use existing immutable road segments and parked props; no display-only
/// models or traffic objects are spawned for the captures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreetScene {
    Junction,
    Scooters,
    Taxi,
    Truck,
}

/// Same real arcade/street camera across normal lighting states and one explicitly
/// diagnostic direct-light-off control. The control is not a gameplay blackout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LightingScene {
    Day,
    Dusk,
    Night,
    NoSpillControl,
    Blackout,
    Restored,
}

impl LightingScene {
    fn minute(self) -> f32 {
        match self {
            Self::Day => 720.0,
            Self::Dusk => 1110.0,
            _ => 1320.0,
        }
    }
}

#[derive(Clone, Copy)]
struct LightingFixture {
    building: u32,
    position: Vec2,
    zoom: f32,
    scene: LightingScene,
}

#[derive(Clone, Copy, Debug)]
enum StreetSubject {
    Approach {
        road: usize,
        segment: usize,
        end: u8,
    },
    Prop(usize),
}

#[derive(Clone, Copy, Debug)]
struct StreetFixture {
    scene: StreetScene,
    subject: StreetSubject,
    position: Vec2,
    zoom: f32,
}

#[derive(Clone, Copy)]
struct CommercialFixture {
    building: u32,
    position: Vec2,
    zoom: f32,
    scene: CommercialScene,
}

#[derive(Clone, Copy)]
enum ExpectedFlow {
    Title,
    Survivor,
    Briefing,
    Prologue,
    Playing,
}

struct Stage {
    name: &'static str,
    size: [u32; 2],
    action: Action,
    flow: ExpectedFlow,
}

const STAGES: &[Stage] = &[
    Stage {
        name: "01-title-landscape",
        size: [1280, 720],
        action: Action::None,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02-title-portrait",
        size: [720, 1280],
        action: Action::None,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02-settings-audio-portrait",
        size: [390, 844],
        action: Action::SettingsAudio,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02-settings-audio-small-landscape",
        size: [800, 360],
        action: Action::None,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02-settings-game-portrait",
        size: [390, 844],
        action: Action::SettingsGame,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02-settings-performance-portrait",
        size: [390, 844],
        action: Action::SettingsPerformance,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02-settings-performance-small-landscape",
        size: [800, 360],
        action: Action::SettingsPerformanceLandscape,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02-settings-diagnostics-portrait",
        size: [390, 844],
        action: Action::SettingsDiagnostics,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02a-title-small-portrait",
        size: [320, 640],
        action: Action::SettingsClose,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "02b-survivor-portrait",
        size: [390, 844],
        action: Action::ChooseSurvivor,
        flow: ExpectedFlow::Survivor,
    },
    Stage {
        name: "02c-survivor-small-landscape",
        size: [800, 360],
        action: Action::None,
        flow: ExpectedFlow::Survivor,
    },
    Stage {
        name: "02d-briefing-small-landscape",
        size: [800, 360],
        action: Action::ReviewDeparture,
        flow: ExpectedFlow::Briefing,
    },
    Stage {
        name: "02e-briefing-portrait",
        size: [390, 844],
        action: Action::None,
        flow: ExpectedFlow::Briefing,
    },
    Stage {
        name: "03-prologue",
        size: [1280, 720],
        action: Action::Depart,
        flow: ExpectedFlow::Prologue,
    },
    Stage {
        name: "04-playing",
        size: [1280, 720],
        action: Action::Skip,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04a-streetscape-portrait",
        size: [390, 844],
        action: Action::Street,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04a1-streetscape-scale70-portrait",
        size: [390, 844],
        action: Action::PerformanceStreet,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04b-entrance-landscape",
        size: [1280, 720],
        action: Action::Entrance,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04c-door-open-landscape",
        size: [1280, 720],
        action: Action::DoorOpen,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d-door-closed-landscape",
        size: [1280, 720],
        action: Action::DoorClose,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d1-barricaded-portrait",
        size: [390, 844],
        action: Action::Barricade,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d2-barricade-blocked-portrait",
        size: [390, 844],
        action: Action::BarricadeBlocked,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d3-barricade-removed-landscape",
        size: [1280, 720],
        action: Action::Unbarricade,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d4-barricade-inventory-portrait",
        size: [320, 640],
        action: Action::BarricadeInventory,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d5-shop-interior-landscape",
        size: [1280, 720],
        action: Action::ShopInterior,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d6-scavenge-portrait",
        size: [390, 844],
        action: Action::Scavenge,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d7-scavenge-partial-portrait",
        size: [320, 640],
        action: Action::ScavengeOne,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d8-scavenge-small-landscape",
        size: [800, 360],
        action: Action::None,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d9-scavenge-empty-portrait",
        size: [390, 844],
        action: Action::ScavengeAll,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d10-shop-depleted-landscape",
        size: [1280, 720],
        action: Action::ShopDepleted,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d11-home-interior-portrait",
        size: [390, 844],
        action: Action::HomeInterior,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d12-exterior-townhouse-landscape",
        size: [1280, 720],
        action: Action::Exterior(ExteriorScene::Townhouse),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d13-exterior-walkup-landscape",
        size: [1280, 720],
        action: Action::Exterior(ExteriorScene::Walkup),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d14-exterior-shophouse-landscape",
        size: [1280, 720],
        action: Action::Exterior(ExteriorScene::Shophouse),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d15-exterior-service-stair-landscape",
        size: [1280, 720],
        action: Action::Exterior(ExteriorScene::ServiceStair),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d16-exterior-rooftop-landscape",
        size: [1280, 720],
        action: Action::Exterior(ExteriorScene::Rooftop),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d17-exterior-cutaway-portrait",
        size: [390, 844],
        action: Action::Exterior(ExteriorScene::Cutaway),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d18-commercial-arcade-landscape",
        size: [1280, 720],
        action: Action::Commercial(CommercialScene::Arcade),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d19-commercial-food-landscape",
        size: [1280, 720],
        action: Action::Commercial(CommercialScene::Food),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d20-commercial-essentials-landscape",
        size: [1280, 720],
        action: Action::Commercial(CommercialScene::Essentials),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d21-commercial-night-landscape",
        size: [1280, 720],
        action: Action::Commercial(CommercialScene::Night),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d22-commercial-blackout-landscape",
        size: [1280, 720],
        action: Action::Commercial(CommercialScene::Blackout),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d23-streets-junction-landscape",
        size: [1280, 720],
        action: Action::Streets(StreetScene::Junction),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d24-streets-scooters-portrait",
        size: [720, 1280],
        action: Action::Streets(StreetScene::Scooters),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d25-streets-taxi-landscape",
        size: [1280, 720],
        action: Action::Streets(StreetScene::Taxi),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d26-streets-truck-landscape",
        size: [1280, 720],
        action: Action::Streets(StreetScene::Truck),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d27-lighting-day-landscape",
        size: [1280, 720],
        action: Action::Lighting(LightingScene::Day),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d28-lighting-dusk-landscape",
        size: [1280, 720],
        action: Action::Lighting(LightingScene::Dusk),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d29-lighting-night-landscape",
        size: [1280, 720],
        action: Action::Lighting(LightingScene::Night),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d30-lighting-no-spill-control-landscape",
        size: [1280, 720],
        action: Action::Lighting(LightingScene::NoSpillControl),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d31-lighting-blackout-landscape",
        size: [1280, 720],
        action: Action::Lighting(LightingScene::Blackout),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04d32-lighting-restored-landscape",
        size: [1280, 720],
        action: Action::Lighting(LightingScene::Restored),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04e-survivor-map-portrait",
        size: [390, 844],
        action: Action::Map,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04f-survivor-map-small-landscape",
        size: [800, 360],
        action: Action::None,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04g-rain-portrait",
        size: [390, 844],
        action: Action::Rain,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04h-night-portrait",
        size: [390, 844],
        action: Action::Night,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04i-blackout-portrait",
        size: [390, 844],
        action: Action::Blackout,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "04j-survival-warnings-portrait",
        size: [320, 640],
        action: Action::Warnings,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "05-shelter-landscape",
        size: [1280, 720],
        action: Action::Shelter,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "06-shelter-small-landscape",
        size: [800, 360],
        action: Action::None,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "07-shelter-portrait",
        size: [720, 1280],
        action: Action::None,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "08-inventory-portrait",
        size: [720, 1280],
        action: Action::Inventory,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "08a-radio-journal-portrait",
        size: [390, 844],
        action: Action::Journal,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "08b-pause-small-landscape",
        size: [800, 360],
        action: Action::Pause,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "08c-settings-paused-small-landscape",
        size: [800, 360],
        action: Action::SettingsPause,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "09-barter-portrait",
        size: [720, 1280],
        action: Action::Trade,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "09w01-utility-signals-landscape",
        size: [1280, 720],
        action: Action::StreetDetail(wave5_exterior::Scene::Utilities),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "09w02-market-cart-landscape",
        size: [1280, 720],
        action: Action::StreetDetail(wave5_exterior::Scene::Market),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "09w03-temple-entrance-landscape",
        size: [1280, 720],
        action: Action::StreetDetail(wave5_exterior::Scene::Temple),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "09w04-controls-portrait",
        size: [390, 844],
        action: Action::Wave5(wave5::Scene::Controls),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "09w05-vehicle-inspect-landscape",
        size: [1280, 720],
        action: Action::Wave5(wave5::Scene::Inspect),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "09w06-vehicle-board-portrait",
        size: [390, 844],
        action: Action::Wave5(wave5::Scene::Board),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "09w07-vehicle-drive-landscape",
        size: [1280, 720],
        action: Action::Wave5(wave5::Scene::Drive),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "09w08-vehicle-service-portrait",
        size: [390, 844],
        action: Action::Wave5(wave5::Scene::Service),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "09w09-scooter-rider-portrait",
        size: [390, 844],
        action: Action::Wave5(wave5::Scene::Rider),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "09w10-armored-board-landscape",
        size: [1280, 720],
        action: Action::Wave5(wave5::Scene::Armor),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "09w11-armored-capture-landscape",
        size: [1280, 720],
        action: Action::Wave5(wave5::Scene::Capture),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "09w12-mounted-controls-portrait",
        size: [390, 844],
        action: Action::Wave5(wave5::Scene::Mounted),
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "10-title-with-save",
        size: [1280, 720],
        action: Action::SaveMenu,
        flow: ExpectedFlow::Title,
    },
    Stage {
        name: "11-continued",
        size: [1280, 720],
        action: Action::Continue,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "12w01-remnant-position-landscape",
        size: [1280, 720],
        action: Action::Remnant,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "12w02-remnant-talk-portrait",
        size: [390, 844],
        action: Action::RemnantTalk,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "12w03-reclaim-title-landscape",
        size: [1280, 720],
        action: Action::Reclaim,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "12w04-jsdf-airdrop-landscape",
        size: [1280, 720],
        action: Action::Airdrop,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "12w05-upstairs-portrait",
        size: [390, 844],
        action: Action::Upstairs,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "12w06-rescue-portrait",
        size: [390, 844],
        action: Action::Rescue,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "12w07-war-landscape",
        size: [1280, 720],
        action: Action::War,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "12w08-sniper-landscape",
        size: [1280, 720],
        action: Action::Sniper,
        flow: ExpectedFlow::Playing,
    },
    Stage {
        name: "12w09-heli-patrol-landscape",
        size: [1280, 720],
        action: Action::Heli,
        flow: ExpectedFlow::Playing,
    },
];

#[derive(Serialize)]
struct Capture {
    stage: String,
    file: String,
    width: u32,
    height: u32,
    sampled_colors: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    exterior: Option<ExteriorEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    commercial: Option<CommercialEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    streets: Option<StreetEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lighting: Option<LightingEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    map_workload: Option<MapWorkload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wave5: Option<serde_json::Value>,
}

/// Counts of map geometry retained at upload, with the real game camera's
/// visibility list. Excludes characters, dynamic doors, interiors and UI.
#[derive(Clone, Serialize)]
struct MapWorkload {
    measurement: &'static str,
    resident_meshes: usize,
    resident_vertices: usize,
    main_view_meshes: usize,
    main_view_vertices: usize,
}

/// Mutually exclusive uploaded groups. A persistent shell contains both the
/// coarse facade and roof, merged by material; it is never counted as roof fittings.
#[derive(Clone, Copy, Default, Serialize)]
struct ExteriorMeshGroup {
    meshes: usize,
    vertices: usize,
    in_main_view: usize,
}

/// The selected lot and actual renderer state travel with its GPU readback.
/// Geometry counts describe workload, never measured device performance.
#[derive(Clone, Serialize)]
struct ExteriorEvidence {
    building_id: u32,
    typology: String,
    scene: String,
    player: [f32; 3],
    camera: [f32; 3],
    zoom: f32,
    projected_bounds: [[f32; 2]; 2],
    balconies: usize,
    service_stair: bool,
    rooftop_shacks: usize,
    roof_access: bool,
    persistent_shell: ExteriorMeshGroup,
    facade_detail: ExteriorMeshGroup,
    roof_fittings: ExteriorMeshGroup,
    total_meshes: usize,
    total_vertices: usize,
    cutaway_hidden: bool,
    streaming: crate::map::MapRenderStats,
}

/// Every feature count is render-derived from the selected existing lot. The
/// uploaded groups include its other shared-material base/shell geometry, so
/// their vertex totals are explicitly not attributed solely to the storefront.
#[derive(Clone, Serialize)]
struct CommercialEvidence {
    visibility_measurement: &'static str,
    building_id: u32,
    family: String,
    sign_index: usize,
    scene: String,
    arcade: bool,
    frontage_width: f32,
    building_height: f32,
    storefront_panels: usize,
    secondary_fascia: bool,
    canopy: bool,
    blade: bool,
    player: [f32; 3],
    camera: [f32; 3],
    zoom: f32,
    projected_bounds: [[f32; 2]; 2],
    commercial_vertices: usize,
    base_batches: ExteriorMeshGroup,
    persistent_shell_batches: ExteriorMeshGroup,
    night: f32,
    power: bool,
    active_lights: usize,
    sign_emissive: [f32; 3],
    streaming: crate::map::MapRenderStats,
}

#[derive(Clone, Serialize)]
struct StreetPixelEvidence {
    point: [u32; 2],
    asphalt_point: [u32; 2],
    paint_rgb: [u8; 3],
    asphalt_rgb: [u8; 3],
    paint_luma: u8,
    asphalt_luma: u8,
    matched: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct StreetPixelProbe {
    paint: [u32; 2],
    asphalt: [u32; 2],
}

#[derive(Clone, Serialize)]
struct StreetBatchEvidence {
    chunk: [i32; 2],
    uploaded: ExteriorMeshGroup,
}

/// Feature counts describe this real source segment/prop, while batch counts
/// describe the complete shared-material street upload in its owner chunks.
#[derive(Clone, Serialize)]
struct StreetEvidence {
    visibility_measurement: &'static str,
    scene: String,
    road_segment: Option<[usize; 2]>,
    junction_end: Option<u8>,
    junction_clearance: Option<f32>,
    approach_arrow: bool,
    prop_indices: Vec<usize>,
    prop_kinds: Vec<String>,
    parking_bays: usize,
    feature_vertices: usize,
    player: [f32; 3],
    camera: [f32; 3],
    zoom: f32,
    projected_bounds: [[f32; 2]; 2],
    batches: Vec<StreetBatchEvidence>,
    marking_pixel_samples: usize,
    marking_pixel_hits: usize,
    marking_pixels: Vec<StreetPixelEvidence>,
    streaming: crate::map::MapRenderStats,
}

#[derive(Clone, Serialize)]
struct LightingSlotEvidence {
    slot: usize,
    source_id: Option<u64>,
    kind: String,
    source: String,
    source_position: [f32; 3],
    source_direction: [f32; 3],
    receiver: [f32; 3],
    source_geometry_bounds: [[f32; 3]; 2],
    source_geometry_verified: bool,
    actual_position: [f32; 3],
    actual_direction: [f32; 3],
    intensity: f32,
    range: f32,
    radius: f32,
    inner_angle: f32,
    outer_angle: f32,
    color: [f32; 3],
    gain: f32,
    shadow_maps: bool,
    contact_shadows: bool,
    inherited_visible: bool,
    view_visible: bool,
    in_main_view_cluster: bool,
}

#[derive(Clone, Serialize)]
struct LightingProbeRejection {
    reason: &'static str,
    building_id: Option<u32>,
    world: [f32; 3],
    point: Option<[u32; 2]>,
}

#[derive(Clone, Serialize)]
struct LightingProbeScreening {
    source_id: u64,
    kind: String,
    attempted: usize,
    accepted_before_cap: usize,
    rejected: std::collections::BTreeMap<&'static str, usize>,
    examples: Vec<LightingProbeRejection>,
}

#[derive(Clone, Serialize, PartialEq)]
struct LightingPixelProbe {
    role: String,
    source_id: Option<u64>,
    /// Geometric coverage, including overlapping cones. The diagnostic tests
    /// the whole city pool, not an individual emitter's isolated contribution.
    geometric_cone_sources: Vec<u64>,
    world: [f32; 3],
    point: [u32; 2],
}

#[derive(Clone, Serialize)]
struct LightingPixelEvidence {
    probe: LightingPixelProbe,
    rgb: [u8; 3],
    luma: u8,
}

#[derive(Clone, Serialize)]
struct LightingPixelComparison {
    role: String,
    source_id: Option<u64>,
    point: [u32; 2],
    reference_rgb: [u8; 3],
    current_rgb: [u8; 3],
    reference_luma: u8,
    current_luma: u8,
    difference: i16,
    matched: bool,
}

#[derive(Clone, Serialize, PartialEq)]
struct LightingMaterialEvidence {
    role: &'static str,
    base_color: [f32; 4],
    emissive: [f32; 3],
    roughness: f32,
    metallic: f32,
}

#[derive(Clone, Serialize)]
struct LightingEvidence {
    measurement: &'static str,
    scene: String,
    diagnostic_direct_light_override: bool,
    building_id: u32,
    player: [f32; 3],
    camera: [f32; 3],
    zoom: f32,
    minute: f32,
    night: f32,
    power: bool,
    wetness: f32,
    ambient: f32,
    ambient_color: [f32; 3],
    sun_illuminance: Vec<f32>,
    sun_color: Vec<[f32; 3]>,
    material_state: Vec<LightingMaterialEvidence>,
    material_emissive: Vec<(&'static str, [f32; 3])>,
    actual_pool_entities: usize,
    actual_active_lights: usize,
    city_point_lights: usize,
    unrelated_point_lights: usize,
    total_spot_lights: usize,
    total_point_lights: usize,
    stable_observation_frames: usize,
    stable_source_slots: bool,
    slots: Vec<LightingSlotEvidence>,
    source_catalog_counts: [usize; 3],
    probes: Vec<LightingPixelEvidence>,
    probe_screening: Vec<LightingProbeScreening>,
    comparison_reference: Option<&'static str>,
    comparisons: Vec<LightingPixelComparison>,
    ground_response_hits: usize,
    ground_response_samples: usize,
    unchanged_control_hits: usize,
    unchanged_control_samples: usize,
    window_response_hits: usize,
    sign_response_hits: usize,
    restoration_hits: usize,
    restoration_samples: usize,
    streaming: crate::map::MapRenderStats,
}

#[derive(Clone, Copy)]
struct ExteriorFixture {
    building: u32,
    position: Vec2,
    zoom: f32,
    scene: ExteriorScene,
}

#[derive(Serialize)]
struct QaReport {
    success: bool,
    scope: &'static str,
    expected_captures: usize,
    renderer: String,
    captures: Vec<Capture>,
    assertions: Vec<String>,
    error: Option<String>,
}

struct Checkpoint {
    day: u32,
    stash: Stash,
    inventory: Inventory,
    role: SelectedRole,
    position: Vec3,
    game_min: f32,
    map: crate::worldmap::MapProgress,
    closed_doors: Vec<u32>,
    barricaded_doors: Vec<u32>,
    loot: Vec<(String, Stash)>,
    fixtures_version: u8,
    vehicles: crate::vehicles::VehicleFleet,
    armor: crate::combat::armor::ArmorFleet,
}

#[derive(Resource)]
struct QaRun {
    output: PathBuf,
    stages: Vec<usize>,
    target: Option<Handle<Image>>,
    stage: usize,
    entered: bool,
    frame: u32,
    ready_frames: u32,
    wait_logged: Instant,
    scene_chunk: Option<crate::map::ChunkKey>,
    entrance_building: Option<u32>,
    interior_building: Option<u32>,
    exterior: Option<ExteriorFixture>,
    commercial: Option<CommercialFixture>,
    streets: Option<StreetFixture>,
    lighting: Option<LightingFixture>,
    lighting_reference_probes: Vec<LightingPixelProbe>,
    lighting_observations: Vec<Vec<Option<u64>>>,
    door_test_id: Option<u32>,
    barricade_planks: Option<u32>,
    loot_fixture: Option<String>,
    loot_expected: Option<Stash>,
    loot_inventory: Option<Inventory>,
    /// The area the Reclaim stage takes back, and the bag before its gift.
    reclaim: Option<(usize, Inventory)>,
    /// Grenades carried before the War stage's throw.
    war_grenades: Option<u32>,
    waiting_capture: bool,
    captured: bool,
    checkpoint: Option<Checkpoint>,
    continuation_checked: bool,
    transport_continuation_checked: bool,
    transport_restore: Option<serde_json::Value>,
    started: Instant,
    report: QaReport,
    finished: bool,
}

#[derive(Component)]
struct CaptureTicket {
    name: String,
    path: PathBuf,
    size: [u32; 2],
    settings_pixels: Option<SettingsPixels>,
    exterior: Option<ExteriorEvidence>,
    commercial: Option<CommercialEvidence>,
    streets: Option<StreetEvidence>,
    street_pixels: Vec<StreetPixelProbe>,
    lighting: Option<LightingEvidence>,
    lighting_pixels: Vec<LightingPixelProbe>,
    map_workload: Option<MapWorkload>,
    wave5: Option<serde_json::Value>,
}

struct SettingsPixels {
    panel: Vec<[u32; 2]>,
    selected_border: Vec<[u32; 2]>,
}

pub struct QaPlugin;

impl Plugin for QaPlugin {
    fn build(&self, app: &mut App) {
        let output = std::env::var_os("DEFEND_TAIPEI_QA_OUT")
            .map_or_else(|| PathBuf::from("qa-artifacts"), PathBuf::from);
        // Development subsets cannot satisfy CI's full capture/save gate.
        let exterior_only =
            std::env::var_os("DEFEND_TAIPEI_QA_EXTERIOR_ONLY").is_some_and(|value| value == "1");
        let commercial_only =
            std::env::var_os("DEFEND_TAIPEI_QA_COMMERCIAL_ONLY").is_some_and(|value| value == "1");
        let streets_only =
            std::env::var_os("DEFEND_TAIPEI_QA_STREETS_ONLY").is_some_and(|value| value == "1");
        let lighting_only =
            std::env::var_os("DEFEND_TAIPEI_QA_LIGHTING_ONLY").is_some_and(|value| value == "1");
        let wave5_only =
            std::env::var_os("DEFEND_TAIPEI_QA_WAVE5_ONLY").is_some_and(|value| value == "1");
        let stages: Vec<_> = STAGES
            .iter()
            .enumerate()
            .filter_map(|(index, stage)| {
                ((!exterior_only
                    && !commercial_only
                    && !streets_only
                    && !lighting_only
                    && !wave5_only)
                    || index == 0
                    || matches!(
                        stage.action,
                        Action::ChooseSurvivor
                            | Action::ReviewDeparture
                            | Action::Depart
                            | Action::Skip
                    )
                    || (wave5_only
                        && matches!(
                            stage.action,
                            Action::Wave5(_)
                                | Action::StreetDetail(_)
                                | Action::SaveMenu
                                | Action::Continue
                        ))
                    || (lighting_only && matches!(stage.action, Action::Lighting(_)))
                    || (!lighting_only
                        && streets_only
                        && matches!(stage.action, Action::Streets(_)))
                    || (!lighting_only
                        && !streets_only
                        && commercial_only
                        && matches!(stage.action, Action::Commercial(_)))
                    || (!lighting_only
                        && !streets_only
                        && !commercial_only
                        && exterior_only
                        && matches!(stage.action, Action::Exterior(_))))
                .then_some(index)
            })
            .collect();
        let scope = if wave5_only {
            "wave5_subset"
        } else if lighting_only {
            "lighting_subset"
        } else if streets_only {
            "streets_subset"
        } else if commercial_only {
            "commercial_subset"
        } else if exterior_only {
            "exterior_subset"
        } else {
            "full"
        };
        let expected_captures = stages.len();
        println!("DTQA scope={scope} expected_captures={expected_captures}");
        app.init_resource::<wave5::Progress>()
            .insert_resource(crate::combat::armor::ArmorQaFreeze(true))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 60.0,
            )))
            .insert_resource(QaRun {
                output,
                stages,
                target: None,
                stage: 0,
                entered: false,
                frame: 0,
                ready_frames: 0,
                wait_logged: Instant::now(),
                scene_chunk: None,
                entrance_building: None,
                interior_building: None,
                exterior: None,
                commercial: None,
                streets: None,
                lighting: None,
                lighting_reference_probes: Vec::new(),
                lighting_observations: Vec::new(),
                door_test_id: None,
                barricade_planks: None,
                loot_fixture: None,
                loot_expected: None,
                loot_inventory: None,
                reclaim: None,
                war_grenades: None,
                waiting_capture: false,
                captured: false,
                checkpoint: None,
                continuation_checked: false,
                transport_continuation_checked: false,
                transport_restore: None,
                started: Instant::now(),
                report: QaReport {
                    success: false,
                    scope,
                    expected_captures,
                    renderer: String::new(),
                    captures: Vec::new(),
                    assertions: Vec::new(),
                    error: None,
                },
                finished: false,
            })
            .add_systems(Last, drive.after(crate::save::service_requests));
    }
}

fn set_target(world: &mut World, run: &mut QaRun, size: [u32; 2]) -> Result<(), String> {
    let world_camera = world
        .query_filtered::<Entity, With<GameCamera>>()
        .iter(world)
        .next()
        .ok_or("Game camera did not spawn")?;
    // Keep the existing QA renderer contract on the actual world camera.
    world.entity_mut(world_camera).insert(Msaa::Off);
    let camera = world
        .query_filtered::<Entity, With<crate::performance::render::PresentationCamera>>()
        .iter(world)
        .next()
        .ok_or("Presentation camera did not spawn")?;
    let window_entity = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .ok_or("Virtual primary window did not spawn")?;
    {
        let mut window = world
            .get_mut::<Window>(window_entity)
            .ok_or("Missing window dimensions")?;
        window.resolution.set_scale_factor_override(Some(1.0));
        window.resolution.set_physical_resolution(size[0], size[1]);
    }
    let mut image =
        Image::new_target_texture(size[0], size[1], TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let handle = world.resource_mut::<Assets<Image>>().add(image);
    world.entity_mut(camera).insert((
        RenderTarget::Image(handle.clone().into()),
        IsDefaultUiCamera,
        Msaa::Off,
    ));
    run.target = Some(handle);
    Ok(())
}

fn flow_matches(world: &World, expected: ExpectedFlow) -> bool {
    match (world.resource::<State<GameFlow>>().get(), expected) {
        (GameFlow::Title, ExpectedFlow::Title) => {
            world.resource::<TitleMenu>().page == TitlePage::Home
        }
        (GameFlow::Title, ExpectedFlow::Survivor) => {
            world.resource::<TitleMenu>().page == TitlePage::Survivor
        }
        (GameFlow::Title, ExpectedFlow::Briefing) => {
            world.resource::<TitleMenu>().page == TitlePage::Briefing
        }
        (GameFlow::Prologue, ExpectedFlow::Prologue)
        | (GameFlow::Playing, ExpectedFlow::Playing) => true,
        _ => false,
    }
}

fn player_position(world: &mut World) -> Result<Vec3, String> {
    world
        .query_filtered::<&Transform, With<Player>>()
        .iter(world)
        .next()
        .map(|t| t.translation)
        .ok_or_else(|| "Player did not spawn".into())
}

fn reset_and_close_settings(world: &mut World) {
    use crate::ui::settings::{SettingsAction as A, SettingsPage};
    world.write_message(A::Page(SettingsPage::Performance));
    world.write_message(A::Reset);
    world.write_message(A::Page(SettingsPage::Game));
    world.write_message(A::Reset);
    world.write_message(A::Page(SettingsPage::Audio));
    world.write_message(A::Reset);
    world.write_message(A::Close);
}

fn enter_stage(world: &mut World, run: &mut QaRun, stage: &Stage) -> Result<(), String> {
    set_target(world, run, stage.size)?;
    run.ready_frames = 0;
    run.wait_logged = Instant::now();
    run.scene_chunk = None;
    run.entrance_building = None;
    run.interior_building = None;
    run.exterior = None;
    run.commercial = None;
    run.streets = None;
    run.lighting = None;
    run.lighting_observations.clear();
    world
        .resource_mut::<crate::map::street::furnishings::SignalPhase>()
        .qa_fixed = matches!(stage.action, Action::Lighting(_)).then_some(0.0);
    println!("DTQA entering {}", stage.name);
    if run.stage > 0
        && matches!(
            STAGES[run.stages[run.stage - 1]].action,
            Action::PerformanceStreet
        )
    {
        // Restore only this explicit QA fixture before continuing the original
        // gameplay route. Runtime preferences never change themselves on close.
        *world.resource_mut::<crate::performance::PerformancePreferences>() =
            crate::performance::PerformancePreferences::default();
    }
    match stage.action {
        Action::Wave5(scene) => {
            let position = wave5::enter(world, scene)?;
            run.scene_chunk = Some(crate::map::ChunkKey::of(position));
        }
        Action::StreetDetail(scene) => {
            world.write_message(OpenPanel(PanelKind::Close));
            world.resource_mut::<crate::ui::PauseOpen>().0 = false;
            let position = wave5_exterior::enter(world, scene)?;
            run.scene_chunk = Some(crate::map::ChunkKey::of(position));
        }
        Action::None => {}
        Action::SettingsAudio => {
            use crate::ui::settings::SettingsAction as A;
            world.write_message(A::Open);
            for (index, value) in [0.65, 0.6, 0.5, 0.4].into_iter().enumerate() {
                world.write_message(A::Volume(index, value));
            }
        }
        Action::SettingsGame => {
            use crate::ui::settings::{SettingsAction as A, SettingsPage};
            world.write_message(A::Page(SettingsPage::Game));
            world.write_message(A::DayHours(2));
            world.write_message(A::Language(crate::i18n::Language::En));
        }
        Action::SettingsPerformance | Action::SettingsPerformanceLandscape => {
            use crate::ui::settings::{PerformanceSection, SettingsAction as A, SettingsPage};
            world.write_message(A::Page(SettingsPage::Performance));
            world.write_message(A::PerformanceSection(PerformanceSection::Display));
            world.write_message(A::PerformanceProfile(
                crate::performance::PerformanceProfile::Balanced,
            ));
            world.write_message(A::FrameCap(30));
            world.write_message(A::RenderScale(70));
            world.write_message(A::Language(
                if matches!(stage.action, Action::SettingsPerformanceLandscape) {
                    crate::i18n::Language::En
                } else {
                    crate::i18n::Language::ZhTw
                },
            ));
        }
        Action::SettingsDiagnostics => {
            use crate::ui::settings::{PerformanceSection, SettingsAction as A};
            world.write_message(A::PerformanceSection(PerformanceSection::Diagnostics));
            world.write_message(A::PerformanceOverlay);
            world.write_message(A::Language(crate::i18n::Language::ZhTw));
        }
        Action::PerformanceStreet => {
            use crate::ui::settings::{SettingsAction as A, SettingsPage};
            // Exercise the real UI actions while paused, then prove closing
            // preserves both the selected preferences and the paused run.
            world.resource_mut::<crate::ui::PauseOpen>().0 = true;
            world.write_message(A::Open);
            world.write_message(A::Page(SettingsPage::Performance));
            world.write_message(A::PerformanceProfile(
                crate::performance::PerformanceProfile::Balanced,
            ));
            world.write_message(A::FrameCap(30));
            world.write_message(A::RenderScale(70));
            world.write_message(A::PerformanceOverlay);
            world.write_message(A::Close);
        }
        Action::SettingsClose => {
            reset_and_close_settings(world);
        }
        Action::SettingsPause => {
            use crate::ui::settings::{SettingsAction as A, SettingsPage};
            world.write_message(A::Open);
            world.write_message(A::Page(SettingsPage::Game));
            world.write_message(A::DayHours(4));
        }
        Action::ChooseSurvivor => {
            world.write_message(TitleAction::NewGame);
        }
        Action::ReviewDeparture => {
            world.write_message(TitleAction::Next);
        }
        Action::Depart => {
            world.write_message(TitleAction::Depart);
        }
        Action::Skip => {
            world.write_message(SkipPrologue);
        }
        Action::Street => {
            // A real street-side camera, with gameplay and the streamed map active.
            // Geometry/material regression evidence, not a synthetic scene.
            let data = crate::map::world();
            let (position, owner) =
                street_view(data).ok_or("No clear main-road sidewalk for QA")?;
            move_view(world, position, 1.35)?;
            run.scene_chunk = Some(owner);
            run.report.assertions.push(format!(
                "Street capture uses a clear sidewalk at ({:.2}, {:.2})",
                position.x, position.y
            ));
        }
        Action::Exterior(scene) => {
            // Actual immutable map lots, ordinary gameplay camera and the same
            // streaming/cutaway systems used on Android. Only location, zoom and
            // daylight are explicit fixtures; no demonstration meshes are spawned.
            let fixture = exterior_fixture(crate::map::world(), scene, stage.size)?;
            println!(
                "DTQA exterior fixture scene={scene:?} lot={} player={:?} zoom={:.3}",
                fixture.building, fixture.position, fixture.zoom
            );
            move_view(world, fixture.position, fixture.zoom)?;
            run.scene_chunk = Some(crate::map::ChunkKey::of(
                crate::map::world().blds[fixture.building as usize].c,
            ));
            run.exterior = Some(fixture);
            world.write_message(OpenPanel(PanelKind::Close));
            world.resource_mut::<crate::ui::PauseOpen>().0 = false;
            world.resource_mut::<Campaign>().game_min = 720.0;
            world
                .resource_mut::<crate::survival::ShelterEnvironment>()
                .power = true;
            let mut progress = world.resource_mut::<crate::worldplay::ScavengeState>();
            progress.weather.next = 5000.0;
            progress.weather.until = 0.0;
            progress.weather.amount = 0.0;
            progress.weather.intensity = 0.0;
            progress.weather.wet = 0.0;
        }
        Action::Streets(scene) => {
            let fixture = street_fixture(crate::map::world(), scene, stage.size)?;
            println!(
                "DTQA streets fixture scene={scene:?} source={:?} player={:?} zoom={:.3}",
                fixture.subject, fixture.position, fixture.zoom
            );
            move_view(world, fixture.position, fixture.zoom)?;
            run.streets = Some(fixture);
            world.write_message(OpenPanel(PanelKind::Close));
            world.resource_mut::<crate::ui::PauseOpen>().0 = false;
            world.resource_mut::<Campaign>().game_min = 720.0;
            world
                .resource_mut::<crate::survival::ShelterEnvironment>()
                .power = true;
            let mut progress = world.resource_mut::<crate::worldplay::ScavengeState>();
            progress.weather.next = 5000.0;
            progress.weather.until = 0.0;
            progress.weather.amount = 0.0;
            progress.weather.intensity = 0.0;
            progress.weather.wet = 0.0;
        }
        Action::Lighting(scene) => {
            let fixture = lighting_fixture(crate::map::world(), scene, stage.size)?;
            println!(
                "DTQA lighting fixture scene={scene:?} building={} player={:?} zoom={}",
                fixture.building, fixture.position, fixture.zoom
            );
            move_view(world, fixture.position, fixture.zoom)?;
            run.scene_chunk = Some(crate::map::ChunkKey::of(
                crate::map::world().blds[fixture.building as usize].c,
            ));
            run.lighting = Some(fixture);
            if scene == LightingScene::Day {
                run.lighting_reference_probes.clear();
            }
            world.write_message(OpenPanel(PanelKind::Close));
            world.resource_mut::<crate::ui::PauseOpen>().0 = false;
            world.resource_mut::<Campaign>().game_min = scene.minute();
            world
                .resource_mut::<crate::survival::ShelterEnvironment>()
                .power = scene != LightingScene::Blackout;
            let mut progress = world.resource_mut::<crate::worldplay::ScavengeState>();
            progress.weather.next = 5000.0;
            progress.weather.until = 0.0;
            progress.weather.amount = 0.0;
            progress.weather.intensity = 0.0;
            progress.weather.wet = 0.0;
        }
        Action::Commercial(scene) => {
            let fixture = commercial_fixture(crate::map::world(), scene, stage.size)?;
            println!(
                "DTQA commercial fixture scene={scene:?} lot={} player={:?} zoom={:.3}",
                fixture.building, fixture.position, fixture.zoom
            );
            move_view(world, fixture.position, fixture.zoom)?;
            run.scene_chunk = Some(crate::map::ChunkKey::of(
                crate::map::world().blds[fixture.building as usize].c,
            ));
            run.commercial = Some(fixture);
            world.write_message(OpenPanel(PanelKind::Close));
            world.resource_mut::<crate::ui::PauseOpen>().0 = false;
            world.resource_mut::<Campaign>().game_min =
                if matches!(scene, CommercialScene::Night | CommercialScene::Blackout) {
                    1320.0
                } else {
                    720.0
                };
            world
                .resource_mut::<crate::survival::ShelterEnvironment>()
                .power = scene != CommercialScene::Blackout;
            let mut progress = world.resource_mut::<crate::worldplay::ScavengeState>();
            progress.weather.next = 5000.0;
            progress.weather.until = 0.0;
            progress.weather.amount = 0.0;
            progress.weather.intensity = 0.0;
            progress.weather.wet = 0.0;
        }
        Action::Entrance => {
            let data = crate::map::world();
            let (building, position) = data
                .blds
                .iter()
                .filter(|building| !building.fill && building.h < 20.0)
                .filter(|building| {
                    !world
                        .resource::<crate::map::Obstacles>()
                        .doors()
                        .is_open(building.id)
                })
                .filter_map(|building| {
                    let door = building.door?;
                    // Our isometric camera looks from +X/+Z: choose an entrance
                    // facing the camera, with a clear place to stand outside it.
                    if door.n.dot(Vec2::ONE.normalize()) < 0.65 {
                        return None;
                    }
                    let position = door.p + door.n * 5.5;
                    (position.cmpge(data.min + 1.0).all()
                        && position.cmple(data.max - 1.0).all()
                        && building.c.distance(position) < 70.0
                        && data.bld_at(position).is_none()
                        && data.free(position, 0.6)
                        && (-5..=55).all(|step| {
                            data.free(door.p + door.n * step as f32 * 0.1, PLAYER_RADIUS)
                        }))
                    .then_some((building, position))
                })
                .min_by(|a, b| {
                    a.1.distance_squared(data.spawn)
                        .total_cmp(&b.1.distance_squared(data.spawn))
                })
                .ok_or("No camera-facing reachable entrance for QA")?;
            move_view(world, position, 1.9)?;
            run.scene_chunk = Some(crate::map::ChunkKey::of(building.c));
            run.entrance_building = Some(building.id);
            run.door_test_id = Some(building.id);
            run.report.assertions.push(format!(
                "Entrance capture uses reachable building {} at ({:.2}, {:.2})",
                building.id, position.x, position.y
            ));
        }
        Action::DoorOpen | Action::DoorClose => {
            let id = run.door_test_id.ok_or("Missing tested entrance")?;
            let building = &crate::map::world().blds[id as usize];
            let door = building.door.ok_or("Tested building has no door")?;
            move_view(world, door.p + door.n * 1.5, 1.9)?;
            world.write_message(crate::worldplay::WorldAction::Interact);
            run.scene_chunk = Some(crate::map::ChunkKey::of(building.c));
            run.entrance_building = Some(id);
        }
        Action::Barricade | Action::Unbarricade | Action::BarricadeInventory => {
            let id = run.door_test_id.ok_or("Missing barricade entrance")?;
            let building = &crate::map::world().blds[id as usize];
            let door = building.door.ok_or("Missing barricade door")?;
            move_view(world, door.p + door.n * 1.5, 2.4)?;
            if matches!(stage.action, Action::Barricade) {
                let mut bag = world.resource_mut::<Inventory>();
                if bag.count("hammer") == 0 {
                    bag.add("hammer", 1);
                }
                bag.add("plank", 2);
                run.barricade_planks = Some(bag.count("plank"));
                run.report.assertions.push("Barricade QA adds an explicit hammer / two-plank fixture, then exercises actual WorldAction handlers".into());
            }
            world.write_message(crate::worldplay::WorldAction::Barricade);
            if matches!(stage.action, Action::BarricadeInventory) {
                world.write_message(OpenPanel(PanelKind::Inventory));
            }
        }
        Action::BarricadeBlocked => {
            world.write_message(crate::worldplay::WorldAction::Interact);
        }
        Action::ShopInterior => {
            world.write_message(OpenPanel(PanelKind::Close));
            world
                .resource_mut::<crate::worldplay::WorldNotice>()
                .text
                .clear();
            world.resource_mut::<Campaign>().notice_t = 0.0;
            let data = crate::map::world();
            let minute = world.resource::<Campaign>().game_min;
            let progress = world.resource::<crate::worldplay::ScavengeState>();
            let fixture = data
                .interiors
                .iter()
                .filter(|f| f.kind == crate::map::interiors::FixtureKind::Shelf)
                .filter(|f| {
                    data.interiors
                        .iter()
                        .filter(|other| other.building == f.building)
                        .count()
                        >= 4
                })
                .filter(|f| {
                    data.blds[f.building as usize]
                        .door
                        .is_some_and(|d| d.p.distance(f.approach) > 3.2)
                })
                .filter(|f| {
                    let stock = crate::worldplay::preview_container_loot(
                        &f.id,
                        f.loot.unwrap(),
                        minute,
                        progress,
                    );
                    stock.stacks().len() >= 2
                        && stock.stacks().iter().map(|(_, n)| *n).sum::<u32>() >= 3
                        && stock
                            .stacks()
                            .iter()
                            .map(|(id, n)| crate::inventory::item(id).unwrap().weight * *n as f32)
                            .sum::<f32>()
                            < 6.0
                })
                .min_by(|a, b| {
                    a.approach
                        .distance_squared(data.spawn)
                        .total_cmp(&b.approach.distance_squared(data.spawn))
                })
                .cloned()
                .ok_or("No reachable furnished shop with a small multi-item shelf")?;
            let stock = crate::worldplay::preview_container_loot(
                &fixture.id,
                fixture.loot.unwrap(),
                minute,
                progress,
            );
            run.loot_expected = Some(stock);
            run.loot_inventory = Some(world.resource::<Inventory>().clone());
            run.loot_fixture = Some(fixture.id.clone());
            run.interior_building = Some(fixture.building);
            move_view(world, fixture.approach, 3.2)?;
            run.scene_chunk = Some(crate::map::ChunkKey::of(fixture.approach));
            run.report.assertions.push(format!("Shop fixture {} at ({:.3}, {:.3}) uses deterministic real stock and shared furniture collision", fixture.id, fixture.approach.x, fixture.approach.y));
        }
        Action::Scavenge => {
            world.write_message(crate::worldplay::WorldAction::Interact);
        }
        Action::ScavengeOne | Action::ScavengeAll => {
            let id = run.loot_fixture.clone().ok_or("Missing shop fixture")?;
            let stock = run.loot_expected.as_mut().ok_or("Missing expected stock")?;
            let bag = run
                .loot_inventory
                .as_mut()
                .ok_or("Missing expected inventory")?;
            let take = if matches!(stage.action, Action::ScavengeOne) {
                let item = stock
                    .stacks()
                    .first()
                    .ok_or("Shelf already empty")?
                    .0
                    .clone();
                if crate::worldplay::pickable_count(bag, &item, 1) != 1 {
                    return Err("QA bag has no room for one shelf item".into());
                }
                stock.take(&item, 1);
                stock.remove_empty();
                bag.add(&item, 1);
                crate::worldplay::LootTake::One(item)
            } else {
                for (item, count) in stock.stacks() {
                    if crate::worldplay::pickable_count(bag, item, *count) != *count {
                        return Err(
                            "QA bag has insufficient capacity for the small shelf fixture".into(),
                        );
                    }
                    bag.add(item, *count);
                }
                *stock = Stash::default();
                crate::worldplay::LootTake::All
            };
            world.write_message(crate::worldplay::WorldAction::TakeLoot {
                container: id,
                take,
            });
        }
        Action::ShopDepleted => {
            world.write_message(OpenPanel(PanelKind::Close));
        }
        Action::HomeInterior => {
            world
                .resource_mut::<crate::worldplay::WorldNotice>()
                .text
                .clear();
            world.resource_mut::<Campaign>().notice_t = 0.0;
            let data = crate::map::world();
            let fixture = data
                .interiors
                .iter()
                .filter(|f| f.kind == crate::map::interiors::FixtureKind::Altar)
                .filter(|f| f.front.dot(crate::camera::CAM_OFFSET.xz().normalize()) > 0.3)
                .filter(|f| {
                    data.interiors
                        .iter()
                        .filter(|other| other.building == f.building)
                        .count()
                        >= 5
                })
                .min_by(|a, b| {
                    a.approach
                        .distance_squared(data.spawn)
                        .total_cmp(&b.approach.distance_squared(data.spawn))
                })
                .ok_or("No furnished home with a family shrine")?;
            move_view(world, fixture.approach, 2.8)?;
            run.interior_building = Some(fixture.building);
            run.scene_chunk = Some(crate::map::ChunkKey::of(fixture.approach));
            run.report.assertions.push(format!(
                "Home interior capture uses building {} with collision-clear furniture approaches",
                fixture.building
            ));
        }
        Action::Map => {
            // Resume the pre-existing route at its exterior-cutaway camera,
            // time, power and weather after the commercial/street/lighting sequences.
            // Exploration from the extra real-world fixture positions remains;
            // these stages are retained, not claimed to have identical pixels.
            if run.stage > 0
                && matches!(
                    STAGES[run.stages[run.stage - 1]].action,
                    Action::Commercial(_) | Action::Streets(_) | Action::Lighting(_)
                )
            {
                let previous =
                    exterior_fixture(crate::map::world(), ExteriorScene::Cutaway, [390, 844])?;
                move_view(world, previous.position, previous.zoom)?;
                world.resource_mut::<Campaign>().game_min = 720.0;
                world
                    .resource_mut::<crate::survival::ShelterEnvironment>()
                    .power = true;
                let mut progress = world.resource_mut::<crate::worldplay::ScavengeState>();
                progress.weather.next = 5000.0;
                progress.weather.until = 0.0;
                progress.weather.amount = 0.0;
                progress.weather.intensity = 0.0;
                progress.weather.wet = 0.0;
            }
            let p = player_position(world)?.xz();
            let mut progress = world.resource_mut::<crate::worldmap::MapProgress>();
            progress.add_note(
                p + Vec2::new(-20.0, 10.0),
                crate::worldmap::NoteKind::Supplies,
            );
            progress.add_note(
                p + Vec2::new(24.0, -12.0),
                crate::worldmap::NoteKind::Danger,
            );
            world.write_message(crate::worldmap::ToggleMap);
        }
        Action::Rain => {
            world.write_message(OpenPanel(PanelKind::Close));
            let (p, owner) = street_view(crate::map::world()).ok_or("No rain street view")?;
            move_view(world, p, 1.35)?;
            run.scene_chunk = Some(owner);
            world.resource_mut::<Campaign>().game_min = 720.0;
            let mut progress = world.resource_mut::<crate::worldplay::ScavengeState>();
            progress.weather.next = 5000.0;
            progress.weather.until = 900.0;
            progress.weather.amount = 0.85;
            progress.weather.intensity = 0.85;
            progress.weather.wet = 1.0;
            run.report.assertions.push("Rain/night captures use explicit visual weather fixtures in the real running world".into());
        }
        Action::Night => {
            world.resource_mut::<Campaign>().game_min = 1320.0;
            let mut progress = world.resource_mut::<crate::worldplay::ScavengeState>();
            progress.weather.until = 0.0;
            progress.weather.amount = 0.0;
        }
        Action::Blackout => {
            world
                .resource_mut::<crate::survival::ShelterEnvironment>()
                .power = false;
        }
        Action::Warnings => {
            let mut needs = world.resource_mut::<crate::worldplay::PlayerNeeds>();
            needs.hunger = 22.0;
            needs.thirst = 18.0;
            needs.fatigue = 82.0;
            world.resource_mut::<crate::combat::GameState>().bleed = true;
            world.resource_mut::<Campaign>().notice_t = 12.0;
            world.resource_mut::<crate::worldplay::WorldNotice>().text = "流血，需要止血。".into();
        }
        Action::Journal => {
            world.write_message(OpenPanel(PanelKind::Journal));
        }
        Action::Pause => {
            world.write_message(OpenPanel(PanelKind::Close));
            world.write_message(crate::ui::TogglePause);
        }
        Action::Shelter => {
            world.resource_mut::<Campaign>().game_min = 1440.0 + 720.0;
            world
                .resource_mut::<crate::survival::ShelterEnvironment>()
                .power = true;
            *world.resource_mut::<crate::worldplay::PlayerNeeds>() = default();
            let mut game = world.resource_mut::<crate::combat::GameState>();
            game.bleed = false;
            game.hp = 100.0;
            move_view(world, crate::player::spawn_point().xz(), 1.0)?;
            world.write_message(OpenPanel(PanelKind::Shelter));
        }
        Action::Inventory => {
            world.write_message(OpenPanel(PanelKind::Inventory));
        }
        Action::Trade => {
            reset_and_close_settings(world);
            // Keep the game paused until the queued settings resets and close
            // have run. Unpausing here would invalidate the open overlay before
            // its action handler can apply either reset.
            // Known authored barter location. This fixture avoids turning a UI
            // regression run into several minutes of walking across the map.
            let pos = TRADE_SPOTS[0].pos;
            let mut query = world.query_filtered::<&mut Transform, With<Player>>();
            let mut player = query.iter_mut(world).next().ok_or("Player did not spawn")?;
            player.translation.x = pos[0];
            player.translation.z = pos[1];
            // Open after the settings close pointer has cleared modal capture.
        }
        Action::SaveMenu => {
            // Stop simulated consumption/cooling between this checkpoint and
            // the following Last-schedule save. Verify the restored values in
            // the same Last schedule as load, before gameplay can advance.
            world.resource_mut::<crate::ui::PauseOpen>().0 = true;
            let position = player_position(world)?;
            run.checkpoint = Some(Checkpoint {
                day: world.resource::<Shelter>().day,
                stash: world.resource::<Shelter>().stash.clone(),
                inventory: world.resource::<Inventory>().clone(),
                role: *world.resource::<SelectedRole>(),
                position,
                game_min: world.resource::<Campaign>().game_min,
                map: world.resource::<crate::worldmap::MapProgress>().clone(),
                closed_doors: world
                    .resource::<crate::worldplay::ScavengeState>()
                    .closed_doors
                    .clone(),
                barricaded_doors: world
                    .resource::<crate::worldplay::ScavengeState>()
                    .barricaded_doors
                    .clone(),
                loot: world
                    .resource::<crate::worldplay::ScavengeState>()
                    .loot
                    .clone(),
                fixtures_version: world
                    .resource::<crate::worldplay::ScavengeState>()
                    .fixtures_version,
                vehicles: world.resource::<crate::vehicles::VehicleFleet>().clone(),
                armor: world.resource::<crate::combat::armor::ArmorFleet>().clone(),
            });
            world.write_message(OpenPanel(PanelKind::Close));
            world.write_message(SaveAndMenu);
        }
        Action::Continue => {
            let status = world.resource::<SaveStatus>();
            if !status.available || status.error.is_some() {
                return Err(format!("Save/menu failed: {}", status.message()));
            }
            run.report
                .assertions
                .push("SaveAndMenu wrote a valid save before resetting the run".into());
            world.write_message(TitleAction::Continue);
        }
        Action::Remnant => {
            world
                .resource_mut::<crate::vehicles::VehicleFleet>()
                .current = None;
            world.write_message(OpenPanel(PanelKind::Close));
            world.resource_mut::<Campaign>().game_min = 1440.0 + 600.0;
            world.resource_mut::<Campaign>().notice_t = 0.0;
            world
                .resource_mut::<crate::worldplay::WorldNotice>()
                .text
                .clear();
            *world.resource_mut::<crate::worldplay::PlayerNeeds>() = default();
            world.resource_mut::<crate::combat::GameState>().bleed = false;
            let site = *crate::map::world()
                .remnants
                .first()
                .ok_or("No ROC remnant position")?;
            move_view(world, site.p - Vec2::from_angle(site.ang) * 0.9, 1.6)?;
        }
        Action::RemnantTalk => {
            world.write_message(crate::dialogue::TalkTo(crate::dialogue::NpcKey::Remnant(0)));
        }
        Action::Reclaim => {
            world.resource_mut::<crate::dialogue::Dialogue>().0 = None;
            world.write_message(OpenPanel(PanelKind::Close));
            let roster = world.resource::<crate::sectors::PatrolRoster>().clone();
            let dead = world
                .resource::<crate::worldplay::ScavengeState>()
                .dead
                .clone();
            let counts = crate::sectors::count_sectors(&roster, &dead);
            let target = (0..crate::sectors::SECTORS.len())
                .filter(|&i| !crate::sectors::outer_sector(i) && counts[i].alive > 0)
                .min_by_key(|&i| counts[i].alive)
                .ok_or("No occupied core area")?;
            run.reclaim = Some((target, world.resource::<Inventory>().clone()));
            let ids: Vec<u32> = roster
                .0
                .iter()
                .filter(|(_, at)| crate::sectors::sector_of(*at) == target)
                .map(|(id, _)| *id)
                .collect();
            let mut killed = Vec::new();
            let mut query = world.query::<(Entity, &mut crate::combat::Enemy, &Transform)>();
            for (entity, mut enemy, tf) in query.iter_mut(world) {
                if ids.contains(&enemy.0.id) && enemy.0.alive() {
                    enemy.0.hp = 0.0;
                    enemy.0.state = crate::combat::SolState::Dead;
                    killed.push((entity, enemy.0.id, enemy.0.kind, tf.translation));
                }
            }
            for (entity, id, kind, pos) in killed {
                world
                    .entity_mut(entity)
                    .insert(crate::combat::Body::new(kind));
                world.write_message(crate::combat::EnemyDied { id, kind, pos });
            }
            let p = player_position(world)?.xz();
            move_view(world, p, 1.0)?;
        }
        Action::Airdrop => {
            world.write_message(crate::dialogue::TalkTo(crate::dialogue::NpcKey::Liaison));
            world.write_message(crate::japan::CallSupport(crate::japan::JpKind::Drop));
            let p = player_position(world)?.xz();
            move_view(world, p, 0.8)?;
        }
        Action::Upstairs => {
            world.resource_mut::<crate::dialogue::Dialogue>().0 = None;
            world.write_message(OpenPanel(PanelKind::Close));
            world
                .resource_mut::<crate::worldplay::WorldNotice>()
                .text
                .clear();
            let p = player_position(world)?.xz();
            let w = crate::map::world();
            let stair = w
                .stairs
                .iter()
                .filter(|s| {
                    !crate::map::upstairs::plan_floor(w, &w.blds[s.building as usize], s, 2)
                        .is_empty()
                })
                .min_by(|a, b| {
                    a.landing
                        .distance_squared(p)
                        .total_cmp(&b.landing.distance_squared(p))
                })
                .copied()
                .ok_or("No walk-up with a furnished second floor")?;
            move_view(world, stair.landing, 1.4)?;
            world.write_message(crate::dialogue::TalkTo(crate::dialogue::NpcKey::StairsUp));
        }
        Action::Rescue => {
            crate::upstairs::force_ground(world);
            world.resource_mut::<crate::dialogue::Dialogue>().0 = None;
            let p = player_position(world)?.xz();
            let day = (world.resource::<Campaign>().game_min / 1440.0).floor() as u32;
            let (karma, home) = {
                let shelter = world.resource::<Shelter>();
                (shelter.karma, shelter.home.map(Vec2::from_array))
            };
            let call = crate::rescue::pick_call(day, karma, p, home).ok_or("No rescue call")?;
            let stair = *crate::map::upstairs::stair_of(crate::map::world(), call.building)
                .ok_or("Rescue building has no stairs")?;
            world.resource_mut::<crate::rescue::RescueState>().call = Some(call.clone());
            move_view(world, stair.landing, 1.4)?;
            for _ in 2..=call.floor {
                world.write_message(crate::dialogue::TalkTo(crate::dialogue::NpcKey::StairsUp));
            }
        }
        Action::War => {
            crate::upstairs::force_ground(world);
            world.resource_mut::<crate::dialogue::Dialogue>().0 = None;
            world.write_message(OpenPanel(PanelKind::Close));
            let p = player_position(world)?.xz();
            world.resource_mut::<Inventory>().add("grenade", 1);
            run.war_grenades = Some(world.resource::<Inventory>().count("grenade"));
            world.write_message(crate::worldplay::WorldAction::Use("grenade".into()));
            world.write_message(crate::war::WarCall::Barrage {
                n: 3,
                at: p + Vec2::new(24.0, -10.0),
            });
            world.write_message(crate::war::WarCall::Strike {
                w: crate::war::ROCKET,
                at: p + Vec2::new(70.0, 40.0),
            });
            move_view(world, p, 0.7)?;
        }
        Action::Sniper => {
            crate::war::clear_sky(world);
            crate::upstairs::force_ground(world);
            world
                .resource_mut::<crate::worldplay::WorldNotice>()
                .text
                .clear();
            world.resource_mut::<crate::combat::GameState>().hp = 100.0;
            let posts = world.resource::<crate::sniper::Posts>().0.clone();
            let dead = world.resource::<crate::sniper::SniperState>().dead.clone();
            let obstacles = world.resource::<crate::map::Obstacles>().clone();
            let spot = posts
                .iter()
                .enumerate()
                .filter(|(i, _)| !dead.contains(&(*i as u8)))
                .flat_map(|(_, p)| {
                    (0..8)
                        .map(move |k| {
                            p.j + Vec2::from_angle(k as f32 * std::f32::consts::FRAC_PI_4) * 6.0
                        })
                        .chain(std::iter::once(p.j))
                        .map(move |q| (p.clone(), q))
                })
                .find(|(p, q)| crate::sniper::sees(p, *q, false, &obstacles))
                .ok_or("No junction in a sniper's sight")?;
            move_view(world, spot.1, 1.0)?;
        }
        Action::Heli => {
            crate::war::clear_sky(world);
            let p = player_position(world)?.xz();
            crate::heli::spawn_qa_heli(world, crate::heli::HeliKind::Z10, p + Vec2::new(6.0, -8.0));
            move_view(world, p, 0.7)?;
        }
    }
    run.entered = true;
    run.frame = 0;
    Ok(())
}

fn move_view(world: &mut World, position: Vec2, zoom: f32) -> Result<(), String> {
    let mut query = world.query_filtered::<&mut Transform, With<Player>>();
    let mut player = query.iter_mut(world).next().ok_or("Player did not spawn")?;
    player.translation.x = position.x;
    player.translation.z = position.y;
    world.resource_mut::<CameraZoom>().0 = zoom;
    Ok(())
}

/// Put the actual player on the far sidewalk, looking across the carriageway.
/// A merely walkable point can still put a roof or tree crown in front of the
/// camera; that would produce a valid image without useful pavement evidence.
fn street_view(data: &crate::map::WorldData) -> Option<(Vec2, crate::map::ChunkKey)> {
    let camera = Vec2::ONE.normalize();
    let mut candidates = Vec::new();
    for road in data
        .roads
        .iter()
        .filter(|road| road.w >= 12.0 && road.w <= 22.0)
    {
        for pair in road.p.windows(2) {
            let length = pair[0].distance(pair[1]);
            if length < 30.0 {
                continue;
            }
            let direction = (pair[1] - pair[0]) / length;
            let normal = Vec2::new(-direction.y, direction.x);
            for side in [-1.0, 1.0] {
                let outward = normal * side;
                if outward.dot(camera) >= -0.35 {
                    continue;
                }
                for along in [0.3, 0.45, 0.6, 0.75] {
                    if (along * length).min((1.0 - along) * length) < road.w / 2.0 + 5.0 {
                        continue;
                    }
                    let middle = pair[0].lerp(pair[1], along);
                    let p = middle + outward * (road.w / 2.0 + 1.1);
                    if !p.cmpge(data.min + 1.0).all()
                        || !p.cmple(data.max - 1.0).all()
                        || !data.free(p, 0.6)
                        || data.bld_at(middle).is_some()
                        || ![-8.0, -4.0, 0.0, 4.0, 8.0].into_iter().all(|distance| {
                            let q = p + direction * distance;
                            data.bld_at(q).is_none() && data.road_dist(q) > -0.05
                        })
                        || data.props.iter().any(|prop| {
                            matches!(prop.kind, crate::map::street::PropKind::Tree(_))
                                && prop.p.distance(p) < 5.0
                        })
                        || (2..=18).any(|distance| {
                            data.bld_at(p + camera * distance as f32)
                                .is_some_and(|building| {
                                    building.h + 3.0 > distance as f32 * 56.0 / 40.0_f32.hypot(40.0)
                                })
                        })
                    {
                        continue;
                    }
                    candidates.push((p, crate::map::ChunkKey::of((pair[0] + pair[1]) / 2.0)));
                }
            }
        }
    }
    candidates.into_iter().min_by(|a, b| {
        a.0.distance_squared(data.spawn)
            .total_cmp(&b.0.distance_squared(data.spawn))
    })
}

fn exterior_bounds(building: &crate::map::data::Bld) -> Vec<Vec3> {
    building
        .pts
        .iter()
        .flat_map(|point| {
            [
                Vec3::new(point.x, 0.0, point.y),
                Vec3::new(point.x, building.h, point.y),
            ]
        })
        .collect()
}

/// Fit real generated geometry, not a large generic padding cube. This keeps a
/// full-building view close enough to inspect its facade while retaining every
/// balcony, stair, sign, roof and antenna that the normal renderer will upload.
fn exterior_geometry_bounds(
    building: &crate::map::data::Bld,
    data: &crate::map::WorldData,
    kit: &crate::map::detail::WindowKit,
) -> Option<Vec<Vec3>> {
    let mut base = crate::map::build::building(building, crate::map::Lod::Near);
    let structure = crate::map::exterior::structure(building, data, crate::map::Lod::Near);
    let detail = crate::map::detail::near_detail(building, kit);
    let fittings = crate::map::detail::near_roof(building);
    if structure.vcount() == 0 || detail.vcount() < 100 || fittings.vcount() < 20 {
        return None;
    }
    // Match runtime material batching before computing AABBs. A combined roof
    // and projecting facade has corner combinations absent from separate AABBs;
    // fitting them separately cropped the first walkup fixture at lot854.
    base.structure = structure;
    base.merge_structure_into_roof();
    let mut bounds = exterior_bounds(building);
    for parts in [base.walls, base.roof, base.base, detail, fittings] {
        for mesh in parts.0.values() {
            let (min, max) = mesh.aabb();
            bounds.extend(box_corners(min, max));
        }
    }
    Some(bounds)
}

fn box_corners(min: Vec3, max: Vec3) -> [Vec3; 8] {
    [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, max.y, max.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(max.x, max.y, max.z),
    ]
}

/// Fit the entire real lot, including rooftop additions, with a useful margin.
/// This changes normal gameplay zoom, not the camera direction or LOD distance.
fn exterior_zoom(bounds: &[Vec3], position: Vec2, size: [u32; 2]) -> Option<f32> {
    let right = Vec3::new(1.0, 0.0, -1.0).normalize();
    let up = crate::camera::CAM_OFFSET.normalize().cross(right);
    let focus = position.extend(0.0).xzy();
    let (mut horizontal, mut vertical) = (0.0_f32, 0.0_f32);
    for point in bounds {
        horizontal = horizontal.max((*point - focus).dot(right).abs());
        vertical = vertical.max((*point - focus).dot(up).abs());
    }
    let aspect = size[0] as f32 / size[1] as f32;
    let half = crate::camera::half_height(aspect, 1.0);
    let zoom = (half * aspect * 0.89 / horizontal.max(0.1))
        .min(half * 0.89 / vertical.max(0.1))
        .min(*crate::camera::ZOOM_RANGE.end());
    (zoom >= *crate::camera::ZOOM_RANGE.start()).then_some(zoom)
}

fn exterior_camera_clear(data: &crate::map::WorldData, building: u32, target: Vec3) -> bool {
    let camera = Vec2::ONE.normalize();
    // A traversable player point alone does not prove the facade can be seen:
    // reject taller lots along the camera-to-subject ray, including roof clutter.
    (2..=32).step_by(2).all(|step| {
        let distance = step as f32;
        let p = target.xz() + camera * distance;
        let height = target.y + distance * crate::camera::CAM_OFFSET.y / 40.0_f32.hypot(40.0);
        data.bld_at(p)
            .is_none_or(|other| other.id == building || other.h + 4.0 < height)
            && !data.props.iter().any(|prop| {
                matches!(prop.kind, crate::map::street::PropKind::Tree(_))
                    && prop.p.distance(p) < 2.0
                    && height < 8.0
            })
    })
}

fn exterior_fixture(
    data: &crate::map::WorldData,
    scene: ExteriorScene,
    size: [u32; 2],
) -> Result<ExteriorFixture, String> {
    use crate::map::exterior::{Typology, classify, plan};

    if scene == ExteriorScene::Cutaway {
        // Use the same walkup as its full facade view, then actually put the
        // player inside its ground floor. The shared cutaway must hide every
        // architecture and rooftop mesh, including the newly added structures.
        let outside = exterior_fixture(data, ExteriorScene::Walkup, [1280, 720])?;
        let b = &data.blds[outside.building as usize];
        let mut candidates = vec![b.c];
        for x in 1..10 {
            for z in 1..10 {
                candidates.push(b.min + (b.max - b.min) * Vec2::new(x as f32, z as f32) / 10.0);
            }
        }
        candidates.sort_by(|a, b0| a.distance_squared(b.c).total_cmp(&b0.distance_squared(b.c)));
        let position = candidates
            .into_iter()
            .find(|p| data.bld_at(*p).is_some_and(|inside| inside.id == b.id) && data.free(*p, 0.6))
            .ok_or_else(|| format!("Exterior cutaway lot {} has no clear interior", b.id))?;
        return Ok(ExteriorFixture {
            building: b.id,
            position,
            zoom: 1.2,
            scene,
        });
    }

    let camera = Vec2::ONE.normalize();
    let mut candidates = Vec::new();
    for b in &data.blds {
        let typology = classify(b);
        let matches = match scene {
            ExteriorScene::Townhouse => typology == Typology::Townhouse,
            ExteriorScene::Walkup | ExteriorScene::ServiceStair => typology == Typology::Walkup,
            ExteriorScene::Shophouse => typology == Typology::Shophouse,
            ExteriorScene::Rooftop => b.shack && typology != Typology::Other,
            ExteriorScene::Cutaway => unreachable!(),
        };
        if !matches
            || b.h > 17.0
            || (b.max - b.min).max_element() > 32.0
            || (scene == ExteriorScene::Walkup && b.lv < 3)
        {
            continue;
        }
        if scene == ExteriorScene::Rooftop {
            let roof = crate::map::roofs::layout(b);
            if roof.shacks.is_empty() || roof.access.is_none() {
                continue;
            }
        }
        let layout = plan(b, data);
        let (centre, normal) = if scene == ExteriorScene::ServiceStair {
            let Some(stair) = layout.stair else { continue };
            (stair.centre, stair.n)
        } else {
            let Some(front) = layout.spec.front else {
                continue;
            };
            if scene != ExteriorScene::Rooftop && layout.balconies.is_empty() {
                continue;
            }
            (front.centre(), front.n)
        };
        if normal.dot(camera) < 0.55
            || !exterior_camera_clear(data, b.id, Vec3::new(centre.x, b.h * 0.55, centre.y))
        {
            continue;
        }
        for distance in [3.5, 5.5, 8.0, 10.0] {
            let position = centre + normal * distance;
            if !position.cmpge(data.min + 1.0).all()
                || !position.cmple(data.max - 1.0).all()
                || data.bld_at(position).is_some()
                || !data.free(position, 0.6)
                || b.c.distance(position) >= 65.0
                || crate::map::exterior_blocks_camera(b, position)
            {
                continue;
            }
            let Some(zoom) = exterior_zoom(&exterior_bounds(b), position, size) else {
                continue;
            };
            candidates.push(ExteriorFixture {
                building: b.id,
                position,
                zoom,
                scene,
            });
            break;
        }
    }
    // Whole-lot readability comes before proximity to spawn. The closest infill
    // lot previously forced a wide, crowded view and could hide its own rear stair.
    candidates.sort_by(|a, b| {
        b.zoom
            .total_cmp(&a.zoom)
            .then_with(|| {
                data.blds[a.building as usize]
                    .fill
                    .cmp(&data.blds[b.building as usize].fill)
            })
            .then_with(|| {
                a.position
                    .distance_squared(data.spawn)
                    .total_cmp(&b.position.distance_squared(data.spawn))
            })
            .then(a.building.cmp(&b.building))
    });
    let kit = crate::map::detail::WindowKit::new();
    let mut fitted = Vec::new();
    for mut fixture in candidates.into_iter().take(24) {
        let b = &data.blds[fixture.building as usize];
        let Some(bounds) = exterior_geometry_bounds(b, data, &kit) else {
            continue;
        };
        let Some(zoom) = exterior_zoom(&bounds, fixture.position, size) else {
            continue;
        };
        fixture.zoom = zoom;
        fitted.push(fixture);
    }
    fitted
        .into_iter()
        .max_by(|a, b| {
            a.zoom
                .total_cmp(&b.zoom)
                .then_with(|| {
                    data.blds[b.building as usize]
                        .fill
                        .cmp(&data.blds[a.building as usize].fill)
                })
                .then_with(|| {
                    b.position
                        .distance_squared(data.spawn)
                        .total_cmp(&a.position.distance_squared(data.spawn))
                })
                .then(b.building.cmp(&a.building))
        })
        .ok_or_else(|| format!("No clear, fully framed real-world {scene:?} exterior fixture"))
}

fn exterior_ready(world: &mut World, fixture: ExteriorFixture) -> bool {
    let id = fixture.building;
    world
        .query::<&crate::map::DetailRoot>()
        .iter(world)
        .any(|root| root.0 == id)
        && world
            .query::<&crate::map::BldWalls>()
            .iter(world)
            .any(|walls| walls.id == id && walls.faded == (fixture.scene == ExteriorScene::Cutaway))
        && exterior_visibility(world, fixture).is_some()
}

fn main_camera_meshes(world: &mut World) -> Option<HashSet<Entity>> {
    world
        .query_filtered::<&bevy::camera::visibility::VisibleEntities, With<GameCamera>>()
        .iter(world)
        .next()
        .map(|visible| visible.iter(TypeId::of::<Mesh3d>()).copied().collect())
}

/// One entity belongs to exactly one evidence group. Persistent shells carry the
/// roof tag for cutaway plus BldStructure for ownership; they never count twice.
fn exterior_group(
    id: u32,
    shell: Option<&crate::map::BldStructure>,
    detail: Option<&crate::map::BldDetail>,
    roof: Option<&crate::map::BldRoof>,
) -> Result<Option<usize>, &'static str> {
    if !shell.is_some_and(|tag| tag.0 == id)
        && !detail.is_some_and(|tag| tag.0 == id)
        && !roof.is_some_and(|tag| tag.0 == id)
    {
        return Ok(None);
    }
    match (shell, detail, roof) {
        (Some(shell), None, Some(roof)) if shell.0 == id && roof.0 == id => Ok(Some(0)),
        (None, Some(detail), None) if detail.0 == id => Ok(Some(1)),
        (None, None, Some(roof)) if roof.0 == id => Ok(Some(2)),
        _ => Err("conflicting exterior ownership/cutaway tags"),
    }
}

/// Wait for propagated visibility and membership of the actual game-camera list.
/// All three groups must exist; the combined shell cannot stand in for fine detail.
fn exterior_visibility(world: &mut World, fixture: ExteriorFixture) -> Option<[usize; 3]> {
    let main_view = main_camera_meshes(world)?;
    let hidden = fixture.scene == ExteriorScene::Cutaway;
    let (mut total, mut visible) = ([0_usize; 3], [0_usize; 3]);
    for (entity, shell, detail, roof, local, inherited, view) in world
        .query_filtered::<(
            Entity,
            Option<&crate::map::BldStructure>,
            Option<&crate::map::BldDetail>,
            Option<&crate::map::BldRoof>,
            &Visibility,
            &InheritedVisibility,
            &ViewVisibility,
        ), Or<(With<crate::map::BldDetail>, With<crate::map::BldRoof>)>>()
        .iter(world)
    {
        let Some(group) = exterior_group(fixture.building, shell, detail, roof).ok()? else {
            continue;
        };
        let in_main_view = main_view.contains(&entity);
        if hidden {
            if *local != Visibility::Hidden || inherited.get() || view.get() || in_main_view {
                return None;
            }
        } else if *local == Visibility::Hidden || !inherited.get() || !view.get() {
            return None;
        }
        total[group] += 1;
        visible[group] += usize::from(in_main_view);
    }
    (total.iter().all(|count| *count > 0) && (hidden || visible.iter().all(|count| *count > 0)))
        .then_some(visible)
}

/// One bounded diagnostic at most every five seconds while a fixture is waiting.
/// Keep the exact reason in the log instead of silently exhausting the QA deadline.
fn exterior_wait_status(world: &mut World, fixture: ExteriorFixture) -> String {
    let id = fixture.building;
    let main_view = main_camera_meshes(world).unwrap_or_default();
    let (mut total, mut local_hidden, mut inherited, mut view, mut main) =
        ([0; 3], [0; 3], [0; 3], [0; 3], [0; 3]);
    let mut points = Vec::new();
    let mut invalid_tags = 0;
    for (
        entity,
        shell,
        detail,
        roof,
        local,
        inherited_visibility,
        view_visibility,
        geometry,
        transform,
    ) in world
        .query_filtered::<(
            Entity,
            Option<&crate::map::BldStructure>,
            Option<&crate::map::BldDetail>,
            Option<&crate::map::BldRoof>,
            &Visibility,
            &InheritedVisibility,
            &ViewVisibility,
            &crate::map::MapMeshGeometry,
            &GlobalTransform,
        ), Or<(With<crate::map::BldDetail>, With<crate::map::BldRoof>)>>()
        .iter(world)
    {
        let group = match exterior_group(id, shell, detail, roof) {
            Ok(Some(group)) => group,
            Ok(None) => continue,
            Err(_) => {
                invalid_tags += 1;
                continue;
            }
        };
        total[group] += 1;
        local_hidden[group] += usize::from(*local == Visibility::Hidden);
        inherited[group] += usize::from(inherited_visibility.get());
        view[group] += usize::from(view_visibility.get());
        main[group] += usize::from(main_view.contains(&entity));
        points.extend(
            box_corners(geometry.min, geometry.max).map(|point| transform.transform_point(point)),
        );
    }
    let walls: Vec<_> = world
        .query::<&crate::map::BldWalls>()
        .iter(world)
        .filter(|walls| walls.id == id)
        .map(|walls| walls.faded)
        .collect();
    let player = player_position(world).ok();
    let blocks = player.map(|player| {
        crate::map::exterior_blocks_camera(&crate::map::world().blds[id as usize], player.xz())
    });
    let bounds = world
        .query_filtered::<(&Camera, &GlobalTransform), With<GameCamera>>()
        .iter(world)
        .next()
        .map(|(camera, transform)| {
            let projected: Vec<_> = points
                .into_iter()
                .filter_map(|point| camera.world_to_viewport(transform, point).ok())
                .collect();
            if projected.is_empty() {
                return "none".to_string();
            }
            let (lo, hi) = projected.into_iter().fold(
                (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
                |(lo, hi), point| (lo.min(point), hi.max(point)),
            );
            format!("{lo:?}..{hi:?}")
        })
        .unwrap_or_else(|| "no-camera".to_string());
    format!(
        "lot={id} cutaway_front={blocks:?} walls_faded={walls:?} groups(shell/facade/fittings)={total:?} hidden={local_hidden:?} inherited={inherited:?} view={view:?} main_view={main:?} invalid_tags={invalid_tags} projected={bounds}"
    )
}

fn map_workload(world: &mut World) -> Option<MapWorkload> {
    let visible = main_camera_meshes(world)?;
    let mut workload = MapWorkload {
        measurement: "uploaded_map_mesh_counts_not_gpu_time_or_device_performance",
        resident_meshes: 0,
        resident_vertices: 0,
        main_view_meshes: 0,
        main_view_vertices: 0,
    };
    for (entity, geometry) in world
        .query_filtered::<(Entity, &crate::map::MapMeshGeometry), With<Mesh3d>>()
        .iter(world)
    {
        workload.resident_meshes += 1;
        workload.resident_vertices += geometry.vertices;
        if visible.contains(&entity) {
            workload.main_view_meshes += 1;
            workload.main_view_vertices += geometry.vertices;
        }
    }
    Some(workload)
}

fn exterior_evidence(
    world: &mut World,
    fixture: ExteriorFixture,
    size: [u32; 2],
) -> Result<ExteriorEvidence, String> {
    let id = fixture.building;
    let b = &crate::map::world().blds[id as usize];
    let layout = crate::map::exterior::plan(b, crate::map::world());
    let roof_layout = crate::map::roofs::layout(b);
    let hidden = fixture.scene == ExteriorScene::Cutaway;
    let visible = exterior_visibility(world, fixture).ok_or_else(|| {
        format!("Exterior {id}: hierarchy or game-camera visibility changed before capture")
    })?;
    let mut groups = [ExteriorMeshGroup::default(); 3];
    let mut uploaded_bounds = Vec::new();
    let mut handles = Vec::new();
    for (shell, detail, roof, mesh, geometry, transform) in world
        .query_filtered::<(
            Option<&crate::map::BldStructure>,
            Option<&crate::map::BldDetail>,
            Option<&crate::map::BldRoof>,
            &Mesh3d,
            &crate::map::MapMeshGeometry,
            &GlobalTransform,
        ), Or<(With<crate::map::BldDetail>, With<crate::map::BldRoof>)>>()
        .iter(world)
    {
        let Some(group) = exterior_group(id, shell, detail, roof)? else {
            continue;
        };
        groups[group].meshes += 1;
        groups[group].vertices += geometry.vertices;
        handles.push(mesh.0.clone());
        uploaded_bounds.extend(
            box_corners(geometry.min, geometry.max).map(|point| transform.transform_point(point)),
        );
    }
    for (group, visible) in groups.iter_mut().zip(visible) {
        group.in_main_view = visible;
    }
    let meshes = world.resource::<Assets<Mesh>>();
    if handles.iter().any(|handle| !meshes.contains(handle)) {
        return Err(format!("Exterior {id}: missing uploaded mesh"));
    }
    if groups
        .iter()
        .any(|group| group.meshes == 0 || group.vertices < 20)
    {
        return Err(format!(
            "Exterior {id}: missing persistent shell, facade detail or roof fittings"
        ));
    }
    let position = player_position(world)?;
    let (camera, transform) = world
        .query_filtered::<(&Camera, &GlobalTransform), With<GameCamera>>()
        .iter(world)
        .next()
        .ok_or("Exterior camera is missing")?;
    let camera_position = transform.translation();
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for point in exterior_bounds(b).into_iter().chain(uploaded_bounds) {
        let projected = camera
            .world_to_viewport(transform, point)
            .map_err(|err| format!("Exterior {id}: cannot project target: {err:?}"))?;
        lo = lo.min(projected);
        hi = hi.max(projected);
    }
    if !hidden
        && (lo.x < size[0] as f32 * 0.04
            || lo.y < size[1] as f32 * 0.04
            || hi.x > size[0] as f32 * 0.96
            || hi.y > size[1] as f32 * 0.96)
    {
        return Err(format!(
            "Exterior {id}: full building is cropped: {lo:?}..{hi:?} in {size:?}"
        ));
    }
    if hidden
        && (crate::map::world()
            .bld_at(position.xz())
            .is_none_or(|inside| inside.id != id)
            || !world
                .query::<(&crate::map::BldWalls, &Transform)>()
                .iter(world)
                .any(|(walls, transform)| {
                    walls.id == id && walls.faded && (transform.scale.y - 0.12).abs() < 0.001
                }))
    {
        return Err(format!(
            "Exterior {id}: cutaway did not follow actual interior player position"
        ));
    }
    Ok(ExteriorEvidence {
        building_id: id,
        typology: format!("{:?}", layout.spec.typology),
        scene: format!("{:?}", fixture.scene),
        player: position.to_array(),
        camera: camera_position.to_array(),
        zoom: world.resource::<CameraZoom>().0,
        projected_bounds: [lo.to_array(), hi.to_array()],
        balconies: layout.balconies.len(),
        service_stair: layout.stair.is_some(),
        rooftop_shacks: roof_layout.shacks.len(),
        roof_access: roof_layout.access.is_some(),
        persistent_shell: groups[0],
        facade_detail: groups[1],
        roof_fittings: groups[2],
        total_meshes: groups.iter().map(|group| group.meshes).sum(),
        total_vertices: groups.iter().map(|group| group.vertices).sum(),
        cutaway_hidden: hidden,
        streaming: world.resource::<crate::map::MapRenderStats>().clone(),
    })
}

/// Focus the actual commercial assembly rather than zooming out to an entire
/// tall roof. The camera offset, projection and gameplay zoom limits stay normal.
fn commercial_zoom(bounds: &[Vec3], position: Vec2, size: [u32; 2]) -> Option<f32> {
    let right = Vec3::new(1.0, 0.0, -1.0).normalize();
    let up = crate::camera::CAM_OFFSET.normalize().cross(right);
    let focus = position.extend(0.0).xzy();
    let (mut horizontal, mut vertical) = (0.0_f32, 0.0_f32);
    for point in bounds {
        horizontal = horizontal.max((*point - focus).dot(right).abs());
        vertical = vertical.max((*point - focus).dot(up).abs());
    }
    let aspect = size[0] as f32 / size[1] as f32;
    let half = crate::camera::half_height(aspect, 1.0);
    // Leave room for the ordinary HUD while keeping the storefront legible.
    let zoom = (half * aspect * 0.76 / horizontal.max(0.1))
        .min(half * 0.68 / vertical.max(0.1))
        .min(*crate::camera::ZOOM_RANGE.end());
    (zoom >= *crate::camera::ZOOM_RANGE.start()).then_some(zoom)
}

fn commercial_fixture(
    data: &crate::map::WorldData,
    scene: CommercialScene,
    size: [u32; 2],
) -> Result<CommercialFixture, String> {
    use crate::map::commercial::{ShopFamily, geometry, plan};

    // This canonical selector makes all three lighting states use the identical
    // lot and ordinary camera. It is independent of mutable game/weather state.
    let kind = match scene {
        CommercialScene::Night | CommercialScene::Blackout => CommercialScene::Food,
        other => other,
    };
    let mut candidates = Vec::new();
    for b in &data.blds {
        let Some(layout) = plan(b, data) else {
            continue;
        };
        let eligible = match kind {
            CommercialScene::Arcade => {
                b.arcade.is_some()
                    && layout.panels.len() >= 2
                    && layout.secondary_fascia
                    && (8.0..=16.0).contains(&layout.frontage.width)
                    && b.h <= 12.0
            }
            CommercialScene::Food => {
                b.arcade.is_none()
                    && layout.family == ShopFamily::Food
                    && layout.canopy.is_some()
                    && layout.blade.is_some()
                    // Both the fascia and the blade's printed side must face
                    // the fixed camera; a front-on blade would be edge-on.
                    && layout.frontage.u.dot(Vec2::ONE.normalize()).abs() >= 0.35
                    && !layout.panels.is_empty()
            }
            CommercialScene::Essentials => {
                matches!(
                    layout.family,
                    ShopFamily::Pharmacy | ShopFamily::Convenience
                ) && !layout.panels.is_empty()
            }
            _ => unreachable!(),
        };
        let front = layout.frontage;
        if !eligible
            || front.n.dot(Vec2::ONE.normalize()) < 0.6
            || front.width > 24.0
            || !exterior_camera_clear(
                data,
                b.id,
                Vec3::new(front.centre().x, 2.4, front.centre().y),
            )
        {
            continue;
        }
        let generated = geometry(b, &layout, crate::map::Lod::Near);
        let bounds: Vec<Vec3> = generated
            .base
            .0
            .values()
            .chain(generated.shell.0.values())
            .flat_map(|mesh| mesh.pos.iter().copied().map(Vec3::from))
            .collect();
        if bounds.len() < 40 {
            continue;
        }
        for distance in [2.0, 3.0, 4.5, 6.0] {
            let position = front.centre() + front.n * distance;
            if !position.cmpge(data.min + 1.0).all()
                || !position.cmple(data.max - 1.0).all()
                || data.bld_at(position).is_some()
                || !data.free(position, 0.6)
                || b.c.distance(position) >= 65.0
                || crate::map::exterior_blocks_camera(b, position)
            {
                continue;
            }
            let Some(zoom) = commercial_zoom(&bounds, position, size) else {
                continue;
            };
            candidates.push(CommercialFixture {
                building: b.id,
                position,
                zoom,
                scene,
            });
            break;
        }
    }
    candidates
        .into_iter()
        .max_by(|a, b| {
            a.zoom
                .total_cmp(&b.zoom)
                .then_with(|| {
                    b.position
                        .distance_squared(data.spawn)
                        .total_cmp(&a.position.distance_squared(data.spawn))
                })
                .then(b.building.cmp(&a.building))
        })
        .ok_or_else(|| {
            format!("No visible, collision-clear real-world {scene:?} commercial fixture")
        })
}

fn commercial_group(
    id: u32,
    base: Option<&crate::map::BldBase>,
    shell: Option<&crate::map::BldStructure>,
) -> Option<usize> {
    match (base, shell) {
        (Some(base), None) if base.0 == id => Some(0),
        (None, Some(shell)) if shell.0 == id => Some(1),
        _ => None,
    }
}

fn commercial_ready(world: &mut World, fixture: CommercialFixture) -> bool {
    let id = fixture.building;
    if !world
        .query::<&crate::map::DetailRoot>()
        .iter(world)
        .any(|root| root.0 == id)
        || !world
            .query::<&crate::map::BldWalls>()
            .iter(world)
            .any(|walls| walls.id == id && !walls.faded)
    {
        return false;
    }
    let Some(main_view) = main_camera_meshes(world) else {
        return false;
    };
    let mut visible = [0_usize; 2];
    let mut sign_visible = false;
    for (entity, base, shell, material, local, inherited, view) in world
        .query_filtered::<(
            Entity,
            Option<&crate::map::BldBase>,
            Option<&crate::map::BldStructure>,
            &crate::map::MapMeshMaterial,
            &Visibility,
            &InheritedVisibility,
            &ViewVisibility,
        ), Or<(With<crate::map::BldBase>, With<crate::map::BldStructure>)>>()
        .iter(world)
    {
        let Some(group) = commercial_group(id, base, shell) else {
            continue;
        };
        if *local == Visibility::Hidden || !inherited.get() {
            return false;
        }
        if view.get() && main_view.contains(&entity) {
            visible[group] += 1;
            sign_visible |= material.0 == crate::map::build::Mat::Sign;
        }
    }
    visible.into_iter().all(|count| count > 0) && sign_visible
}

/// Explain missing base/sign batches and visibility while asynchronous chunks
/// settle; a blocked fixture must not silently consume the full QA deadline.
fn commercial_wait_status(world: &mut World, fixture: CommercialFixture) -> String {
    let id = fixture.building;
    let main_view = main_camera_meshes(world).unwrap_or_default();
    let (mut total, mut hidden, mut inherited, mut view, mut main) = (
        [0_usize; 2],
        [0_usize; 2],
        [0_usize; 2],
        [0_usize; 2],
        [0_usize; 2],
    );
    let mut signs = Vec::new();
    for (entity, base, shell, material, local, inherited_visibility, view_visibility) in world
        .query_filtered::<(
            Entity,
            Option<&crate::map::BldBase>,
            Option<&crate::map::BldStructure>,
            &crate::map::MapMeshMaterial,
            &Visibility,
            &InheritedVisibility,
            &ViewVisibility,
        ), Or<(With<crate::map::BldBase>, With<crate::map::BldStructure>)>>()
        .iter(world)
    {
        let Some(group) = commercial_group(id, base, shell) else {
            continue;
        };
        total[group] += 1;
        hidden[group] += usize::from(*local == Visibility::Hidden);
        inherited[group] += usize::from(inherited_visibility.get());
        view[group] += usize::from(view_visibility.get());
        main[group] += usize::from(main_view.contains(&entity));
        if material.0 == crate::map::build::Mat::Sign {
            signs.push((group, view_visibility.get(), main_view.contains(&entity)));
        }
    }
    let walls: Vec<_> = world
        .query::<&crate::map::BldWalls>()
        .iter(world)
        .filter(|wall| wall.id == id)
        .map(|wall| wall.faded)
        .collect();
    let detail = world
        .query::<&crate::map::DetailRoot>()
        .iter(world)
        .any(|root| root.0 == id);
    let position = player_position(world).ok();
    let blocks = position.map(|p| {
        crate::map::exterior_blocks_camera(&crate::map::world().blds[id as usize], p.xz())
    });
    format!(
        "commercial lot={id} player={position:?} cutaway_front={blocks:?} walls_faded={walls:?} detail_root={detail} groups(base/shell)={total:?} hidden={hidden:?} inherited={inherited:?} view={view:?} main_view={main:?} sign(group/view/main)={signs:?}"
    )
}

fn commercial_evidence(
    world: &mut World,
    fixture: CommercialFixture,
    size: [u32; 2],
) -> Result<CommercialEvidence, String> {
    use crate::map::build::{Lod, Mat};

    let id = fixture.building;
    let data = crate::map::world();
    let b = &data.blds[id as usize];
    let layout = crate::map::commercial::plan(b, data)
        .ok_or_else(|| format!("Commercial {id}: plan disappeared"))?;
    let generated = crate::map::commercial::geometry(b, &layout, Lod::Near);
    let features = [generated.base, generated.shell];
    let commercial_vertices = features.iter().map(|parts| parts.vcount()).sum();
    if commercial_vertices < 40 || layout.panels.is_empty() {
        return Err(format!("Commercial {id}: empty promised storefront"));
    }
    // Recreate exactly the runtime material batching. Comparing the full batch
    // counts and bounds catches omitted/misrouted storefront uploads without
    // allocating separate per-shop materials or pretending an AABB is a draw.
    let mut all = crate::map::build::building(b, Lod::Near);
    all.structure = crate::map::exterior::structure(b, data, Lod::Near);
    all.merge_structure_into_roof();
    let expected = [all.base, all.roof];
    let main_view =
        main_camera_meshes(world).ok_or("Commercial camera has no visible mesh list")?;
    let mut groups = [ExteriorMeshGroup::default(); 2];
    let mut found = [HashSet::new(), HashSet::new()];
    let mut handles = Vec::new();
    for (entity, base, shell, material, mesh, uploaded, transform, local, inherited, view) in world
        .query_filtered::<(
            Entity,
            Option<&crate::map::BldBase>,
            Option<&crate::map::BldStructure>,
            &crate::map::MapMeshMaterial,
            &Mesh3d,
            &crate::map::MapMeshGeometry,
            &GlobalTransform,
            &Visibility,
            &InheritedVisibility,
            &ViewVisibility,
        ), Or<(With<crate::map::BldBase>, With<crate::map::BldStructure>)>>()
        .iter(world)
    {
        let Some(group) = commercial_group(id, base, shell) else {
            continue;
        };
        let key = material.0;
        let planned = expected[group]
            .0
            .get(&key)
            .ok_or_else(|| format!("Commercial {id}: unexpected {key:?} batch in group {group}"))?;
        let (min, max) = planned.aabb();
        if !found[group].insert(key)
            || uploaded.vertices != planned.vcount()
            || uploaded.min.distance(min) > 0.001
            || uploaded.max.distance(max) > 0.001
            || transform.translation().length() > 0.001
            || *local == Visibility::Hidden
            || !inherited.get()
        {
            return Err(format!(
                "Commercial {id}: actual {key:?} upload differs from generated batch"
            ));
        }
        let visible = view.get() && main_view.contains(&entity);
        if features[group].0.contains_key(&key) && !visible {
            return Err(format!(
                "Commercial {id}: {key:?} storefront material absent from main view"
            ));
        }
        groups[group].meshes += 1;
        groups[group].vertices += uploaded.vertices;
        groups[group].in_main_view += usize::from(visible);
        handles.push(mesh.0.clone());
    }
    for (group, (full, feature)) in expected.iter().zip(&features).enumerate() {
        let expected_materials: HashSet<_> = full
            .0
            .iter()
            .filter_map(|(material, mesh)| (!mesh.is_empty()).then_some(*material))
            .collect();
        if found[group] != expected_materials {
            return Err(format!(
                "Commercial {id}: incomplete uploaded group {group}"
            ));
        }
        for (material, mesh) in &feature.0 {
            let full = full.0.get(material).ok_or_else(|| {
                format!("Commercial {id}: {material:?} commercial part not integrated")
            })?;
            // Every feature vertex must survive the material merge. Checking this
            // against the runtime builder detects a valid but unused planner.
            let key = |p: [f32; 3]| p.map(|value| (value * 1000.0).round() as i32);
            let vertices: HashSet<_> = full.pos.iter().copied().map(key).collect();
            if mesh
                .pos
                .iter()
                .copied()
                .any(|point| !vertices.contains(&key(point)))
            {
                return Err(format!(
                    "Commercial {id}: feature geometry missing from {material:?} runtime batch"
                ));
            }
        }
    }
    if handles
        .iter()
        .any(|handle| !world.resource::<Assets<Mesh>>().contains(handle))
    {
        return Err(format!("Commercial {id}: referenced mesh asset is missing"));
    }
    if !features[1].0.contains_key(&Mat::Sign) {
        return Err(format!(
            "Commercial {id}: original typed sign texture is missing"
        ));
    }
    let position = player_position(world)?;
    if position.xz().distance(fixture.position) > 0.001
        || data.bld_at(position.xz()).is_some()
        || !data.free(position.xz(), 0.6)
        || crate::map::exterior_blocks_camera(b, position.xz())
    {
        return Err(format!(
            "Commercial {id}: storefront camera is inside/blocked or moved"
        ));
    }
    let (camera, transform) = world
        .query_filtered::<(&Camera, &GlobalTransform), With<GameCamera>>()
        .iter(world)
        .next()
        .ok_or("Commercial camera is missing")?;
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    for point in features
        .iter()
        .flat_map(|parts| parts.0.values())
        .flat_map(|mesh| mesh.pos.iter().copied().map(Vec3::from))
    {
        let point = camera
            .world_to_viewport(transform, point)
            .map_err(|err| format!("Commercial {id}: cannot project feature: {err:?}"))?;
        lo = lo.min(point);
        hi = hi.max(point);
    }
    if lo.x < size[0] as f32 * 0.08
        || lo.y < size[1] as f32 * 0.08
        || hi.x > size[0] as f32 * 0.92
        || hi.y > size[1] as f32 * 0.92
        || (hi - lo).min_element() < 35.0
    {
        return Err(format!(
            "Commercial {id}: cropped or unreadably small storefront {lo:?}..{hi:?}"
        ));
    }
    let camera_position = transform.translation().to_array();
    let atmosphere = world.resource::<crate::map::atmosphere::AtmosphereState>();
    let signs = &world.resource::<crate::map::textures::MapMaterials>().signs;
    let emissive = world
        .resource::<Assets<StandardMaterial>>()
        .get(signs)
        .ok_or("Commercial sign material is missing")?
        .emissive;
    let sign_emissive = [emissive.red, emissive.green, emissive.blue];
    let lighting_ok = match fixture.scene {
        CommercialScene::Night => {
            atmosphere.night > 0.9
                && atmosphere.power
                && atmosphere.active_lights > 0
                && atmosphere.active_lights <= 6
                && sign_emissive.iter().all(|value| *value > 0.8)
        }
        CommercialScene::Blackout => {
            atmosphere.night > 0.9
                && !atmosphere.power
                && atmosphere.active_lights == 0
                && sign_emissive.iter().all(|value| *value == 0.0)
        }
        _ => atmosphere.night < 0.1 && atmosphere.power,
    };
    if !lighting_ok {
        return Err(format!(
            "Commercial {id}: lighting/power state mismatch: {atmosphere:?} emissive={sign_emissive:?}"
        ));
    }
    Ok(CommercialEvidence {
        visibility_measurement: "uploaded_batches_and_main_view_membership_not_pixel_occlusion",
        building_id: id,
        family: format!("{:?}", layout.family),
        sign_index: layout.sign_index,
        scene: format!("{:?}", fixture.scene),
        arcade: b.arcade.is_some(),
        frontage_width: layout.frontage.width,
        building_height: b.h,
        storefront_panels: layout.panels.len(),
        secondary_fascia: layout.secondary_fascia,
        canopy: layout.canopy.is_some(),
        blade: layout.blade.is_some(),
        player: position.to_array(),
        camera: camera_position,
        zoom: world.resource::<CameraZoom>().0,
        projected_bounds: [lo.to_array(), hi.to_array()],
        commercial_vertices,
        base_batches: groups[0],
        persistent_shell_batches: groups[1],
        night: atmosphere.night,
        power: atmosphere.power,
        active_lights: atmosphere.active_lights,
        sign_emissive,
        streaming: world.resource::<crate::map::MapRenderStats>().clone(),
    })
}

fn lighting_fixture(
    data: &crate::map::WorldData,
    scene: LightingScene,
    size: [u32; 2],
) -> Result<LightingFixture, String> {
    use crate::map::atmosphere::{CITY_LIGHT_COUNT, LightKind, LightSiteIndex, plan_city_lights};
    use crate::map::commercial::{geometry, plan};
    let kit = crate::map::detail::WindowKit::new();
    let sites = plan_city_lights(data);
    let index = LightSiteIndex::new(&sites);
    let mut candidates = Vec::new();
    let mut rejected = [0_usize; 5];
    // The old compact-shop showcase has only two admissible arcades: one has
    // no originally occupied windows and the other has no nearby street lamp.
    // Lighting uses its own real-world selector, without changing either shop.
    for building in &data.blds {
        if building.arcade.is_none() || building.sign.is_none() {
            continue;
        }
        let Some(layout) = plan(building, data) else {
            continue;
        };
        let front = layout.frontage;
        if front.n.dot(Vec2::ONE.normalize()) < 0.6
            || front.width > 24.0
            || !exterior_camera_clear(
                data,
                building.id,
                Vec3::new(front.centre().x, 2.4, front.centre().y),
            )
        {
            rejected[0] += 1;
            continue;
        }
        let mut windows = crate::map::build::Parts::default();
        crate::map::detail::windows(building, &kit, &mut windows);
        let glow = windows.0.get(&crate::map::build::Mat::WindowGlow);
        let window_points = glow.map_or_else(Vec::new, |mesh| {
            mesh.pos
                .iter()
                .zip(&mesh.nrm)
                .filter(|(_, normal)| {
                    Vec3::from(**normal).dot(crate::camera::CAM_OFFSET.normalize()) > 0.2
                })
                .map(|(point, _)| Vec3::from(*point))
                .collect::<Vec<_>>()
        });
        if window_points.is_empty() {
            rejected[1] += 1;
            continue;
        }
        let lowest = window_points
            .iter()
            .map(|point| point.y)
            .fold(f32::MAX, f32::min);
        let generated = geometry(building, &layout, crate::map::Lod::Near);
        let mut bounds: Vec<Vec3> = generated
            .base
            .0
            .values()
            .chain(generated.shell.0.values())
            .flat_map(|mesh| mesh.pos.iter().copied().map(Vec3::from))
            .collect();
        // Frame the existing lower occupied room together with the shop and
        // ground receivers; the tall roof need not dictate the lighting camera.
        bounds.extend(
            window_points
                .into_iter()
                .filter(|point| point.y <= lowest + 1.5),
        );
        'positions: for distance in [2.0, 3.0, 4.5, 6.0, 7.5, 9.0] {
            for lateral in [0.0, -1.5, 1.5, -3.0, 3.0] {
                let position = front.centre() + front.n * distance + front.u * lateral;
                if !position.cmpge(data.min + 1.0).all()
                    || !position.cmple(data.max - 1.0).all()
                    || !street_player_position_clear(data, position)
                    || building.c.distance(position) >= 65.0
                    || crate::map::exterior_blocks_camera(building, position)
                {
                    rejected[2] += 1;
                    continue;
                }
                let selection = index.select(&sites, position, &[None; CITY_LIGHT_COUNT]);
                let selected: Vec<_> = selection
                    .into_iter()
                    .flatten()
                    .filter_map(|id| sites.iter().find(|site| site.id == id))
                    .collect();
                if ![LightKind::Street, LightKind::Sign, LightKind::Arcade]
                    .iter()
                    .all(|kind| selected.iter().any(|site| site.kind == *kind))
                {
                    rejected[3] += 1;
                    continue;
                }
                let mut framed = bounds.clone();
                for site in selected
                    .iter()
                    .filter(|site| site.kind != LightKind::Street)
                    .chain(
                        selected
                            .iter()
                            .filter(|site| site.kind == LightKind::Street)
                            .take(1),
                    )
                {
                    framed.push(site.receiver - Vec3::new(1.2, 0.0, 1.2));
                    framed.push(site.receiver + Vec3::new(1.2, 0.0, 1.2));
                }
                let Some(zoom) = commercial_zoom(&framed, position, size) else {
                    rejected[4] += 1;
                    continue;
                };
                candidates.push(LightingFixture {
                    building: building.id,
                    position,
                    zoom,
                    scene,
                });
                break 'positions;
            }
        }
    }
    candidates.into_iter().max_by(|a, b| a.zoom.total_cmp(&b.zoom)
        .then_with(|| b.position.distance_squared(data.spawn).total_cmp(&a.position.distance_squared(data.spawn)))
        .then(b.building.cmp(&a.building)))
        .ok_or_else(|| format!("No original lighting fixture; rejection counts frontage/window/vehicle/source/framing={rejected:?}"))
}

fn lighting_ready(world: &mut World, fixture: LightingFixture) -> bool {
    use crate::map::{BldDetail, BldRoof, MapMeshMaterial, build::Mat};
    if !commercial_ready(
        world,
        CommercialFixture {
            building: fixture.building,
            position: fixture.position,
            zoom: fixture.zoom,
            scene: CommercialScene::Arcade,
        },
    ) {
        return false;
    }
    let Some(main_view) = main_camera_meshes(world) else {
        return false;
    };
    let mut window = false;
    let mut arcade = false;
    for (entity, detail, roof, material, inherited, visibility) in world
        .query::<(
            Entity,
            Option<&BldDetail>,
            Option<&BldRoof>,
            &MapMeshMaterial,
            &InheritedVisibility,
            &ViewVisibility,
        )>()
        .iter(world)
    {
        if inherited.get() && visibility.get() && main_view.contains(&entity) {
            window |= detail.is_some_and(|owner| owner.0 == fixture.building)
                && material.0 == Mat::WindowGlow;
            arcade |= roof.is_some_and(|owner| owner.0 == fixture.building)
                && material.0 == Mat::ArcadeGlow;
        }
    }
    window && arcade
}

fn lighting_color(color: Color) -> [f32; 3] {
    let color = color.to_srgba();
    [color.red, color.green, color.blue]
}

/// Check the emitter against independently generated original fixture geometry.
/// An emissive material is evidence of the fixture, never evidence of spill.
fn lighting_source_bounds(
    site: &crate::map::atmosphere::LightSite,
) -> Result<[[f32; 3]; 2], String> {
    use crate::map::{atmosphere::LightSource, build::Mat, street::PropKind};
    let data = crate::map::world();
    let (parts, material, allowance) = match site.source {
        LightSource::Street { prop_index } => {
            let prop = data
                .props
                .get(prop_index)
                .ok_or("City light references missing prop")?;
            if prop.kind != PropKind::Lamp {
                return Err("City street source is not a street lamp".into());
            }
            (crate::map::street::prop_geometry(prop), Mat::Lamp, 0.22)
        }
        LightSource::Sign { building_id } => {
            let building = data
                .blds
                .get(building_id as usize)
                .ok_or("City light references missing sign building")?;
            if building.sign.is_none() {
                return Err("City sign source has no original sign".into());
            }
            let mut meshes = crate::map::build::building(building, crate::map::Lod::Near);
            meshes.roof.merge(meshes.base);
            meshes.roof.merge(meshes.structure);
            (meshes.roof, Mat::Sign, 0.65)
        }
        LightSource::Arcade {
            building_id,
            bay_index,
        } => {
            let building = data
                .blds
                .get(building_id as usize)
                .ok_or("City light references missing arcade building")?;
            let fixture = crate::map::atmosphere::arcade_lamp_position(building, bay_index)
                .ok_or("City arcade source references missing original bay")?;
            if site.position.distance(fixture) > 0.075 {
                return Err("City arcade emitter moved away from original bay fixture".into());
            }
            (
                crate::map::build::building(building, crate::map::Lod::Near).roof,
                Mat::ArcadeGlow,
                0.10,
            )
        }
    };
    let mesh = parts
        .0
        .get(&material)
        .ok_or_else(|| format!("City source has no original {material:?} geometry"))?;
    let (lo, hi) = mesh.pos.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(lo, hi), point| (lo.min(Vec3::from(*point)), hi.max(Vec3::from(*point))),
    );
    if mesh.is_empty()
        || !site.position.cmpge(lo - allowance).all()
        || !site.position.cmple(hi + allowance).all()
        || site.id != site.source.id()
        || site.kind != site.source.kind()
        || site.direction.y >= -0.7
        || (site.direction.length() - 1.0).abs() > 0.0001
        || site.position.distance(site.receiver) >= site.range
    {
        return Err(format!(
            "City source {} fails original geometry/direction/receiver bounds",
            site.id
        ));
    }
    Ok([lo.to_array(), hi.to_array()])
}

fn lighting_evidence(
    world: &mut World,
    fixture: LightingFixture,
    observations: &[Vec<Option<u64>>],
) -> Result<LightingEvidence, String> {
    use crate::map::atmosphere::{
        AtmosphereState, CITY_LIGHT_COUNT, CityLightSites, CitySpill, LightKind, lighting_profile,
    };
    let sites = world.resource::<CityLightSites>().sites.clone();
    let mut catalog_counts = [0; 3];
    for site in &sites {
        catalog_counts[match site.kind {
            LightKind::Street => 0,
            LightKind::Sign => 1,
            LightKind::Arcade => 2,
        }] += 1;
    }
    let power = fixture.scene != LightingScene::Blackout;
    let diagnostic = fixture.scene == LightingScene::NoSpillControl;
    let profile = lighting_profile(fixture.scene.minute(), power, 0.0, 0.0);
    let player = player_position(world)?;
    if player.xz().distance(fixture.position) > 0.001
        || !street_player_position_clear(crate::map::world(), player.xz())
    {
        return Err("Lighting player is moved or inside a parked vehicle/wall".into());
    }
    let camera = world
        .query_filtered::<&GlobalTransform, With<GameCamera>>()
        .iter(world)
        .next()
        .ok_or("Lighting camera is missing")?
        .translation()
        .to_array();
    let city_points = world
        .query_filtered::<&PointLight, With<CitySpill>>()
        .iter(world)
        .count();
    let unrelated_points = world
        .query_filtered::<&PointLight, Without<CitySpill>>()
        .iter(world)
        .count();
    let pool_entities = world.query::<&CitySpill>().iter(world).count();
    let total_spots = world.query::<&SpotLight>().iter(world).count();
    let total_points = world.query::<&PointLight>().iter(world).count();
    let main_view_lights: HashSet<Entity> = world
        .query_filtered::<&bevy::camera::visibility::VisibleEntities, With<GameCamera>>()
        .iter(world)
        .next()
        .map(|visible| {
            visible
                .iter(TypeId::of::<bevy::light::cluster::ClusterVisibilityClass>())
                .copied()
                .collect()
        })
        .unwrap_or_default();
    let mut slots = Vec::new();
    let mut identities = HashSet::new();
    for (entity, slot, light, transform, global, inherited, view) in world
        .query::<(
            Entity,
            &CitySpill,
            &SpotLight,
            &Transform,
            &GlobalTransform,
            &InheritedVisibility,
            &ViewVisibility,
        )>()
        .iter(world)
    {
        let actual_position = global.translation();
        let direction = transform.rotation * -Vec3::Z;
        let site = slot
            .site
            .map(|id| {
                sites.iter().find(|site| site.id == id).ok_or_else(|| {
                    format!("City slot {} references missing source {id}", slot.slot)
                })
            })
            .transpose()?;
        let mut bounds = [[0.0; 3]; 2];
        if let Some(site) = site {
            bounds = lighting_source_bounds(site)?;
            let expected = if diagnostic {
                0.0
            } else {
                site.intensity * profile.lamp_strength * slot.gain
            };
            if !identities.insert(site.id)
                || (site.kind == LightKind::Street) != (slot.slot < 4)
                || actual_position.distance(site.position) > 0.001
                || direction.distance(site.direction) > 0.0001
                || (light.range - site.range).abs() > 0.0001
                || (light.inner_angle - site.inner_angle).abs() > 0.0001
                || (light.outer_angle - site.outer_angle).abs() > 0.0001
                || lighting_color(light.color) != lighting_color(site.color)
                || (light.intensity - expected).abs() > site.intensity * 0.002 + 0.01
                || (profile.lamp_strength > 0.0
                    && site.position.xz().distance(player.xz()) > 36.001)
            {
                return Err(format!(
                    "City spotlight {} differs from its original source, slot class or real light parameters",
                    slot.slot
                ));
            }
        } else if light.intensity != 0.0 {
            return Err("Unassigned city spotlight is emitting".into());
        }
        if light.shadow_maps_enabled
            || light.contact_shadows_enabled
            || !light.intensity.is_finite()
            || light.intensity < 0.0
            || !(0.0..=1.0).contains(&slot.gain)
        {
            return Err("City spotlight exceeds the shadowless pool contract".into());
        }
        slots.push(LightingSlotEvidence {
            slot: slot.slot,
            source_id: slot.site,
            kind: site.map_or_else(|| "None".into(), |site| format!("{:?}", site.kind)),
            source: site.map_or_else(|| "None".into(), |site| format!("{:?}", site.source)),
            source_position: site.map_or([0.0; 3], |site| site.position.to_array()),
            source_direction: site.map_or([0.0; 3], |site| site.direction.to_array()),
            receiver: site.map_or([0.0; 3], |site| site.receiver.to_array()),
            source_geometry_bounds: bounds,
            source_geometry_verified: site.is_some(),
            actual_position: actual_position.to_array(),
            actual_direction: direction.to_array(),
            intensity: light.intensity,
            range: light.range,
            radius: light.radius,
            inner_angle: light.inner_angle,
            outer_angle: light.outer_angle,
            color: lighting_color(light.color),
            gain: slot.gain,
            shadow_maps: light.shadow_maps_enabled,
            contact_shadows: light.contact_shadows_enabled,
            inherited_visible: inherited.get(),
            view_visible: view.get(),
            in_main_view_cluster: main_view_lights.contains(&entity),
        });
    }
    slots.sort_by_key(|slot| slot.slot);
    let active = slots.iter().filter(|slot| slot.intensity > 0.0).count();
    let stable = observations.len() == 30 && observations.windows(2).all(|pair| pair[0] == pair[1]);
    if pool_entities != CITY_LIGHT_COUNT
        || total_spots != CITY_LIGHT_COUNT
        || total_points != 1
        || slots.len() != CITY_LIGHT_COUNT
        || city_points != 0
        || slots
            .iter()
            .enumerate()
            .any(|(index, slot)| slot.slot != index)
        || !stable
    {
        return Err(format!(
            "Lighting pool/stability mismatch: entities={pool_entities} spots={} points={city_points} stable={stable}",
            slots.len()
        ));
    }
    let atmosphere = world.resource::<AtmosphereState>();
    if atmosphere.power != power
        || (atmosphere.night - profile.night).abs() > 0.002
        || atmosphere.wetness != 0.0
        || atmosphere.active_lights != active
        || ((diagnostic || profile.lamp_strength == 0.0) && active != 0)
        || (!diagnostic && profile.lamp_strength > 0.0 && active == 0)
        || (profile.lamp_strength > 0.0
            && slots
                .iter()
                .any(|slot| slot.source_id.is_some() && slot.gain < 0.999))
    {
        return Err(format!(
            "Lighting actual pool disagrees with gameplay/profile: {atmosphere:?}, active={active}"
        ));
    }
    let night = atmosphere.night;
    let wetness = atmosphere.wetness;
    let city = world
        .resource::<crate::map::textures::MapMaterials>()
        .clone();
    let mut handles = vec![
        ("street_lamp", city.lamp),
        ("occupied_window", city.window_glow),
        ("arcade_fixture", city.arcade_glow),
        ("sign", city.signs),
        ("projecting_sign", city.vsigns),
        ("road", city.road),
        ("ground", city.ground),
        ("walk", city.walk),
        ("curb", city.curb),
    ];
    for handle in city.arcade_floor {
        handles.push(("arcade_floor", handle));
    }
    let material_state = handles
        .into_iter()
        .map(|(role, handle)| {
            let material = world
                .resource::<Assets<StandardMaterial>>()
                .get(&handle)
                .ok_or_else(|| format!("Lighting material {role} is absent"))?;
            let base = material.base_color.to_srgba();
            Ok(LightingMaterialEvidence {
                role,
                base_color: [base.red, base.green, base.blue, base.alpha],
                emissive: [
                    material.emissive.red,
                    material.emissive.green,
                    material.emissive.blue,
                ],
                roughness: material.perceptual_roughness,
                metallic: material.metallic,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let material_emissive = material_state
        .iter()
        .take(5)
        .map(|material| (material.role, material.emissive))
        .collect::<Vec<_>>();
    if material_emissive.iter().any(|(_, rgb)| {
        if profile.lamp_strength == 0.0 {
            rgb.iter().any(|value| *value != 0.0)
        } else {
            rgb.iter().any(|value| *value <= 0.0)
        }
    }) {
        return Err("Lighting emission does not follow power/daylight".into());
    }
    let sun = world
        .query::<&DirectionalLight>()
        .iter(world)
        .map(|light| (light.illuminance, lighting_color(light.color)))
        .collect::<Vec<_>>();
    let ambient = world.resource::<GlobalAmbientLight>();
    let expected_sun_color = [
        0.58 + 0.42 * profile.daylight,
        0.67 + 0.26 * profile.daylight,
        0.9 - 0.1 * profile.daylight,
    ];
    let expected_ambient_color = [
        0.55 + 0.26 * profile.daylight,
        0.65 + 0.24 * profile.daylight,
        0.84 + 0.16 * profile.daylight,
    ];
    if (ambient.brightness - profile.ambient).abs() > 1.0
        || sun.len() != 1
        || sun.iter().any(|(illuminance, color)| {
            (illuminance - profile.sun).abs() > 24.0
                || color
                    .iter()
                    .zip(expected_sun_color)
                    .any(|(actual, expected)| (actual - expected).abs() > 0.002)
        })
        || lighting_color(ambient.color)
            .iter()
            .zip(expected_ambient_color)
            .any(|(actual, expected)| (actual - expected).abs() > 0.002)
    {
        return Err("Lighting actual ambient/sun does not follow the unchanged curve".into());
    }
    Ok(LightingEvidence {
        measurement: "actual_shadowless_spot_components_source_geometry_and_paired_gpu_pixels_not_full_occlusion",
        scene: format!("{:?}", fixture.scene),
        diagnostic_direct_light_override: diagnostic,
        building_id: fixture.building,
        player: player.to_array(),
        camera,
        zoom: world.resource::<CameraZoom>().0,
        minute: fixture.scene.minute(),
        night,
        power,
        wetness,
        ambient: ambient.brightness,
        ambient_color: lighting_color(ambient.color),
        sun_illuminance: sun.iter().map(|value| value.0).collect(),
        sun_color: sun.iter().map(|value| value.1).collect(),
        material_emissive,
        material_state,
        actual_pool_entities: pool_entities,
        actual_active_lights: active,
        city_point_lights: city_points,
        unrelated_point_lights: unrelated_points,
        total_spot_lights: total_spots,
        total_point_lights: total_points,
        stable_observation_frames: observations.len(),
        stable_source_slots: stable,
        slots,
        source_catalog_counts: catalog_counts,
        probes: Vec::new(),
        probe_screening: Vec::new(),
        comparison_reference: None,
        comparisons: Vec::new(),
        ground_response_hits: 0,
        ground_response_samples: 0,
        unchanged_control_hits: 0,
        unchanged_control_samples: 0,
        window_response_hits: 0,
        sign_response_hits: 0,
        restoration_hits: 0,
        restoration_samples: 0,
        streaming: world.resource::<crate::map::MapRenderStats>().clone(),
    })
}

/// The original arcade is open below its ceiling. Treating it as a solid tower
/// incorrectly removes every floor receiver; other buildings still screen rays.
fn lighting_camera_obstruction(
    data: &crate::map::WorldData,
    target: Vec3,
    trees: &[Vec2],
) -> Option<(&'static str, Option<u32>)> {
    use crate::map::data::{ARC, GF};
    let direction = Vec2::ONE.normalize();
    for step in 1..=144 {
        let distance = step as f32 * 0.25;
        let p = target.xz() + direction * distance;
        let height = target.y + distance * crate::camera::CAM_OFFSET.y / 40.0_f32.hypot(40.0);
        if let Some(building) = data.bld_at(p) {
            let open = building.h + 3.5 < height
                || building.arcade.is_some_and(|arcade| {
                    let delta = p - arcade.a;
                    (0.0..=arcade.dl).contains(&delta.dot(arcade.u))
                        && (0.0..=ARC).contains(&-delta.dot(arcade.n))
                        && height < GF - 0.3
                });
            if !open {
                return Some(("building_envelope", Some(building.id)));
            }
        }
        if height < 8.0 && trees.iter().any(|tree| tree.distance(p) < 2.3) {
            return Some(("tree_envelope", None));
        }
    }
    None
}

fn lighting_in_cone(slot: &LightingSlotEvidence, point: Vec3, margin: f32) -> bool {
    if slot.source_id.is_none() {
        return false;
    }
    let delta = point - Vec3::from(slot.actual_position);
    let length = delta.length();
    length > 0.001
        && length < slot.range + margin
        && Vec3::from(slot.actual_direction).dot(delta / length)
            > (slot.outer_angle + margin / length).cos()
}

fn lighting_pixel_probes(
    world: &mut World,
    fixture: LightingFixture,
    size: [u32; 2],
    evidence: &mut LightingEvidence,
) -> Result<Vec<LightingPixelProbe>, String> {
    use crate::map::build::Mat;
    let data = crate::map::world();
    let building = &data.blds[fixture.building as usize];
    let (camera, camera_transform) = world
        .query_filtered::<(&Camera, &GlobalTransform), With<GameCamera>>()
        .iter(world)
        .next()
        .ok_or("Lighting camera is absent")?;
    let project = |point: Vec3| -> Option<[u32; 2]> {
        let pixel = camera.world_to_viewport(camera_transform, point).ok()?;
        let (x, y) = (pixel.x / size[0] as f32, pixel.y / size[1] as f32);
        // Exclude status text, minimap, touch controls and interaction prompts.
        if !(0.035..=0.965).contains(&x)
            || !(0.07..=0.82).contains(&y)
            || (x < 0.32 && y < 0.33)
            || (x < 0.23 && y > 0.63)
            || ((0.27..=0.71).contains(&x) && y > 0.65)
            || (x > 0.80 && y > 0.62)
        {
            return None;
        }
        Some([pixel.x.round() as u32, pixel.y.round() as u32])
    };
    // Use original prop triangles to reject a ground point covered by a parked
    // car, scooter or lamp body in this actual fixed-camera view.
    let mut triangles = Vec::new();
    for prop in data
        .props
        .iter()
        .filter(|prop| prop.p.distance(fixture.position) < 42.0)
    {
        for mesh in crate::map::street::prop_geometry(prop).0.values() {
            for ids in mesh.idx.as_chunks::<3>().0 {
                triangles.push(ids.map(|id| Vec3::from(mesh.pos[id as usize])));
            }
        }
    }
    // The arcade opening test alone cannot see columns, soffit beams or shop
    // panels. Screen their original triangles as well, including the owning
    // building's occupied-window frames. A receiver above its floor does not
    // intersect that floor along the positive camera ray.
    let mut building_triangles = Vec::new();
    for owner in data
        .blds
        .iter()
        .filter(|owner| owner.c.distance(fixture.position) < 35.0)
    {
        let meshes = crate::map::build::building(owner, crate::map::Lod::Near);
        for parts in [&meshes.base, &meshes.roof, &meshes.structure, &meshes.walls] {
            for mesh in parts.0.values() {
                for ids in mesh.idx.as_chunks::<3>().0 {
                    building_triangles.push(ids.map(|id| Vec3::from(mesh.pos[id as usize])));
                }
            }
        }
        if owner.id == fixture.building {
            let detail =
                crate::map::detail::near_detail(owner, &crate::map::detail::WindowKit::new());
            for mesh in detail.0.values() {
                for ids in mesh.idx.as_chunks::<3>().0 {
                    building_triangles.push(ids.map(|id| Vec3::from(mesh.pos[id as usize])));
                }
            }
        }
    }
    // Bound tree screening to this fixture and the entire 36 m camera ray;
    // avoid rescanning every city prop for every sub-step of every probe.
    let trees: Vec<_> = data
        .props
        .iter()
        .filter(|prop| {
            matches!(prop.kind, crate::map::street::PropKind::Tree(_))
                && prop.p.distance(fixture.position) < 90.0
        })
        .map(|prop| prop.p)
        .collect();
    let mut screening = Vec::new();
    let mut probes = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |role: &str,
                   source_id: Option<u64>,
                   point: Vec3,
                   screen_props: bool|
     -> Result<(), (&'static str, Option<u32>)> {
        let Some(pixel) = project(point) else {
            return Err(("outside_view_or_hud", None));
        };
        if let Some(reason) = lighting_camera_obstruction(data, point, &trees) {
            return Err(reason);
        }
        if street_prop_occludes(point, &building_triangles) {
            return Err(("building_triangle", None));
        }
        if screen_props && street_prop_occludes(point, &triangles) {
            return Err(("prop_triangle", None));
        }
        if !seen.insert(pixel) {
            return Err(("duplicate_pixel", None));
        }
        probes.push(LightingPixelProbe {
            role: role.into(),
            source_id,
            geometric_cone_sources: evidence
                .slots
                .iter()
                .filter(|slot| lighting_in_cone(slot, point, 0.0))
                .filter_map(|slot| slot.source_id)
                .collect(),
            world: point.to_array(),
            point: pixel,
        });
        Ok(())
    };
    for slot in &evidence.slots {
        if slot.source_id.is_none() {
            continue;
        }
        let receiver = Vec3::from(slot.receiver);
        let mut screened = LightingProbeScreening {
            source_id: slot.source_id.expect("assigned source"),
            kind: slot.kind.clone(),
            attempted: 0,
            accepted_before_cap: 0,
            rejected: Default::default(),
            examples: Vec::new(),
        };
        // The street cone is wider than an arcade bay. The original ±1.2 m
        // core can sit entirely behind an actual foreground building; examine
        // its wider real pavement footprint without accepting translucent walls.
        let extent = if slot.kind == "Street" { 6 } else { 2 };
        for ix in -extent..=extent {
            for iz in -extent..=extent {
                let point = receiver + Vec3::new(ix as f32 * 0.6, 0.006, iz as f32 * 0.6);
                screened.attempted += 1;
                let ground = match data.bld_at(point.xz()) {
                    None => slot.kind != "Arcade",
                    Some(owner) => {
                        slot.kind == "Arcade"
                            && owner.arcade.is_some_and(|arcade| {
                                let d = point.xz() - arcade.a;
                                (0.0..=arcade.dl).contains(&d.dot(arcade.u))
                                    && (0.0..=crate::map::data::ARC).contains(&-d.dot(arcade.n))
                            })
                    }
                };
                let result = if !lighting_in_cone(slot, point, 0.0) {
                    Err(("outside_source_cone", None))
                } else if point.xz().distance(fixture.position) < 1.4 {
                    Err(("near_player", None))
                } else if !ground {
                    Err((
                        "wrong_ground_surface",
                        data.bld_at(point.xz()).map(|owner| owner.id),
                    ))
                } else {
                    add(&format!("Ground{}", slot.kind), slot.source_id, point, true)
                };
                match result {
                    Ok(()) => screened.accepted_before_cap += 1,
                    Err((reason, building_id)) => {
                        let count = screened.rejected.entry(reason).or_default();
                        *count += 1;
                        if *count <= 3 {
                            screened.examples.push(LightingProbeRejection {
                                reason,
                                building_id,
                                world: point.to_array(),
                                point: project(point),
                            });
                        }
                    }
                }
            }
        }
        screening.push(screened);
    }
    for x in -9..=9 {
        for z in -9..=9 {
            let point = Vec3::new(
                fixture.position.x + x as f32 * 1.2,
                0.07,
                fixture.position.y + z as f32 * 1.2,
            );
            if point.xz().distance(fixture.position) < 1.8
                || data.bld_at(point.xz()).is_some()
                || evidence
                    .slots
                    .iter()
                    .any(|slot| lighting_in_cone(slot, point, 0.8))
            {
                continue;
            }
            let _ = add("GroundControl", None, point, true);
        }
    }
    let detail = crate::map::detail::near_detail(building, &crate::map::detail::WindowKit::new());
    let mut meshes = crate::map::build::building(building, crate::map::Lod::Near);
    meshes.roof.merge(meshes.base);
    meshes.roof.merge(meshes.structure);
    for (role, mesh) in [
        ("OccupiedWindow", detail.0.get(&Mat::WindowGlow)),
        ("Sign", meshes.roof.0.get(&Mat::Sign)),
    ] {
        let Some(mesh) = mesh else {
            return Err(format!("Lighting {role} source geometry is absent"));
        };
        for ids in mesh.idx.as_chunks::<3>().0 {
            let normal = Vec3::from(mesh.nrm[ids[0] as usize]);
            if normal.dot(crate::camera::CAM_OFFSET.normalize()) < 0.2 {
                continue;
            }
            let p = ids.map(|id| Vec3::from(mesh.pos[id as usize]));
            for weights in [[0.2, 0.4, 0.4], [0.4, 0.2, 0.4], [0.4, 0.4, 0.2]] {
                let point =
                    p[0] * weights[0] + p[1] * weights[1] + p[2] * weights[2] + normal * 0.015;
                let _ = add(role, None, point, false);
            }
        }
    }
    // Deterministic caps retain multiple locations from each source family.
    let mut counts = std::collections::HashMap::<String, usize>::new();
    probes.retain(|probe| {
        let count = counts.entry(probe.role.clone()).or_default();
        *count += 1;
        *count
            <= if probe.role == "GroundControl" {
                24
            } else {
                48
            }
    });
    evidence.probe_screening = screening;
    Ok(probes)
}

fn verify_lighting_sequence(run: &QaRun, current: &LightingEvidence) -> Result<(), String> {
    let previous = |scene: &str| {
        run.report
            .captures
            .iter()
            .filter_map(|capture| capture.lighting.as_ref())
            .find(|evidence| evidence.scene == scene)
    };
    if current.scene != "Day" {
        let day = previous("Day").ok_or("Lighting sequence has no day capture")?;
        if (
            day.building_id,
            day.player,
            day.camera,
            day.zoom,
            day.wetness,
        ) != (
            current.building_id,
            current.player,
            current.camera,
            current.zoom,
            current.wetness,
        ) {
            return Err(
                "Lighting sequence changed the actual camera/player/geometry fixture".into(),
            );
        }
    }
    if matches!(
        current.scene.as_str(),
        "NoSpillControl" | "Blackout" | "Restored"
    ) {
        let night = previous("Night").ok_or("Lighting sequence has no powered night capture")?;
        for (old, new) in night.slots.iter().zip(&current.slots) {
            if (
                old.slot,
                old.source_id,
                old.actual_position,
                old.actual_direction,
                old.range,
                old.inner_angle,
                old.outer_angle,
                old.color,
            ) != (
                new.slot,
                new.source_id,
                new.actual_position,
                new.actual_direction,
                new.range,
                new.inner_angle,
                new.outer_angle,
                new.color,
            ) {
                return Err("Lighting sequence replaced/moved a selected source during control/blackout/restoration".into());
            }
        }
        if current.scene != "Blackout"
            && (
                night.minute,
                night.power,
                night.night,
                night.ambient,
                night.ambient_color,
                &night.sun_illuminance,
                &night.sun_color,
                &night.material_state,
            ) != (
                current.minute,
                current.power,
                current.night,
                current.ambient,
                current.ambient_color,
                &current.sun_illuminance,
                &current.sun_color,
                &current.material_state,
            )
        {
            return Err(
                "Direct-light control/restoration changed ambient, sun or actual material state"
                    .into(),
            );
        }
        if current.scene == "Restored"
            && night.slots.iter().zip(&current.slots).any(|(old, new)| {
                (old.intensity - new.intensity).abs() > 0.1 || old.gain != new.gain
            })
        {
            return Err(
                "Power restoration did not restore original source intensities/gains".into(),
            );
        }
    }
    Ok(())
}

fn lighting_pixel_comparison(
    captures: &[Capture],
    current: &mut LightingEvidence,
) -> Result<(), String> {
    // Validate after readback so a blocked receiver leaves its real PNG and
    // pixel-failure JSON for inspection rather than failing before capture.
    if !["OccupiedWindow", "Sign"]
        .iter()
        .all(|role| current.probes.iter().any(|pixel| pixel.probe.role == *role))
    {
        return Err("Lighting camera has no visible original occupied-window/sign probes".into());
    }
    if !matches!(
        current.scene.as_str(),
        "NoSpillControl" | "Blackout" | "Restored"
    ) {
        return Ok(());
    }
    let night = captures
        .iter()
        .filter_map(|capture| capture.lighting.as_ref())
        .find(|evidence| evidence.scene == "Night")
        .ok_or("Lighting pixel comparison has no original night readback")?;
    if night.probes.len() != current.probes.len()
        || night
            .probes
            .iter()
            .zip(&current.probes)
            .any(|(old, new)| old.probe != new.probe)
    {
        return Err(
            "Lighting comparison did not use exactly the original night world/pixel coordinates"
                .into(),
        );
    }
    current.comparison_reference = Some("Night");
    for (reference, pixel) in night.probes.iter().zip(&current.probes) {
        let difference = i16::from(reference.luma) - i16::from(pixel.luma);
        let max_channel_delta = reference
            .rgb
            .iter()
            .zip(pixel.rgb)
            .map(|(old, new)| old.abs_diff(new))
            .max()
            .unwrap_or(0);
        let ground = pixel.probe.role.starts_with("Ground") && pixel.probe.role != "GroundControl";
        let matched = match current.scene.as_str() {
            "NoSpillControl" if ground => difference >= 8,
            "NoSpillControl" => max_channel_delta <= 3,
            "Blackout" => difference >= 15,
            "Restored" => max_channel_delta <= 4,
            _ => false,
        };
        if current.scene == "NoSpillControl" {
            if ground {
                current.ground_response_samples += 1;
                current.ground_response_hits += usize::from(matched);
            }
            if pixel.probe.role == "GroundControl" {
                current.unchanged_control_samples += 1;
                current.unchanged_control_hits += usize::from(matched);
            }
        }
        if current.scene == "Blackout" {
            if pixel.probe.role == "OccupiedWindow" {
                current.window_response_hits += usize::from(matched);
            }
            if pixel.probe.role == "Sign" {
                current.sign_response_hits += usize::from(matched);
            }
        }
        if current.scene == "Restored" {
            current.restoration_samples += 1;
            current.restoration_hits += usize::from(matched);
        }
        current.comparisons.push(LightingPixelComparison {
            role: pixel.probe.role.clone(),
            source_id: pixel.probe.source_id,
            point: pixel.probe.point,
            reference_rgb: reference.rgb,
            current_rgb: pixel.rgb,
            reference_luma: reference.luma,
            current_luma: pixel.luma,
            difference,
            matched,
        });
    }
    if current.scene == "NoSpillControl" {
        let families = ["GroundStreet", "GroundSign", "GroundArcade"];
        if current.ground_response_samples < 9
            || current.ground_response_hits * 4 < current.ground_response_samples
            || current.unchanged_control_samples < 4
            || current.unchanged_control_hits * 4 < current.unchanged_control_samples * 3
            || families.iter().any(|role| {
                current
                    .comparisons
                    .iter()
                    .filter(|sample| sample.role == *role)
                    .count()
                    < 3
                    || !current
                        .comparisons
                        .iter()
                        .any(|sample| sample.role == *role && sample.matched)
            })
            || night
                .probes
                .iter()
                .filter_map(|pixel| pixel.probe.source_id)
                .any(|id| {
                    night
                        .slots
                        .iter()
                        .find(|slot| slot.source_id == Some(id))
                        .is_none_or(|slot| {
                            !slot.inherited_visible
                                || !slot.view_visible
                                || !slot.in_main_view_cluster
                        })
                })
        {
            return Err(format!(
                "Actual spotlights lack local ground response at unchanged ambient/emission: {}/{} lit samples, {}/{} unchanged controls; per-family hits {:?}",
                current.ground_response_hits,
                current.ground_response_samples,
                current.unchanged_control_hits,
                current.unchanged_control_samples,
                families.map(|role| (
                    role,
                    current
                        .comparisons
                        .iter()
                        .filter(|sample| sample.role == role && sample.matched)
                        .count()
                ))
            ));
        }
    }
    if current.scene == "Blackout"
        && (current.window_response_hits == 0 || current.sign_response_hits == 0)
    {
        return Err(format!(
            "Power loss does not visibly extinguish original occupied windows/signs: window={} sign={}",
            current.window_response_hits, current.sign_response_hits
        ));
    }
    if current.scene == "Restored"
        && (current.restoration_samples == 0
            || current.restoration_hits * 10 < current.restoration_samples * 9)
    {
        return Err(format!(
            "Restored scene fails the same-camera night RGB comparison: {}/{}",
            current.restoration_hits, current.restoration_samples
        ));
    }
    Ok(())
}

/// The catalog is render-derived and is built once per fixture selection, not
/// once per candidate. Source world/prop records never change during QA.
struct StreetCatalog {
    roads: Vec<crate::map::street::RoadMarkingPlan>,
    parking: Vec<crate::map::street::ParkingPlan>,
}

impl StreetCatalog {
    fn new(data: &crate::map::WorldData) -> Self {
        Self {
            roads: crate::map::street::road_marking_plans(data),
            parking: crate::map::street::parking_plans(data),
        }
    }
}

struct StreetFeatures {
    parts: Vec<(crate::map::ChunkKey, crate::map::build::Parts)>,
    prop_indices: Vec<usize>,
    parking_bays: usize,
    junction_clearance: Option<f32>,
    approach_arrow: bool,
}

fn street_prop_indices(data: &crate::map::WorldData, fixture: StreetFixture) -> Vec<usize> {
    use crate::map::street::PropKind;
    let StreetSubject::Prop(index) = fixture.subject else {
        return Vec::new();
    };
    if fixture.scene != StreetScene::Scooters {
        return vec![index];
    }
    let anchor = data.props[index];
    let mut scooters: Vec<_> = data
        .props
        .iter()
        .enumerate()
        .filter(|(_, prop)| {
            prop.kind == PropKind::Scooter
                && prop.p.distance(anchor.p) < 3.1
                && (prop.ang - anchor.ang).abs() < 0.01
        })
        .map(|(id, _)| id)
        .collect();
    scooters.sort_unstable();
    scooters.truncate(4);
    if let Some((lamp, _)) = data
        .props
        .iter()
        .enumerate()
        .filter(|(_, prop)| prop.kind == PropKind::Lamp && prop.p.distance(anchor.p) < 9.0)
        .min_by(|a, b| {
            a.1.p
                .distance_squared(anchor.p)
                .total_cmp(&b.1.p.distance_squared(anchor.p))
                .then(a.0.cmp(&b.0))
        })
    {
        scooters.push(lamp);
    }
    scooters
}

fn street_features(
    data: &crate::map::WorldData,
    fixture: StreetFixture,
    catalog: &StreetCatalog,
) -> Result<StreetFeatures, String> {
    use crate::map::{ChunkKey, build::Parts, street};
    let mut parts: Vec<(ChunkKey, Parts)> = Vec::new();
    let mut add = |key: ChunkKey, geometry: Parts| {
        if let Some((_, existing)) = parts.iter_mut().find(|(owner, _)| *owner == key) {
            existing.merge(geometry);
        } else {
            parts.push((key, geometry));
        }
    };
    let mut result = StreetFeatures {
        parts: Vec::new(),
        prop_indices: street_prop_indices(data, fixture),
        parking_bays: 0,
        junction_clearance: None,
        approach_arrow: false,
    };
    match fixture.subject {
        StreetSubject::Approach { road, segment, end } => {
            let layout = catalog
                .roads
                .iter()
                .find(|plan| plan.road_index == road && plan.segment_index == segment)
                .ok_or("Street fixture source segment no longer has a marking plan")?;
            let approach = layout
                .approaches
                .iter()
                .find(|approach| approach.end == end)
                .ok_or("Street fixture source approach is missing")?;
            add(
                ChunkKey::of((layout.a + layout.b) / 2.0),
                street::approach_geometry(approach),
            );
            result.junction_clearance = Some(approach.clearance);
            result.approach_arrow = approach.has_arrow;
        }
        StreetSubject::Prop(_) => {
            for index in &result.prop_indices {
                let prop = &data.props[*index];
                add(ChunkKey::of(prop.p), street::prop_geometry(prop));
                if let Some(parking) = catalog
                    .parking
                    .iter()
                    .find(|plan| plan.prop_index == *index)
                {
                    add(ChunkKey::of(prop.p), street::parking_geometry(parking));
                    result.parking_bays += 1;
                }
            }
        }
    }
    parts.sort_by_key(|(key, _)| *key);
    result.parts = parts;
    Ok(result)
}

/// Reject obvious foreground buildings and tree crowns along the normal camera
/// direction. This is candidate screening, not a claim of full pixel occlusion.
fn street_camera_clear(data: &crate::map::WorldData, target: Vec3) -> bool {
    use crate::map::street::PropKind;
    let direction = Vec2::ONE.normalize();
    (1..=36).all(|step| {
        let distance = step as f32;
        let p = target.xz() + direction * distance;
        let height = target.y + distance * crate::camera::CAM_OFFSET.y / 40.0_f32.hypot(40.0);
        data.bld_at(p)
            .is_none_or(|building| building.h + 3.5 < height)
            && !data.props.iter().any(|prop| {
                matches!(prop.kind, PropKind::Tree(_)) && prop.p.distance(p) < 2.3 && height < 8.0
            })
    })
}

/// free() only measures distance to wall/cover segments: the centre inside a
/// closed parked-car rectangle can be far from all four edges. A QA teleport
/// must also remain outside the original solid vehicle footprint, with room for
/// the player radius, rather than placing an unreachable camera inside a body.
fn street_player_position_clear(data: &crate::map::WorldData, position: Vec2) -> bool {
    if data.bld_at(position).is_some() || !data.free(position, 0.6) {
        return false;
    }
    !data.props.iter().any(|prop| {
        use crate::map::street::PropKind;
        let (half_width, half_length) = match prop.kind {
            PropKind::Car | PropKind::Taxi => (0.9, 2.2),
            PropKind::Truck => (1.15, 3.25),
            // Scooters are non-colliding decoration, but a review camera should
            // still avoid placing the player's body through the parked model.
            PropKind::Scooter => (0.36, 0.87),
            _ => return false,
        };
        let delta = position - prop.p;
        let along = Vec2::new(prop.ang.sin(), prop.ang.cos());
        let across = Vec2::new(prop.ang.cos(), -prop.ang.sin());
        delta.dot(across).abs() <= half_width + 0.6 && delta.dot(along).abs() <= half_length + 0.6
    })
}

fn street_fixture(
    data: &crate::map::WorldData,
    scene: StreetScene,
    size: [u32; 2],
) -> Result<StreetFixture, String> {
    use crate::map::street::PropKind;
    let catalog = StreetCatalog::new(data);
    let mut subjects = Vec::new();
    if scene == StreetScene::Junction {
        for road in &catalog.roads {
            for approach in &road.approaches {
                if approach.has_arrow && (9.0..=22.0).contains(&approach.width) {
                    subjects.push(StreetSubject::Approach {
                        road: road.road_index,
                        segment: road.segment_index,
                        end: approach.end,
                    });
                }
            }
        }
    } else {
        let kind = match scene {
            StreetScene::Scooters => PropKind::Scooter,
            StreetScene::Taxi => PropKind::Taxi,
            StreetScene::Truck => PropKind::Truck,
            StreetScene::Junction => unreachable!(),
        };
        subjects.extend(
            data.props.iter().enumerate().filter_map(|(index, prop)| {
                (prop.kind == kind).then_some(StreetSubject::Prop(index))
            }),
        );
    }
    let source_count = subjects.len();
    let mut missing_features = 0;
    let mut blocked_views = 0;
    let mut candidates = Vec::new();
    for subject in subjects {
        let mut fixture = StreetFixture {
            scene,
            subject,
            position: Vec2::ZERO,
            zoom: 1.0,
        };
        let features = street_features(data, fixture, &catalog)?;
        if scene == StreetScene::Scooters {
            let scooters = features
                .prop_indices
                .iter()
                .filter(|index| data.props[**index].kind == PropKind::Scooter)
                .count();
            let lamps = features
                .prop_indices
                .iter()
                .filter(|index| data.props[**index].kind == PropKind::Lamp)
                .count();
            if scooters < 2 || lamps != 1 || features.parking_bays < 2 {
                missing_features += 1;
                continue;
            }
        }
        let bounds: Vec<Vec3> = features
            .parts
            .iter()
            .flat_map(|(_, parts)| {
                parts
                    .0
                    .values()
                    .flat_map(|mesh| mesh.pos.iter().copied().map(Vec3::from))
            })
            .collect();
        if bounds.len() < 40 {
            continue;
        }
        let (lo, hi) = bounds.iter().copied().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(lo, hi), point| (lo.min(point), hi.max(point)),
        );
        let centre = (lo + hi) / 2.0;
        let screen_targets = [
            centre,
            Vec3::new(lo.x, 0.15, lo.z),
            Vec3::new(lo.x, 0.15, hi.z),
            Vec3::new(hi.x, 0.15, lo.z),
            Vec3::new(hi.x, 0.15, hi.z),
        ];
        if !screen_targets
            .into_iter()
            .all(|point| street_camera_clear(data, point))
        {
            blocked_views += 1;
            continue;
        }
        // Ground markings can use their centre. Parked vehicles retain their
        // real collision; stand at a nearby clear road point and fit both them
        // and, for scooters, the actual nearby lamp into the ordinary camera.
        let mut positions = vec![centre.xz()];
        for distance in [1.5, 2.5, 3.5, 5.0] {
            for direction in [
                Vec2::ONE.normalize(),
                Vec2::new(1.0, -1.0).normalize(),
                -Vec2::ONE.normalize(),
                Vec2::new(-1.0, 1.0).normalize(),
            ] {
                positions.push(centre.xz() + direction * distance);
            }
        }
        for position in positions {
            if !position.cmpge(data.min + 1.0).all()
                || !position.cmple(data.max - 1.0).all()
                || !street_player_position_clear(data, position)
                || features
                    .parts
                    .iter()
                    .any(|(owner, _)| owner.dist(position) >= 58.0)
            {
                continue;
            }
            let Some(zoom) = commercial_zoom(&bounds, position, size) else {
                continue;
            };
            fixture.position = position;
            fixture.zoom = zoom;
            candidates.push(fixture);
            break;
        }
    }
    candidates
        .into_iter()
        .max_by(|a, b| {
            a.zoom.total_cmp(&b.zoom).then_with(|| {
                b.position
                    .distance_squared(data.spawn)
                    .total_cmp(&a.position.distance_squared(data.spawn))
            })
        })
        .ok_or_else(|| format!("No visible, collision-clear real-world {scene:?} street fixture: {source_count} sources, {missing_features} lack required features, {blocked_views} fail foreground screening"))
}

fn street_owner_keys(
    data: &crate::map::WorldData,
    fixture: StreetFixture,
) -> Vec<crate::map::ChunkKey> {
    let mut owners = match fixture.subject {
        StreetSubject::Approach { road, segment, .. } => {
            let source = &data.roads[road];
            vec![crate::map::ChunkKey::of(
                (source.p[segment] + source.p[segment + 1]) / 2.0,
            )]
        }
        StreetSubject::Prop(_) => street_prop_indices(data, fixture)
            .into_iter()
            .map(|index| crate::map::ChunkKey::of(data.props[index].p))
            .collect(),
    };
    owners.sort_unstable();
    owners.dedup();
    owners
}

fn street_ready(world: &mut World, fixture: StreetFixture) -> bool {
    let Some(main_view) = main_camera_meshes(world) else {
        return false;
    };
    for key in street_owner_keys(crate::map::world(), fixture) {
        let Some(root) = world
            .query::<(Entity, &crate::map::Chunk)>()
            .iter(world)
            .find_map(|(entity, chunk)| {
                (chunk.key == key && chunk.lod == crate::map::Lod::Near).then_some(entity)
            })
        else {
            return false;
        };
        if !world
            .query::<(
                Entity,
                &ChildOf,
                &crate::map::MapMeshMaterial,
                &InheritedVisibility,
                &ViewVisibility,
            )>()
            .iter(world)
            .any(|(entity, parent, _, inherited, view)| {
                parent.parent() == root
                    && inherited.get()
                    && view.get()
                    && main_view.contains(&entity)
            })
        {
            return false;
        }
    }
    true
}

fn street_pixel_luma(rgb: [u8; 3]) -> u8 {
    ((54 * rgb[0] as u32 + 183 * rgb[1] as u32 + 19 * rgb[2] as u32) / 256) as u8
}

fn street_marking_pixel_match(paint: [u8; 3], asphalt: [u8; 3]) -> bool {
    let lo = *paint.iter().min().unwrap();
    let hi = *paint.iter().max().unwrap();
    // Neutral asphalt in CI77 is approximately RGB(114,110,99): brightness
    // alone at 90 accepted the missing-paint negative control. Require a much
    // lighter neutral surface AND local contrast against unpainted carriageway.
    lo >= 165
        && hi - lo <= 65
        && street_pixel_luma(paint).saturating_sub(street_pixel_luma(asphalt)) >= 35
}

fn street_painted_triangles(features: &StreetFeatures, catalog: &StreetCatalog) -> Vec<[Vec2; 3]> {
    use crate::map::{build::Mat, street};
    let (lo, hi) = features
        .parts
        .iter()
        .flat_map(|(_, parts)| parts.0.values())
        .flat_map(|mesh| mesh.pos.iter().copied().map(|point| Vec3::from(point).xz()))
        .fold(
            (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
            |(lo, hi), point| (lo.min(point), hi.max(point)),
        );
    let (lo, hi) = (lo - 2.0, hi + 2.0);
    let mut triangles = Vec::new();
    let mut add = |parts: crate::map::build::Parts| {
        if let Some(mesh) = parts.0.get(&Mat::Marking) {
            triangles.extend(mesh.idx.as_chunks::<3>().0.iter().map(|indices| {
                [indices[0], indices[1], indices[2]]
                    .map(|index| Vec3::from(mesh.pos[index as usize]).xz())
            }));
        }
    };
    for road in &catalog.roads {
        let margin = Vec2::splat(road.width * 0.5 + 1.0);
        let min = road.a.min(road.b) - margin;
        let max = road.a.max(road.b) + margin;
        if min.x <= hi.x && max.x >= lo.x && min.y <= hi.y && max.y >= lo.y {
            add(street::road_marking_geometry(road));
        }
    }
    for parking in &catalog.parking {
        let margin = parking.length.max(parking.width);
        if parking.center.x + margin >= lo.x
            && parking.center.x - margin <= hi.x
            && parking.center.y + margin >= lo.y
            && parking.center.y - margin <= hi.y
        {
            add(street::parking_geometry(parking));
        }
    }
    triangles
}

/// Orthographic paint-to-camera ray against the selected source prop surfaces.
/// It prevents a bright scooter body/plate from masquerading as the paint beneath
/// it; this small fixture check is not a whole-world visibility renderer.
fn street_prop_occludes(origin: Vec3, triangles: &[[Vec3; 3]]) -> bool {
    let direction = crate::camera::CAM_OFFSET.normalize();
    triangles.iter().any(|triangle| {
        let e1 = triangle[1] - triangle[0];
        let e2 = triangle[2] - triangle[0];
        let p = direction.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-7 {
            return false;
        }
        let inverse = det.recip();
        let offset = origin - triangle[0];
        let u = offset.dot(p) * inverse;
        if !(0.0..=1.0).contains(&u) {
            return false;
        }
        let q = offset.cross(e1);
        let v = direction.dot(q) * inverse;
        if v < 0.0 || u + v > 1.0 {
            return false;
        }
        e2.dot(q) * inverse > 0.015
    })
}

fn street_asphalt_control(
    data: &crate::map::WorldData,
    point: Vec3,
    triangle: [Vec3; 3],
    painted: &[[Vec2; 3]],
    occluders: &[[Vec3; 3]],
) -> Option<Vec3> {
    let edge = [
        triangle[1] - triangle[0],
        triangle[2] - triangle[1],
        triangle[0] - triangle[2],
    ]
    .into_iter()
    .max_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))?
    .xz()
    .normalize_or_zero();
    let normal = Vec2::new(-edge.y, edge.x);
    // Compare with nearby carriageway under nearly the same lighting. Controls
    // must lie outside every nearby source paint triangle, with an AA margin,
    // and outside existing solid-prop/building collision.
    for distance in [0.35, 0.5, 0.7, 1.0, 1.4] {
        for direction in [normal, -normal, edge, -edge] {
            let q = point.xz() + direction * distance;
            if data.road_dist(q) > -0.16 || data.bld_at(q).is_some() || !data.free(q, 0.1) {
                continue;
            }
            // Scooters intentionally have no gameplay collision, so free() by
            // itself could choose their dark seat/body as an "asphalt" control.
            // Reject every existing low prop footprint explicitly as well.
            if data.props.iter().any(|prop| {
                use crate::map::street::PropKind;
                let (width, length) = match prop.kind {
                    PropKind::Scooter => (0.4, 0.9),
                    PropKind::Car | PropKind::Taxi => (1.0, 2.3),
                    PropKind::Truck => (1.3, 3.4),
                    PropKind::Lamp => (0.4, 0.4),
                    PropKind::Tree(_) => (2.6, 2.6),
                };
                let delta = q - prop.p;
                let along = Vec2::new(prop.ang.sin(), prop.ang.cos());
                let across = Vec2::new(prop.ang.cos(), -prop.ang.sin());
                delta.dot(along).abs() <= length && delta.dot(across).abs() <= width
            }) {
                continue;
            }
            if painted.iter().any(|triangle| {
                crate::map::data::pip(q, triangle)
                    || (0..3).any(|i| {
                        crate::map::data::seg_dist(q, triangle[i], triangle[(i + 1) % 3]) < 0.14
                    })
            }) {
                continue;
            }
            let target = Vec3::new(q.x, point.y, q.y);
            if !street_prop_occludes(target, occluders) {
                return Some(target);
            }
        }
    }
    None
}

fn street_evidence(
    world: &mut World,
    fixture: StreetFixture,
    size: [u32; 2],
) -> Result<(StreetEvidence, Vec<StreetPixelProbe>), String> {
    use crate::map::{Chunk, Lod, MapMeshGeometry, MapMeshMaterial, build::Mat};
    let data = crate::map::world();
    let catalog = StreetCatalog::new(data);
    let features = street_features(data, fixture, &catalog)?;
    let main_view = main_camera_meshes(world).ok_or("Street camera has no visible mesh list")?;
    let mut batches = Vec::new();
    let mut handles = Vec::new();
    let vertex_key = |p: [f32; 3]| p.map(|value| (value * 1000.0).round() as i32);
    for (key, feature) in &features.parts {
        let root = world
            .query::<(Entity, &Chunk)>()
            .iter(world)
            .find_map(|(entity, chunk)| {
                (chunk.key == *key && chunk.lod == Lod::Near).then_some(entity)
            })
            .ok_or_else(|| format!("Street source chunk {key:?} is not Near"))?;
        let expected = crate::map::build_chunk(data, *key, Lod::Near).merged;
        let mut found = HashSet::new();
        let mut uploaded = ExteriorMeshGroup::default();
        for (entity, parent, material, mesh, metadata, transform, local, inherited, view) in world
            .query::<(
                Entity,
                &ChildOf,
                &MapMeshMaterial,
                &Mesh3d,
                &MapMeshGeometry,
                &GlobalTransform,
                &Visibility,
                &InheritedVisibility,
                &ViewVisibility,
            )>()
            .iter(world)
        {
            if parent.parent() != root {
                continue;
            }
            let planned = expected
                .0
                .get(&material.0)
                .ok_or_else(|| format!("Street {key:?}: unexpected {:?} material", material.0))?;
            let (min, max) = planned.aabb();
            if !found.insert(material.0)
                || metadata.vertices != planned.vcount()
                || metadata.min.distance(min) > 0.001
                || metadata.max.distance(max) > 0.001
                || transform.translation().length() > 0.001
                || *local == Visibility::Hidden
                || !inherited.get()
            {
                return Err(format!(
                    "Street {key:?}: {:?} uploaded geometry differs from runtime chunk",
                    material.0
                ));
            }
            let visible = view.get() && main_view.contains(&entity);
            if feature.0.contains_key(&material.0) && !visible {
                return Err(format!(
                    "Street {key:?}: feature material {:?} absent from main view",
                    material.0
                ));
            }
            uploaded.meshes += 1;
            uploaded.vertices += metadata.vertices;
            uploaded.in_main_view += usize::from(visible);
            handles.push(mesh.0.clone());
        }
        let expected_materials: HashSet<_> = expected
            .0
            .iter()
            .filter_map(|(material, mesh)| (!mesh.is_empty()).then_some(*material))
            .collect();
        if found != expected_materials {
            return Err(format!(
                "Street {key:?}: incomplete uploaded material batches"
            ));
        }
        for (material, mesh) in &feature.0 {
            let full = expected.0.get(material).ok_or_else(|| {
                format!("Street {key:?}: {material:?} feature material not integrated")
            })?;
            let vertices: HashSet<_> = full.pos.iter().copied().map(vertex_key).collect();
            if mesh
                .pos
                .iter()
                .copied()
                .any(|point| !vertices.contains(&vertex_key(point)))
            {
                return Err(format!(
                    "Street {key:?}: {material:?} feature vertices missing from runtime chunk"
                ));
            }
        }
        batches.push(StreetBatchEvidence {
            chunk: [key.0, key.1],
            uploaded,
        });
    }
    if handles
        .iter()
        .any(|handle| !world.resource::<Assets<Mesh>>().contains(handle))
    {
        return Err("Street upload references a missing mesh asset".into());
    }
    let player = player_position(world)?;
    if player.xz().distance(fixture.position) > 0.001
        || !street_player_position_clear(data, player.xz())
    {
        return Err("Street camera position moved or overlaps existing collision".into());
    }
    let (camera, transform) = world
        .query_filtered::<(&Camera, &GlobalTransform), With<GameCamera>>()
        .iter(world)
        .next()
        .ok_or("Street camera is missing")?;
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    let mut probes = Vec::new();
    let painted_ground = street_painted_triangles(&features, &catalog);
    let occluders: Vec<_> = features
        .parts
        .iter()
        .flat_map(|(_, parts)| parts.0.iter())
        .filter(|(material, _)| **material != Mat::Marking)
        .flat_map(|(_, mesh)| {
            mesh.idx.as_chunks::<3>().0.iter().map(|indices| {
                [indices[0], indices[1], indices[2]]
                    .map(|index| Vec3::from(mesh.pos[index as usize]))
            })
        })
        .collect();
    let mut feature_vertices = 0;
    for (_, parts) in &features.parts {
        for (material, mesh) in &parts.0 {
            feature_vertices += mesh.vcount();
            for point in &mesh.pos {
                let projected = camera
                    .world_to_viewport(transform, Vec3::from(*point))
                    .map_err(|err| format!("Street feature cannot be projected: {err:?}"))?;
                lo = lo.min(projected);
                hi = hi.max(projected);
            }
            if *material == Mat::Marking {
                for triangle in mesh.idx.as_chunks::<3>().0.iter() {
                    let triangle = [triangle[0], triangle[1], triangle[2]]
                        .map(|index| Vec3::from(mesh.pos[index as usize]));
                    let point = triangle.into_iter().sum::<Vec3>() / 3.0;
                    if street_prop_occludes(point, &occluders) {
                        continue;
                    }
                    let Some(control) =
                        street_asphalt_control(data, point, triangle, &painted_ground, &occluders)
                    else {
                        continue;
                    };
                    let project = |point| {
                        camera
                            .world_to_viewport(transform, point)
                            .map(|pixel| [pixel.x.round() as u32, pixel.y.round() as u32])
                    };
                    let sample = project(point).map_err(|err| {
                        format!("Street marking pixel cannot be projected: {err:?}")
                    })?;
                    let asphalt = project(control).map_err(|err| {
                        format!("Street asphalt control cannot be projected: {err:?}")
                    })?;
                    if sample[0] < size[0]
                        && sample[1] < size[1]
                        && asphalt[0] < size[0]
                        && asphalt[1] < size[1]
                        && (sample[0].abs_diff(asphalt[0]) >= 3
                            || sample[1].abs_diff(asphalt[1]) >= 3)
                    {
                        probes.push(StreetPixelProbe {
                            paint: sample,
                            asphalt,
                        });
                    }
                }
            }
        }
    }
    probes.sort_unstable();
    probes.dedup();
    if lo.x < size[0] as f32 * 0.08
        || lo.y < size[1] as f32 * 0.08
        || hi.x > size[0] as f32 * 0.92
        || hi.y > size[1] as f32 * 0.92
        || (hi - lo).min_element() < 65.0
    {
        return Err(format!(
            "Street {:?}: cropped or unreadably small feature {lo:?}..{hi:?}",
            fixture.scene
        ));
    }
    if matches!(fixture.scene, StreetScene::Junction | StreetScene::Scooters) && probes.len() < 8 {
        return Err(format!(
            "Street {:?}: insufficient actual marking triangle probes",
            fixture.scene
        ));
    }
    let atmosphere = world.resource::<crate::map::atmosphere::AtmosphereState>();
    if atmosphere.night >= 0.1 || !atmosphere.power {
        return Err("Street scene did not reach its explicit daylight/power fixture".into());
    }
    let (road_segment, junction_end) = match fixture.subject {
        StreetSubject::Approach { road, segment, end } => (Some([road, segment]), Some(end)),
        StreetSubject::Prop(_) => (None, None),
    };
    let prop_kinds = features
        .prop_indices
        .iter()
        .map(|index| format!("{:?}", data.props[*index].kind))
        .collect();
    Ok((
        StreetEvidence {
            visibility_measurement: "uploaded_batches_and_main_view_membership_with_marking_pixel_probes_not_full_occlusion",
            scene: format!("{:?}", fixture.scene),
            road_segment,
            junction_end,
            junction_clearance: features.junction_clearance,
            approach_arrow: features.approach_arrow,
            prop_indices: features.prop_indices,
            prop_kinds,
            parking_bays: features.parking_bays,
            feature_vertices,
            player: player.to_array(),
            camera: transform.translation().to_array(),
            zoom: world.resource::<CameraZoom>().0,
            projected_bounds: [lo.to_array(), hi.to_array()],
            batches,
            marking_pixel_samples: 0,
            marking_pixel_hits: 0,
            marking_pixels: Vec::new(),
            streaming: world.resource::<crate::map::MapRenderStats>().clone(),
        },
        probes,
    ))
}

fn check_loaded_transport(world: &World, run: &mut QaRun) -> Result<(), String> {
    let checkpoint = run
        .checkpoint
        .as_ref()
        .ok_or("Missing transport checkpoint")?;
    let mut expected = checkpoint.vehicles.clone();
    if let Some(vehicle) = expected.current_mut() {
        vehicle.speed = 0.0;
    }
    let saved_vehicles = serde_json::to_value(&checkpoint.vehicles).unwrap();
    let expected_vehicles = serde_json::to_value(&expected).unwrap();
    let restored_vehicles =
        serde_json::to_value(world.resource::<crate::vehicles::VehicleFleet>()).unwrap();
    let saved_armor = serde_json::to_value(&checkpoint.armor).unwrap();
    let restored_armor =
        serde_json::to_value(world.resource::<crate::combat::armor::ArmorFleet>()).unwrap();
    if restored_vehicles != expected_vehicles {
        return Err("Continue did not restore the exact fleet, occupied vehicle, fuel, damage, keys, turret and mounted ammunition before simulation resumed".into());
    }
    if restored_armor != saved_armor {
        return Err(
            "Continue changed hostile armour, destruction or captured-vehicle links".into(),
        );
    }
    run.transport_continuation_checked = true;
    run.transport_restore = Some(serde_json::json!({
        "saved_vehicles": saved_vehicles,
        "expected_vehicles": expected_vehicles,
        "restored_vehicles": restored_vehicles,
        "saved_armor": saved_armor,
        "restored_armor": restored_armor,
    }));
    run.report.assertions.push("Continue restored exact vehicle fleet, fuel, chassis damage, keys, occupied vehicle, turret, mounted ammunition and armour/capture links before simulation resumed; moving vehicle speed resets safely to zero".into());
    Ok(())
}

fn check_continuation(world: &mut World, run: &mut QaRun) -> Result<(), String> {
    if !run.transport_continuation_checked {
        return Err(
            "Continue did not expose a verified transport restore before simulation resumed".into(),
        );
    }
    let checkpoint = run.checkpoint.as_ref().ok_or("Missing checkpoint")?;
    let position = player_position(world)?;
    let shelter = world.resource::<Shelter>();
    if shelter.day != checkpoint.day || shelter.stash != checkpoint.stash {
        return Err("Continue reset the shelter day or stockpile".into());
    }
    if world.resource::<Inventory>() != &checkpoint.inventory {
        return Err("Continue changed the carried inventory".into());
    }
    if world.resource::<SelectedRole>() != &checkpoint.role {
        return Err("Continue changed the selected survivor".into());
    }
    if position.distance(checkpoint.position) > 0.1 {
        return Err("Continue changed the saved player position".into());
    }
    if world.resource::<Campaign>().game_min < checkpoint.game_min {
        return Err("Continue reset the campaign clock".into());
    }
    let map = world.resource::<crate::worldmap::MapProgress>();
    if map.notes != checkpoint.map.notes || !map.revealed.is_superset(&checkpoint.map.revealed) {
        return Err("Continue lost survivor map notes or exploration".into());
    }
    if world
        .resource::<crate::worldplay::ScavengeState>()
        .closed_doors
        != checkpoint.closed_doors
    {
        return Err("Continue changed closed door state".into());
    }
    let progress = world.resource::<crate::worldplay::ScavengeState>();
    if progress.barricaded_doors != checkpoint.barricaded_doors
        || progress.loot != checkpoint.loot
        || progress.fixtures_version != checkpoint.fixtures_version
    {
        return Err(
            "Continue lost barricades, shelf leftovers or fixture migration version".into(),
        );
    }
    run.report
        .assertions
        .push("Continue preserved day, stockpile, inventory, survivor, position, clock, map notes/exploration, closed/barricaded doors and exact shelf leftovers".into());
    run.continuation_checked = true;
    Ok(())
}

fn finish(world: &mut World, run: &mut QaRun) {
    run.finished = true;
    run.report.success = run.report.error.is_none()
        && run.report.captures.len() == run.stages.len()
        && (matches!(
            run.report.scope,
            "exterior_subset" | "commercial_subset" | "streets_subset" | "lighting_subset"
        ) || run.continuation_checked);
    let report_path = run.output.join("qa-report.json");
    let result = serde_json::to_vec_pretty(&run.report)
        .map_err(|err| err.to_string())
        .and_then(|bytes| fs::write(&report_path, bytes).map_err(|err| err.to_string()));
    if let Err(err) = result {
        run.report.success = false;
        eprintln!("DTQA could not write report: {err}");
    }
    println!(
        "DTQA result={} screenshots={} report={}",
        run.report.success,
        run.report.captures.len(),
        report_path.display()
    );
    if let Some(error) = &run.report.error {
        eprintln!("DTQA failure: {error}");
    }
    world.write_message(if run.report.success {
        AppExit::Success
    } else {
        AppExit::error()
    });
}

fn drive(world: &mut World) {
    let Some(mut run) = world.remove_resource::<QaRun>() else {
        return;
    };
    if run.finished {
        world.insert_resource(run);
        return;
    }
    if run.report.error.is_some() || run.stage >= run.stages.len() {
        finish(world, &mut run);
        world.insert_resource(run);
        return;
    }
    let result = step(world, &mut run);
    if let Err(error) = result {
        run.report.error = Some(error);
        finish(world, &mut run);
    }
    world.insert_resource(run);
}

fn step(world: &mut World, run: &mut QaRun) -> Result<(), String> {
    // Retain the complete 68-scene Wave 4 route and its gates. Twelve new
    // Wave 5 scenes share the existing 30-minute deadline, which must be
    // validated by the full run rather than silently relaxed.
    let deadline = std::env::var("DEFEND_TAIPEI_QA_DEADLINE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800);
    if run.started.elapsed() > Duration::from_secs(deadline) {
        return Err(format!("Visual QA exceeded its {deadline}-second deadline"));
    }
    if run.report.renderer.is_empty() {
        fs::create_dir_all(&run.output).map_err(|err| err.to_string())?;
        run.report.renderer = world.get_resource::<RenderAdapterInfo>().map_or_else(
            || "Bevy offscreen renderer".into(),
            |adapter| {
                format!(
                    "{} ({:?}, {:?})",
                    adapter.name, adapter.backend, adapter.device_type
                )
            },
        );
    }
    if run.captured {
        run.captured = false;
        run.waiting_capture = false;
        run.entered = false;
        run.stage += 1;
        if run.stage == run.stages.len() {
            finish(world, run);
            return Ok(());
        }
    }
    let stage = &STAGES[run.stages[run.stage]];
    if !run.entered {
        enter_stage(world, run, stage)?;
    }
    run.frame += 1;
    if matches!(stage.action, Action::Continue)
        && !run.transport_continuation_checked
        && !world
            .resource::<bevy::ecs::message::Messages<crate::save::LoadCompleted>>()
            .is_empty()
    {
        check_loaded_transport(world, run)?;
    }
    if let Action::Wave5(scene) = stage.action {
        wave5::frame(world, scene, run.frame)?;
    }
    if let Some(fixture) = run.lighting {
        // This is the only direct-light override. It lives in the native QA
        // plugin, runs after production lighting, and never changes power,
        // ambient, material emission, source selection or any saved field.
        world.resource_mut::<Campaign>().game_min = fixture.scene.minute();
        if fixture.scene == LightingScene::NoSpillControl {
            for mut light in world
                .query_filtered::<&mut SpotLight, With<crate::map::atmosphere::CitySpill>>()
                .iter_mut(world)
            {
                light.intensity = 0.0;
            }
            world
                .resource_mut::<crate::map::atmosphere::AtmosphereState>()
                .active_lights = 0;
        }
        let mut slots: Vec<_> = world
            .query::<&crate::map::atmosphere::CitySpill>()
            .iter(world)
            .map(|slot| (slot.slot, slot.site))
            .collect();
        slots.sort_by_key(|slot| slot.0);
        if run.frame > SETTLE_FRAMES / 2 {
            run.lighting_observations
                .push(slots.into_iter().map(|slot| slot.1).collect());
            if run.lighting_observations.len() > 30 {
                run.lighting_observations.remove(0);
            }
        }
    }
    if run.frame == SETTLE_FRAMES / 2 && matches!(stage.action, Action::PerformanceStreet) {
        let settings = world.resource::<crate::ui::settings::SettingsUi>();
        let preferences = world.resource::<crate::performance::PerformancePreferences>();
        if settings.open
            || settings.captured
            || !world.resource::<crate::ui::PauseOpen>().0
            || preferences.profile != crate::performance::PerformanceProfile::Balanced
            || preferences.frame_cap != 30
            || preferences.render_scale != 70
            || !preferences.overlay
            || world.resource::<Campaign>().day_hours != 1
        {
            return Err(
                "Performance settings close changed preferences, resumed pause or altered this run"
                    .into(),
            );
        }
        run.report.assertions.push("Closing performance settings preserved the selected profile, 30 FPS request, 70% 3D resolution and overlay, kept the run paused and did not change its day length".into());
        world.resource_mut::<crate::ui::PauseOpen>().0 = false;
    }
    if run.frame == SETTLE_FRAMES / 2 && matches!(stage.action, Action::Trade) {
        let settings = world.resource::<crate::ui::settings::SettingsUi>();
        if settings.open
            || settings.captured
            || !world.resource::<crate::ui::PauseOpen>().0
            || world
                .resource::<crate::settings::GamePreferences>()
                .day_hours
                != 1
            || *world.resource::<crate::performance::PerformancePreferences>()
                != crate::performance::PerformancePreferences::default()
            || *world.resource::<crate::audio::AudioSettings>()
                != crate::audio::AudioSettings::default()
            || *world.resource::<crate::i18n::Language>() != crate::i18n::Language::ZhTw
            || world.resource::<Campaign>().day_hours != 1
            || world
                .resource::<crate::settings::SettingsStatus>()
                .error
                .is_some()
        {
            return Err(
                "Paused settings did not reset all pages and release capture before trade".into(),
            );
        }
        run.report.assertions.push(
            "Paused settings reset performance, audio, next-run day length and language; closing released capture while preserving pause and the current run".into(),
        );
        world.resource_mut::<crate::ui::PauseOpen>().0 = false;
        world.write_message(OpenPanel(PanelKind::Trade));
    }
    if run.frame == SETTLE_FRAMES / 2
        && matches!(
            stage.action,
            Action::Rain
                | Action::Night
                | Action::Blackout
                | Action::Exterior(_)
                | Action::Commercial(_)
                | Action::Streets(_)
                | Action::Lighting(_)
        )
    {
        // The explicit clock/weather fixture can dispatch several catch-up
        // broadcasts at once. Preserve their received-history flags, but clear
        // their overlays so the weather/lighting evidence shows the street.
        world.resource_mut::<Campaign>().notice_t = 0.0;
        world
            .resource_mut::<crate::worldplay::WorldNotice>()
            .text
            .clear();
    }
    if run.frame > MAX_STAGE_FRAMES {
        return Err(format!(
            "Stage {} did not complete: flow={:?}",
            stage.name,
            world.resource::<State<GameFlow>>().get()
        ));
    }
    if run.waiting_capture || run.frame < SETTLE_FRAMES || !flow_matches(world, stage.flow) {
        return Ok(());
    }
    if let Some(error) = &world.resource::<SaveStatus>().error {
        return Err(format!(
            "Save service failed during {}: {error}",
            stage.name
        ));
    }
    let font = world.resource::<UiFont>().0.clone();
    if world.resource::<Assets<Font>>().get(&font).is_none() {
        return Ok(());
    }
    if world
        .query_filtered::<Entity, With<crate::map::Chunk>>()
        .iter(world)
        .next()
        .is_none()
    {
        return Ok(());
    }
    if matches!(
        stage.action,
        Action::Street
            | Action::PerformanceStreet
            | Action::Entrance
            | Action::DoorOpen
            | Action::DoorClose
            | Action::Rain
            | Action::ShopInterior
            | Action::HomeInterior
            | Action::Exterior(_)
            | Action::Commercial(_)
            | Action::Streets(_)
            | Action::Lighting(_)
            | Action::Wave5(_)
            | Action::StreetDetail(_)
            | Action::Remnant
            | Action::Heli
    ) {
        let key = crate::map::ChunkKey::of(player_position(world)?.xz());
        let keys = [Some(key), run.scene_chunk];
        let chunks_ready = keys.into_iter().flatten().all(|key| {
            world
                .query::<&crate::map::Chunk>()
                .iter(world)
                .any(|chunk| chunk.key == key && chunk.lod == crate::map::Lod::Near)
        });
        let detail_ready = run.entrance_building.is_none_or(|id| {
            world
                .query::<&crate::map::BldWalls>()
                .iter(world)
                .any(|walls| walls.id == id && !walls.faded)
                && world
                    .query::<&crate::map::DetailRoot>()
                    .iter(world)
                    .any(|root| root.0 == id)
                && world
                    .query::<(&crate::map::BldDetail, &Visibility)>()
                    .iter(world)
                    .any(|(detail, visibility)| detail.0 == id && *visibility != Visibility::Hidden)
        });
        let interior_ready = run.interior_building.is_none_or(|id| {
            world
                .query::<&crate::map::interiors::InteriorView>()
                .iter(world)
                .any(|room| room.building == id)
        });
        let exterior_loaded = run
            .exterior
            .is_none_or(|fixture| exterior_ready(world, fixture));
        let commercial_loaded = run
            .commercial
            .is_none_or(|fixture| commercial_ready(world, fixture));
        let streets_loaded = run
            .streets
            .is_none_or(|fixture| street_ready(world, fixture));
        let lighting_loaded = run
            .lighting
            .is_none_or(|fixture| lighting_ready(world, fixture));
        if !chunks_ready
            || !detail_ready
            || !interior_ready
            || !exterior_loaded
            || !commercial_loaded
            || !streets_loaded
            || !lighting_loaded
        {
            if run.wait_logged.elapsed() >= Duration::from_secs(5) {
                let exterior = run
                    .exterior
                    .map(|fixture| exterior_wait_status(world, fixture))
                    .unwrap_or_default();
                let commercial = run
                    .commercial
                    .map(|fixture| commercial_wait_status(world, fixture))
                    .unwrap_or_default();
                println!(
                    "DTQA waiting stage={} frame={} chunks={} detail={} interior={} exterior={} commercial={} streets={} lighting={} {} {}",
                    stage.name,
                    run.frame,
                    chunks_ready,
                    detail_ready,
                    interior_ready,
                    exterior_loaded,
                    commercial_loaded,
                    streets_loaded,
                    lighting_loaded,
                    exterior,
                    commercial
                );
                run.wait_logged = Instant::now();
            }
            run.ready_frames = 0;
            return Ok(());
        }
        // Chunk entities appear before their GPU upload. Leave several complete
        // render intervals after all target geometry and details become ready.
        run.ready_frames += 1;
        if run.ready_frames < 4 {
            return Ok(());
        }
    }
    if matches!(stage.action, Action::DoorOpen | Action::DoorClose) {
        let id = run.door_test_id.ok_or("Missing door interaction test")?;
        let open = matches!(stage.action, Action::DoorOpen);
        let door = crate::map::world().blds[id as usize].door.unwrap();
        let obstacles = world.resource::<crate::map::Obstacles>();
        if obstacles.doors().is_open(id) != open || obstacles.blocked(door.p, PLAYER_RADIUS) == open
        {
            return Err(format!(
                "Door {id} interaction did not synchronize visible state and collision (expected open={open})"
            ));
        }
        if !world
            .query::<&crate::doors::DoorLeaf>()
            .iter(world)
            .any(|leaf| leaf.id == id)
        {
            return Err("Door collider has no visible streamed leaf".into());
        }
        run.report.assertions.push(format!(
            "Real Interact set door {id} open={open}; collision and streamed leaf match"
        ));
    }
    if matches!(stage.action, Action::Remnant) {
        let built = world
            .query::<(
                &crate::remnant::RemnantMan,
                &crate::human::RigParts,
                &Visibility,
            )>()
            .iter(world)
            .filter(|(man, _, vis)| man.squad == 0 && **vis != Visibility::Hidden)
            .count();
        let p = player_position(world)?.xz();
        let reach = world
            .resource::<crate::dialogue::NpcSpots>()
            .nearest(p, |_| true)
            .map(|s| s.key);
        if built < 4 || reach != Some(crate::dialogue::NpcKey::Remnant(0)) {
            run.ready_frames = 0;
            return Ok(());
        }
        let hint = world
            .resource::<crate::worldplay::InteractionHint>()
            .detail
            .clone();
        run.report.assertions.push(format!(
            "ROC remnant position 0: four rigged soldiers visible, sandbags in dynamic cover, Interact reaches the leader ({hint})"
        ));
    }
    if matches!(stage.action, Action::RemnantTalk) {
        let Some(card) = world.resource::<crate::dialogue::Dialogue>().0.clone() else {
            return Ok(());
        };
        let keys: Vec<_> = card.options.iter().map(|o| o.key).collect();
        if !keys.contains(&crate::dialogue::DialogueKey::RemnantSupply(0))
            || !keys.contains(&crate::dialogue::DialogueKey::RemnantIntel(0))
            || card.body.is_empty()
        {
            return Err(format!("Remnant card lacks its options: {keys:?}"));
        }
        run.report.assertions.push(format!(
            "Talking to {} opened the leader's card with {} options",
            card.title,
            card.options.len()
        ));
    }
    if matches!(stage.action, Action::Reclaim) {
        let (target, before) = run.reclaim.clone().ok_or("Missing reclaim fixture")?;
        let name = crate::sectors::SECTORS[target].zh;
        let taken = world
            .resource::<Campaign>()
            .reclaimed
            .iter()
            .any(|n| n == name);
        let title = world.resource::<crate::sectors::ReclaimTitle>().clone();
        if !taken || title.age.is_none() {
            return Ok(());
        }
        let bag = world.resource::<Inventory>();
        let gift = crate::sectors::reclaim_gift(target);
        if gift
            .iter()
            .any(|(key, n)| bag.count(key) != before.count(key) + n)
        {
            return Err(format!(
                "Reclaiming {name} did not hand over its gift exactly"
            ));
        }
        run.report.assertions.push(format!(
            "Killing every soldier assigned to {name} reclaimed it: title '{}', gift {gift:?} added to the bag",
            title.title
        ));
    }
    if matches!(stage.action, Action::Upstairs) {
        let up = world.resource::<crate::upstairs::Upstairs>();
        let Some((building, floor)) = up.floor() else {
            run.ready_frames = 0;
            return Ok(());
        };
        let fixtures = up.fixtures().len();
        let height = up.height();
        let y = player_position(world)?.y;
        if floor != 2 || fixtures == 0 || (y - height).abs() > 0.01 || height < 2.5 {
            return Err(format!(
                "Stairs did not reach a furnished second floor: floor={floor} fixtures={fixtures} y={y} height={height}"
            ));
        }
        if !world.resource::<crate::combat::PlayerStance>().upstairs {
            return Err("Soldiers still track the player upstairs".into());
        }
        run.ready_frames += 1;
        if run.ready_frames < 20 {
            return Ok(());
        }
        let hint = world
            .resource::<crate::worldplay::InteractionHint>()
            .clone();
        run.report.assertions.push(format!(
            "Real Interact on the landing climbed building {building} to floor 2 at {height:.1} m with {fixtures} fixtures; hint '{} / {}'",
            hint.label, hint.detail
        ));
    }
    if matches!(stage.action, Action::Rescue) {
        let state = world.resource::<crate::rescue::RescueState>().clone();
        let call = state.call.clone().ok_or("Rescue call vanished")?;
        if !call.done {
            run.ready_frames = 0;
            let on_floor = world.resource::<crate::upstairs::Upstairs>().floor()
                == Some((call.building, call.floor));
            let family = world
                .resource::<crate::dialogue::NpcSpots>()
                .0
                .iter()
                .any(|s| s.key == crate::dialogue::NpcKey::Family);
            if on_floor && family && world.resource::<crate::dialogue::Dialogue>().0.is_none() {
                world.write_message(crate::dialogue::TalkTo(crate::dialogue::NpcKey::Family));
            }
            return Ok(());
        }
        let card = world
            .resource::<crate::dialogue::Dialogue>()
            .0
            .clone()
            .ok_or("Rescue answered without the family's card")?;
        run.ready_frames += 1;
        if run.ready_frames < 10 {
            return Ok(());
        }
        run.report.assertions.push(format!(
            "Rescue: climbed to floor {} of building {} near {}, found the family and answered ('{}')",
            call.floor, call.building, call.street, card.title
        ));
    }
    if matches!(stage.action, Action::War) {
        let stats = *world.resource::<crate::war::WarStats>();
        let pending = crate::war::shells_pending(world);
        let before = run.war_grenades.ok_or("Missing grenade fixture")?;
        if stats.thrown == 0 || run.frame < SETTLE_FRAMES + 170 {
            return Ok(());
        }
        if world.resource::<Inventory>().count("grenade") + 1 != before {
            return Err("Throwing did not spend the grenade".into());
        }
        if pending < 2 || stats.shells < 3 || stats.rockets < 1 {
            return Err(format!(
                "War rounds missing: pending={pending} stats={stats:?}"
            ));
        }
        run.report.assertions.push(format!(
            "War: a real Use threw a grenade from the bag; {} mortar rounds and a rocket fall under red rings ({pending} still in the air), war level {:.2}",
            stats.shells, stats.level
        ));
    }
    if matches!(stage.action, Action::Sniper) {
        let Some(i) = world.resource::<crate::sniper::SniperLock>().0 else {
            return Ok(());
        };
        let name = world.resource::<crate::sniper::Posts>().0[i].name.clone();
        run.report.assertions.push(format!(
            "Sniper over {name} locked on the player in the open by day (glint and laser before the shot)"
        ));
    }
    if matches!(stage.action, Action::Airdrop) {
        world.resource_mut::<crate::dialogue::Dialogue>().0 = None;
        if world.resource::<crate::japan::JapanStats>().landed == 0 {
            run.ready_frames = 0;
            return Ok(());
        }
        run.ready_frames += 1;
        if run.ready_frames < 45 {
            return Ok(());
        }
        let progress = world.resource::<crate::worldplay::ScavengeState>();
        let Some((id, at)) = progress
            .drops
            .iter()
            .find(|(id, _)| id.starts_with("jpdrop"))
            .cloned()
        else {
            return Err("Airdrop landed without a crate".into());
        };
        let stock = progress
            .loot
            .iter()
            .find(|(key, _)| *key == id)
            .map(|(_, s)| s.clone())
            .ok_or("Airdrop crate has no stock")?;
        if stock.count("type20") + stock.count("type89") != 1 || stock.count("a556n") != 60 {
            return Err(format!(
                "Airdrop crate contents are wrong: {:?}",
                stock.stacks()
            ));
        }
        let state = world.resource::<crate::japan::JapanState>();
        if !state.met || state.ready(crate::japan::JpKind::Drop) {
            return Err("Airdrop did not spend its sortie".into());
        }
        let p = player_position(world)?.xz();
        run.report.assertions.push(format!(
            "JSDF radio: the C-2 crate landed {:.0} m from the player with {:?}; the call spent a battery and started the 6-minute cooldown",
            Vec2::from_array(at).distance(p),
            stock.stacks()
        ));
    }
    if matches!(stage.action, Action::Heli) {
        let stats = *world.resource::<crate::heli::HeliStats>();
        if stats.spawned == 0 {
            run.ready_frames = 0;
            return Ok(());
        }
        let spotlights = world.query::<&SpotLight>().iter(world).count();
        if spotlights > 6 {
            return Err(format!("Heli spawned extra spotlights ({spotlights} > 6)"));
        }
        run.ready_frames += 1;
        if run.ready_frames < 20 {
            return Ok(());
        }
        run.report.assertions.push(format!(
            "PLA Heli v24.24: spawned Z-10 gunship overhead ({} spawned, {spotlights} city spotlights)",
            stats.spawned
        ));
    }
    if matches!(
        stage.action,
        Action::Barricade
            | Action::BarricadeBlocked
            | Action::Unbarricade
            | Action::BarricadeInventory
    ) {
        let id = run.door_test_id.ok_or("Missing barricade check")?;
        let expected = !matches!(stage.action, Action::Unbarricade);
        let obstacles = world.resource::<crate::map::Obstacles>();
        let door = crate::map::world().blds[id as usize].door.unwrap();
        let base = run.barricade_planks.ok_or("Missing plank baseline")?;
        if obstacles.doors().is_barricaded(id) != expected
            || obstacles.doors().is_open(id)
            || !obstacles.blocked(door.p, PLAYER_RADIUS)
            || world.resource::<Inventory>().count("plank") != base - u32::from(expected)
            || world.resource::<Inventory>().count("hammer") == 0
        {
            return Err(format!(
                "Barricade action failed collision or material conservation at {id}: expected barricaded={expected}"
            ));
        }
        run.report.assertions.push(format!("{}: door {id} barricaded={expected}, cannot open while boarded, plank count conserved and hammer retained", stage.name));
    }
    if matches!(
        stage.action,
        Action::Scavenge | Action::ScavengeOne | Action::ScavengeAll | Action::ShopDepleted
    ) {
        let id = run.loot_fixture.as_ref().ok_or("Missing shelf check")?;
        let stock = world
            .resource::<crate::worldplay::ScavengeState>()
            .loot
            .iter()
            .find(|(key, _)| key == id)
            .map(|(_, stock)| stock);
        if stock != run.loot_expected.as_ref()
            || Some(world.resource::<Inventory>()) != run.loot_inventory.as_ref()
        {
            return Err(format!(
                "Selective loot did not conserve exact shelf and bag contents during {}",
                stage.name
            ));
        }
        if !matches!(stage.action, Action::ShopDepleted) {
            let session = world.resource::<crate::worldplay::ScavengeSession>();
            if !session.reachable
                || session
                    .target
                    .as_ref()
                    .is_none_or(|target| &target.id != id)
                || !world.resource::<crate::ui::UiModal>().open
            {
                return Err("Selective loot panel is not attached to its reachable fixture".into());
            }
        }
        run.report.assertions.push(format!(
            "{}: exact shelf/bag conservation verified for {id}",
            stage.name
        ));
    }
    if matches!(stage.action, Action::Map) {
        if !world.resource::<crate::worldmap::MapOpen>().0
            || !world.resource::<crate::ui::UiModal>().open
            || world.resource::<crate::worldmap::MapProgress>().notes.len() != 2
        {
            return Err(
                "Survivor map did not open with input capture and two saved annotations".into(),
            );
        }
        run.report.assertions.push(
            "Survivor map opens with real exploration, two annotations and modal input capture"
                .into(),
        );
    }
    if matches!(
        stage.action,
        Action::SettingsAudio
            | Action::SettingsGame
            | Action::SettingsClose
            | Action::SettingsPause
    ) {
        use crate::ui::settings::{SettingsPage, SettingsUi};
        let settings = world.resource::<SettingsUi>();
        let preferences = world.resource::<crate::settings::GamePreferences>();
        let audio = world.resource::<crate::audio::AudioSettings>();
        let correct = match stage.action {
            Action::SettingsAudio => {
                settings.open
                    && settings.page == SettingsPage::Audio
                    && (audio.master - 0.65).abs() < 0.001
                    && (audio.effects - 0.6).abs() < 0.001
                    && (audio.ambience - 0.5).abs() < 0.001
                    && (audio.radio - 0.4).abs() < 0.001
            }
            Action::SettingsGame => {
                settings.open
                    && settings.page == SettingsPage::Game
                    && preferences.day_hours == 2
                    && *world.resource::<crate::i18n::Language>() == crate::i18n::Language::En
            }
            Action::SettingsClose => {
                !settings.open
                    && !settings.captured
                    && preferences.day_hours == 1
                    && *audio == crate::audio::AudioSettings::default()
                    && *world.resource::<crate::performance::PerformancePreferences>()
                        == crate::performance::PerformancePreferences::default()
                    && *world.resource::<crate::i18n::Language>() == crate::i18n::Language::ZhTw
            }
            Action::SettingsPause => {
                settings.open
                    && preferences.day_hours == 4
                    && world.resource::<Campaign>().day_hours == 1
                    && world.resource::<crate::ui::PauseOpen>().0
            }
            _ => false,
        };
        if !correct
            || world
                .resource::<crate::settings::SettingsStatus>()
                .error
                .is_some()
        {
            return Err(format!(
                "Settings actions, persistence or current-run isolation failed in {}",
                stage.name
            ));
        }
        run.report.assertions.push(format!("{}: settings actions, independent preferences, modal state and current-run day length verified", stage.name));
    }
    if matches!(
        stage.action,
        Action::SettingsPerformance
            | Action::SettingsPerformanceLandscape
            | Action::SettingsDiagnostics
            | Action::PerformanceStreet
    ) {
        use crate::performance::{
            EffectivePerformance, PerformancePreferences, PerformanceProfile,
        };
        use crate::ui::settings::{PerformanceSection, SettingsPage, SettingsUi};
        let preferences = world.resource::<PerformancePreferences>();
        let effective = world.resource::<EffectivePerformance>();
        let dimensions = world.resource::<crate::performance::render::RenderDimensions>();
        let settings = world.resource::<SettingsUi>();
        let diagnostics = matches!(stage.action, Action::SettingsDiagnostics);
        let closed = matches!(stage.action, Action::PerformanceStreet);
        if preferences.profile != PerformanceProfile::Balanced
            || preferences.frame_cap != 30
            || preferences.render_scale != 70
            || effective.render_scale != 70
            || effective.target_fps != 30
            || dimensions.ui != stage.size
            || dimensions.scale != 70
            || (!closed && (!settings.open || settings.page != SettingsPage::Performance))
            || (!closed
                && settings.performance_section
                    != if diagnostics {
                        PerformanceSection::Diagnostics
                    } else {
                        PerformanceSection::Display
                    })
            || (closed
                && (settings.open
                    || settings.captured
                    || world.resource::<crate::ui::PauseOpen>().0))
        {
            return Err(format!(
                "Performance settings, render policy or modal state mismatch in {}",
                stage.name
            ));
        }
        let scene = dimensions.scene;
        for (actual, full) in scene.into_iter().zip(stage.size) {
            let expected = full as f32 * 0.7;
            if (actual as f32 - expected).abs() > 2.0 || actual >= full {
                return Err(format!(
                    "{}: 70% scene extent {scene:?} does not match native UI {:?}",
                    stage.name, stage.size
                ));
            }
        }
        let target = world
            .query_filtered::<&RenderTarget, With<GameCamera>>()
            .iter(world)
            .next()
            .ok_or("Game camera has no render target")?;
        let RenderTarget::Image(target) = target else {
            return Err("Game camera bypassed the scaled scene image".into());
        };
        let image = world
            .resource::<Assets<Image>>()
            .get(&target.handle)
            .ok_or("Scaled scene image is missing")?;
        if image.width() != scene[0] || image.height() != scene[1] {
            return Err("Reported scene extent does not match actual render texture".into());
        }
        if world
            .resource::<crate::settings::SettingsStatus>()
            .error
            .is_some()
        {
            return Err("Performance settings could not be saved".into());
        }
        run.report.assertions.push(format!("{}: requested 30 FPS and 70% scene image {}x{} with independent native {}x{} UI, actual render texture and settings state verified; fixed-timestep visual QA is not a device benchmark", stage.name, scene[0], scene[1], stage.size[0], stage.size[1]));
    }
    if matches!(stage.action, Action::Trade) {
        let settings = world.resource::<crate::ui::settings::SettingsUi>();
        if world.resource::<crate::ui::PauseOpen>().0
            || settings.open
            || settings.captured
            || !world.resource::<crate::ui::UiModal>().open
        {
            return Err("Deferred trade panel did not open after closing paused settings".into());
        }
        run.report.assertions.push(
            "Deferred trade panel acquired modal input after settings closed and pause was released"
                .into(),
        );
    }
    if matches!(
        stage.action,
        Action::Rain | Action::Night | Action::Blackout
    ) {
        let atmosphere = world.resource::<crate::map::atmosphere::AtmosphereState>();
        let ok = match stage.action {
            Action::Rain => {
                atmosphere.rain_streaks > 0
                    && atmosphere.rain_streaks <= 240
                    && atmosphere.wetness > 0.5
            }
            Action::Night => {
                atmosphere.night > 0.9
                    && atmosphere.power
                    && atmosphere.active_lights > 0
                    && atmosphere.active_lights <= 6
            }
            Action::Blackout => {
                atmosphere.night > 0.9 && !atmosphere.power && atmosphere.active_lights == 0
            }
            _ => unreachable!(),
        };
        if !ok {
            return Err(format!(
                "Atmosphere mismatch for {}: {atmosphere:?}",
                stage.name
            ));
        }
        run.report.assertions.push(format!(
            "{}: night={:.2}, power={}, wetness={:.2}, lights={}, rain={}",
            stage.name,
            atmosphere.night,
            atmosphere.power,
            atmosphere.wetness,
            atmosphere.active_lights,
            atmosphere.rain_streaks
        ));
    }
    if matches!(stage.action, Action::Continue) && !run.continuation_checked {
        check_continuation(world, run)?;
    }
    // State alone is insufficient: a repeatedly rebuilt subtree can have
    // open=true while its extracted UI is blank. Check real layout/visibility,
    // then also verify opaque panel and selected-control pixels after readback.
    let settings_pixels = settings_pixels(world, stage.size)?;
    let exterior = run
        .exterior
        .map(|fixture| exterior_evidence(world, fixture, stage.size))
        .transpose()?;
    let commercial = run
        .commercial
        .map(|fixture| commercial_evidence(world, fixture, stage.size))
        .transpose()?;
    if let Some(evidence) = &commercial {
        if matches!(
            stage.action,
            Action::Commercial(CommercialScene::Night | CommercialScene::Blackout)
        ) {
            let day = run
                .report
                .captures
                .iter()
                .find_map(|capture| {
                    capture
                        .commercial
                        .as_ref()
                        .filter(|value| value.scene == "Food")
                })
                .ok_or("Commercial night comparison is missing its daylight capture")?;
            if evidence.building_id != day.building_id
                || evidence.player != day.player
                || evidence.camera != day.camera
                || evidence.zoom != day.zoom
                || evidence.projected_bounds != day.projected_bounds
            {
                return Err(
                    "Commercial day/night/blackout camera or feature bounds drifted".into(),
                );
            }
        }
        run.report.assertions.push(format!(
            "{}: real lot {} {} sign atlas cell {}, {} solid-flank panels, canopy={}, blade={}, {} commercial vertices within verified uploaded base/shell batches; night={:.2} power={} sign_emissive={:?}; actual main-camera visibility and storefront framing checked",
            stage.name, evidence.building_id, evidence.family, evidence.sign_index,
            evidence.storefront_panels, evidence.canopy, evidence.blade, evidence.commercial_vertices,
            evidence.night, evidence.power, evidence.sign_emissive
        ));
    }
    let (streets, street_pixels) = if let Some(fixture) = run.streets {
        let (evidence, probes) = street_evidence(world, fixture, stage.size)?;
        run.report.assertions.push(format!(
            "{}: real road segment {:?}, source props {:?}, {} parking bays, {} feature vertices retained in street batches with verified upload material/count/bounds metadata; normal camera, clear player position, projected bounds and main-view membership checked; projected white markings also require GPU pixel evidence",
            stage.name, evidence.road_segment, evidence.prop_indices,
            evidence.parking_bays, evidence.feature_vertices
        ));
        (Some(evidence), probes)
    } else {
        (None, Vec::new())
    };
    let (lighting, lighting_pixels) = if let Some(fixture) = run.lighting {
        let mut evidence = lighting_evidence(world, fixture, &run.lighting_observations)?;
        let pixels = if matches!(
            fixture.scene,
            LightingScene::NoSpillControl | LightingScene::Blackout | LightingScene::Restored
        ) {
            if run.lighting_reference_probes.is_empty() {
                return Err("Lighting A/B sequence has no canonical Night probes".into());
            }
            if let Some(night) = run
                .report
                .captures
                .iter()
                .filter_map(|capture| capture.lighting.as_ref())
                .find(|capture| capture.scene == "Night")
            {
                evidence.probe_screening = night.probe_screening.clone();
            }
            run.lighting_reference_probes.clone()
        } else {
            lighting_pixel_probes(world, fixture, stage.size, &mut evidence)?
        };
        if fixture.scene == LightingScene::Night {
            run.lighting_reference_probes = pixels.clone();
        }
        verify_lighting_sequence(run, &evidence)?;
        run.report.assertions.push(format!("{}: actual {} shadowless city SpotLight entities, {} with nonzero intensity, {} stable selection observations; source geometry, actual transforms/directions and power/material states checked; diagnostic_direct_light_override={}; projected receiver pixels require controlled A/B GPU evidence, not emission alone", stage.name, evidence.actual_pool_entities, evidence.actual_active_lights, evidence.stable_observation_frames, evidence.diagnostic_direct_light_override));
        (Some(evidence), pixels)
    } else {
        (None, Vec::new())
    };
    let wave5 = match stage.action {
        Action::Wave5(scene) => Some(wave5::evidence(world, scene)?),
        Action::Continue => {
            let mut evidence = run
                .transport_restore
                .clone()
                .ok_or("Missing transport load-frame evidence")?;
            evidence["scene"] = serde_json::json!("Continued");
            evidence["transport_restore_exact"] =
                serde_json::json!(run.transport_continuation_checked);
            evidence["measurement"] = serde_json::json!(
                "saved, expected and restored states captured and compared in load frame before simulation resumed; expected differs only by zeroing occupied vehicle speed"
            );
            evidence["vehicles"] =
                serde_json::to_value(world.resource::<crate::vehicles::VehicleFleet>()).unwrap();
            evidence["current"] =
                serde_json::json!(world.resource::<crate::vehicles::VehicleFleet>().current);
            evidence["armor"] =
                serde_json::to_value(world.resource::<crate::combat::armor::ArmorFleet>()).unwrap();
            Some(evidence)
        }
        Action::StreetDetail(scene) => match wave5_exterior::evidence(world, scene) {
            Ok(evidence) => Some(evidence),
            Err(reason) => {
                if run.frame > 900 {
                    return Err(reason);
                }
                if run.wait_logged.elapsed() > Duration::from_secs(15) {
                    println!("DTQA wave5 pending: {reason}");
                    run.wait_logged = Instant::now();
                }
                return Ok(());
            }
        },
        _ => None,
    };
    if let Some(evidence) = &wave5 {
        run.report.assertions.push(format!(
            "{}: native Wave 5 {:?} action/state/upload/main-view evidence passed",
            stage.name,
            evidence.get("scene")
        ));
    }
    let map_workload = matches!(stage.flow, ExpectedFlow::Playing)
        .then(|| map_workload(world))
        .flatten();
    if let Some(evidence) = &exterior {
        run.report.assertions.push(format!(
            "{}: actual lot {} {}, persistent shell {} vertices, facade detail {} vertices, roof fittings {} vertices across {} unique material meshes; cutaway_hidden={}, full_exterior_framing={}; merged shell includes coarse facade and roof; real GPU readback, workload counts are not a device benchmark",
            stage.name, evidence.building_id, evidence.typology,
            evidence.persistent_shell.vertices, evidence.facade_detail.vertices, evidence.roof_fittings.vertices,
            evidence.total_meshes, evidence.cutaway_hidden, !evidence.cutaway_hidden
        ));
    }
    let image = run.target.clone().ok_or("No render target")?;
    world
        .spawn((
            Screenshot::image(image),
            CaptureTicket {
                name: stage.name.into(),
                path: run.output.join(format!("{}.png", stage.name)),
                size: stage.size,
                settings_pixels,
                exterior,
                commercial,
                streets,
                street_pixels,
                lighting,
                lighting_pixels,
                map_workload,
                wave5,
            },
        ))
        .observe(capture_finished);
    run.waiting_capture = true;
    Ok(())
}

fn settings_pixels(world: &mut World, size: [u32; 2]) -> Result<Option<SettingsPixels>, String> {
    use crate::ui::settings::{PerformanceSection, SettingsPage, SettingsRoot, SettingsUi};
    let settings = world.resource::<SettingsUi>();
    if !settings.open {
        return Ok(None);
    }
    let page = settings.page;
    let section = settings.performance_section;
    let profile = world
        .resource::<crate::performance::PerformancePreferences>()
        .profile;
    let presentation = world
        .query_filtered::<Entity, With<crate::performance::render::PresentationCamera>>()
        .single(world)
        .map_err(|_| "Missing unique UI presentation camera")?;
    let mut roots = world.query_filtered::<(
        &InheritedVisibility,
        &ComputedNode,
        &ComputedUiTargetCamera,
        &Children,
    ), With<SettingsRoot>>();
    let (visibility, node, target, children) = roots
        .single(world)
        .map_err(|_| "Open settings did not have exactly one rendered root")?;
    if !visibility.get()
        || node
            .size()
            .distance(Vec2::new(size[0] as f32, size[1] as f32))
            > 1.0
        || target.get() != Some(presentation)
        || children.len() < 6
    {
        return Err(format!(
            "Open settings root was not visible, laid out at full viewport, or attached to presentation camera: visible={} size={:?} children={}",
            visibility.get(),
            node.size(),
            children.len()
        ));
    }
    let layout = crate::ui::settings::settings_layout(Vec2::new(size[0] as f32, size[1] as f32));
    let selected = match page {
        SettingsPage::Audio => layout.tabs[0],
        SettingsPage::Game | SettingsPage::Controls => layout.tabs[1],
        SettingsPage::Performance if section == PerformanceSection::Diagnostics => {
            layout.performance_sections[1]
        }
        SettingsPage::Performance => {
            layout.profiles[match profile {
                crate::performance::PerformanceProfile::Auto => 0,
                crate::performance::PerformanceProfile::Battery => 1,
                crate::performance::PerformanceProfile::Balanced => 2,
                crate::performance::PerformanceProfile::Quality => 3,
            }]
        }
    };
    let mut panel = Vec::with_capacity(6);
    let mut selected_border = Vec::with_capacity(6);
    for fraction in [0.25, 0.5, 0.75] {
        let y = (layout.frame.min.y + layout.frame.height() * fraction).round() as u32;
        panel.push([(layout.frame.min.x + 4.0).round() as u32, y]);
        panel.push([(layout.frame.max.x - 4.0).round() as u32, y]);
        let x = (selected.min.x + selected.width() * fraction).round() as u32;
        selected_border.push([x, selected.min.y.ceil() as u32]);
        selected_border.push([x, selected.max.y.ceil() as u32 - 1]);
    }
    Ok(Some(SettingsPixels {
        panel,
        selected_border,
    }))
}

fn capture_finished(
    event: On<ScreenshotCaptured>,
    tickets: Query<&CaptureTicket>,
    mut run: ResMut<QaRun>,
) {
    let Ok(ticket) = tickets.get(event.entity) else {
        return;
    };
    let result = (|| -> Result<Capture, String> {
        let image = event
            .image
            .clone()
            .try_into_dynamic()
            .map_err(|err| err.to_string())?
            .to_rgba8();
        if image.width() != ticket.size[0] || image.height() != ticket.size[1] {
            return Err(format!("{} has unexpected dimensions", ticket.name));
        }
        if let Some(probes) = &ticket.settings_pixels {
            let matches = |point: [u32; 2], expected: [u8; 3], tolerance: u8| {
                let pixel = image.get_pixel(point[0], point[1]).0;
                (0..3).all(|channel| pixel[channel].abs_diff(expected[channel]) <= tolerance)
            };
            // These clear interior margins and straight selected-button edges
            // distinguish a real settings overlay from its underlying title.
            // Merely counting colors accepted the wrong screen in this bug.
            if !probes
                .panel
                .iter()
                .all(|point| matches(*point, [15, 19, 18], 2))
                || !probes
                    .selected_border
                    .iter()
                    .all(|point| matches(*point, [209, 173, 102], 8))
            {
                return Err(format!(
                    "{}: captured pixels do not contain the opaque settings panel and selected control border",
                    ticket.name
                ));
            }
        }
        let mut streets = ticket.streets.clone();
        if let Some(evidence) = streets.as_mut() {
            let neighborhood = |point: [u32; 2]| {
                let mut pixels = Vec::with_capacity(9);
                for dx in -1_i32..=1 {
                    for dy in -1_i32..=1 {
                        let x = point[0].saturating_add_signed(dx).min(image.width() - 1);
                        let y = point[1].saturating_add_signed(dy).min(image.height() - 1);
                        let pixel = image.get_pixel(x, y).0;
                        pixels.push([pixel[0], pixel[1], pixel[2]]);
                    }
                }
                pixels.sort_by_key(|rgb| street_pixel_luma(*rgb));
                pixels
            };
            let pixels: Vec<_> = ticket
                .street_pixels
                .iter()
                .map(|probe| {
                    let asphalt_rgb = neighborhood(probe.asphalt)[4];
                    let nearby_paint = neighborhood(probe.paint);
                    let paint_rgb = nearby_paint
                        .iter()
                        .copied()
                        .rev()
                        .find(|rgb| street_marking_pixel_match(*rgb, asphalt_rgb))
                        .unwrap_or(nearby_paint[8]);
                    StreetPixelEvidence {
                        point: probe.paint,
                        asphalt_point: probe.asphalt,
                        paint_rgb,
                        asphalt_rgb,
                        paint_luma: street_pixel_luma(paint_rgb),
                        asphalt_luma: street_pixel_luma(asphalt_rgb),
                        matched: street_marking_pixel_match(paint_rgb, asphalt_rgb),
                    }
                })
                .collect();
            let hits = pixels.iter().filter(|pixel| pixel.matched).count();
            evidence.marking_pixels = pixels;
            evidence.marking_pixel_samples = ticket.street_pixels.len();
            evidence.marking_pixel_hits = hits;
            if !ticket.street_pixels.is_empty() && hits * 4 < ticket.street_pixels.len() {
                // Preserve the failed readback and every paired RGB/coordinate so
                // a renderer/fixture failure is directly reviewable in CI.
                image.save(&ticket.path).map_err(|err| err.to_string())?;
                let debug = serde_json::to_vec_pretty(evidence).map_err(|err| err.to_string())?;
                fs::write(ticket.path.with_extension("pixel-failure.json"), debug)
                    .map_err(|err| err.to_string())?;
                return Err(format!(
                    "{}: white road/parking paint lacks paired asphalt contrast in projected GPU samples ({hits}/{})",
                    ticket.name,
                    ticket.street_pixels.len()
                ));
            }
        }
        let mut lighting = ticket.lighting.clone();
        if let Some(evidence) = lighting.as_mut() {
            for probe in &ticket.lighting_pixels {
                let mut samples = Vec::with_capacity(9);
                for dy in -1_i32..=1 {
                    for dx in -1_i32..=1 {
                        let x = probe.point[0]
                            .saturating_add_signed(dx)
                            .min(image.width() - 1);
                        let y = probe.point[1]
                            .saturating_add_signed(dy)
                            .min(image.height() - 1);
                        let p = image.get_pixel(x, y).0;
                        samples.push([p[0], p[1], p[2]]);
                    }
                }
                samples.sort_by_key(|rgb| street_pixel_luma(*rgb));
                let rgb = samples[4];
                evidence.probes.push(LightingPixelEvidence {
                    probe: probe.clone(),
                    rgb,
                    luma: street_pixel_luma(rgb),
                });
            }
            if let Err(error) = lighting_pixel_comparison(&run.report.captures, evidence) {
                image.save(&ticket.path).map_err(|err| err.to_string())?;
                let debug = serde_json::to_vec_pretty(evidence).map_err(|err| err.to_string())?;
                fs::write(ticket.path.with_extension("pixel-failure.json"), debug)
                    .map_err(|err| err.to_string())?;
                return Err(format!("{}: {error}", ticket.name));
            }
        }
        let colors: HashSet<_> = image
            .pixels()
            .step_by(37)
            .map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect();
        if colors.len() < 8 {
            return Err(format!(
                "{} appears blank ({} sampled colors)",
                ticket.name,
                colors.len()
            ));
        }
        image.save(&ticket.path).map_err(|err| err.to_string())?;
        Ok(Capture {
            stage: ticket.name.clone(),
            file: ticket.path.display().to_string(),
            width: image.width(),
            height: image.height(),
            sampled_colors: colors.len(),
            exterior: ticket.exterior.clone(),
            commercial: ticket.commercial.clone(),
            streets,
            lighting,
            map_workload: ticket.map_workload.clone(),
            wave5: ticket.wave5.clone(),
        })
    })();
    match result {
        Ok(capture) => {
            println!(
                "DTQA captured {} {}x{} colors={}",
                capture.stage, capture.width, capture.height, capture.sampled_colors
            );
            run.report.captures.push(capture);
            run.captured = true;
        }
        Err(error) => run.report.error = Some(error),
    }
}

#[cfg(test)]
mod exterior_tests {
    use super::*;

    #[test]
    fn lighting_sequence_reuses_reachable_original_fixture_and_three_real_source_families() {
        use crate::map::atmosphere::{
            CITY_LIGHT_COUNT, LightKind, LightSiteIndex, plan_city_lights,
        };
        let data = crate::map::world();
        let day = lighting_fixture(data, LightingScene::Day, [1280, 720]).unwrap();
        for scene in [
            LightingScene::Dusk,
            LightingScene::Night,
            LightingScene::NoSpillControl,
            LightingScene::Blackout,
            LightingScene::Restored,
        ] {
            let fixture = lighting_fixture(data, scene, [1280, 720]).unwrap();
            assert_eq!(
                (fixture.building, fixture.position, fixture.zoom),
                (day.building, day.position, day.zoom)
            );
            assert!(street_player_position_clear(data, fixture.position));
        }
        let sites = plan_city_lights(data);
        let slots =
            LightSiteIndex::new(&sites).select(&sites, day.position, &[None; CITY_LIGHT_COUNT]);
        let mut kinds = HashSet::new();
        for id in slots.into_iter().flatten() {
            let site = sites.iter().find(|site| site.id == id).unwrap();
            assert!(site.position.xz().distance(day.position) <= 32.0);
            lighting_source_bounds(site).unwrap();
            kinds.insert(site.kind);
        }
        assert_eq!(
            kinds,
            HashSet::from([LightKind::Street, LightKind::Sign, LightKind::Arcade])
        );
        assert_eq!(
            STAGES
                .iter()
                .filter(|stage| matches!(stage.action, Action::Lighting(_)))
                .count(),
            6
        );
        assert_eq!(STAGES.len(), 89);
        eprintln!(
            "Lighting fixture lot={} player={:?} zoom={} selected={slots:?}",
            day.building, day.position, day.zoom
        );
    }

    #[test]
    fn street_camera_rejects_vehicle_interiors_and_decorative_scooter_overlap() {
        use crate::map::street::PropKind;
        let data = crate::map::world();
        for (kind, half_width, half_length) in [
            (PropKind::Taxi, 0.9, 2.2),
            (PropKind::Truck, 1.15, 3.25),
            (PropKind::Scooter, 0.36, 0.87),
        ] {
            let (prop, outside) = data
                .props
                .iter()
                .filter(|prop| prop.kind == kind && data.free(prop.p, 0.6))
                .find_map(|prop| {
                    let along = Vec2::new(prop.ang.sin(), prop.ang.cos());
                    let across = Vec2::new(prop.ang.cos(), -prop.ang.sin());
                    [
                        across * (half_width + 1.0),
                        -across * (half_width + 1.0),
                        along * (half_length + 1.0),
                        -along * (half_length + 1.0),
                    ]
                    .into_iter()
                    .map(|offset| prop.p + offset)
                    .find(|point| {
                        data.road_dist(*point) < 0.0 && street_player_position_clear(data, *point)
                    })
                    .map(|outside| (prop, outside))
                })
                .expect(
                    "real parked prop with a perimeter-clear centre and an outside street point",
                );
            assert!(!street_player_position_clear(data, prop.p));
            assert!(street_player_position_clear(data, outside));
        }
    }

    #[test]
    fn street_pixel_probes_reject_foreground_prop_surfaces() {
        let centre = crate::camera::CAM_OFFSET.normalize() * 2.0;
        let right = Vec3::new(1.0, 0.0, -1.0).normalize();
        let up = crate::camera::CAM_OFFSET.normalize().cross(right);
        let triangle = [centre - right - up, centre + right - up, centre + up];
        assert!(street_prop_occludes(Vec3::ZERO, &[triangle]));
        assert!(!street_prop_occludes(right * 4.0, &[triangle]));
        assert!(!street_prop_occludes(centre * 2.0, &[triangle]));
    }

    #[test]
    fn street_marking_pixels_reject_real_asphalt_negative_controls() {
        // Unpainted carriageway pixels from the verified CI77 04a capture.
        let asphalt = [
            [114, 109, 98],
            [114, 110, 99],
            [120, 115, 104],
            [119, 115, 104],
            [120, 116, 104],
        ];
        for paint in asphalt {
            for road in asphalt {
                assert!(!street_marking_pixel_match(paint, road));
            }
            assert!(street_marking_pixel_match([238, 234, 219], paint));
        }
        // Bright neutral pavement or UI alone also fails without local contrast.
        assert!(!street_marking_pixel_match(
            [220, 218, 208],
            [210, 208, 198]
        ));
    }

    #[test]
    fn street_views_use_existing_sources_clear_positions_and_admitted_features() {
        use crate::map::street::PropKind;
        let data = crate::map::world();
        let catalog = StreetCatalog::new(data);
        for (scene, size) in [
            (StreetScene::Junction, [1280, 720]),
            (StreetScene::Scooters, [720, 1280]),
            (StreetScene::Taxi, [1280, 720]),
            (StreetScene::Truck, [1280, 720]),
        ] {
            let fixture = street_fixture(data, scene, size).unwrap();
            let features = street_features(data, fixture, &catalog).unwrap();
            assert!(street_player_position_clear(data, fixture.position));
            assert!(crate::camera::ZOOM_RANGE.contains(&fixture.zoom));
            assert!(
                features
                    .parts
                    .iter()
                    .all(|(owner, _)| owner.dist(fixture.position) < 58.0)
            );
            assert!(
                features
                    .parts
                    .iter()
                    .map(|(_, parts)| parts.vcount())
                    .sum::<usize>()
                    >= 40
            );
            match scene {
                StreetScene::Junction => {
                    assert!(features.prop_indices.is_empty());
                    assert!(features.approach_arrow);
                    assert!(features.junction_clearance.is_some_and(|value| value > 0.0));
                }
                StreetScene::Scooters => {
                    assert!(features.parking_bays >= 2);
                    assert!(
                        features
                            .prop_indices
                            .iter()
                            .filter(|index| data.props[**index].kind == PropKind::Scooter)
                            .count()
                            >= 2
                    );
                    assert_eq!(
                        features
                            .prop_indices
                            .iter()
                            .filter(|index| data.props[**index].kind == PropKind::Lamp)
                            .count(),
                        1
                    );
                }
                StreetScene::Taxi | StreetScene::Truck => {
                    assert_eq!(features.prop_indices.len(), 1);
                    let expected = if scene == StreetScene::Taxi {
                        PropKind::Taxi
                    } else {
                        PropKind::Truck
                    };
                    assert_eq!(data.props[features.prop_indices[0]].kind, expected);
                }
            }
            eprintln!(
                "Street fixture {scene:?}: source={:?}, props={:?}, bays={}, position={:?}, zoom={:.3}",
                fixture.subject,
                features.prop_indices,
                features.parking_bays,
                fixture.position,
                fixture.zoom
            );
        }
    }

    #[test]
    fn commercial_views_use_real_features_clear_positions_and_identical_lighting_cameras() {
        use crate::map::commercial::{ShopFamily, plan};

        let data = crate::map::world();
        let size = [1280, 720];
        let food = commercial_fixture(data, CommercialScene::Food, size).unwrap();
        for scene in [
            CommercialScene::Arcade,
            CommercialScene::Food,
            CommercialScene::Essentials,
            CommercialScene::Night,
            CommercialScene::Blackout,
        ] {
            let fixture = commercial_fixture(data, scene, size).unwrap();
            let building = &data.blds[fixture.building as usize];
            let layout = plan(building, data).unwrap();
            assert!(data.bld_at(fixture.position).is_none());
            assert!(data.free(fixture.position, 0.6));
            assert!(!crate::map::exterior_blocks_camera(
                building,
                fixture.position
            ));
            assert!(crate::camera::ZOOM_RANGE.contains(&fixture.zoom));
            assert!(building.c.distance(fixture.position) < 65.0);
            assert!(!layout.panels.is_empty());
            assert_eq!(layout.sign_index, building.sign.unwrap().idx);
            match scene {
                CommercialScene::Arcade => {
                    assert!(building.arcade.is_some());
                    assert!(layout.panels.len() >= 2);
                    assert!(layout.secondary_fascia);
                    assert!((8.0..=16.0).contains(&layout.frontage.width));
                    assert!(building.h <= 12.0);
                }
                CommercialScene::Essentials => {
                    assert!(matches!(
                        layout.family,
                        ShopFamily::Pharmacy | ShopFamily::Convenience
                    ));
                }
                _ => {
                    assert_eq!(layout.family, ShopFamily::Food);
                    assert!(building.arcade.is_none());
                    assert!(layout.canopy.is_some());
                    assert!(layout.blade.is_some());
                    assert!(layout.frontage.u.dot(Vec2::ONE.normalize()).abs() >= 0.35);
                    assert_eq!(fixture.building, food.building);
                    assert_eq!(fixture.position, food.position);
                    assert_eq!(fixture.zoom, food.zoom);
                }
            }
            eprintln!(
                "Commercial fixture {scene:?}: lot={} family={:?} panels={} canopy={} blade={} position={:?} zoom={:.3} frontage={:.2} height={:.2}",
                building.id,
                layout.family,
                layout.panels.len(),
                layout.canopy.is_some(),
                layout.blade.is_some(),
                fixture.position,
                fixture.zoom,
                layout.frontage.width,
                building.h
            );
        }
    }

    #[test]
    fn exterior_views_select_real_reachable_lots_with_the_promised_features() {
        use crate::map::exterior::{Typology, classify, plan};

        let data = crate::map::world();
        let cases = [
            (ExteriorScene::Townhouse, Some(Typology::Townhouse)),
            (ExteriorScene::Walkup, Some(Typology::Walkup)),
            (ExteriorScene::Shophouse, Some(Typology::Shophouse)),
            (ExteriorScene::ServiceStair, Some(Typology::Walkup)),
            (ExteriorScene::Rooftop, None),
            (ExteriorScene::Cutaway, Some(Typology::Walkup)),
        ];
        for (scene, expected) in cases {
            let size = if scene == ExteriorScene::Cutaway {
                [390, 844]
            } else {
                [1280, 720]
            };
            let fixture = exterior_fixture(data, scene, size).unwrap();
            let b = &data.blds[fixture.building as usize];
            let layout = plan(b, data);
            if let Some(expected) = expected {
                assert_eq!(classify(b), expected);
            }
            assert!(
                data.free(fixture.position, 0.6),
                "{scene:?}: player intersects collision"
            );
            assert!(crate::camera::ZOOM_RANGE.contains(&fixture.zoom));
            if scene != ExteriorScene::Cutaway {
                assert!(
                    !crate::map::exterior_blocks_camera(b, fixture.position),
                    "{scene:?}: an outdoor fixture must not hide its own subject"
                );
            }
            assert!(
                b.c.distance(fixture.position) < 65.0,
                "target is outside near-detail radius"
            );
            match scene {
                ExteriorScene::Townhouse | ExteriorScene::Walkup | ExteriorScene::Shophouse => {
                    assert!(
                        !layout.balconies.is_empty(),
                        "{scene:?}: no actual balcony layout"
                    );
                    assert!(data.bld_at(fixture.position).is_none());
                }
                ExteriorScene::ServiceStair => assert!(layout.stair.is_some()),
                ExteriorScene::Rooftop => {
                    let roof = crate::map::roofs::layout(b);
                    assert!(!roof.shacks.is_empty());
                    assert!(roof.access.is_some());
                }
                ExteriorScene::Cutaway => {
                    assert_eq!(
                        data.bld_at(fixture.position).map(|inside| inside.id),
                        Some(b.id)
                    );
                }
            }
            eprintln!(
                "Exterior fixture {scene:?}: lot={} position={:?} zoom={:.3}",
                b.id, fixture.position, fixture.zoom
            );
        }
    }
}
