//! A tiny software 2D canvas: just enough of `CanvasRenderingContext2D` (fillRect, strokes,
//! linear / radial gradients, polygons, ellipses, ImageData) to port the original game's
//! procedural textures (`src/map/textures.ts`, `arcade.ts`, `rooftin.ts`, `walkup.ts`) 1:1.
//! Pixels are straight-alpha sRGB in 0..1; `to_image` builds mip levels and a repeat sampler.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

/// `Math.random()` stand-in: deterministic so every device paints the same city.
pub struct Rng(pub u64);
impl Rng {
    pub fn f(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / ((1u64 << 24) as f32)
    }
}

/// Parse "#rrggbb" or (r, g, b, a) into RGBA 0..1.
pub fn css(hexs: &str) -> [f32; 4] {
    let h = u32::from_str_radix(hexs.trim_start_matches('#'), 16).unwrap_or(0);
    [
        ((h >> 16) & 255) as f32 / 255.0,
        ((h >> 8) & 255) as f32 / 255.0,
        (h & 255) as f32 / 255.0,
        1.0,
    ]
}

pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> [f32; 4] {
    [r / 255.0, g / 255.0, b / 255.0, a]
}

pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[f32; 4]>,
}

impl Canvas {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            px: vec![[0.0, 0.0, 0.0, 1.0]; w * h],
        }
    }

    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, c: [f32; 4], cov: f32) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let a = (c[3] * cov).clamp(0.0, 1.0);
        let p = &mut self.px[y as usize * self.w + x as usize];
        for k in 0..3 {
            p[k] = p[k] * (1.0 - a) + c[k] * a;
        }
        p[3] = p[3] + a * (1.0 - p[3]);
    }

    /// `fillRect` with fractional-edge coverage (canvas antialiasing).
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: [f32; 4]) {
        let (x0, y0, x1, y1) = (x.min(x + w), y.min(y + h), x.max(x + w), y.max(y + h));
        for py in y0.floor() as i32..y1.ceil() as i32 {
            let cy = ((py + 1) as f32).min(y1) - (py as f32).max(y0);
            if cy <= 0.0 {
                continue;
            }
            for px in x0.floor() as i32..x1.ceil() as i32 {
                let cx = ((px + 1) as f32).min(x1) - (px as f32).max(x0);
                if cx > 0.0 {
                    self.blend(px, py, c, cx * cy);
                }
            }
        }
    }

    /// Vertical linear gradient (createLinearGradient(0, g0, 0, g1)) filling a rect.
    pub fn grad_rect_v(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        g0: f32,
        g1: f32,
        c0: [f32; 4],
        c1: [f32; 4],
    ) {
        for py in y.floor() as i32..(y + h).ceil() as i32 {
            let t = (((py as f32 + 0.5) - g0) / (g1 - g0)).clamp(0.0, 1.0);
            let c = lerp4(c0, c1, t);
            self.fill_rect(x, py as f32, w, 1.0, c);
        }
    }

    /// Horizontal linear gradient (createLinearGradient(g0, 0, g1, 0)) with 3 stops.
    pub fn grad_rect_h3(&mut self, x: f32, y: f32, w: f32, h: f32, c: [[f32; 4]; 3]) {
        for px in x.floor() as i32..(x + w).ceil() as i32 {
            let t = ((px as f32 + 0.5 - x) / w).clamp(0.0, 1.0);
            let col = if t < 0.5 {
                lerp4(c[0], c[1], t * 2.0)
            } else {
                lerp4(c[1], c[2], t * 2.0 - 1.0)
            };
            self.fill_rect(px as f32, y, 1.0, h, col);
        }
    }

    /// Even-odd polygon fill with 4× vertical supersampling.
    pub fn fill_poly(&mut self, pts: &[(f32, f32)], c: [f32; 4]) {
        if pts.len() < 3 {
            return;
        }
        let ymin = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor() as i32;
        let ymax = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil() as i32;
        let ss = 4;
        for py in ymin..ymax {
            let mut row = vec![0.0f32; self.w + 2];
            for s in 0..ss {
                let sy = py as f32 + (s as f32 + 0.5) / ss as f32;
                let mut xs = Vec::new();
                for i in 0..pts.len() {
                    let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                    if (a.1 > sy) != (b.1 > sy) {
                        xs.push(a.0 + (sy - a.1) / (b.1 - a.1) * (b.0 - a.0));
                    }
                }
                xs.sort_by(|a, b| a.total_cmp(b));
                for seg in xs.chunks(2) {
                    if seg.len() < 2 {
                        break;
                    }
                    let (x0, x1) = (seg[0].max(0.0), seg[1].min(self.w as f32));
                    for px in x0.floor() as i32..x1.ceil() as i32 {
                        let cov = ((px + 1) as f32).min(x1) - (px as f32).max(x0);
                        if cov > 0.0 && px >= 0 {
                            row[px as usize] += cov / ss as f32;
                        }
                    }
                }
            }
            for (px, cov) in row.iter().enumerate().take(self.w) {
                if *cov > 0.0 {
                    self.blend(px as i32, py, c, cov.min(1.0));
                }
            }
        }
    }

    pub fn fill_ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: [f32; 4]) {
        let n = 24;
        let pts: Vec<(f32, f32)> = (0..n)
            .map(|k| {
                let a = k as f32 / n as f32 * std::f32::consts::TAU;
                (cx + a.cos() * rx, cy + a.sin() * ry)
            })
            .collect();
        self.fill_poly(&pts, c);
    }

    /// `stroke` of a line segment with the given width (butt caps).
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, w: f32, c: [f32; 4]) {
        let d = Vec2::new(x1 - x0, y1 - y0);
        let l = d.length();
        if l < 1e-4 {
            return;
        }
        let n = Vec2::new(-d.y, d.x) / l * (w / 2.0);
        self.fill_poly(
            &[
                (x0 + n.x, y0 + n.y),
                (x1 + n.x, y1 + n.y),
                (x1 - n.x, y1 - n.y),
                (x0 - n.x, y0 - n.y),
            ],
            c,
        );
    }

    pub fn polyline(&mut self, pts: &[(f32, f32)], w: f32, c: [f32; 4]) {
        for s in pts.windows(2) {
            self.line(s[0].0, s[0].1, s[1].0, s[1].1, w, c);
        }
    }

    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, lw: f32, c: [f32; 4]) {
        let o = lw / 2.0;
        self.fill_rect(x - o, y - o, w + lw, lw, c);
        self.fill_rect(x - o, y + h - o, w + lw, lw, c);
        self.fill_rect(x - o, y + o, lw, h - lw, c);
        self.fill_rect(x + w - o, y + o, lw, h - lw, c);
    }

    /// Radial gradient blob (createRadialGradient(0,0,0, 0,0,r) inside a scaled square).
    pub fn radial(&mut self, cx: f32, cy: f32, r: f32, sy: f32, c0: [f32; 4], c1: [f32; 4]) {
        for py in (cy - r * sy).floor() as i32..(cy + r * sy).ceil() as i32 {
            for px in (cx - r).floor() as i32..(cx + r).ceil() as i32 {
                let dx = (px as f32 + 0.5 - cx) / r;
                let dy = (py as f32 + 0.5 - cy) / (r * sy);
                let t = (dx * dx + dy * dy).sqrt();
                if t < 1.0 {
                    self.blend(px, py, lerp4(c0, c1, t), 1.0);
                }
            }
        }
    }

    /// RGBA8 sRGB bytes of the canvas.
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.w * self.h * 4);
        for p in &self.px {
            for v in p {
                out.push((v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
            }
        }
        out
    }

    /// GPU image with a full mip chain (box filtered in linear light) and a repeating sampler.
    pub fn to_image(&self, srgb: bool, repeat: bool) -> Image {
        image_with_mips(self.w, self.h, &self.bytes(), srgb, repeat)
    }
}

pub fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

fn to_lin(v: u8) -> f32 {
    let s = v as f32 / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}
fn to_srgb(l: f32) -> u8 {
    let s = if l <= 0.0031308 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (s.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// RGBA8 → Image with mips. Without mips the iso camera's minified wall textures shimmer.
pub fn image_with_mips(w: usize, h: usize, rgba8: &[u8], srgb: bool, repeat: bool) -> Image {
    let mut data = rgba8.to_vec();
    let mut levels = 1u32;
    let (mut cw, mut ch) = (w, h);
    let mut cur = rgba8.to_vec();
    while cw > 1 || ch > 1 {
        let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for k in 0..4 {
                    let mut acc = 0.0;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(cw - 1);
                        let sy = (y * 2 + dy).min(ch - 1);
                        let v = cur[(sy * cw + sx) * 4 + k];
                        acc += if srgb && k < 3 {
                            to_lin(v)
                        } else {
                            v as f32 / 255.0
                        };
                    }
                    acc /= 4.0;
                    next[(y * nw + x) * 4 + k] = if srgb && k < 3 {
                        to_srgb(acc)
                    } else {
                        (acc * 255.0 + 0.5) as u8
                    };
                }
            }
        }
        data.extend_from_slice(&next);
        cur = next;
        cw = nw;
        ch = nh;
        levels += 1;
    }
    let mut img = Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba8.to_vec(),
        if srgb {
            TextureFormat::Rgba8UnormSrgb
        } else {
            TextureFormat::Rgba8Unorm
        },
        RenderAssetUsages::RENDER_WORLD,
    );
    img.data = Some(data);
    img.texture_descriptor.mip_level_count = levels;
    let mode = if repeat {
        ImageAddressMode::Repeat
    } else {
        ImageAddressMode::ClampToEdge
    };
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: mode,
        address_mode_v: mode,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 4,
        ..default()
    });
    img
}
