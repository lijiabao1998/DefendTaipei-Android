//! Single-pass Snapdragon Game Super Resolution (SGSR 1.0-inspired) spatial
//! upscaler implemented as a Bevy `UiMaterial`.
//!
//! When render scale is `100%` (`enabled = 0.0`), the fragment shader is an
//! exact passthrough so desktop QA captures and native-resolution views lose
//! zero fidelity. When scale < 100 (`60%`, `70%`, `85%`), edge-directed spatial
//! sharpening with local neighborhood clamping sharpens the 3D scene while UI
//! overlays stay at full native resolution.

use bevy::{
    asset::uuid_handle,
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::{Shader, ShaderRef},
};

pub const SGSR_SHADER_HANDLE: Handle<Shader> = uuid_handle!("8c3f1d2a-69c4-4f88-9e12-5a770c4e1201");

#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub(crate) struct SgsrMaterial {
    /// `xy`: `1.0 / scene_dimensions`, `z`: sharpness, `w`: `1.0` when enabled (`scale < 100`).
    #[uniform(0)]
    pub params: Vec4,
    #[texture(1)]
    #[sampler(2)]
    pub scene: Handle<Image>,
}

impl UiMaterial for SgsrMaterial {
    fn fragment_shader() -> ShaderRef {
        SGSR_SHADER_HANDLE.into()
    }
}

pub(super) fn install(app: &mut App) {
    let shader = Shader::from_wgsl(
        include_str!("../../../assets/shaders/sgsr.wgsl"),
        "shaders/sgsr.wgsl",
    );
    app.world_mut()
        .resource_mut::<Assets<Shader>>()
        .insert(SGSR_SHADER_HANDLE.id(), shader)
        .expect("insert synchronous SGSR shader");
    app.add_plugins(UiMaterialPlugin::<SgsrMaterial>::default());
}
