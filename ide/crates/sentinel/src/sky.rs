//! The DeepSeek hero background, ported from their WebGL2 fragment shader
//! (module 87799 of the deepseek.com bundle) with the exact parameters the
//! homepage passes it:
//! `{ colors:["#8AA3D6","#FFFFFF","#FFFFFF"], speed:14, distortion:20,
//!    swirl:12, swirlIterations:8, scale:0.5, rotation:-5, proportion:50,
//!    softness:100, shape:"checks", shapeScale:10, offsetX:0, offsetY:65 }`.
//!
//! The pattern is evaluated on the CPU into a small texture (the field is
//! low-frequency, so linear upscale is indistinguishable) and animated by the
//! same three time loops as `u_loop1..3`.

use eframe::egui::{self, Color32, ColorImage, Rect, TextureOptions, Vec2};

/// Periods of the three uniform loops, in seconds (module 46984, pattern
/// branch: `r=[420,460,90]`), and the time scale the render loop applies:
/// `elapsed += 0.001 * delta_ms * (speed / 14)` — speed is 14 on the page.
const LOOP_PERIODS: [f64; 3] = [420.0, 460.0, 90.0];
const LOOP_RADIUS: f64 = 0.07;

/// Uniform values exactly as the homepage configures them.
const NOISE_SCALE: f64 = 0.0005 + 0.006 * 0.5; // scale: 0.5
const OFFSET: (f64, f64) = (0.0, 65.0 / 100.0);
const ROTATION: f64 = -5.0 / 90.0; // multiplied by PI/2 in the shader
const DISTORTION: f64 = 20.0 / 100.0;
const SWIRL: f64 = 12.0 / 50.0;
const SWIRL_ITERATIONS: u32 = 8;
const SHAPE_SCALE: f64 = 10.0 / 100.0;
const COLOR1: [f32; 3] = [0.541, 0.639, 0.839]; // #8AA3D6
const WHITE: [f32; 3] = [1.0, 1.0, 1.0];

/// The vertical gradient of the hero section behind the pattern layer.
const SKY_TOP: Color32 = Color32::from_rgb(156, 193, 231); // #9cc1e7
const SKY_BOTTOM: Color32 = Color32::from_rgb(250, 250, 250);

#[derive(Default)]
pub struct SkyField {
    elapsed: [f64; 3],
    last: Option<f64>,
    texture: Option<egui::TextureHandle>,
    redraw_in: f32,
}

impl SkyField {
    /// Paint the animated background into `rect`. `dt` is applied internally
    /// from the context clock, capped so tab switches do not jump the field.
    pub fn paint(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let now = ui.ctx().input(|i| i.time);
        let dt = self.last.map_or(0.0, |last| (now - last).clamp(0.0, 0.1));
        self.last = Some(now);
        for (index, period) in LOOP_PERIODS.iter().enumerate() {
            self.elapsed[index] = (self.elapsed[index] + dt) % period;
        }
        self.redraw_in -= dt as f32;

        let size = (
            (rect.width() / 3.0).round().clamp(160.0, 480.0) as usize,
            (rect.height() / 3.0).round().clamp(100.0, 320.0) as usize,
        );
        let rebuild = self.texture.is_none()
            || self.redraw_in <= 0.0
            || self
                .texture
                .as_ref()
                .is_some_and(|texture| texture.size() != [size.0, size.1]);
        if rebuild {
            let image = render(size.0, size.1, rect, self.elapsed);
            match self.texture.take() {
                Some(mut texture) => {
                    texture.set(image, TextureOptions::LINEAR);
                    self.texture = Some(texture);
                }
                None => {
                    let handle =
                        ui.ctx()
                            .load_texture("sky-pattern", image, TextureOptions::LINEAR);
                    self.texture = Some(handle);
                }
            }
            self.redraw_in = 1.0 / 30.0;
        }
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, SKY_BOTTOM);
        // The hero section gradient behind the (masked) pattern layer.
        vertical_gradient(painter, rect, SKY_TOP, SKY_BOTTOM);
        if let Some(texture) = &self.texture {
            painter.image(
                texture.id(),
                rect,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }
}

/// Exact port of the pattern fragment shader main().
fn render(width: usize, height: usize, rect: Rect, elapsed: [f64; 3]) -> ColorImage {
    let loops = loop_uniforms(elapsed);
    let resolution = (
        f64::from(rect.width().max(1.0)),
        f64::from(rect.height().max(1.0)),
    );
    let rotation = ROTATION * std::f64::consts::FRAC_PI_2;
    let (rot_cos, rot_sin) = (rotation.cos(), rotation.sin());
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            // gl_FragCoord origin is bottom-left; egui images are top-down.
            let mut uv = (
                (x as f64 + 0.5) / width as f64,
                1.0 - (y as f64 + 0.5) / height as f64,
            );
            uv.0 -= 0.5;
            uv.1 -= 0.5;
            uv.0 *= NOISE_SCALE * resolution.0;
            uv.1 *= NOISE_SCALE * resolution.1;
            let rotated = (
                rot_cos * uv.0 - rot_sin * uv.1,
                rot_sin * uv.0 + rot_cos * uv.1,
            );
            uv = (rotated.0 + 0.5 + OFFSET.0, rotated.1 + 0.5 + OFFSET.1);

            let n1 = noise(uv.0 + loops[0].0, uv.1 + loops[0].1);
            let n2 = noise(uv.0 * 2.0 - loops[1].0, uv.1 * 2.0 - loops[1].1);
            let angle = n1 * std::f64::consts::TAU;
            uv.0 += 4.0 * DISTORTION * n2 * angle.cos();
            uv.1 += 4.0 * DISTORTION * n2 * angle.sin();
            for i in 1..=SWIRL_ITERATIONS {
                let i = f64::from(i);
                let weight = SWIRL / i;
                uv.0 += weight
                    * (loops[2].1 * (i * 1.5 * uv.1).cos() - loops[2].0 * (i * 1.5 * uv.1).sin());
                uv.1 += weight * (loops[2].1 * (i * uv.0).cos() - loops[2].0 * (i * uv.0).sin());
            }

            let shape_uv = (
                uv.0 * (0.5 + 3.5 * SHAPE_SCALE),
                uv.1 * (0.5 + 3.5 * SHAPE_SCALE),
            );
            let shape = 0.5 + 0.5 * shape_uv.0.sin() * shape_uv.1.cos();
            // proportion is exactly 0.5 on the homepage, so the correction
            // term `0.48 * sign(0) * sqrt(0)` is zero.
            let mixer = shape;

            // blend_multi with colorCount=3 and softness=1 (edge=0).
            let mut col = COLOR1;
            let r1 = smoothstep(0.0, 0.7, mixer as f32);
            col[0] = col[0] + (WHITE[0] - col[0]) * r1;
            col[1] = col[1] + (WHITE[1] - col[1]) * r1;
            col[2] = col[2] + (WHITE[2] - col[2]) * r1;
            let r2 = smoothstep(0.3, 1.0, mixer as f32);
            col[0] = col[0] + (WHITE[0] - col[0]) * r2;
            col[1] = col[1] + (WHITE[1] - col[1]) * r2;
            col[2] = col[2] + (WHITE[2] - col[2]) * r2;

            // The CSS mask over the whole layer: #000000fc 0%,
            // #000000e8 8.98%, transparent 100% — piecewise linear alpha.
            let t = y as f64 / height.max(1) as f64;
            let mask = if t <= 0.0898 {
                0.988 + (0.910 - 0.988) * (t / 0.0898)
            } else {
                0.910 * (1.0 - (t - 0.0898) / (1.0 - 0.0898))
            };
            let mask = mask.clamp(0.0, 1.0) as f32;
            pixels.push(Color32::from_rgba_unmultiplied(
                (col[0] * 255.0).round() as u8,
                (col[1] * 255.0).round() as u8,
                (col[2] * 255.0).round() as u8,
                (mask * 255.0).round() as u8,
            ));
        }
    }
    ColorImage {
        size: [width, height],
        source_size: Vec2::new(width as f32, height as f32),
        pixels,
    }
}

/// Module 46984 loop uniforms for the pattern branch.
fn loop_uniforms(elapsed: [f64; 3]) -> [(f64, f64); 3] {
    let mut out = [(0.0, 0.0); 3];
    for (index, period) in LOOP_PERIODS.iter().enumerate() {
        let theta = elapsed[index] / period * std::f64::consts::TAU;
        let (s, c) = (theta.sin(), theta.cos());
        let radius = LOOP_RADIUS * period / std::f64::consts::TAU;
        out[index] = if index == 2 {
            (s, c)
        } else {
            (radius * (s + c - 1.0), radius * (s + 1.0 - c))
        };
    }
    out
}

/// GLSL-style value noise: `fract(sin(dot(st, vec2(12.9898, 78.233))) * 43758.5453123)`
/// hashed on the integer lattice with a smoothstep blend. f64 keeps the hash
/// stable for the large lattice coordinates the distortion reaches.
fn noise(x: f64, y: f64) -> f64 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let a = hash(ix, iy);
    let b = hash(ix + 1.0, iy);
    let c = hash(ix, iy + 1.0);
    let d = hash(ix + 1.0, iy + 1.0);
    let ux = fx * fx * (3.0 - 2.0 * fx);
    let uy = fy * fy * (3.0 - 2.0 * fy);
    let top = a + (b - a) * ux;
    let bottom = c + (d - c) * ux;
    top + (bottom - top) * uy
}

fn hash(x: f64, y: f64) -> f64 {
    let value = (x * 12.9898 + y * 78.233).sin() * 43758.5453123;
    value - value.floor()
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn vertical_gradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.indices = vec![0, 1, 2, 2, 1, 3];
    painter.add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_and_bounded() {
        let a = noise(1.7, -3.2);
        let b = noise(1.7, -3.2);
        assert_eq!(a, b);
        assert!((0.0..=1.0).contains(&a), "value noise stays in [0, 1]");
        let varied = (0..16)
            .map(|i| noise(f64::from(i) * 0.37, f64::from(i) * 1.13))
            .collect::<Vec<_>>();
        assert!(varied.iter().any(|v| *v > 0.4));
        assert!(varied.iter().any(|v| *v < 0.6));
    }

    #[test]
    fn loop_uniforms_follow_the_pattern_formula() {
        let loops = loop_uniforms([0.0, 0.0, 0.0]);
        let radius1 = LOOP_RADIUS * LOOP_PERIODS[0] / std::f64::consts::TAU;
        assert!((loops[0].0 - radius1 * (0.0 + 1.0 - 1.0)).abs() < 1e-9);
        let loops = loop_uniforms([LOOP_PERIODS[0] / 4.0, 0.0, LOOP_PERIODS[2] / 4.0]);
        assert!((loops[2].0 - 1.0).abs() < 1e-9, "quarter period: sin=1");
        assert!(loops[2].1.abs() < 1e-9, "quarter period: cos=0");
    }
}
