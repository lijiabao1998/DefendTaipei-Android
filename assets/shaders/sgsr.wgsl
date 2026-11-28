struct UiVertexOutput {
    @location(0) uv: vec2<f32>,
    @location(1) border_widths: vec4<f32>,
    @location(2) border_radius_x: vec4<f32>,
    @location(3) border_radius_y: vec4<f32>,
    @location(4) @interpolate(flat) size: vec2<f32>,
    @builtin(position) position: vec4<f32>,
};

// params.xy = 1.0 / scene_size, params.z = sharpness (0..1), params.w = enabled (> 0.5)
@group(1) @binding(0) var<uniform> params: vec4<f32>;
@group(1) @binding(1) var scene_tex: texture_2d<f32>;
@group(1) @binding(2) var scene_sampler: sampler;

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let base = textureSample(scene_tex, scene_sampler, in.uv);
    if params.w < 0.5 {
        return vec4<f32>(base.rgb, 1.0);
    }
    let texel = params.xy;
    let inv_diag = texel * 0.75;

    // 5-tap cardinal cross around pixel center for single-pass Snapdragon GSR 1.0-style edge sharpening.
    let n = textureSample(scene_tex, scene_sampler, in.uv + vec2<f32>(0.0, -texel.y)).rgb;
    let s = textureSample(scene_tex, scene_sampler, in.uv + vec2<f32>(0.0, texel.y)).rgb;
    let w = textureSample(scene_tex, scene_sampler, in.uv + vec2<f32>(-texel.x, 0.0)).rgb;
    let e = textureSample(scene_tex, scene_sampler, in.uv + vec2<f32>(texel.x, 0.0)).rgb;

    let min_c = min(base.rgb, min(min(n, s), min(w, e)));
    let max_c = max(base.rgb, max(max(n, s), max(w, e)));

    let lc = luma(base.rgb);
    let ln = luma(n);
    let ls = luma(s);
    let lw = luma(w);
    let le = luma(e);

    // Directional edge responsiveness: suppress ringing on flat/noisy texels, boost clean edges.
    let grad_h = abs(lw - le);
    let grad_v = abs(ln - ls);
    let edge_dir = max(grad_h, grad_v);
    let high_pass = lc - 0.25 * (ln + ls + lw + le);

    let d1 = textureSample(scene_tex, scene_sampler, in.uv + inv_diag).rgb;
    let d2 = textureSample(scene_tex, scene_sampler, in.uv - inv_diag).rgb;
    let wide = 0.25 * (n + s + w + e) * 0.65 + 0.5 * (d1 + d2) * 0.35;

    let sharp = params.z * clamp(0.45 + edge_dir * 1.4, 0.25, 1.25);
    let upscaled = clamp(base.rgb + (base.rgb - wide) * sharp + vec3<f32>(high_pass * sharp * 0.22), min_c, max_c);
    return vec4<f32>(upscaled, 1.0);
}
